//! The viewport's lines: every move of the run, and every line that is not a
//! move, as instanced quads; and the one table of the kinds they are drawn as,
//! which the shaders' constants are made from.

use eframe::wgpu;

use super::content::{Outline, OutlineKind, PlaneKind, View};
use super::gpu::{PipelineSpec, linear_rgba, pipeline, source};
use super::v3;
use crate::ui::theme;
use crate::view::scene::Scene;

pub(super) const LINE_SHADER: &str = include_str!("shaders/line.wgsl");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct LineInstance {
    pub(super) a: [f32; 3],
    pub(super) b: [f32; 3],
    pub(super) kind: u32,
    pub(super) op: u32,
}

/// When a kind of line that is not a move is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShownWith {
    /// With the stock's outline, as ticked under Show.
    Stock,
    /// With the work zero's axes, likewise.
    Origin,
    /// Always, whenever there is one to draw.
    Always,
}

/// A kind of line that is not a move: what it shows, its colour, whether it is
/// drawn twice as wide, to read over the part, and when it is shown.
struct LineKind {
    outline: OutlineKind,
    color: [u8; 3],
    wide: bool,
    shown: ShownWith,
}

/// A kind of line drawn thin, shown always.
const fn thin(outline: OutlineKind, color: [u8; 3]) -> LineKind {
    LineKind {
        outline,
        color,
        wide: false,
        shown: ShownWith::Always,
    }
}

/// A kind of line drawn wide, shown always: picks and the boundary as chosen.
const fn wide(outline: OutlineKind, color: [u8; 3]) -> LineKind {
    LineKind {
        wide: true,
        ..thin(outline, color)
    }
}

/// A kind of line shown as ticked under Show.
const fn ticked(outline: OutlineKind, color: [u8; 3], shown: ShownWith) -> LineKind {
    LineKind {
        shown,
        ..thin(outline, color)
    }
}

/// Every kind of line that is not a move. The kind the shader knows one by is
/// its place here, after the move roles ([`theme::ROLES`]).
const LINE_KINDS: [LineKind; 14] = [
    ticked(OutlineKind::Stock, theme::MATERIAL, ShownWith::Stock),
    ticked(OutlineKind::AxisX, theme::AXIS_X, ShownWith::Origin),
    ticked(OutlineKind::AxisY, theme::AXIS_Y, ShownWith::Origin),
    ticked(OutlineKind::AxisZ, theme::AXIS_Z, ShownWith::Origin),
    wide(OutlineKind::Selected, theme::SELECTED),
    wide(OutlineKind::Hover, theme::HOVER),
    wide(OutlineKind::Boundary, theme::BOUNDARY),
    // Where the tool's centre may go against the boundary, and the hatching over
    // what it cuts outside the part.
    thin(OutlineKind::Limit, theme::BOUNDARY_LIMIT),
    thin(OutlineKind::Channel, theme::CHANNEL),
    // Only uploaded while a stock is being picked.
    thin(OutlineKind::Ghost, theme::GHOST),
    plane(PlaneKind::Clearance),
    plane(PlaneKind::Retract),
    plane(PlaneKind::Top),
    plane(PlaneKind::Bottom),
];

/// The edges of one of an operation's height planes, in its colour.
const fn plane(kind: PlaneKind) -> LineKind {
    thin(OutlineKind::Plane(kind), kind.color())
}

/// Kinds of line that are moves: one a role.
const MOVE_KINDS: usize = theme::ROLES.len();

/// Kinds of line in all, and so colours.
pub(super) const KINDS: usize = MOVE_KINDS + LINE_KINDS.len();

// Each has a bit of the shader's 32-bit masks.
const _: () = assert!(KINDS <= 32);

/// Each kind of line that is not a move with its kind, as the shader knows it.
fn numbered() -> impl Iterator<Item = (u32, &'static LineKind)> {
    (0u32..).skip(MOVE_KINDS).zip(&LINE_KINDS)
}

/// The kind the shader draws `outline` as.
pub(super) fn kind_of(outline: OutlineKind) -> u32 {
    numbered()
        .find(|(_, k)| k.outline == outline)
        .map_or(0, |(i, _)| i)
}

/// Each kind's colour, in linear light: the move roles', then the rest's.
pub(super) fn kind_colors() -> [[f32; 4]; KINDS] {
    let mut out = [[0.0; 4]; KINDS];
    let roles = theme::ROLES.map(theme::role_srgb);
    let colors = roles.into_iter().chain(LINE_KINDS.iter().map(|k| k.color));
    for (slot, c) in out.iter_mut().zip(colors) {
        *slot = linear_rgba(c, 1.0);
    }
    out
}

/// A bit for each kind of line `view` shows: the move roles it shows, the
/// stock's outline and the work zero's axes as ticked, and the rest always.
pub(super) fn kinds_shown(view: &View) -> u32 {
    let roles = view.roles & ((1 << MOVE_KINDS) - 1);
    numbered().fold(roles, |mask, (i, k)| {
        let on = match k.shown {
            ShownWith::Stock => view.stock,
            ShownWith::Origin => view.origin,
            ShownWith::Always => true,
        };
        if on { mask | (1 << i) } else { mask }
    })
}

/// The operation `view` shows alone, as the shader takes it: one more than its
/// index, and zero for none.
pub(super) fn only_op(view: &View) -> u32 {
    view.only.map_or(0, |i| {
        u32::try_from(i)
            .ok()
            .and_then(|i| i.checked_add(1))
            .unwrap_or(u32::MAX)
    })
}

/// The shaders' constants for the kinds of line: how many there are, how many of
/// them are moves, and which are drawn wide, a bit each.
pub(super) fn wgsl_constants() -> String {
    let wide = numbered()
        .filter(|(_, k)| k.wide)
        .fold(0u32, |mask, (i, _)| mask | (1 << i));
    format!(
        "const LINE_KINDS: u32 = {KINDS}u;\n\
         const MOVE_KINDS: u32 = {MOVE_KINDS}u;\n\
         const WIDE_KINDS: u32 = {wide}u;\n"
    )
}

/// The scene's moves as lines, in machine order, an arc as several; and how many
/// lines the moves up to and including each one make, so the playhead, which
/// counts moves, can say how many lines are showing.
pub(super) fn line_instances(scene: &Scene) -> (Vec<LineInstance>, Vec<u32>) {
    let mut lines = Vec::with_capacity(scene.segments.len());
    let mut ends = Vec::with_capacity(scene.segments.len());
    for s in &scene.segments {
        for (a, b) in s.pieces() {
            lines.push(LineInstance {
                a: v3(a),
                b: v3(b),
                kind: theme::role_index(s.role),
                op: s.op,
            });
        }
        ends.push(u32::try_from(lines.len()).unwrap_or(u32::MAX));
    }
    (lines, ends)
}

/// How many lines are showing when `shown` moves are.
pub(super) fn lines_shown(ends: &[u32], shown: usize) -> u32 {
    shown
        .checked_sub(1)
        .and_then(|i| ends.get(i).or(ends.last()))
        .copied()
        .unwrap_or(0)
}

pub(super) fn outline_instances(outline: &[Outline]) -> Vec<LineInstance> {
    outline
        .iter()
        .map(|o| LineInstance {
            a: v3(o.a),
            b: v3(o.b),
            kind: kind_of(o.kind),
            op: 0,
        })
        .collect()
}

/// How much of a line shows where the part hides it: the blend constant its
/// hidden pipeline mixes it in by.
pub(super) const HIDDEN_LINE: f64 = 0.35;

/// The pipeline that draws every line, toolpaths and outlines alike, as
/// instanced quads. `hidden`, the one that draws them only where something is in
/// front, mixed in by [`HIDDEN_LINE`], and leaves the depth alone.
pub(super) fn line_pipeline_for(
    device: &wgpu::Device,
    frame_layout: &wgpu::BindGroupLayout,
    hidden: bool,
) -> wgpu::RenderPipeline {
    let line_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("camcraft lines"),
        source: wgpu::ShaderSource::Wgsl(source(LINE_SHADER).into()),
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
            blend: hidden.then_some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Constant,
                    dst_factor: wgpu::BlendFactor::OneMinusConstant,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            }),
            depth_write: !hidden,
            compare: if hidden {
                wgpu::CompareFunction::Greater
            } else {
                wgpu::CompareFunction::LessEqual
            },
            bias: wgpu::DepthBiasState::default(),
        },
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests;
