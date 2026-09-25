//! The viewport on the GPU: its shaders and pipelines, the offscreen targets it
//! draws into, what is uploaded of the content it is given, and the passes that
//! draw a frame.

use camcraft_geom::mesh::TriMesh;
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use eframe::wgpu;
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::content::{Content, Outline, OutlineKind, Plane, PlaneKind};
use super::stock_gpu::{self, StockGpu};
use super::{FrameInput, v3};
use crate::ui::theme;
use crate::view::part_ao::PartAo;
use crate::view::scene::Scene;

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
pub(super) const CLEARANCE: u32 = 15;
pub(super) const TOP: u32 = 16;
pub(super) const BOTTOM: u32 = 17;
/// Line kinds in all, and so colours.
const KINDS: usize = 18;

pub(super) const COMMON: &str = include_str!("shaders/common.wgsl");

const MESH_SHADER: &str = include_str!("shaders/mesh.wgsl");

const LINE_SHADER: &str = include_str!("shaders/line.wgsl");

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

/// Unshared vertices with a face normal each, so every triangle shades flat. Flat
/// shading shows facets, which is honest about what the engine is cutting against.
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
    scene: Option<SceneGpu>,
    outline: Option<(u64, wgpu::Buffer, u32)>,
    /// The height planes as uploaded: each one's fill, and all their edges.
    planes: Option<PlanesGpu>,
}

/// A run's toolpaths and cutters on the GPU, with the key they were made under.
struct SceneGpu {
    key: u64,
    moves: Option<(wgpu::Buffer, u32)>,
    /// One per operation; `None` where the job named no tool that could be drawn.
    cutters: Vec<Option<(wgpu::Buffer, u32)>>,
}

/// Height planes on the GPU, with the planes they were made from.
struct PlanesGpu {
    from: Vec<Plane>,
    fills: Vec<(PlaneKind, wgpu::Buffer)>,
    lines: (wgpu::Buffer, u32),
}

pub(super) struct Gpu {
    stock_pipeline: wgpu::RenderPipeline,
    pub(super) stock_layout: wgpu::BindGroupLayout,
    pub(super) stock: Option<StockGpu>,
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

/// What sets one of the viewport's pipelines apart: they share the targets, the
/// sample count and everything else.
pub(super) struct PipelineSpec<'a> {
    pub(super) label: &'a str,
    pub(super) layout: &'a wgpu::PipelineLayout,
    pub(super) module: &'a wgpu::ShaderModule,
    /// The vertex shader's entry point; the fragment shader's is `fs`.
    pub(super) vertex: &'a str,
    pub(super) buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    pub(super) blend: Option<wgpu::BlendState>,
    pub(super) depth_write: bool,
    pub(super) compare: wgpu::CompareFunction,
    pub(super) bias: wgpu::DepthBiasState,
}

/// A pipeline drawing triangles, unculled, into the offscreen targets.
pub(super) fn pipeline(device: &wgpu::Device, spec: &PipelineSpec<'_>) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(spec.label),
        layout: Some(spec.layout),
        vertex: wgpu::VertexState {
            module: spec.module,
            entry_point: Some(spec.vertex),
            buffers: spec.buffers,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(spec.depth_write),
            depth_compare: Some(spec.compare),
            stencil: wgpu::StencilState::default(),
            bias: spec.bias,
        }),
        multisample: wgpu::MultisampleState {
            count: SAMPLES,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        fragment: Some(wgpu::FragmentState {
            module: spec.module,
            entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR,
                blend: spec.blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The pipeline that draws every line, toolpaths and outlines alike, as
/// instanced quads.
fn line_pipeline(
    device: &wgpu::Device,
    frame_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let line_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("camcraft lines"),
        source: wgpu::ShaderSource::Wgsl(format!("{COMMON}{LINE_SHADER}").into()),
    });
    let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("camcraft lines"),
        bind_group_layouts: &[Some(frame_layout)],
        immediate_size: 0,
    });
    pipeline(
        device,
        &PipelineSpec {
            label: "camcraft lines",
            layout: &line_layout,
            module: &line_module,
            vertex: "vs",
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<LineInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x3, 1 => Float32x3, 2 => Uint32, 3 => Uint32
                ],
            })],
            blend: None,
            depth_write: true,
            compare: wgpu::CompareFunction::LessEqual,
            bias: wgpu::DepthBiasState::default(),
        },
    )
}

/// `data` in a new vertex buffer, with how many items it holds.
fn vertex_buffer<T: bytemuck::Pod>(
    rs: &RenderState,
    label: &str,
    data: &[T],
) -> (wgpu::Buffer, u32) {
    let buffer = rs
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(data),
            usage: wgpu::BufferUsages::VERTEX,
        });
    (buffer, u32::try_from(data.len()).unwrap_or(u32::MAX))
}

/// As [`vertex_buffer`], or nothing for no `data`: a buffer may not be empty, and
/// an empty one would draw nothing anyway.
fn vertex_buffer_of<T: bytemuck::Pod>(
    rs: &RenderState,
    label: &str,
    data: &[T],
) -> Option<(wgpu::Buffer, u32)> {
    (!data.is_empty()).then(|| vertex_buffer(rs, label, data))
}

/// Keep `slot`, an upload made under a key, in step with `wanted`, the key it
/// should be made under now and what from: made again by `make` when the key has
/// changed, dropped when nothing is wanted, and otherwise left as it is.
fn keep_up<K: PartialEq, W, U>(
    slot: &mut Option<U>,
    key: impl Fn(&U) -> K,
    wanted: Option<(K, W)>,
    make: impl FnOnce(K, W) -> Option<U>,
) {
    match wanted {
        Some((k, w)) if slot.as_ref().map(key).as_ref() != Some(&k) => *slot = make(k, w),
        Some(_) => {}
        None => *slot = None,
    }
}

impl Gpu {
    pub(super) fn new(rs: &RenderState) -> Self {
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
        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("camcraft mesh"),
            bind_group_layouts: &[Some(&frame_layout), Some(&object_layout)],
            immediate_size: 0,
        });

        let mesh_vertex = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
        };
        let mesh_buffers = [Some(mesh_vertex)];
        let mesh_pipeline_biased = |label, blend, depth_write, compare, bias| {
            pipeline(
                device,
                &PipelineSpec {
                    label,
                    layout: &mesh_layout,
                    module: &mesh_module,
                    vertex: "vs",
                    buffers: &mesh_buffers,
                    blend,
                    depth_write,
                    compare,
                    bias,
                },
            )
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

        let line_pipeline = line_pipeline(device, &frame_layout);

        let (stock_pipeline, stock_layout) = stock_gpu::stock_pipeline(device, &frame_layout);

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
                outline: None,
                planes: None,
            },
        }
    }

    /// Upload whatever changed since the last frame.
    pub(super) fn upload(&mut self, rs: &RenderState, content: &Content<'_>) {
        self.upload_stock(rs, content.stock.as_ref());

        keep_up(
            &mut self.up.part,
            |p| p.0,
            content.part,
            |key, mesh| {
                let v = flat_vertices(mesh_triangles(mesh));
                let (buffer, n) = vertex_buffer(rs, "camcraft part", &v);
                Some((key, buffer, n))
            },
        );

        for (slot, wanted) in [
            (&mut self.up.selection, content.selection),
            (&mut self.up.hover, content.hover),
            (&mut self.up.ghost, content.ghost),
        ] {
            keep_up(
                slot,
                |h| h.key,
                wanted,
                |key, h| {
                    let faces = flat_vertices(h.faces.iter().copied());
                    Some(HighlightGpu {
                        key,
                        faces: vertex_buffer_of(rs, "camcraft picked faces", &faces),
                        lines: vertex_buffer_of(
                            rs,
                            "camcraft picked lines",
                            &outline_instances(&h.lines),
                        ),
                    })
                },
            );
        }

        if content.planes.is_empty() {
            self.up.planes = None;
        } else if self
            .up
            .planes
            .as_ref()
            .is_none_or(|p| p.from != content.planes)
        {
            let fills = content
                .planes
                .iter()
                .map(|p| {
                    let [a, b, c, d] = p.corners().map(|c| Vec3::from(v3(c)));
                    let v = flat_vertices([[a, b, c], [a, c, d]].into_iter());
                    (p.kind, vertex_buffer(rs, "camcraft plane", &v).0)
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
            self.up.planes = Some(PlanesGpu {
                from: content.planes.to_vec(),
                fills,
                lines: vertex_buffer(rs, "camcraft plane edges", &outline_instances(&edges)),
            });
        }

        keep_up(
            &mut self.up.outline,
            |o| o.0,
            content.outline,
            |key, outline| {
                let lines = outline_instances(outline);
                vertex_buffer_of(rs, "camcraft outline", &lines).map(|(buffer, n)| (key, buffer, n))
            },
        );

        keep_up(
            &mut self.up.scene,
            |s| s.key,
            content.scene,
            |key, scene| {
                Some(SceneGpu {
                    key,
                    moves: vertex_buffer_of(rs, "camcraft moves", &line_instances(scene)),
                    cutters: scene
                        .ops
                        .iter()
                        .map(|op| {
                            let v = flat_vertices(op.cutter.iter().copied());
                            vertex_buffer_of(rs, "camcraft cutter", &v)
                        })
                        .collect(),
                })
            },
        );
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
    pub(super) fn render(
        &mut self,
        rs: &RenderState,
        size: [u32; 2],
        input: &FrameInput,
    ) -> Option<egui::TextureId> {
        let texture = self.targets(rs, size)?;
        let targets = self.targets.as_ref()?;
        let view = input.view;

        let shown = u32::try_from(view.shown).unwrap_or(u32::MAX);
        self.write_uniforms(rs, input, shown);

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
            self.draw_rest(&mut pass, input, part.is_some(), shown);
        }
        rs.queue.submit([encoder.finish()]);
        Some(texture)
    }

    /// Write the frame's uniforms, and each object's: colours, masks, the part,
    /// the tints of picks and planes, and where the cutter is.
    fn write_uniforms(&self, rs: &RenderState, input: &FrameInput, shown: u32) {
        let view = input.view;
        let mut colors = [[0.0; 4]; KINDS];
        for (slot, role) in colors.iter_mut().zip(theme::ROLES) {
            *slot = theme::linear(theme::role_srgb(role)).into_rgba();
        }
        for (kind, color) in [
            (STOCK, theme::MATERIAL),
            (AXIS_X, theme::AXIS_X),
            (AXIS_Y, theme::AXIS_Y),
            (AXIS_Z, theme::AXIS_Z),
            (SELECTED, theme::SELECTED),
            (HOVER, theme::HOVER),
            (BOUNDARY, theme::BOUNDARY),
            (GHOST, theme::GHOST),
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
        let frame = FrameUniforms {
            view_proj: input.view_proj.to_cols_array_2d(),
            // From the front right and above, where the camera starts.
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
    }

    /// Everything over the part, into `pass`: what could be picked in, picked
    /// faces, the material, height planes, lines, and the cutter.
    fn draw_rest(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        input: &FrameInput,
        part: bool,
        shown: u32,
    ) {
        let view = input.view;
        if part {
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
        if let Some((buffer, n)) = self.up.scene.as_ref().and_then(|s| s.moves.as_ref()) {
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
            && let Some(Some((buffer, n))) = self.up.scene.as_ref().and_then(|s| s.cutters.get(op))
        {
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.set_pipeline(&self.cutter_pipeline);
            pass.set_bind_group(1, &self.cutter_object.bind_group, &[]);
            pass.draw(0..*n, 0..1);
        }
    }
}

/// An RGB colour with full alpha.
pub(super) trait IntoRgba {
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
    use glam::DVec3;

    /// Every shader parses and validates as the GPU will be given it, so a slip in
    /// one fails here rather than when the app first draws.
    #[test]
    fn every_shader_is_valid_wgsl() {
        for (name, body) in [
            ("mesh", MESH_SHADER),
            ("line", LINE_SHADER),
            ("stock", stock_gpu::STOCK_SHADER),
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
    /// the move's place in the program, which is what the playhead compares against.
    #[test]
    fn line_instances_keep_machine_order_and_roles() {
        use crate::view::scene::{Scene, Segment};
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
