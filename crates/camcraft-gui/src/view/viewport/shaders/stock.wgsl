// One tile of the material: see `stock/gpu_tiles.rs`.
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
    // The material's roughness, metalness.
    finish: vec4<f32>,
};
@group(1) @binding(0) var<uniform> g: Grid;
@group(1) @binding(1) var heights: texture_2d<f32>;
@group(1) @binding(2) var codes: texture_2d<u32>;
// How much sky each cell sees: see `stock/stock_ao.rs`.
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
    // Every line of sight runs the same way when the view is orthographic, from
    // in front of all the stock.
    let eye = select(f.eye.xyz, i.world - f.sight.xyz * f.sight.w, f.sight.w > 0.0);
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
    let shade = mix(1.0, seen, SKY_STRENGTH);
    var out: Hit;
    out.color = vec4<f32>(to_srgb(lit(color, g.finish, normal, -dir) * shade), 1.0);
    let clip = f.view_proj * vec4<f32>(at, 1.0);
    out.depth = clip.z / clip.w;
    return out;
}
