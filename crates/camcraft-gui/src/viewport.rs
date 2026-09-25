//! The 3D view: the part, every operation's toolpath, the stock and the cutter,
//! drawn Z-up with orbit, pan and zoom.
//!
//! Rendered offscreen into its own colour and depth textures, with its own
//! multisampling, and shown in egui as an image. Drawing straight into egui's render
//! pass would need a depth buffer for the whole window, and would tie the pipelines
//! to however the window (or the headless snapshot renderer) happened to be
//! configured. Offscreen, the viewport owns everything it draws with.
//!
//! Toolpaths are instanced screen-space quads rather than GPU lines: a GPU line is
//! one physical pixel, half a point on a Retina display, and too thin to read.
//! Every segment is uploaded once per run, in machine order, so its instance index
//! *is* its order. Visibility by role and operation, and the scrub, are then a
//! uniform the vertex shader reads, not a buffer rebuilt on every change.
//!
//! What is picked, and what is under the pointer, is drawn over the part: its faces
//! again, in the accent, through a pipeline that passes where the part's own depth
//! is equal, so they lie exactly on it; its edges as wide lines; its points as
//! marks painted over the picture. What the view hits is not decided here: the view
//! reports where the pointer is and whether it clicked, as a ray, and
//! [`crate::pick`] answers.

use camcraft_geom::heightmap::narrow;
use camcraft_geom::mesh::Aabb;
use camcraft_geom::mesh::TriMesh;
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use eframe::wgpu;
use glam::{DVec2, DVec3, Mat4, Vec2, Vec3};
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::part_ao::PartAo;
use crate::pick::Projector;
use crate::pyramid::Pyramid;
use crate::scene::Scene;
use crate::stock_ao;
use crate::stock_tiles::{self, Tile};
use crate::stock_view::{CLASSES, Class, ColorMode, NO_CELL, StockImage, cell_color};
use crate::theme;
use camcraft_sim::stock::CellRect;

/// Offscreen colour format: what egui expects of a native texture.
const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;
/// Line kind for the stock outline, after the seven move roles; the work zero's
/// axes follow it.
const STOCK: u32 = 7;
/// Line kinds for the work zero's X, Y and Z axes.
const AXIS_X: u32 = 8;
const AXIS_Y: u32 = 9;
const AXIS_Z: u32 = 10;
/// Line kinds for picked edges, the edge under the pointer, and the boundary an
/// operation is kept to. Always shown, and drawn wider.
const SELECTED: u32 = 11;
const HOVER: u32 = 12;
const BOUNDARY: u32 = 13;
/// Line kind for a stock that could be picked into the setup: faint, and drawn
/// only while it is being picked.
const GHOST: u32 = 14;
/// Line kinds for the edges of an operation's height planes: clearance, top and
/// bottom.
const CLEARANCE: u32 = 15;
const TOP: u32 = 16;
const BOTTOM: u32 = 17;
/// Line kinds in all, and so colours.
const KINDS: usize = 18;
/// Toolpath width, in points.
const LINE_WIDTH: f32 = 1.25;

const COMMON: &str = r"
struct Frame {
    view_proj: mat4x4<f32>,
    light: vec4<f32>,
    // Viewport width and height in pixels, line width in pixels, depth pull.
    viewport: vec4<f32>,
    // Visible kinds (roles, then stock and axes), visible operations, segments shown.
    masks: vec4<u32>,
    colors: array<vec4<f32>, 18>,
    // Where the eye is.
    eye: vec4<f32>,
};
@group(0) @binding(0) var<uniform> f: Frame;

// egui shows a native texture as gamma-encoded, so light in linear and encode.
fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}
";

const MESH_SHADER: &str = r"
struct Object {
    model: mat4x4<f32>,
    color: vec4<f32>,
};
@group(1) @binding(0) var<uniform> o: Object;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};
struct VertexOut {
    // Invariant, so a picked face drawn again over the part lands at exactly the
    // part's depth, and the equal depth test passes.
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
};

@vertex
fn vs(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = f.view_proj * o.model * vec4<f32>(v.position, 1.0);
    // Objects are only ever turned and moved, never scaled, so the model matrix
    // turns normals too.
    out.normal = (o.model * vec4<f32>(v.normal, 0.0)).xyz;
    return out;
}

@fragment
fn fs(i: VertexOut) -> @location(0) vec4<f32> {
    // Two-sided: a mesh from an STL may face either way, and both sides are part.
    let n = normalize(i.normal);
    let diffuse = abs(dot(n, f.light.xyz));
    let lit = o.color.rgb * (0.25 + 0.75 * diffuse);
    return vec4<f32>(to_srgb(lit), o.color.a);
}
";

const LINE_SHADER: &str = r"
struct LineIn {
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) kind: u32,
    @location(3) op: u32,
};
struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32, l: LineIn) -> LineOut {
    var out: LineOut;
    out.color = vec4<f32>(0.0);
    let kind_on = ((f.masks.x >> l.kind) & 1u) == 1u;
    let is_move = l.kind < 7u;
    let op_on = l.op >= 32u || ((f.masks.y >> l.op) & 1u) == 1u;
    let scrubbed = ii < f.masks.z;
    let ca = f.view_proj * vec4<f32>(l.a, 1.0);
    let cb = f.view_proj * vec4<f32>(l.b, 1.0);
    // Hidden, past the scrub, or crossing the eye: put it outside the clip volume.
    if (!kind_on || (is_move && (!op_on || !scrubbed)) || ca.w <= 1e-6 || cb.w <= 1e-6) {
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }

    let half = f.viewport.xy * 0.5;
    let sa = ca.xy / ca.w * half;
    let sb = cb.xy / cb.w * half;
    var dir = sb - sa;
    if (length(dir) < 1e-6) {
        dir = vec2<f32>(1.0, 0.0);
    }
    dir = normalize(dir);
    let normal = vec2<f32>(-dir.y, dir.x);

    // Two triangles: which end, which side.
    var ends = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var sides = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let t = ends[vi];
    var clip = mix(ca, cb, t);
    // Picks and the boundary are drawn twice as wide, to read over the part.
    let width = select(f.viewport.z, f.viewport.z * 2.0, l.kind >= 11u && l.kind <= 13u);
    let offset = normal * sides[vi] * width * 0.5 + dir * (t * 2.0 - 1.0) * width * 0.5;
    clip = vec4<f32>(clip.xy + offset / half * clip.w, clip.z, clip.w);
    // A cut runs exactly on the surface it follows; pull it towards the eye so the
    // surface cannot flicker through it. Scaled by w, so the pull is constant in NDC.
    clip.z = clip.z - f.viewport.w * clip.w;
    out.clip = clip;
    out.color = f.colors[l.kind];
    return out;
}

@fragment
fn fs(i: LineOut) -> @location(0) vec4<f32> {
    return vec4<f32>(to_srgb(i.color.rgb), 1.0);
}
";

/// The material, drawn by tracing each pixel's ray through its heights.
///
/// The stock is a height per cell: from above, a column of material with a flat
/// top, which is exactly what 3-axis cutting leaves. So each column is drawn as
/// one: a ray that meets the side of a taller column meets a vertical wall, and
/// one that comes down onto a column meets its top. Nothing joins neighbouring
/// cells with a slope, so a wall the cutter left is drawn vertical, at any
/// resolution.
///
/// Only the stock's box is rasterised. Its far faces trace the ray from the eye
/// down [`crate::pyramid`]'s levels of maxima (the height texture's mips),
/// skipping every square the ray passes over in one step, to the cell it first
/// meets; the near faces stand aside. The hit writes its own depth, so paths and
/// the cutter sit in front of it or behind it as they are. Tops are lit by the
/// surface's slope, found from the neighbouring cells that are not across a
/// wall, so a curved surface reads as smooth; the side of a column is lit as a
/// wall only where it stands more than [`WALL_CELLS`] cells tall.
const STOCK_SHADER: &str = r"
// One tile of the material: see `stock_tiles.rs`.
struct Grid {
    // The texture's cell (0, 0) in X and Y, cell size, floor.
    origin_pitch_floor: vec4<f32>,
    // Cells the textures hold across and down, levels in the height pyramid.
    size: vec4<u32>,
    // Top of the material, the step that makes a wall.
    top_wall: vec4<f32>,
    // The tile's own cells across and down, the apron around them, the colours:
    // 0 the material's, 1 the verifier's.
    core: vec4<u32>,
    // Linear colours: below the part, leftover faint and strong, on the part,
    // off the part, and the material.
    palette: array<vec4<f32>, 6>,
};
@group(1) @binding(0) var<uniform> g: Grid;
@group(1) @binding(1) var heights: texture_2d<f32>;
@group(1) @binding(2) var codes: texture_2d<u32>;
// How much sky each cell sees: see `stock_ao.rs`.
@group(1) @binding(3) var sky: texture_2d<f32>;

// How much a cell's occlusion darkens it: some light still comes in from the
// side a pocket's walls do not block, and a wall-to-wall black reads as a hole.
const SKY_STRENGTH: f32 = 0.8;

struct BoxOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

struct Hit {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

// The box's twelve triangles, as corners: bit 0 is X, bit 1 Y, bit 2 Z.
var<private> BOX: array<u32, 36> = array<u32, 36>(
    0u, 2u, 1u, 1u, 2u, 3u,  4u, 5u, 6u, 5u, 7u, 6u,
    0u, 1u, 4u, 1u, 5u, 4u,  2u, 6u, 3u, 3u, 6u, 7u,
    0u, 4u, 2u, 2u, 4u, 6u,  1u, 3u, 5u, 3u, 7u, 5u,
);

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> BoxOut {
    let c = BOX[vi];
    let o = g.origin_pitch_floor;
    let a = f32(g.core.z);
    let lo = vec3<f32>(o.x + (a - 0.5) * o.z, o.y + (a - 0.5) * o.z, o.w);
    let hi = vec3<f32>(
        o.x + (a + f32(g.core.x) - 0.5) * o.z,
        o.y + (a + f32(g.core.y) - 0.5) * o.z,
        g.top_wall.x,
    );
    let p = select(lo, hi, vec3<bool>((c & 1u) != 0u, (c & 2u) != 0u, (c & 4u) != 0u));
    var out: BoxOut;
    out.clip = f.view_proj * vec4<f32>(p, 1.0);
    out.world = p;
    return out;
}

// A cell's colour from its code: its class times 64, plus a leftover's shade.
fn code_color(c: vec2<i32>) -> vec3<f32> {
    let code = textureLoad(codes, c, 0).r;
    let kind = min(code / 64u, 3u);
    if (g.core.w == 0u) {
        return select(g.palette[5].rgb, g.palette[0].rgb, kind == 0u && code != 255u);
    }
    if (code == 255u) { return g.palette[4].rgb; }
    if (kind == 1u) {
        return mix(g.palette[1].rgb, g.palette[2].rgb, f32(code % 64u) / 63.0);
    }
    return g.palette[array<u32, 4>(0u, 1u, 3u, 4u)[kind]].rgb;
}

// A cell's height; the floor outside the stock.
fn height(c: vec2<i32>) -> f32 {
    if (c.x < 0 || c.y < 0 || c.x >= i32(g.size.x) || c.y >= i32(g.size.y)) {
        return g.origin_pitch_floor.w;
    }
    return textureLoad(heights, c, 0).r;
}

// How much sky a cell sees; all of it past the tile.
fn open_at(c: vec2<i32>) -> f32 {
    if (c.x < 0 || c.y < 0 || c.x >= i32(g.size.x) || c.y >= i32(g.size.y)) {
        return 1.0;
    }
    return textureLoad(sky, c, 0).r;
}

// How much sky is seen at `p` (in cells) on top of cell `own`: between the four
// cells round it, leaving out any across a wall, so a pocket's shade does not
// creep up onto the top beside it.
fn open_between(p: vec2<f32>, own: vec2<i32>) -> f32 {
    let q = p - 0.5;
    let c0 = vec2<i32>(floor(q));
    let t = q - floor(q);
    let h = height(own);
    var sum = 0.0;
    var weight = 0.0;
    for (var j = 0; j < 2; j++) {
        for (var i = 0; i < 2; i++) {
            let c = c0 + vec2<i32>(i, j);
            let w = select(1.0 - t.x, t.x, i == 1) * select(1.0 - t.y, t.y, j == 1);
            if (abs(height(c) - h) < g.top_wall.y) {
                sum += w * open_at(c);
                weight += w;
            }
        }
    }
    if (weight < 1e-4) { return open_at(own); }
    return sum / weight;
}

// A neighbour's height for the slope, or this cell's own across a wall.
fn joined(own: f32, c: vec2<i32>) -> f32 {
    let h = height(c);
    return select(own, h, abs(h - own) < g.top_wall.y);
}

// The surface's slope at a cell, as a normal.
fn smooth_normal(c: vec2<i32>) -> vec3<f32> {
    let own = height(c);
    let p = g.origin_pitch_floor.z;
    let dx = (joined(own, c + vec2<i32>(1, 0)) - joined(own, c - vec2<i32>(1, 0))) / (2.0 * p);
    let dy = (joined(own, c + vec2<i32>(0, 1)) - joined(own, c - vec2<i32>(0, 1))) / (2.0 * p);
    return normalize(vec3<f32>(-dx, -dy, 1.0));
}

// A wall's facing at height `z` of cell `c`: away from the neighbours that are
// lower than that, the nearer counting more. The side of one cell faces along an
// axis, but a round wall is a staircase of cells, and lit by their faces it
// would stripe; the cells around it know which way the wall really faces.
fn wall_normal(c: vec2<i32>, z: f32, fallback: vec3<f32>) -> vec3<f32> {
    var away = vec2<f32>(0.0);
    for (var dy = -3; dy <= 3; dy++) {
        for (var dx = -3; dx <= 3; dx++) {
            let d = vec2<f32>(f32(dx), f32(dy));
            let r2 = dot(d, d);
            if (r2 == 0.0 || r2 > 10.0) { continue; }
            if (height(c + vec2<i32>(dx, dy)) < z) {
                away += d / r2;
            }
        }
    }
    if (dot(away, away) < 1e-6) { return fallback; }
    return vec3<f32>(normalize(away), 0.0);
}

@fragment
fn fs(i: BoxOut) -> Hit {
    let o = g.origin_pitch_floor;
    let eye = f.eye.xyz;
    let dir = normalize(i.world - eye);
    // In cells across, millimetres up: cell (i, j) spans [i, i + 1) x [j, j + 1).
    let ro = vec3<f32>((eye.x - o.x) / o.z + 0.5, (eye.y - o.y) / o.z + 0.5, eye.z);
    let rd = vec3<f32>(dir.x / o.z, dir.y / o.z, dir.z);
    let a = f32(g.core.z);

    // Where the ray is inside the box, and which face it comes in by.
    var t0 = 0.0;
    var t1 = 1e30;
    var n = vec3<f32>(0.0, 0.0, 1.0);
    let box_lo = vec3<f32>(a, a, o.w);
    let box_hi = vec3<f32>(a + f32(g.core.x), a + f32(g.core.y), g.top_wall.x);
    for (var a = 0; a < 3; a++) {
        if (abs(rd[a]) < 1e-12) {
            if (ro[a] < box_lo[a] || ro[a] > box_hi[a]) { discard; }
            continue;
        }
        let ta = (box_lo[a] - ro[a]) / rd[a];
        let tb = (box_hi[a] - ro[a]) / rd[a];
        let near = min(ta, tb);
        if (near > t0) {
            t0 = near;
            n = vec3<f32>(0.0);
            n[a] = -sign(rd[a]);
        }
        t1 = min(t1, max(ta, tb));
    }
    // The near faces leave the work to the far ones, which every ray has.
    if (t0 > t1 || distance(i.world, eye) < t1 - max(1e-3, t1 * 1e-5)) { discard; }

    // From where the ray comes into the box, so its distances stay small and
    // exact however far off the eye is; and which side of a cell boundary a point
    // on one is taken to be, leaning a hair the way the ray goes.
    let start = max(t0, 0.0);
    let o2 = ro + rd * start;
    let span = t1 - start;
    let lean = sign(rd.xy) * 1e-3;
    let top = i32(g.size.z) - 1;
    var level = top;
    var t = 0.0;
    var dipped = false;
    var found = false;
    var up = false;
    var cell = vec2<i32>(0);
    for (var step = 0; step < 1024; step++) {
        if (t > span) { break; }
        let p = o2 + rd * t;
        let s = f32(1 << u32(level));
        let c = vec2<i32>(floor((p.xy + lean) / s));
        let dims = vec2<i32>(textureDimensions(heights, level));
        let h = textureLoad(heights, clamp(c, vec2<i32>(0), dims - 1), level).r;
        // Where the ray leaves this square, and across which side.
        let lo = vec2<f32>(c) * s;
        var tx = 1e30;
        var ty = 1e30;
        if (rd.x > 0.0) { tx = (lo.x + s - o2.x) / rd.x; } else if (rd.x < 0.0) { tx = (lo.x - o2.x) / rd.x; }
        if (rd.y > 0.0) { ty = (lo.y + s - o2.y) / rd.y; } else if (rd.y < 0.0) { ty = (lo.y - o2.y) / rd.y; }
        if (p.z <= h) {
            // In among material: a wall if it came in sideways.
            if (level == 0) { found = true; up = dipped; cell = c; break; }
            level -= 1;
            continue;
        }
        var th = 1e30;
        if (rd.z < 0.0) { th = t + (h - p.z) / rd.z; }
        if (th < min(tx, ty)) {
            // Comes down to this square's highest before leaving it: look closer.
            t = th;
            if (level == 0) { found = true; up = true; cell = c; break; }
            dipped = true;
            level -= 1;
            continue;
        }
        // Over it all: on to the next square, and wider.
        if (tx < ty) {
            t = tx;
            n = vec3<f32>(-sign(rd.x), 0.0, 0.0);
        } else {
            t = ty;
            n = vec3<f32>(0.0, -sign(rd.y), 0.0);
        }
        dipped = false;
        level = min(level + 1, top);
    }
    if (!found) { discard; }
    // The tile's own cells only: its apron is its neighbours' to draw.
    let first = vec2<i32>(i32(g.core.z));
    cell = clamp(cell, first, first + vec2<i32>(g.core.xy) - 1);

    var normal = n;
    if (up || n.z > 0.5) {
        normal = smooth_normal(cell);
    }
    let at = eye + dir * (start + t);
    let on_grid = vec2<f32>((at.x - o.x) / o.z + 0.5, (at.y - o.y) / o.z + 0.5);
    var seen = open_between(on_grid, cell);
    if (!up && n.z == 0.0) {
        // A step too small to be a wall is the surface's slope, seen edge on.
        let behind = cell + vec2<i32>(i32(n.x), i32(n.y));
        let foot = height(behind);
        if (height(cell) - foot < g.top_wall.y) {
            normal = smooth_normal(cell);
        } else {
            normal = wall_normal(cell, at.z, n);
            // A wall sees what the floor at its foot sees, at its foot, and what
            // the top sees at its top. Both are read a little way out from the
            // wall along the way it faces, and blended with their neighbours as
            // a top is: a round wall is a staircase of cells, often with
            // leftover material of all heights at its foot, and read cell by
            // cell at the wall itself they would stripe it.
            let out_from = normal.xy * 2.0;
            let foot_at = on_grid + out_from;
            let foot_cell = vec2<i32>(floor(foot_at));
            let low = min(height(foot_cell), foot);
            let up_it = clamp((at.z - low) / max(height(cell) - low, 1e-6), 0.0, 1.0);
            seen = mix(
                open_between(foot_at, foot_cell),
                open_between(on_grid - out_from * 0.5, cell),
                up_it,
            );
        }
    }
    let color = code_color(cell);
    let diffuse = abs(dot(normal, f.light.xyz));
    let shade = mix(1.0, seen, SKY_STRENGTH);
    var out: Hit;
    out.color = vec4<f32>(to_srgb(color * (0.3 + 0.7 * diffuse) * shade), 1.0);
    let clip = f.view_proj * vec4<f32>(at, 1.0);
    out.depth = clip.z / clip.w;
    return out;
}
";

/// How many cells tall a step between neighbouring columns must be to be lit as
/// a wall rather than as the surface's slope: about 72 degrees.
pub(crate) const WALL_CELLS: f32 = 3.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GridUniforms {
    origin_pitch_floor: [f32; 4],
    size: [u32; 4],
    top_wall: [f32; 4],
    core: [u32; 4],
    palette: [[f32; 4]; 6],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniforms {
    view_proj: [[f32; 4]; 4],
    light: [f32; 4],
    viewport: [f32; 4],
    masks: [u32; 4],
    colors: [[f32; 4]; KINDS],
    eye: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ObjectUniforms {
    model: [[f32; 4]; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LineInstance {
    a: [f32; 3],
    b: [f32; 3],
    kind: u32,
    op: u32,
}

fn v3(p: DVec3) -> [f32; 3] {
    [narrow(p.x), narrow(p.y), narrow(p.z)]
}

/// Unshared vertices with a face normal each, so every triangle shades flat, as
/// the web inspector did. Flat shading shows facets, which is honest about what
/// the engine is cutting against.
fn flat_vertices(triangles: impl Iterator<Item = [Vec3; 3]>) -> Vec<Vertex> {
    let mut out = Vec::new();
    for [a, b, c] in triangles {
        let n = (b - a).cross(c - a).normalize_or_zero().to_array();
        for p in [a, b, c] {
            out.push(Vertex {
                position: p.to_array(),
                normal: n,
            });
        }
    }
    out
}

fn mesh_triangles(mesh: &TriMesh) -> impl Iterator<Item = [Vec3; 3]> + '_ {
    (0..mesh.len())
        .filter_map(|i| mesh.triangle(i))
        .map(|t| t.map(|p| Vec3::from(v3(p))))
}

fn line_instances(scene: &Scene) -> Vec<LineInstance> {
    scene
        .segments
        .iter()
        .map(|s| LineInstance {
            a: v3(s.a),
            b: v3(s.b),
            kind: theme::role_index(s.role),
            op: s.op,
        })
        .collect()
}

/// Each height plane's name and height, beside its corner furthest right on the
/// screen, where it is clearest of the part. Labels that would overlap are moved
/// apart, keeping their order down the screen.
fn plane_labels(ui: &egui::Ui, painter: &egui::Painter, projector: &Projector, planes: &[Plane]) {
    let font = theme::caption(ui.style());
    let mut labels: Vec<(egui::Pos2, &Plane)> = planes
        .iter()
        .filter_map(|p| {
            let at = p
                .corners()
                .into_iter()
                .filter_map(|c| projector.screen(c))
                .max_by(|a, b| a.x.total_cmp(&b.x))?;
            Some((at, p))
        })
        .collect();
    labels.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
    let rect = painter.clip_rect();
    let mut below = f32::NEG_INFINITY;
    for (at, plane) in labels {
        let galley = painter.layout_no_wrap(
            plane.label.clone(),
            font.clone(),
            theme::egui_color(plane.kind.color()),
        );
        let size = galley.size() + egui::vec2(12.0, 4.0);
        let x = (at.x + 8.0).min(rect.right() - size.x - 4.0);
        let y = (at.y - size.y / 2.0).max(below + 2.0);
        let bubble = egui::Rect::from_min_size(egui::pos2(x, y), size);
        below = bubble.bottom();
        painter.rect_filled(
            bubble,
            theme::RADIUS_SM,
            theme::egui_color(theme::SURFACE_2),
        );
        painter.galley(
            bubble.min + egui::vec2(6.0, 2.0),
            galley,
            egui::Color32::WHITE,
        );
    }
}

fn outline_instances(outline: &[Outline]) -> Vec<LineInstance> {
    outline
        .iter()
        .map(|o| LineInstance {
            a: v3(o.a),
            b: v3(o.b),
            kind: match o.kind {
                OutlineKind::Stock => STOCK,
                OutlineKind::AxisX => AXIS_X,
                OutlineKind::AxisY => AXIS_Y,
                OutlineKind::AxisZ => AXIS_Z,
                OutlineKind::Selected => SELECTED,
                OutlineKind::Hover => HOVER,
                OutlineKind::Boundary => BOUNDARY,
                OutlineKind::Ghost => GHOST,
                OutlineKind::Plane(k) => k.line_kind(),
            },
            op: 0,
        })
        .collect()
}

/// What an outline line shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineKind {
    /// An edge of the stock.
    Stock,
    /// The work zero's X axis.
    AxisX,
    /// Its Y axis.
    AxisY,
    /// Its Z axis.
    AxisZ,
    /// A picked edge.
    Selected,
    /// The edge under the pointer.
    Hover,
    /// The boundary an operation is kept to.
    Boundary,
    /// A stock that could be picked into the setup.
    Ghost,
    /// The edge of one of an operation's height planes.
    Plane(PlaneKind),
}

/// Which of an operation's heights a plane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaneKind {
    /// Where it moves between cuts.
    Clearance,
    /// Where cutting starts.
    Top,
    /// How deep it cuts.
    Bottom,
}

impl PlaneKind {
    /// All of them, in the order their colours are kept.
    const ALL: [PlaneKind; 3] = [PlaneKind::Clearance, PlaneKind::Top, PlaneKind::Bottom];

    /// Its colour, from the toolpath palette: rapids fly at the clearance, leads
    /// come in at the top, and cuts reach the bottom.
    pub fn color(self) -> [u8; 3] {
        match self {
            PlaneKind::Clearance => theme::PATH_RAPID,
            PlaneKind::Top => theme::PATH_LEAD,
            PlaneKind::Bottom => theme::PATH_CUT,
        }
    }

    fn line_kind(self) -> u32 {
        match self {
            PlaneKind::Clearance => CLEARANCE,
            PlaneKind::Top => TOP,
            PlaneKind::Bottom => BOTTOM,
        }
    }

    fn index(self) -> usize {
        match self {
            PlaneKind::Clearance => 0,
            PlaneKind::Top => 1,
            PlaneKind::Bottom => 2,
        }
    }
}

/// A level plane across the stock, in the setup's frame, showing one of an
/// operation's heights.
#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
    /// Which height.
    pub kind: PlaneKind,
    /// Its height.
    pub z: f64,
    /// Its corners, seen from above.
    pub lo: DVec2,
    pub hi: DVec2,
    /// What it is called in the view, beside its corner.
    pub label: String,
}

impl Plane {
    /// Its corners, round the rectangle.
    fn corners(&self) -> [DVec3; 4] {
        let (lo, hi, z) = (self.lo, self.hi, self.z);
        [
            DVec3::new(lo.x, lo.y, z),
            DVec3::new(hi.x, lo.y, z),
            DVec3::new(hi.x, hi.y, z),
            DVec3::new(lo.x, hi.y, z),
        ]
    }
}

/// A line that is not a move: the stock's edges and the work zero's axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    /// One end.
    pub a: DVec3,
    /// The other.
    pub b: DVec3,
    /// What it shows.
    pub kind: OutlineKind,
}

/// What to show this frame, decided by the controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Draw the part.
    pub part: bool,
    /// Draw the stock's outline.
    pub stock: bool,
    /// Draw the work zero's axes.
    pub origin: bool,
    /// Bit per role, in [`theme::ROLES`] order.
    pub roles: u32,
    /// Bit per operation, from zero. Operations past 32 are always shown.
    pub ops: u32,
    /// Segments shown, in machine order.
    pub shown: usize,
    /// The cutter to draw, as an operation index, and where its tip is.
    pub cutter: Option<(usize, DVec3)>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            part: true,
            stock: false,
            origin: true,
            roles: u32::MAX,
            ops: u32::MAX,
            shown: usize::MAX,
            cutter: None,
        }
    }
}

/// Features to draw over the part, in the setup's frame: what is picked, or what
/// is under the pointer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Highlight {
    /// Faces, as their triangles.
    pub faces: Vec<[Vec3; 3]>,
    /// Edges, and the boundary an operation is kept to.
    pub lines: Vec<Outline>,
    /// Points.
    pub markers: Vec<DVec3>,
}

/// Where the pointer is over the view, as the picker needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerEvent {
    /// From the eye through the pointer: where it starts, and a unit direction.
    pub ray: (DVec3, DVec3),
    /// The pointer, in points.
    pub pos: egui::Pos2,
    /// Whether it clicked this frame: pressed and let go without dragging, so an
    /// orbit never picks.
    pub clicked: bool,
    /// Where the setup's frame is drawn.
    pub projector: Projector,
}

/// What the viewport is asked to draw, with numbers that change whenever the
/// underlying data does, so each piece is uploaded once.
pub struct Content<'a> {
    /// The part mesh.
    pub part: Option<(u64, &'a TriMesh)>,
    /// Changes when a different model is opened, and only then: the view is framed
    /// on it once. Re-running the same model must not snap the camera back.
    pub framing: u64,
    /// The run's toolpaths and cutters.
    pub scene: Option<(u64, &'a Scene)>,
    /// The stock's edges and the work zero's axes.
    pub outline: Option<(u64, &'a [Outline])>,
    /// The box around all the stock: the view frames it with the part, and its
    /// zoom and clipping reach it, however small the part is on it.
    pub stock_box: Option<Aabb>,
    /// The simulated material to draw.
    pub stock: Option<StockShow<'a>>,
    /// What is visible.
    pub view: View,
    /// What is picked.
    pub selection: Option<(u64, &'a Highlight)>,
    /// What is under the pointer.
    pub hover: Option<(u64, &'a Highlight)>,
    /// What could be picked into the setup but is not in it, drawn faintly.
    pub ghost: Option<(u64, &'a Highlight)>,
    /// What to tell the person, over the view: what a click does now.
    pub hint: Option<&'a str>,
    /// An operation's heights, as planes across the stock.
    pub planes: &'a [Plane],
}

/// What changed in the material since it was last uploaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StockChange {
    /// Nothing.
    Nothing,
    /// Any of it.
    All,
    /// These cells.
    Cells(Vec<CellRect>),
}

/// The material's image: one of a stage's, or playback's, shared with its thread
/// and locked only while it is read.
pub enum StockRef<'a> {
    /// Held by the view.
    Plain(&'a StockImage),
    /// Shared with the playback thread.
    Shared(&'a std::sync::Mutex<StockImage>),
}

impl StockRef<'_> {
    /// Look at the image, locked for no longer than `f` takes.
    pub fn with<R>(&self, f: impl FnOnce(&StockImage) -> R) -> R {
        match self {
            StockRef::Plain(image) => f(image),
            StockRef::Shared(m) => f(&m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)),
        }
    }
}

/// Material to draw, and how much of it to upload.
pub struct StockShow<'a> {
    /// Changes whenever the image is a different one.
    pub key: (u64, usize),
    /// The material.
    pub image: StockRef<'a>,
    /// What changed in it since it was last uploaded under the same key.
    pub changed: StockChange,
    /// How to colour it.
    pub mode: ColorMode,
}

/// Where the cells of `rect` start in an image `nx` cells wide with `bytes` per
/// cell, and how many bytes one of its rows spans.
fn rect_layout(rect: CellRect, nx: u32, bytes: u32) -> (u64, u32) {
    let offset = (u64::from(rect.y0) * u64::from(nx) + u64::from(rect.x0)) * u64::from(bytes);
    (offset, nx * bytes)
}

/// Write `rect` of mip `level` of `texture` from `data`, a grid `w` cells wide
/// with `bytes` per cell whose `from` rectangle holds it.
#[allow(clippy::too_many_arguments)]
fn write_texture(
    rs: &RenderState,
    texture: &wgpu::Texture,
    level: u32,
    data: &[u8],
    bytes: u32,
    w: u32,
    from: CellRect,
    rect: CellRect,
) {
    let (offset, bytes_per_row) = rect_layout(from, w, bytes);
    rs.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: level,
            origin: wgpu::Origin3d {
                x: rect.x0,
                y: rect.y0,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(rect.height()),
        },
        wgpu::Extent3d {
            width: rect.width(),
            height: rect.height(),
            depth_or_array_layers: 1,
        },
    );
}

/// What to write of a tile: its cells (its own coordinates) and, for a whole
/// tile, their heights, codes and sky; and the changed cells of each pyramid
/// level.
struct TilePlan {
    cells: CellRect,
    whole: Option<(Vec<f32>, Vec<u8>, Vec<u8>)>,
    levels: Vec<CellRect>,
}

/// Bring a tile's pyramid up to date with `rects` of `image` (every cell, when
/// there are none), and say what to write. `None` when nothing of it changed.
fn plan_tile(
    tile: &mut TileGpu,
    rects: Option<&[CellRect]>,
    image: &StockImage,
    floor: f32,
) -> Option<TilePlan> {
    let t = tile.tile;
    let base = |x: u32, y: u32| {
        t.global(x, y, image.nx, image.ny)
            .and_then(|(gx, gy)| {
                image
                    .heights
                    .get(gy as usize * image.nx as usize + gx as usize)
                    .copied()
            })
            .unwrap_or(floor)
    };
    let Some(rects) = rects else {
        tile.pyramid = Pyramid::new(t.tex_w, t.tex_h, base);
        let cells = CellRect {
            x0: 0,
            y0: 0,
            x1: t.w - 1,
            y1: t.h - 1,
        };
        return Some(TilePlan {
            cells,
            whole: Some((
                rows(t, cells, image, &image.heights, floor),
                rows(t, cells, image, &image.codes, NO_CELL),
                rows(t, cells, image, &image.ao, stock_ao::OPEN),
            )),
            levels: tile
                .pyramid
                .levels
                .iter()
                .map(crate::pyramid::Level::all)
                .collect(),
        });
    };
    let local: Vec<CellRect> = rects.iter().filter_map(|r| t.local(*r)).collect();
    let cells = local.iter().copied().reduce(CellRect::union)?;
    // Each level's changed cells, as one rectangle: one write a level, however
    // many pieces changed, since each write has its own cost.
    let mut union: Vec<Option<CellRect>> = vec![None; tile.pyramid.levels.len()];
    for r in &local {
        for (u, c) in union.iter_mut().zip(tile.pyramid.update(*r, base)) {
            *u = CellRect::join(*u, Some(c));
        }
    }
    Some(TilePlan {
        cells,
        whole: None,
        levels: union.into_iter().flatten().collect(),
    })
}

/// Tile `t`'s cells `r` of `cells` (the image's, `nx` a row), row by row, with
/// `outside` where they are off the stock: whole rows copied at a time.
fn rows<T: Copy>(t: Tile, r: CellRect, image: &StockImage, cells: &[T], outside: T) -> Vec<T> {
    let (w, h) = (r.width() as usize, r.height() as usize);
    let mut out = vec![outside; w * h];
    // The stock columns this rectangle covers, and where they start in it.
    let gx0 = t.x0 + i64::from(r.x0);
    let lo = gx0.max(0);
    let hi = (gx0 + i64::from(r.width())).min(i64::from(image.nx));
    if lo >= hi {
        return out;
    }
    let (Ok(skip), Ok(lo), Ok(hi)) = (
        usize::try_from(lo - gx0),
        usize::try_from(lo),
        usize::try_from(hi),
    ) else {
        return out;
    };
    let nx = image.nx as usize;
    for (y, row) in (r.y0..=r.y1).zip(out.chunks_exact_mut(w)) {
        let Ok(gy) = usize::try_from(t.y0 + i64::from(y)) else {
            continue;
        };
        if gy >= image.ny as usize {
            continue;
        }
        if let (Some(src), Some(dst)) = (
            cells.get(gy * nx + lo..gy * nx + hi),
            row.get_mut(skip..skip + (hi - lo)),
        ) {
            dst.copy_from_slice(src);
        }
    }
    out
}

/// The stock shader's colours, linear: below the part, leftover faint and
/// strong, on the part, off the part, and the material. The same colours the
/// legend shows, from the same functions.
fn palette() -> [[f32; 4]; 6] {
    let faint = cell_color(ColorMode::Compare, Class::Leftover, 0.0);
    let strong = cell_color(ColorMode::Compare, Class::Leftover, f64::INFINITY);
    let [gouge, _, on, off] = CLASSES.map(|c| cell_color(ColorMode::Compare, c, 0.0));
    let material = cell_color(ColorMode::Material, Class::OnPart, 0.0);
    [gouge, faint, strong, on, off, material].map(|c| theme::linear(c).into_rgba())
}

impl Content<'_> {
    /// Nothing to draw.
    pub fn empty() -> Self {
        Content {
            part: None,
            framing: 0,
            scene: None,
            outline: None,
            stock_box: None,
            stock: None,
            view: View::default(),
            selection: None,
            hover: None,
            ghost: None,
            hint: None,
            planes: &[],
        }
    }
}

struct Targets {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolved: wgpu::TextureView,
}

struct Object {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// A highlight on the GPU: its faces and its lines, either of which may be empty.
struct HighlightGpu {
    key: u64,
    faces: Option<(wgpu::Buffer, u32)>,
    lines: Option<(wgpu::Buffer, u32)>,
}

struct Uploaded {
    part: Option<(u64, wgpu::Buffer, u32)>,
    selection: Option<HighlightGpu>,
    hover: Option<HighlightGpu>,
    ghost: Option<HighlightGpu>,
    scene: Option<u64>,
    moves: Option<(wgpu::Buffer, u32)>,
    outline: Option<(u64, wgpu::Buffer, u32)>,
    /// One per operation; `None` where the job named no tool that could be drawn.
    cutters: Vec<Option<(wgpu::Buffer, u32)>>,
    /// The height planes as uploaded: each one's fill, and all their edges.
    planes: Option<PlanesGpu>,
}

/// Height planes on the GPU, with the planes they were made from.
struct PlanesGpu {
    from: Vec<Plane>,
    fills: Vec<(PlaneKind, wgpu::Buffer)>,
    lines: (wgpu::Buffer, u32),
}

/// One tile of the material on the GPU.
struct TileGpu {
    tile: Tile,
    /// Its levels of maxima above level 0, as uploaded.
    pyramid: Pyramid,
    heights: wgpu::Texture,
    codes: wgpu::Texture,
    /// How much sky each cell sees.
    sky: wgpu::Texture,
    grid: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// The material on the GPU, as tiles, sized to one stock.
struct StockGpu {
    key: (u64, usize),
    size: [u32; 2],
    tiles: Vec<TileGpu>,
}

struct Gpu {
    stock_pipeline: wgpu::RenderPipeline,
    stock_layout: wgpu::BindGroupLayout,
    stock: Option<StockGpu>,
    part_pipeline: wgpu::RenderPipeline,
    highlight_pipeline: wgpu::RenderPipeline,
    cutter_pipeline: wgpu::RenderPipeline,
    cutter_hidden_pipeline: wgpu::RenderPipeline,
    plane_pipeline: wgpu::RenderPipeline,
    plane_hidden_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    frame: wgpu::Buffer,
    frame_group: wgpu::BindGroup,
    part_object: Object,
    selection_object: Object,
    hover_object: Object,
    ghost_object: Object,
    ghost_hidden_object: Object,
    cutter_object: Object,
    /// The part's occlusion, worked out on the screen.
    part_ao: PartAo,
    /// A tint per kind of height plane: where it is in sight, and where it is not.
    plane_objects: [(Object, Object); 3],
    targets: Option<Targets>,
    texture: Option<egui::TextureId>,
    up: Uploaded,
}

/// The viewport and everything it keeps between frames.
pub struct Viewport {
    camera: Camera,
    gpu: Option<Gpu>,
    /// The framing key the camera was last framed for.
    framed: Option<u64>,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            camera: Camera::framing(Vec3::ZERO, 50.0),
            gpu: None,
            framed: None,
        }
    }
}

impl Viewport {
    /// The camera as it stands, to keep while something else is shown.
    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// Look through `camera`, kept from before, or frame the next part shown under
    /// `framing` afresh when there is none.
    pub fn use_camera(&mut self, camera: Option<Camera>, framing: u64) {
        match camera {
            Some(c) => {
                self.camera = c;
                self.framed = Some(framing);
            }
            None => self.framed = None,
        }
    }

    /// Move the view by `by`, for when everything it shows has moved that much.
    pub fn shift(&mut self, by: DVec3) {
        self.camera.shift(Vec3::from(v3(by)));
    }

    /// Bring the view in on `point`, keeping the angle it is seen from.
    pub fn focus(&mut self, point: DVec3, radius: f32) {
        self.camera.focus(Vec3::from(v3(point)), radius);
    }

    /// Draw the viewport into the rest of `ui`, and say where the pointer is over
    /// it, if it is.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        render_state: Option<&RenderState>,
        content: &Content<'_>,
    ) -> Option<PointerEvent> {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        self.handle_input(ui, &response, rect);
        let aspect = rect.width() / rect.height().max(1.0);
        let projector = Projector::new(self.camera.view_projection(aspect), rect);
        let event = response
            .hover_pos()
            .filter(|_| !response.dragged())
            .map(|pos| {
                let ndc: Vec2 = projector.ndc(pos);
                PointerEvent {
                    ray: self.camera.ray(aspect, ndc),
                    pos,
                    clicked: response.clicked(),
                    projector,
                }
            });

        let Some(rs) = render_state else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "3D view needs a GPU (wgpu), which is not available",
                theme::font(ui.style(), egui::TextStyle::Body),
                ui.visuals().warn_fg_color,
            );
            return event;
        };
        let gpu = self.gpu.get_or_insert_with(|| Gpu::new(rs));
        gpu.upload(rs, content);

        // Frame each newly opened model once, with its stock; after that the view
        // is the person's, though it can always reach all the stock.
        let scene_box = match (
            content.part.and_then(|(_, m)| m.bounds()),
            content.stock_box,
        ) {
            (Some(a), Some(b)) => Some(a.union(&b)),
            (a, b) => a.or(b),
        };
        if let Some(b) = scene_box {
            let centre = Vec3::from(v3((b.min + b.max) * 0.5));
            let radius = narrow(b.extent().length() * 0.5);
            if self.framed != Some(content.framing) && content.part.is_some() {
                self.camera = Camera::framing(centre, radius);
                self.framed = Some(content.framing);
            }
            self.camera.cover(radius);
        }

        let ppp = ui.ctx().pixels_per_point();
        let px = [
            (rect.width() * ppp).round().max(1.0),
            (rect.height() * ppp).round().max(1.0),
        ];
        // Sizes in pixels are small, positive integers; the float came from egui.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let size = [px[0] as u32, px[1] as u32];
        let reach =
            content
                .part
                .and_then(|(_, m)| m.bounds())
                .map_or(stock_ao::reach_mm(0.0), |b| {
                    let e = b.extent();
                    stock_ao::reach_mm(e.x.max(e.y))
                });
        let frame = FrameInput {
            view_proj: self.camera.view_projection(aspect),
            projection: self.camera.projection(aspect),
            reach: narrow(reach),
            eye: self.camera.eye(),
            pixels: px,
            line_width: LINE_WIDTH * ppp,
            view: content.view,
            stock: content.stock.is_some(),
        };
        let Some(texture) = gpu.render(rs, size, &frame) else {
            return event;
        };
        ui.painter().image(
            texture,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        let painter = ui.painter_at(rect);
        plane_labels(ui, &painter, &projector, content.planes);

        // Picked points, then the one under the pointer, over the picture.
        let edge = egui::Stroke::new(1.5, theme::egui_color(theme::BG));
        for at in content.selection.iter().flat_map(|(_, h)| &h.markers) {
            if let Some(p) = projector.screen(*at) {
                painter.circle(p, 5.0, theme::egui_color(theme::SELECTED), edge);
            }
        }
        for at in content.hover.iter().flat_map(|(_, h)| &h.markers) {
            if let Some(p) = projector.screen(*at) {
                let ring = egui::Stroke::new(2.0, theme::egui_color(theme::HOVER));
                painter.circle_stroke(p, 7.0, ring);
                painter.circle_filled(p, 2.5, theme::egui_color(theme::HOVER));
            }
        }
        if let Some(hint) = content.hint {
            let galley = painter.layout_no_wrap(
                hint.to_string(),
                theme::font(ui.style(), egui::TextStyle::Body),
                theme::egui_color(theme::BG),
            );
            // Low in the view, clear of the card describing what was picked, which
            // sits at the top.
            let at = rect.center_bottom() - egui::vec2(0.0, 44.0);
            let bubble = egui::Rect::from_center_size(
                at - egui::vec2(0.0, galley.size().y / 2.0),
                galley.size() + egui::vec2(20.0, 10.0),
            );
            painter.rect_filled(bubble, 4.0, theme::egui_color(theme::SELECTED));
            painter.galley(
                bubble.center() - galley.size() / 2.0,
                galley,
                egui::Color32::BLACK,
            );
        }

        let faint = ui.visuals().weak_text_color();
        if content.part.is_none() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Add a model to the job to see it here",
                theme::font(ui.style(), egui::TextStyle::Heading),
                faint,
            );
        }
        ui.painter().text(
            rect.left_bottom() + egui::vec2(12.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            "drag to orbit · shift-drag or right-drag to pan · scroll to zoom · R resets",
            theme::caption(ui.style()),
            faint,
        );
        event
    }

    fn handle_input(&mut self, ui: &egui::Ui, response: &egui::Response, rect: egui::Rect) {
        let shift = ui.input(|i| i.modifiers.shift);
        let d = response.drag_delta();
        // Pan with the middle or secondary button, or shift-drag: a trackpad has no
        // middle button. Orbit with a plain primary drag.
        if response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary)
            || (shift && response.dragged_by(egui::PointerButton::Primary))
        {
            self.camera.pan(d.x, d.y, rect.height());
        } else if response.dragged_by(egui::PointerButton::Primary) {
            self.camera.orbit(d.x, d.y);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.camera.zoom(scroll);
            }
            let typing = ui.ctx().memory(|m| m.focused().is_some());
            if !typing && ui.input(|i| i.key_pressed(egui::Key::R)) {
                self.camera.reset();
            }
        }
    }
}

struct FrameInput {
    view_proj: Mat4,
    /// The projection alone, for the part's occlusion.
    projection: Mat4,
    /// How far, in millimetres, the part's occlusion reaches.
    reach: f32,
    eye: Vec3,
    pixels: [f32; 2],
    line_width: f32,
    view: View,
    /// Whether there is material to draw this frame.
    stock: bool,
}

impl Gpu {
    fn new(rs: &RenderState) -> Self {
        let device = &rs.device;
        let uniform_layout = |label| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            })
        };
        let frame_layout = uniform_layout("camcraft frame");
        let object_layout = uniform_layout("camcraft object");
        let buffer = |label, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let group = |layout: &wgpu::BindGroupLayout, buffer: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        let frame = buffer("camcraft frame", std::mem::size_of::<FrameUniforms>());
        let frame_group = group(&frame_layout, &frame);
        let object = |label| {
            let uniforms = buffer(label, std::mem::size_of::<ObjectUniforms>());
            let bind_group = group(&object_layout, &uniforms);
            Object {
                uniforms,
                bind_group,
            }
        };

        let mesh_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("camcraft mesh"),
            source: wgpu::ShaderSource::Wgsl(format!("{COMMON}{MESH_SHADER}").into()),
        });
        let line_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("camcraft lines"),
            source: wgpu::ShaderSource::Wgsl(format!("{COMMON}{LINE_SHADER}").into()),
        });
        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("camcraft mesh"),
            bind_group_layouts: &[Some(&frame_layout), Some(&object_layout)],
            immediate_size: 0,
        });
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("camcraft lines"),
            bind_group_layouts: &[Some(&frame_layout)],
            immediate_size: 0,
        });

        let mesh_vertex = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
        };
        let mesh_pipeline_biased = |label, blend, depth_write, compare, bias| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&mesh_layout),
                vertex: wgpu::VertexState {
                    module: &mesh_module,
                    entry_point: Some("vs"),
                    buffers: &[Some(mesh_vertex.clone())],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(compare),
                    stencil: wgpu::StencilState::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &mesh_module,
                    entry_point: Some("fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let mesh_pipeline = |label, blend, depth_write, compare| {
            mesh_pipeline_biased(
                label,
                blend,
                depth_write,
                compare,
                wgpu::DepthBiasState::default(),
            )
        };
        let part_pipeline = mesh_pipeline("camcraft part", None, true, wgpu::CompareFunction::Less);
        // Translucent, drawn last, and not hiding what is behind it.
        let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
        // Faces drawn again over the part, where it is: the same triangles at the same
        // depth pass `LessEqual`, and nothing behind the part does.
        let highlight_pipeline = mesh_pipeline(
            "camcraft highlight",
            blend,
            false,
            wgpu::CompareFunction::LessEqual,
        );
        let cutter_pipeline =
            mesh_pipeline("camcraft cutter", blend, false, wgpu::CompareFunction::Less);
        // What is drawn again, fainter, where something hides it: a stock or model
        // that could be picked in may lie inside the part, and must still show.
        let cutter_hidden_pipeline = mesh_pipeline(
            "camcraft cutter, hidden",
            blend,
            false,
            wgpu::CompareFunction::Greater,
        );
        // A height plane often lies on the stock's top or a flat of the part: pulled
        // towards the eye, it wins there rather than flickering with the face.
        let pull = wgpu::DepthBiasState {
            constant: -4,
            slope_scale: -2.0,
            clamp: 0.0,
        };
        let plane_pipeline = mesh_pipeline_biased(
            "camcraft plane",
            blend,
            false,
            wgpu::CompareFunction::Less,
            pull,
        );
        let plane_hidden_pipeline = mesh_pipeline_biased(
            "camcraft plane, hidden",
            blend,
            false,
            wgpu::CompareFunction::Greater,
            pull,
        );

        let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("camcraft lines"),
            layout: Some(&line_layout),
            vertex: wgpu::VertexState {
                module: &line_module,
                entry_point: Some("vs"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LineInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3, 1 => Float32x3, 2 => Uint32, 3 => Uint32
                    ],
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &line_module,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        let stock_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camcraft stock"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let stock_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("camcraft stock"),
            source: wgpu::ShaderSource::Wgsl(format!("{COMMON}{STOCK_SHADER}").into()),
        });
        let stock_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("camcraft stock"),
                bind_group_layouts: &[Some(&frame_layout), Some(&stock_layout)],
                immediate_size: 0,
            });
        let stock_pipeline = |label, entry| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&stock_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &stock_module,
                    entry_point: Some(entry),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &stock_module,
                    entry_point: Some("fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let stock_pipeline = stock_pipeline("camcraft stock", "vs");

        Self {
            stock_pipeline,
            stock_layout,
            stock: None,
            part_pipeline,
            highlight_pipeline,
            cutter_pipeline,
            cutter_hidden_pipeline,
            plane_pipeline,
            plane_hidden_pipeline,
            line_pipeline,
            frame,
            frame_group,
            part_object: object("camcraft part"),
            selection_object: object("camcraft selection"),
            hover_object: object("camcraft hover"),
            ghost_object: object("camcraft ghost"),
            ghost_hidden_object: object("camcraft ghost, hidden"),
            cutter_object: object("camcraft cutter"),
            part_ao: PartAo::new(device, COLOR, SAMPLES),
            plane_objects: PlaneKind::ALL
                .map(|_| (object("camcraft plane"), object("camcraft plane, hidden"))),
            targets: None,
            texture: None,
            up: Uploaded {
                part: None,
                selection: None,
                hover: None,
                ghost: None,
                scene: None,
                moves: None,
                outline: None,
                cutters: Vec::new(),
                planes: None,
            },
        }
    }

    /// Upload the material, when it changed. Textures are made once per grid size;
    /// after that, a new stage is a texture write, and a step of playback writes
    /// only the cells it changed, in the tiles that hold them, and the cells of
    /// their pyramids above.
    fn upload_stock(&mut self, rs: &RenderState, stock: Option<&StockShow<'_>>) {
        let Some(show) = stock else {
            return;
        };
        show.image.with(|image| self.upload_image(rs, show, image));
    }

    fn upload_image(&mut self, rs: &RenderState, show: &StockShow<'_>, image: &StockImage) {
        let key = show.key;
        let size = [image.nx, image.ny];
        if size[0] == 0 || size[1] == 0 {
            self.stock = None;
            return;
        }
        let same = self
            .stock
            .as_ref()
            .is_some_and(|s| s.key == key && s.size == size);
        let rects = match &show.changed {
            _ if !same => None,
            StockChange::Nothing => None,
            StockChange::Cells(cells) if cells.iter().all(|r| r.x1 < size[0] && r.y1 < size[1]) => {
                Some(cells.clone())
            }
            StockChange::Cells(_) | StockChange::All => None,
        };
        if same && show.changed == StockChange::Nothing {
            self.write_grids(rs, image, show.mode);
            return;
        }
        if self.stock.as_ref().is_none_or(|s| s.size != size) {
            let tiles = stock_tiles::tiles(size[0], size[1])
                .into_iter()
                .map(|t| self.tile_textures(rs, t, image))
                .collect();
            self.stock = Some(StockGpu { key, size, tiles });
        }
        let Some(gpu) = self.stock.as_mut() else {
            return;
        };
        let floor = narrow(image.floor);
        // First, in parallel, each tile's pyramid brought up to date and what to
        // write of it; then the writes, which the queue takes one at a time.
        let plans: Vec<Option<TilePlan>> = {
            use rayon::prelude::*;
            gpu.tiles
                .par_iter_mut()
                .map(|tile| plan_tile(tile, rects.as_deref(), image, floor))
                .collect()
        };
        for (tile, plan) in gpu.tiles.iter().zip(plans) {
            let Some(plan) = plan else { continue };
            let r = plan.cells;
            match &plan.whole {
                // A step's cells are all on the stock: written straight from the
                // image, which holds them row by row at the stock's width.
                None => {
                    if let Some(from) = tile.tile.stock_rect(r, image.nx, image.ny) {
                        let heights: &[u8] = bytemuck::cast_slice(&image.heights);
                        write_texture(rs, &tile.heights, 0, heights, 4, image.nx, from, r);
                        write_texture(rs, &tile.codes, 0, &image.codes, 1, image.nx, from, r);
                        write_texture(rs, &tile.sky, 0, &image.ao, 1, image.nx, from, r);
                    }
                }
                // The whole tile, apron past the stock's edge and all.
                Some((heights, codes, sky)) => {
                    let whole = CellRect {
                        x0: 0,
                        y0: 0,
                        x1: r.width() - 1,
                        y1: r.height() - 1,
                    };
                    let hs: &[u8] = bytemuck::cast_slice(heights);
                    write_texture(rs, &tile.heights, 0, hs, 4, r.width(), whole, r);
                    write_texture(rs, &tile.codes, 0, codes, 1, r.width(), whole, r);
                    write_texture(rs, &tile.sky, 0, sky, 1, r.width(), whole, r);
                }
            }
            for ((level, r), l) in (1u32..).zip(&plan.levels).zip(&tile.pyramid.levels) {
                write_texture(
                    rs,
                    &tile.heights,
                    level,
                    bytemuck::cast_slice(&l.data),
                    4,
                    l.w,
                    *r,
                    *r,
                );
            }
        }
        gpu.key = key;
        self.write_grids(rs, image, show.mode);
    }

    /// Each tile's grid: where it is, how high its material stands, the colours.
    fn write_grids(&self, rs: &RenderState, image: &StockImage, mode: ColorMode) {
        let Some(gpu) = &self.stock else {
            return;
        };
        let palette = palette();
        let floor = narrow(image.floor);
        for tile in &gpu.tiles {
            let t = tile.tile;
            #[allow(clippy::cast_precision_loss)]
            let (ox, oy) = (t.x0 as f64, t.y0 as f64);
            let grid = GridUniforms {
                origin_pitch_floor: [
                    narrow(image.origin.x + ox * image.pitch),
                    narrow(image.origin.y + oy * image.pitch),
                    narrow(image.pitch),
                    floor,
                ],
                size: [
                    t.w,
                    t.h,
                    u32::try_from(tile.pyramid.levels.len() + 1).unwrap_or(1),
                    0,
                ],
                top_wall: [
                    tile.pyramid.top().max(floor),
                    WALL_CELLS * narrow(image.pitch),
                    0.0,
                    0.0,
                ],
                core: [
                    t.core.width(),
                    t.core.height(),
                    stock_tiles::APRON,
                    u32::from(mode == ColorMode::Compare),
                ],
                palette,
            };
            rs.queue
                .write_buffer(&tile.grid, 0, bytemuck::bytes_of(&grid));
        }
    }

    /// A tile's textures and grid.
    fn tile_textures(&self, rs: &RenderState, tile: Tile, image: &StockImage) -> TileGpu {
        let device = &rs.device;
        let texture = |label, format, [w, h]: [u32; 2], mips: u32| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: mips,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let floor = narrow(image.floor);
        let pyramid = Pyramid::new(tile.tex_w, tile.tex_h, |_, _| floor);
        let mips = u32::try_from(pyramid.levels.len() + 1).unwrap_or(1);
        let heights = texture(
            "camcraft stock heights",
            wgpu::TextureFormat::R32Float,
            [tile.tex_w, tile.tex_h],
            mips,
        );
        let codes = texture(
            "camcraft stock codes",
            wgpu::TextureFormat::R8Uint,
            [tile.w, tile.h],
            1,
        );
        let sky = texture(
            "camcraft stock sky",
            wgpu::TextureFormat::R8Unorm,
            [tile.w, tile.h],
            1,
        );
        let grid = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camcraft stock grid"),
            size: std::mem::size_of::<GridUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camcraft stock"),
            layout: &self.stock_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: grid.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view(&heights)),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view(&codes)),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&view(&sky)),
                },
            ],
        });
        TileGpu {
            tile,
            pyramid,
            heights,
            codes,
            sky,
            grid,
            bind_group,
        }
    }

    /// Upload whatever changed since the last frame.
    fn upload(&mut self, rs: &RenderState, content: &Content<'_>) {
        self.upload_stock(rs, content.stock.as_ref());
        let vertices = |label, data: &[u8], usage| {
            rs.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: data,
                    usage,
                })
        };
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);

        match content.part {
            Some((g, mesh)) if self.up.part.as_ref().map(|p| p.0) != Some(g) => {
                let v = flat_vertices(mesh_triangles(mesh));
                let buf = vertices(
                    "camcraft part",
                    bytemuck::cast_slice(&v),
                    wgpu::BufferUsages::VERTEX,
                );
                self.up.part = Some((g, buf, count(v.len())));
            }
            None => self.up.part = None,
            _ => {}
        }

        for (slot, wanted) in [
            (&mut self.up.selection, content.selection),
            (&mut self.up.hover, content.hover),
            (&mut self.up.ghost, content.ghost),
        ] {
            match wanted {
                Some((key, h)) if slot.as_ref().map(|s| s.key) != Some(key) => {
                    let faces = flat_vertices(h.faces.iter().copied());
                    let lines = outline_instances(&h.lines);
                    let usage = wgpu::BufferUsages::VERTEX;
                    // A buffer may not be empty: an empty half draws nothing instead.
                    *slot = Some(HighlightGpu {
                        key,
                        faces: (!faces.is_empty()).then(|| {
                            let buf = vertices(
                                "camcraft picked faces",
                                bytemuck::cast_slice(&faces),
                                usage,
                            );
                            (buf, count(faces.len()))
                        }),
                        lines: (!lines.is_empty()).then(|| {
                            let buf = vertices(
                                "camcraft picked lines",
                                bytemuck::cast_slice(&lines),
                                usage,
                            );
                            (buf, count(lines.len()))
                        }),
                    });
                }
                None => *slot = None,
                _ => {}
            }
        }

        if content.planes.is_empty() {
            self.up.planes = None;
        } else if self
            .up
            .planes
            .as_ref()
            .is_none_or(|p| p.from != content.planes)
        {
            let usage = wgpu::BufferUsages::VERTEX;
            let fills = content
                .planes
                .iter()
                .map(|p| {
                    let [a, b, c, d] = p.corners().map(|c| Vec3::from(v3(c)));
                    let v = flat_vertices([[a, b, c], [a, c, d]].into_iter());
                    (
                        p.kind,
                        vertices("camcraft plane", bytemuck::cast_slice(&v), usage),
                    )
                })
                .collect();
            let edges: Vec<Outline> = content
                .planes
                .iter()
                .flat_map(|p| {
                    let c = p.corners();
                    let next = c.into_iter().cycle().skip(1);
                    c.into_iter().zip(next).map(move |(a, b)| Outline {
                        a,
                        b,
                        kind: OutlineKind::Plane(p.kind),
                    })
                })
                .collect();
            let lines = outline_instances(&edges);
            self.up.planes = Some(PlanesGpu {
                from: content.planes.to_vec(),
                fills,
                lines: (
                    vertices("camcraft plane edges", bytemuck::cast_slice(&lines), usage),
                    count(lines.len()),
                ),
            });
        }

        match content.outline {
            Some((g, outline)) if self.up.outline.as_ref().map(|o| o.0) != Some(g) => {
                let lines = outline_instances(outline);
                // A buffer may not be empty; an empty outline draws nothing instead.
                self.up.outline = (!lines.is_empty()).then(|| {
                    let buf = vertices(
                        "camcraft outline",
                        bytemuck::cast_slice(&lines),
                        wgpu::BufferUsages::VERTEX,
                    );
                    (g, buf, count(lines.len()))
                });
            }
            None => self.up.outline = None,
            _ => {}
        }

        match content.scene {
            Some((g, scene)) if self.up.scene != Some(g) => {
                let moves = line_instances(scene);
                let usage = wgpu::BufferUsages::VERTEX;
                // A buffer may not be empty; an empty scene draws nothing instead.
                self.up.moves = (!moves.is_empty()).then(|| {
                    (
                        vertices("camcraft moves", bytemuck::cast_slice(&moves), usage),
                        count(moves.len()),
                    )
                });
                self.up.cutters = scene
                    .ops
                    .iter()
                    .map(|op| {
                        let v = flat_vertices(op.cutter.iter().copied());
                        (!v.is_empty()).then(|| {
                            let buf = vertices("camcraft cutter", bytemuck::cast_slice(&v), usage);
                            (buf, count(v.len()))
                        })
                    })
                    .collect();
                self.up.scene = Some(g);
            }
            None => {
                self.up.scene = None;
                self.up.moves = None;
                self.up.cutters.clear();
            }
            _ => {}
        }
    }

    /// Make sure the offscreen targets are `size`, and that egui knows the texture.
    fn targets(&mut self, rs: &RenderState, size: [u32; 2]) -> Option<egui::TextureId> {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return self.texture;
        }
        let device = &rs.device;
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let make = |label, format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let targets = Targets {
            size,
            msaa: make(
                "camcraft viewport msaa",
                COLOR,
                SAMPLES,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
            // Read by the part's occlusion, between drawing the part and the rest.
            depth: make(
                "camcraft viewport depth",
                DEPTH,
                SAMPLES,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            resolved: make(
                "camcraft viewport",
                COLOR,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
        };
        self.part_ao.resize(device, size, &targets.depth);
        let mut renderer = rs.renderer.write();
        let id = match self.texture {
            Some(id) => {
                renderer.update_egui_texture_from_wgpu_texture(
                    device,
                    &targets.resolved,
                    wgpu::FilterMode::Linear,
                    id,
                );
                id
            }
            None => renderer.register_native_texture(
                device,
                &targets.resolved,
                wgpu::FilterMode::Linear,
            ),
        };
        self.targets = Some(targets);
        self.texture = Some(id);
        Some(id)
    }

    fn render(
        &mut self,
        rs: &RenderState,
        size: [u32; 2],
        input: &FrameInput,
    ) -> Option<egui::TextureId> {
        let texture = self.targets(rs, size)?;
        let targets = self.targets.as_ref()?;
        let view = input.view;

        let mut colors = [[0.0; 4]; KINDS];
        for (slot, role) in colors.iter_mut().zip(theme::ROLES) {
            *slot = theme::linear(theme::role_srgb(role)).into_rgba();
        }
        for (kind, color) in [
            (STOCK, theme::GRAY_7),
            (AXIS_X, theme::AXIS_X),
            (AXIS_Y, theme::AXIS_Y),
            (AXIS_Z, theme::AXIS_Z),
            (SELECTED, theme::SELECTED),
            (HOVER, theme::HOVER),
            (BOUNDARY, theme::BOUNDARY),
            (GHOST, theme::GRAY_5),
            (CLEARANCE, PlaneKind::Clearance.color()),
            (TOP, PlaneKind::Top.color()),
            (BOTTOM, PlaneKind::Bottom.color()),
        ] {
            if let Some(slot) = colors.get_mut(kind as usize) {
                *slot = theme::linear(color).into_rgba();
            }
        }
        let axes = (1 << AXIS_X) | (1 << AXIS_Y) | (1 << AXIS_Z);
        let kinds = (view.roles & 0x7f)
            | if view.stock { 1 << STOCK } else { 0 }
            | if view.origin { axes } else { 0 }
            | (1 << SELECTED)
            | (1 << HOVER)
            | (1 << BOUNDARY)
            | (1 << GHOST)
            | (1 << CLEARANCE)
            | (1 << TOP)
            | (1 << BOTTOM);
        let shown = u32::try_from(view.shown).unwrap_or(u32::MAX);
        let frame = FrameUniforms {
            view_proj: input.view_proj.to_cols_array_2d(),
            // From the front right and above, as the inspector lit it.
            light: Vec3::new(0.4, -0.7, 0.9).normalize().extend(0.0).to_array(),
            viewport: [input.pixels[0], input.pixels[1], input.line_width, 2e-4],
            masks: [kinds, view.ops, shown, 0],
            colors,
            eye: input.eye.extend(1.0).to_array(),
        };
        rs.queue
            .write_buffer(&self.frame, 0, bytemuck::bytes_of(&frame));
        let part = ObjectUniforms {
            model: Mat4::IDENTITY.to_cols_array_2d(),
            color: theme::linear(theme::PART).into_rgba(),
        };
        rs.queue
            .write_buffer(&self.part_object.uniforms, 0, bytemuck::bytes_of(&part));
        for (object, color, alpha) in [
            (&self.selection_object, theme::SELECTED, 0.55),
            (&self.hover_object, theme::HOVER, 0.4),
            (&self.ghost_object, theme::GHOST, 0.22),
            (&self.ghost_hidden_object, theme::GHOST, 0.1),
        ] {
            let mut color = theme::linear(color).into_rgba();
            color[3] = alpha;
            let tint = ObjectUniforms {
                model: Mat4::IDENTITY.to_cols_array_2d(),
                color,
            };
            rs.queue
                .write_buffer(&object.uniforms, 0, bytemuck::bytes_of(&tint));
        }
        for (kind, (seen, hidden)) in PlaneKind::ALL.iter().zip(&self.plane_objects) {
            // Faint: the part must still be read through them.
            for (object, alpha) in [(seen, 0.16), (hidden, 0.06)] {
                let mut color = theme::linear(kind.color()).into_rgba();
                color[3] = alpha;
                let tint = ObjectUniforms {
                    model: Mat4::IDENTITY.to_cols_array_2d(),
                    color,
                };
                rs.queue
                    .write_buffer(&object.uniforms, 0, bytemuck::bytes_of(&tint));
            }
        }
        if let Some((_, tip)) = view.cutter {
            let mut color = theme::linear(theme::AMBER_5).into_rgba();
            // Opaque enough to find at a glance on a large part, and still
            // showing what it passes through.
            color[3] = 0.8;
            let model = Mat4::from_translation(Vec3::from(v3(tip))).to_cols_array_2d();
            let cutter = ObjectUniforms { model, color };
            rs.queue
                .write_buffer(&self.cutter_object.uniforms, 0, bytemuck::bytes_of(&cutter));
        }

        let bg = theme::linear(theme::BG);
        let mut encoder = rs
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("camcraft viewport"),
            });
        let part = self.up.part.as_ref().filter(|_| view.part);
        // The part alone first, kept, so its occlusion can be worked out from the
        // depth it leaves and darken it before anything else is drawn over it.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("camcraft viewport part"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.msaa,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(bg[0]),
                            g: f64::from(bg[1]),
                            b: f64::from(bg[2]),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.frame_group, &[]);
            if let Some((_, buffer, n)) = part {
                pass.set_pipeline(&self.part_pipeline);
                pass.set_bind_group(1, &self.part_object.bind_group, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..*n, 0..1);
            }
        }
        if part.is_some() {
            self.part_ao.frame(&rs.queue, input.projection, input.reach);
            self.part_ao.draw(&mut encoder, &targets.msaa);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("camcraft viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.msaa,
                    resolve_target: Some(&targets.resolved),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Discard,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.frame_group, &[]);

            if part.is_some() {
                // What could be picked in, faintly, and fainter still where the
                // part hides it: it may lie inside the part, and must still show.
                if let Some(HighlightGpu {
                    faces: Some((buffer, n)),
                    ..
                }) = &self.up.ghost
                {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.set_pipeline(&self.cutter_hidden_pipeline);
                    pass.set_bind_group(1, &self.ghost_hidden_object.bind_group, &[]);
                    pass.draw(0..*n, 0..1);
                    pass.set_pipeline(&self.cutter_pipeline);
                    pass.set_bind_group(1, &self.ghost_object.bind_group, &[]);
                    pass.draw(0..*n, 0..1);
                }

                // Picked faces over the part, then the one under the pointer.
                pass.set_pipeline(&self.highlight_pipeline);
                for (up, object) in [
                    (&self.up.selection, &self.selection_object),
                    (&self.up.hover, &self.hover_object),
                ] {
                    if let Some(HighlightGpu {
                        faces: Some((buffer, n)),
                        ..
                    }) = up
                    {
                        pass.set_bind_group(1, &object.bind_group, &[]);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.draw(0..*n, 0..1);
                    }
                }
            }

            if let (true, Some(stock)) = (input.stock, &self.stock) {
                pass.set_pipeline(&self.stock_pipeline);
                for tile in &stock.tiles {
                    pass.set_bind_group(1, &tile.bind_group, &[]);
                    pass.draw(0..36, 0..1);
                }
            }

            // Height planes, over everything solid: fainter where the part hides
            // them, as the bottom often lies under it.
            if let Some(planes) = &self.up.planes {
                for (pipeline, hidden) in [
                    (&self.plane_hidden_pipeline, true),
                    (&self.plane_pipeline, false),
                ] {
                    pass.set_pipeline(pipeline);
                    for (kind, buffer) in &planes.fills {
                        let Some((seen, behind)) = self.plane_objects.get(kind.index()) else {
                            continue;
                        };
                        let object = if hidden { behind } else { seen };
                        pass.set_bind_group(1, &object.bind_group, &[]);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.draw(0..6, 0..1);
                    }
                }
            }

            pass.set_pipeline(&self.line_pipeline);
            if let Some((buffer, n)) = &self.up.moves {
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..6, 0..(*n).min(shown));
            }
            if let Some((_, buffer, n)) = &self.up.outline {
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..6, 0..*n);
            }
            if let Some(planes) = &self.up.planes {
                let (buffer, n) = &planes.lines;
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..6, 0..*n);
            }
            for up in [&self.up.ghost, &self.up.selection, &self.up.hover] {
                if let Some(HighlightGpu {
                    lines: Some((buffer, n)),
                    ..
                }) = up
                {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..6, 0..*n);
                }
            }

            // Only where it is in sight: the material and the part hide it as they
            // would the real tool.
            if let Some((op, _)) = view.cutter
                && let Some(Some((buffer, n))) = self.up.cutters.get(op)
            {
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.set_pipeline(&self.cutter_pipeline);
                pass.set_bind_group(1, &self.cutter_object.bind_group, &[]);
                pass.draw(0..*n, 0..1);
            }
        }
        rs.queue.submit([encoder.finish()]);
        Some(texture)
    }
}

/// An RGB colour with full alpha.
trait IntoRgba {
    fn into_rgba(self) -> [f32; 4];
}

impl IntoRgba for [f32; 3] {
    fn into_rgba(self) -> [f32; 4] {
        [self[0], self[1], self[2], 1.0]
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests {
    use super::*;
    use camcraft_geom::synthetic;

    /// Every shader parses and validates as the GPU will be given it, so a slip in
    /// one fails here rather than when the app first draws.
    #[test]
    fn every_shader_is_valid_wgsl() {
        for (name, body) in [
            ("mesh", MESH_SHADER),
            ("line", LINE_SHADER),
            ("stock", STOCK_SHADER),
        ] {
            let source = format!("{COMMON}{body}");
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
    }

    /// A rectangle's copy starts at its first cell and strides whole image rows.
    #[test]
    fn a_rect_is_copied_from_its_place_in_the_whole_image() {
        let r = CellRect {
            x0: 3,
            y0: 2,
            x1: 5,
            y1: 4,
        };
        assert_eq!(rect_layout(r, 10, 4), ((2 * 10 + 3) * 4, 40));
        let whole = CellRect {
            x0: 0,
            y0: 0,
            x1: 9,
            y1: 6,
        };
        assert_eq!(rect_layout(whole, 10, 4), (0, 40));
    }

    #[test]
    fn every_triangle_becomes_three_flat_shaded_vertices() {
        let mesh = synthetic::plane(0.0, 10.0);
        let v = flat_vertices(mesh_triangles(&mesh));
        assert_eq!(v.len(), mesh.len() * 3);
        for tri in v.chunks(3) {
            // Flat: the three corners share one normal, of unit length.
            assert!(tri.iter().all(|x| x.normal == tri[0].normal));
            let n = Vec3::from(tri[0].normal);
            assert!((n.length() - 1.0).abs() < 1e-5);
            // A plane at Z=0 faces straight up or straight down.
            assert!(n.z.abs() > 0.999);
        }
    }

    /// The instance buffer is the scene in order, so the GPU's instance index is
    /// the move's place in the program, which is what the scrub compares against.
    #[test]
    fn line_instances_keep_machine_order_and_roles() {
        use crate::scene::{Scene, Segment};
        use camcraft_core::toolpath::MoveRole;
        let seg = |x: f64, role, op| Segment {
            a: DVec3::new(x, 0.0, 0.0),
            b: DVec3::new(x + 1.0, 0.0, 0.0),
            role,
            op,
            feed: None,
            ends_at: 0.0,
        };
        let scene = Scene {
            segments: vec![
                seg(0.0, MoveRole::Rapid, 0),
                seg(1.0, MoveRole::Cut, 0),
                seg(2.0, MoveRole::Plunge, 1),
            ],
            ..Scene::default()
        };
        let moves = line_instances(&scene);
        let got: Vec<(f32, u32, u32)> = moves.iter().map(|m| (m.a[0], m.kind, m.op)).collect();
        assert_eq!(got, vec![(0.0, 1, 0), (1.0, 0, 0), (2.0, 2, 1)]);
        let outline = outline_instances(&[
            Outline {
                a: DVec3::ZERO,
                b: DVec3::X,
                kind: OutlineKind::Stock,
            },
            Outline {
                a: DVec3::ZERO,
                b: DVec3::Z,
                kind: OutlineKind::AxisZ,
            },
        ]);
        let kinds: Vec<u32> = outline.iter().map(|o| o.kind).collect();
        assert_eq!(kinds, [STOCK, AXIS_Z]);
    }
}
