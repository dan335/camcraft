//! The viewport on the GPU: its shaders and pipelines, the offscreen targets it
//! draws into, and the passes that draw a frame. What is uploaded of the content
//! it is given is kept in [`super::upload`], its lines are [`super::lines`], and
//! the material is [`super::stock_gpu`].

use eframe::egui;
use eframe::egui_wgpu::RenderState;
use eframe::wgpu;
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::content::PlaneKind;
use super::lines::{self, HIDDEN_LINE, KINDS, kind_colors, kinds_shown, lines_shown, only_op};
use super::stock_gpu::{self, StockGpu};
use super::upload::{HighlightGpu, Uploaded, Vertex};
use super::{FrameInput, v3};
use crate::ui::theme;
use crate::view::part_ao::PartAo;

/// Offscreen colour format: what egui expects of a native texture.
const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;

const COMMON: &str = include_str!("shaders/common.wgsl");

const MESH_SHADER: &str = include_str!("shaders/mesh.wgsl");

/// A shader's whole source: the kinds of line's constants, what every shader
/// shares, then `body`.
pub(super) fn source(body: &str) -> String {
    format!("{}{COMMON}{body}", lines::wgsl_constants())
}

/// What changes from frame to frame of the frame's uniforms. First in its buffer,
/// so a frame writes only these.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniforms {
    view_proj: [[f32; 4]; 4],
    viewport: [f32; 4],
    masks: [u32; 4],
    eye: [f32; 4],
    sight: [f32; 4],
}

/// The rest of the frame's uniforms, written once: each kind of line's colour.
/// The light is the shaders' own (`common.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameConstants {
    colors: [[f32; 4]; KINDS],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ObjectUniforms {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    finish: [f32; 4],
}

/// An sRGB colour in linear light, at `alpha`.
pub(super) fn linear_rgba(c: [u8; 3], alpha: f32) -> [f32; 4] {
    let [r, g, b] = theme::linear(c);
    [r, g, b, alpha]
}

struct Targets {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolved: wgpu::TextureView,
}

impl Targets {
    /// A pass drawing into them. The first of a frame, given the colour to clear
    /// to, clears them and keeps what it draws for the next; the last loads them
    /// and resolves the picture, keeping nothing else.
    fn pass<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        label: &str,
        clear: Option<[f32; 3]>,
    ) -> wgpu::RenderPass<'e> {
        let (color, depth, store) = match clear {
            Some([r, g, b]) => (
                wgpu::LoadOp::Clear(wgpu::Color {
                    r: f64::from(r),
                    g: f64::from(g),
                    b: f64::from(b),
                    a: 1.0,
                }),
                wgpu::LoadOp::Clear(1.0),
                wgpu::StoreOp::Store,
            ),
            None => (
                wgpu::LoadOp::Load,
                wgpu::LoadOp::Load,
                wgpu::StoreOp::Discard,
            ),
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.msaa,
                resolve_target: clear.is_none().then_some(&self.resolved),
                ops: wgpu::Operations { load: color, store },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations { load: depth, store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }
}

struct Object {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
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
    /// Lines where the part hides them, faint: picks and boundaries under it.
    line_hidden_pipeline: wgpu::RenderPipeline,
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
    plane_objects: [(Object, Object); 4],
    targets: Option<Targets>,
    texture: Option<egui::TextureId>,
    /// What is uploaded of the content.
    pub(super) up: Uploaded,
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
        // Uniforms made with what they hold, and written again only where a frame
        // changes them.
        let buffer = |label, contents: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
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
        let per_frame: FrameUniforms = bytemuck::Zeroable::zeroed();
        let mut frame_contents = bytemuck::bytes_of(&per_frame).to_vec();
        frame_contents.extend_from_slice(bytemuck::bytes_of(&FrameConstants {
            colors: kind_colors(),
        }));
        let frame = buffer("camcraft frame", &frame_contents);
        let frame_group = group(&frame_layout, &frame);
        // An object in `color` at `alpha` and of `finish`, where it is.
        let object = |label, color, alpha, finish: theme::Finish| {
            let uniforms = ObjectUniforms {
                model: Mat4::IDENTITY.to_cols_array_2d(),
                color: linear_rgba(color, alpha),
                finish: finish.uniform(),
            };
            let uniforms = buffer(label, bytemuck::bytes_of(&uniforms));
            let bind_group = group(&object_layout, &uniforms);
            Object {
                uniforms,
                bind_group,
            }
        };

        let mesh_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("camcraft mesh"),
            source: wgpu::ShaderSource::Wgsl(source(MESH_SHADER).into()),
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

        let line_hidden_pipeline = lines::line_pipeline_for(device, &frame_layout, true);
        let line_pipeline = lines::line_pipeline_for(device, &frame_layout, false);

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
            line_hidden_pipeline,
            frame,
            frame_group,
            part_object: object("camcraft part", theme::PART, 1.0, theme::PART_FINISH),
            selection_object: object(
                "camcraft selection",
                theme::SELECTED,
                0.55,
                theme::MARK_FINISH,
            ),
            hover_object: object("camcraft hover", theme::HOVER, 0.4, theme::MARK_FINISH),
            ghost_object: object("camcraft ghost", theme::GHOST, 0.22, theme::MARK_FINISH),
            ghost_hidden_object: object(
                "camcraft ghost, hidden",
                theme::GHOST,
                0.1,
                theme::MARK_FINISH,
            ),
            // Opaque enough to find at a glance on a large part, and still
            // showing what it passes through.
            cutter_object: object("camcraft cutter", theme::AMBER_5, 0.8, theme::MARK_FINISH),
            part_ao: PartAo::new(device, COLOR, SAMPLES),
            // Faint: the part must still be read through them.
            plane_objects: PlaneKind::ALL.map(|k| {
                (
                    object("camcraft plane", k.color(), 0.16, theme::MARK_FINISH),
                    object(
                        "camcraft plane, hidden",
                        k.color(),
                        0.06,
                        theme::MARK_FINISH,
                    ),
                )
            }),
            targets: None,
            texture: None,
            up: Uploaded::none(),
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

    pub(super) fn render(
        &mut self,
        rs: &RenderState,
        size: [u32; 2],
        input: &FrameInput,
    ) -> Option<egui::TextureId> {
        let texture = self.targets(rs, size)?;
        let targets = self.targets.as_ref()?;
        let view = input.view;

        // The playhead counts moves; the GPU draws lines, several to an arc.
        let shown = match self.up.scene.as_ref() {
            Some(s) => lines_shown(&s.ends, view.shown),
            None => u32::try_from(view.shown).unwrap_or(u32::MAX),
        };
        self.write_uniforms(rs, input, shown);

        let mut encoder = rs
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("camcraft viewport"),
            });
        let part = self.up.part.as_ref().filter(|_| view.part);
        // The part alone first, kept, so its occlusion can be worked out from the
        // depth it leaves and darken it before anything else is drawn over it.
        {
            let bg = theme::linear(theme::BG);
            let mut pass = targets.pass(&mut encoder, "camcraft viewport part", Some(bg));
            pass.set_bind_group(0, &self.frame_group, &[]);
            if let Some((_, buffer, n)) = part {
                pass.set_pipeline(&self.part_pipeline);
                pass.set_bind_group(1, &self.part_object.bind_group, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..*n, 0..1);
            }
        }
        if part.is_some() {
            self.part_ao.frame(
                &rs.queue,
                input.projection,
                input.sight.is_some(),
                input.reach,
            );
            self.part_ao.draw(&mut encoder, &targets.msaa);
        }
        {
            let mut pass = targets.pass(&mut encoder, "camcraft viewport", None);
            pass.set_bind_group(0, &self.frame_group, &[]);
            self.draw_rest(&mut pass, input, part.is_some(), shown);
        }
        rs.queue.submit([encoder.finish()]);
        Some(texture)
    }

    /// Write what changes from frame to frame of the uniforms: the view, the
    /// masks of what is shown, the eye, and where the cutter is. The rest was
    /// written when they were made.
    fn write_uniforms(&self, rs: &RenderState, input: &FrameInput, shown: u32) {
        let view = input.view;
        let frame = FrameUniforms {
            view_proj: input.view_proj.to_cols_array_2d(),
            viewport: [input.pixels[0], input.pixels[1], input.line_width, 2e-4],
            masks: [kinds_shown(&view), view.ops, shown, only_op(&view)],
            eye: input.eye.extend(1.0).to_array(),
            sight: match input.sight {
                Some((forward, length)) => forward.extend(length).to_array(),
                None => [0.0; 4],
            },
        };
        rs.queue
            .write_buffer(&self.frame, 0, bytemuck::bytes_of(&frame));
        if let Some((_, tip)) = view.cutter {
            // Its model matrix, which leads its uniforms.
            let model = Mat4::from_translation(Vec3::from(v3(tip))).to_cols_array_2d();
            rs.queue
                .write_buffer(&self.cutter_object.uniforms, 0, bytemuck::bytes_of(&model));
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

        // Picks and boundaries where the part hides them, faintly: a boundary
        // round the part's bottom, or a pocket's floor, lies under it.
        pass.set_pipeline(&self.line_hidden_pipeline);
        pass.set_blend_constant(wgpu::Color {
            r: HIDDEN_LINE,
            g: HIDDEN_LINE,
            b: HIDDEN_LINE,
            a: HIDDEN_LINE,
        });
        for up in [&self.up.selection, &self.up.hover] {
            if let Some(HighlightGpu {
                lines: Some((buffer, n)),
                ..
            }) = up
            {
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..6, 0..*n);
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

#[cfg(test)]
#[allow(clippy::panic)]
pub(super) mod tests {
    use super::*;

    /// Every shader parses and validates as the GPU will be given it, so a slip in
    /// one fails here rather than when the app first draws.
    #[test]
    fn every_shader_is_valid_wgsl() {
        for (name, source) in [
            ("mesh", source(MESH_SHADER)),
            ("line", source(lines::LINE_SHADER)),
            ("stock", source(stock_gpu::STOCK_SHADER)),
            ("part ao", crate::view::part_ao::SHADER.to_string()),
        ] {
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

    /// The frame's uniforms are laid out as the shaders read them: what changes
    /// each frame first, then the colours, each kind's in its place.
    #[test]
    fn the_frame_is_laid_out_as_the_shaders_read_it() {
        let (offset, span) = layout(MESH_SHADER, "Frame");
        let size = std::mem::size_of::<FrameUniforms>();
        assert_eq!(offset("colors"), Some(size));
        assert_eq!(
            offset("masks"),
            Some(std::mem::offset_of!(FrameUniforms, masks))
        );
        assert_eq!(
            offset("sight"),
            Some(std::mem::offset_of!(FrameUniforms, sight))
        );
        assert_eq!(span, size + std::mem::size_of::<FrameConstants>());
    }

    /// Where each member of the struct `name` in `shader` sits, and its size.
    pub(in crate::view::viewport) fn layout(
        shader: &str,
        name: &str,
    ) -> (impl Fn(&str) -> Option<usize>, usize) {
        let source = source(shader);
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        let found = module
            .types
            .iter()
            .find(|(_, t)| t.name.as_deref() == Some(name))
            .map(|(_, t)| t.inner.clone());
        let Some(naga::TypeInner::Struct { members, span }) = found else {
            panic!("no {name}");
        };
        let offset = move |member: &str| {
            members
                .iter()
                .find(|m| m.name.as_deref() == Some(member))
                .map(|m| m.offset as usize)
        };
        (offset, span as usize)
    }

    /// An object's uniforms, finish and all, are laid out as the mesh shader
    /// reads them.
    #[test]
    fn an_object_is_laid_out_as_the_shaders_read_it() {
        let (offset, span) = layout(MESH_SHADER, "Object");
        assert_eq!(
            offset("finish"),
            Some(std::mem::offset_of!(ObjectUniforms, finish))
        );
        assert_eq!(span, std::mem::size_of::<ObjectUniforms>());
    }
}
