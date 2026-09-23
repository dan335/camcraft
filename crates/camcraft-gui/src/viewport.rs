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

use camcraft_geom::heightmap::narrow;
use camcraft_geom::mesh::TriMesh;
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use eframe::wgpu;
use glam::{DVec3, Mat4, Vec3};
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::scene::Scene;
use crate::stock_view::StockImage;
use crate::theme;

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
/// Line kinds in all, and so colours.
const KINDS: usize = 12;
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
    colors: array<vec4<f32>, 12>,
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
    @builtin(position) clip: vec4<f32>,
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
    let offset = normal * sides[vi] * f.viewport.z * 0.5 + dir * (t * 2.0 - 1.0) * f.viewport.z * 0.5;
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

/// The material: a grid displaced by a height texture and coloured by a colour
/// texture, one texel per stock cell. No vertex buffer: each vertex finds its cell
/// from its index, so changing the stock is only ever a texture write, whole now and
/// the tool's footprint during playback.
const STOCK_SHADER: &str = r"
struct Grid {
    // Cell (0, 0) in X and Y, cell size, floor.
    origin_pitch_floor: vec4<f32>,
    // Columns, rows.
    size: vec4<u32>,
};
@group(1) @binding(0) var<uniform> g: Grid;
@group(1) @binding(1) var heights: texture_2d<f32>;
@group(1) @binding(2) var colors: texture_2d<f32>;

struct StockOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
};

fn height(ix: i32, iy: i32) -> f32 {
    let c = vec2<i32>(clamp(ix, 0, i32(g.size.x) - 1), clamp(iy, 0, i32(g.size.y) - 1));
    return textureLoad(heights, c, 0).r;
}

fn world(ix: i32, iy: i32, z: f32) -> vec3<f32> {
    let o = g.origin_pitch_floor;
    return vec3<f32>(o.x + f32(ix) * o.z, o.y + f32(iy) * o.z, z);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn cell_color(ix: i32, iy: i32) -> vec3<f32> {
    let c = vec2<i32>(clamp(ix, 0, i32(g.size.x) - 1), clamp(iy, 0, i32(g.size.y) - 1));
    return to_linear(textureLoad(colors, c, 0).rgb);
}

// Two triangles per quad, as (dx, dy) corners.
var<private> CORNERS: array<vec2<i32>, 6> = array<vec2<i32>, 6>(
    vec2<i32>(0, 0), vec2<i32>(1, 0), vec2<i32>(1, 1),
    vec2<i32>(0, 0), vec2<i32>(1, 1), vec2<i32>(0, 1),
);

@vertex
fn vs_top(@builtin(vertex_index) vi: u32) -> StockOut {
    let quads_x = g.size.x - 1u;
    let quad = vi / 6u;
    let corner = CORNERS[vi % 6u];
    let ix = i32(quad % quads_x) + corner.x;
    let iy = i32(quad / quads_x) + corner.y;
    let z = height(ix, iy);
    // Normal from the neighbouring cells: the surface's own slope.
    let p = g.origin_pitch_floor.z;
    let dx = (height(ix + 1, iy) - height(ix - 1, iy)) / (2.0 * p);
    let dy = (height(ix, iy + 1) - height(ix, iy - 1)) / (2.0 * p);
    var out: StockOut;
    out.clip = f.view_proj * vec4<f32>(world(ix, iy, z), 1.0);
    out.normal = normalize(vec3<f32>(-dx, -dy, 1.0));
    out.color = cell_color(ix, iy);
    return out;
}

// The four sides, down to the floor, so the material reads as a block.
@vertex
fn vs_skirt(@builtin(vertex_index) vi: u32) -> StockOut {
    let nx = i32(g.size.x);
    let ny = i32(g.size.y);
    let quad = i32(vi / 6u);
    let corner = CORNERS[vi % 6u];
    // Sides in order: y = 0, x = last, y = last, x = 0.
    let along_x = nx - 1;
    let along_y = ny - 1;
    var ix: i32;
    var iy: i32;
    var n: vec3<f32>;
    if (quad < along_x) {
        ix = quad + corner.x; iy = 0; n = vec3<f32>(0.0, -1.0, 0.0);
    } else if (quad < along_x + along_y) {
        ix = nx - 1; iy = quad - along_x + corner.x; n = vec3<f32>(1.0, 0.0, 0.0);
    } else if (quad < 2 * along_x + along_y) {
        ix = quad - along_x - along_y + corner.x; iy = ny - 1; n = vec3<f32>(0.0, 1.0, 0.0);
    } else {
        ix = 0; iy = quad - 2 * along_x - along_y + corner.x; n = vec3<f32>(-1.0, 0.0, 0.0);
    }
    let top = height(ix, iy);
    let z = select(g.origin_pitch_floor.w, top, corner.y == 1);
    var out: StockOut;
    out.clip = f.view_proj * vec4<f32>(world(ix, iy, z), 1.0);
    out.normal = n;
    out.color = cell_color(ix, iy);
    return out;
}

@fragment
fn fs(i: StockOut) -> @location(0) vec4<f32> {
    let n = normalize(i.normal);
    let diffuse = abs(dot(n, f.light.xyz));
    return vec4<f32>(to_srgb(i.color * (0.3 + 0.7 * diffuse)), 1.0);
}
";

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GridUniforms {
    origin_pitch_floor: [f32; 4],
    size: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniforms {
    view_proj: [[f32; 4]; 4],
    light: [f32; 4],
    viewport: [f32; 4],
    masks: [u32; 4],
    colors: [[f32; 4]; KINDS],
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
    /// The simulated material to draw, with a key that changes whenever it does.
    pub stock: Option<((u64, usize), &'a StockImage)>,
    /// What is visible.
    pub view: View,
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

struct Uploaded {
    part: Option<(u64, wgpu::Buffer, u32)>,
    scene: Option<u64>,
    moves: Option<(wgpu::Buffer, u32)>,
    outline: Option<(u64, wgpu::Buffer, u32)>,
    /// One per operation; `None` where the job named no tool that could be drawn.
    cutters: Vec<Option<(wgpu::Buffer, u32)>>,
}

/// The material's textures and grid, sized to one stock.
struct StockGpu {
    key: (u64, usize),
    size: [u32; 2],
    heights: wgpu::Texture,
    colors: wgpu::Texture,
    grid: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct Gpu {
    stock_top_pipeline: wgpu::RenderPipeline,
    stock_skirt_pipeline: wgpu::RenderPipeline,
    stock_layout: wgpu::BindGroupLayout,
    stock: Option<StockGpu>,
    part_pipeline: wgpu::RenderPipeline,
    cutter_pipeline: wgpu::RenderPipeline,
    cutter_hidden_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    frame: wgpu::Buffer,
    frame_group: wgpu::BindGroup,
    part_object: Object,
    cutter_object: Object,
    cutter_hidden_object: Object,
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

    /// Bring the view in on `point`, keeping the angle it is seen from.
    pub fn focus(&mut self, point: DVec3, radius: f32) {
        self.camera.focus(Vec3::from(v3(point)), radius);
    }

    /// Draw the viewport into the rest of `ui`.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        render_state: Option<&RenderState>,
        content: &Content<'_>,
    ) {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        self.handle_input(ui, &response, rect);

        let Some(rs) = render_state else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "3D view needs a GPU (wgpu), which is not available",
                theme::font(ui.style(), egui::TextStyle::Body),
                ui.visuals().warn_fg_color,
            );
            return;
        };
        let gpu = self.gpu.get_or_insert_with(|| Gpu::new(rs));
        gpu.upload(rs, content);

        // Frame each newly opened model once; after that the view is the person's.
        if self.framed != Some(content.framing)
            && let Some(b) = content.part.and_then(|(_, m)| m.bounds())
        {
            let centre = Vec3::from(v3((b.min + b.max) * 0.5));
            self.camera = Camera::framing(centre, narrow(b.extent().length() * 0.5));
            self.framed = Some(content.framing);
        }

        let ppp = ui.ctx().pixels_per_point();
        let px = [
            (rect.width() * ppp).round().max(1.0),
            (rect.height() * ppp).round().max(1.0),
        ];
        // Sizes in pixels are small, positive integers; the float came from egui.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let size = [px[0] as u32, px[1] as u32];
        let aspect = rect.width() / rect.height().max(1.0);
        let frame = FrameInput {
            view_proj: self.camera.view_projection(aspect),
            pixels: px,
            line_width: LINE_WIDTH * ppp,
            view: content.view,
            stock: content.stock.is_some(),
        };
        let Some(texture) = gpu.render(rs, size, &frame) else {
            return;
        };
        ui.painter().image(
            texture,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

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
        let mesh_pipeline = |label, blend, depth_write, compare| {
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
                    bias: wgpu::DepthBiasState::default(),
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
        let part_pipeline = mesh_pipeline("camcraft part", None, true, wgpu::CompareFunction::Less);
        // Translucent, drawn last, and not hiding what is behind it.
        let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
        let cutter_pipeline =
            mesh_pipeline("camcraft cutter", blend, false, wgpu::CompareFunction::Less);
        // The same cutter where something hides it, fainter. The marker's job is to
        // show where the tool is, so it must never simply vanish into a pocket.
        let cutter_hidden_pipeline = mesh_pipeline(
            "camcraft cutter, hidden",
            blend,
            false,
            wgpu::CompareFunction::Greater,
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
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
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
        let stock_top_pipeline = stock_pipeline("camcraft stock top", "vs_top");
        let stock_skirt_pipeline = stock_pipeline("camcraft stock sides", "vs_skirt");

        Self {
            stock_top_pipeline,
            stock_skirt_pipeline,
            stock_layout,
            stock: None,
            part_pipeline,
            cutter_pipeline,
            cutter_hidden_pipeline,
            line_pipeline,
            frame,
            frame_group,
            part_object: object("camcraft part"),
            cutter_object: object("camcraft cutter"),
            cutter_hidden_object: object("camcraft cutter, hidden"),
            targets: None,
            texture: None,
            up: Uploaded {
                part: None,
                scene: None,
                moves: None,
                outline: None,
                cutters: Vec::new(),
            },
        }
    }

    /// Upload the material, when it changed. Textures are made once per grid size;
    /// after that, a new stage is a texture write.
    fn upload_stock(&mut self, rs: &RenderState, stock: Option<((u64, usize), &StockImage)>) {
        let Some((key, image)) = stock else {
            return;
        };
        if self.stock.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        let size = [image.nx, image.ny];
        let limit = rs.device.limits().max_texture_dimension_2d;
        if size[0] < 2 || size[1] < 2 || size[0] > limit || size[1] > limit {
            self.stock = None;
            return;
        }
        if self.stock.as_ref().is_none_or(|s| s.size != size) {
            self.stock = Some(self.stock_textures(rs, key, size));
        }
        let Some(gpu) = self.stock.as_mut() else {
            return;
        };
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let write = |texture: &wgpu::Texture, data: &[u8], bytes: u32| {
            rs.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size[0] * bytes),
                    rows_per_image: Some(size[1]),
                },
                extent,
            );
        };
        write(&gpu.heights, bytemuck::cast_slice(&image.heights), 4);
        write(&gpu.colors, bytemuck::cast_slice(&image.colors), 4);
        let grid = GridUniforms {
            origin_pitch_floor: [
                narrow(image.origin.x),
                narrow(image.origin.y),
                narrow(image.pitch),
                narrow(image.floor),
            ],
            size: [size[0], size[1], 0, 0],
        };
        rs.queue
            .write_buffer(&gpu.grid, 0, bytemuck::bytes_of(&grid));
        gpu.key = key;
    }

    fn stock_textures(&self, rs: &RenderState, key: (u64, usize), size: [u32; 2]) -> StockGpu {
        let device = &rs.device;
        let texture = |label, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let heights = texture("camcraft stock heights", wgpu::TextureFormat::R32Float);
        let colors = texture("camcraft stock colours", wgpu::TextureFormat::Rgba8Unorm);
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
                    resource: wgpu::BindingResource::TextureView(&view(&colors)),
                },
            ],
        });
        StockGpu {
            key,
            size,
            heights,
            colors,
            grid,
            bind_group,
        }
    }

    /// Upload whatever changed since the last frame.
    fn upload(&mut self, rs: &RenderState, content: &Content<'_>) {
        self.upload_stock(rs, content.stock);
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
            depth: make(
                "camcraft viewport depth",
                DEPTH,
                SAMPLES,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
            resolved: make(
                "camcraft viewport",
                COLOR,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
        };
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
        ] {
            if let Some(slot) = colors.get_mut(kind as usize) {
                *slot = theme::linear(color).into_rgba();
            }
        }
        let axes = (1 << AXIS_X) | (1 << AXIS_Y) | (1 << AXIS_Z);
        let kinds = (view.roles & 0x7f)
            | if view.stock { 1 << STOCK } else { 0 }
            | if view.origin { axes } else { 0 };
        let shown = u32::try_from(view.shown).unwrap_or(u32::MAX);
        let frame = FrameUniforms {
            view_proj: input.view_proj.to_cols_array_2d(),
            // From the front right and above, as the inspector lit it.
            light: Vec3::new(0.4, -0.7, 0.9).normalize().extend(0.0).to_array(),
            viewport: [input.pixels[0], input.pixels[1], input.line_width, 2e-4],
            masks: [kinds, view.ops, shown, 0],
            colors,
        };
        rs.queue
            .write_buffer(&self.frame, 0, bytemuck::bytes_of(&frame));
        let part = ObjectUniforms {
            model: Mat4::IDENTITY.to_cols_array_2d(),
            color: theme::linear(theme::PART).into_rgba(),
        };
        rs.queue
            .write_buffer(&self.part_object.uniforms, 0, bytemuck::bytes_of(&part));
        if let Some((_, tip)) = view.cutter {
            let mut color = theme::linear(theme::AMBER_5).into_rgba();
            // Opaque enough to find at a glance on a large part, and still
            // showing what it passes through.
            color[3] = 0.8;
            let model = Mat4::from_translation(Vec3::from(v3(tip))).to_cols_array_2d();
            let cutter = ObjectUniforms { model, color };
            rs.queue
                .write_buffer(&self.cutter_object.uniforms, 0, bytemuck::bytes_of(&cutter));
            color[3] = 0.35;
            let hidden = ObjectUniforms { model, color };
            rs.queue.write_buffer(
                &self.cutter_hidden_object.uniforms,
                0,
                bytemuck::bytes_of(&hidden),
            );
        }

        let bg = theme::linear(theme::BG);
        let mut encoder = rs
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("camcraft viewport"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("camcraft viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.msaa,
                    resolve_target: Some(&targets.resolved),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(bg[0]),
                            g: f64::from(bg[1]),
                            b: f64::from(bg[2]),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Discard,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.frame_group, &[]);

            if let (true, Some((_, buffer, n))) = (view.part, &self.up.part) {
                pass.set_pipeline(&self.part_pipeline);
                pass.set_bind_group(1, &self.part_object.bind_group, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..*n, 0..1);
            }

            if let (true, Some(stock)) = (input.stock, &self.stock) {
                let quads = |a: u32, b: u32| a.saturating_sub(1) * b.saturating_sub(1);
                let [nx, ny] = stock.size;
                pass.set_bind_group(1, &stock.bind_group, &[]);
                pass.set_pipeline(&self.stock_top_pipeline);
                pass.draw(0..quads(nx, ny) * 6, 0..1);
                pass.set_pipeline(&self.stock_skirt_pipeline);
                pass.draw(
                    0..2 * (nx.saturating_sub(1) + ny.saturating_sub(1)) * 6,
                    0..1,
                );
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

            if let Some((op, _)) = view.cutter
                && let Some(Some((buffer, n))) = self.up.cutters.get(op)
            {
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.set_pipeline(&self.cutter_hidden_pipeline);
                pass.set_bind_group(1, &self.cutter_hidden_object.bind_group, &[]);
                pass.draw(0..*n, 0..1);
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
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use camcraft_geom::synthetic;

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
