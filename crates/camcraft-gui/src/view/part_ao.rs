//! Ambient occlusion for the part, worked out on the screen.
//!
//! The material is shaded on its own heights ([`crate::stock::stock_ao`]); the part
//! is a mesh, so it is shaded as GTAO does: from the depth the part leaves, each
//! pixel looks out along a few directions on the screen for the highest horizon,
//! and turns the horizons into the share of cosine-weighted sky left open.
//!
//! The part is drawn alone first; its depth is read at half the view's size (a
//! quarter of the pixels), each pixel turning its directions by its place in a 4
//! by 4 pattern; a 4 by 4 blur that stays on one surface takes the pattern out;
//! and the result darkens the part's pixels where they are, before anything else
//! is drawn over them. So paths, planes, picks and the cutter are never darkened,
//! and the material, drawn after, keeps its own shade. It runs on every frame the
//! view draws, never only when the view stops moving: a shade that comes and goes
//! with the pointer is seen far more than one that costs a little.

use eframe::wgpu;
use glam::Mat4;

/// How much the occlusion darkens the part, 0..1. The part is drawn gamma-encoded,
/// where the same factor darkens more than it would in linear light.
const STRENGTH: f32 = 0.7;

/// Its three passes: the occlusion found at half size, blurred, and laid over
/// the part.
pub(crate) const SHADER: &str = include_str!("viewport/shaders/part_ao.wgsl");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    inv_proj: [[f32; 4]; 4],
    size: [f32; 4],
    params: [f32; 4],
}

/// What is made for one size of view.
struct Sized {
    size: [u32; 2],
    gtao: wgpu::BindGroup,
    blur: wgpu::BindGroup,
    composite: wgpu::BindGroup,
    raw: wgpu::TextureView,
    smooth: wgpu::TextureView,
}

/// The part's occlusion: its pipelines, and what they draw into.
pub struct PartAo {
    layout: wgpu::BindGroupLayout,
    gtao: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    /// A 1 by 1 texture for the binding the first pass does not read.
    blank: wgpu::TextureView,
    sized: Option<Sized>,
}

const HALF: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

impl PartAo {
    /// For a view drawn in `color` with `samples` samples a pixel.
    pub fn new(device: &wgpu::Device, color: wgpu::TextureFormat, samples: u32) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camcraft part ao"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("camcraft part ao"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("camcraft part ao"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label, entry, format, samples, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: samples,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // What is there, times what the pass gives.
        let multiply = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::Src,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let gtao = pipeline("camcraft part ao", "gtao", HALF, 1, None);
        let blur = pipeline("camcraft part ao blur", "blur", HALF, 1, None);
        let composite = pipeline(
            "camcraft part ao composite",
            "composite",
            color,
            samples,
            Some(multiply),
        );
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camcraft part ao"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blank = half_texture(device, "camcraft part ao blank", [1, 1]);
        Self {
            layout,
            gtao,
            blur,
            composite,
            uniforms,
            blank,
            sized: None,
        }
    }

    /// Ready for a view `size` pixels across whose depth is `depth`.
    pub fn resize(&mut self, device: &wgpu::Device, size: [u32; 2], depth: &wgpu::TextureView) {
        let half = [size[0].div_ceil(2).max(1), size[1].div_ceil(2).max(1)];
        let raw = half_texture(device, "camcraft part ao found", half);
        let smooth = half_texture(device, "camcraft part ao smoothed", half);
        let group = |label, found: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(found),
                    },
                ],
            })
        };
        self.sized = Some(Sized {
            size,
            gtao: group("camcraft part ao", &self.blank),
            blur: group("camcraft part ao blur", &raw),
            composite: group("camcraft part ao composite", &smooth),
            raw,
            smooth,
        });
    }

    /// Set up for this frame: the view's projection, and how far, in
    /// millimetres, a point looks for what shades it.
    pub fn frame(&self, queue: &wgpu::Queue, projection: Mat4, orthographic: bool, reach: f32) {
        let Some(sized) = &self.sized else {
            return;
        };
        #[allow(clippy::cast_precision_loss)]
        let (w, h) = (sized.size[0] as f32, sized.size[1] as f32);
        let uniforms = Uniforms {
            inv_proj: projection.inverse().to_cols_array_2d(),
            size: [w, h, (w / 2.0).ceil().max(1.0), (h / 2.0).ceil().max(1.0)],
            params: [
                reach,
                projection.y_axis.y * h / 2.0,
                STRENGTH,
                if orthographic { 1.0 } else { 0.0 },
            ],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Work the occlusion out from the depth and darken `color` (the view's
    /// many-sampled picture, as the part left it) by it.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView) {
        let Some(sized) = &self.sized else {
            return;
        };
        let steps = [
            (&self.gtao, &sized.gtao, &sized.raw, true),
            (&self.blur, &sized.blur, &sized.smooth, true),
            (&self.composite, &sized.composite, color, false),
        ];
        for (pipeline, group, target, clear) in steps {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("camcraft part ao"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if clear {
                            wgpu::LoadOp::Clear(wgpu::Color::WHITE)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

fn half_texture(device: &wgpu::Device, label: &str, [w, h]: [u32; 2]) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HALF,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}
