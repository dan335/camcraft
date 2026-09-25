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
//! *is* its order. Visibility by role and operation, and the playhead, are then a
//! uniform the vertex shader reads, not a buffer rebuilt on every change.
//!
//! What is picked, and what is under the pointer, is drawn over the part: its faces
//! again, in the accent, through a pipeline that passes where the part's own depth
//! is equal, so they lie exactly on it; its edges as wide lines; its points as
//! marks painted over the picture. What the view hits is not decided here: the view
//! reports where the pointer is and whether it clicked, as a ray, and
//! [`crate::picking::ray`] answers.

mod content;
mod gpu;
mod stock_gpu;

use camcraft_geom::heightmap::narrow;
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use glam::{DVec3, Mat4, Vec2, Vec3};

use crate::picking::ray::Projector;
use crate::stock::stock_ao;
use crate::ui::theme;
use crate::view::camera::Camera;
use gpu::Gpu;

pub use content::{
    Content, Highlight, Outline, OutlineKind, Plane, PlaneKind, PointerEvent, StockChange,
    StockRef, StockShow, View,
};
pub(crate) use stock_gpu::WALL_CELLS;

/// Toolpath width, in points.
const LINE_WIDTH: f32 = 1.25;

fn v3(p: DVec3) -> [f32; 3] {
    [narrow(p.x), narrow(p.y), narrow(p.z)]
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
                crate::ui::widgets::Level::Warn.color(),
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

        paint_over(ui, rect, &projector, content);
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

/// What is painted over the picture: the height planes' labels, picked points and
/// the one under the pointer, the hint, and what the view says when it is empty.
fn paint_over(ui: &egui::Ui, rect: egui::Rect, projector: &Projector, content: &Content<'_>) {
    let painter = ui.painter_at(rect);
    plane_labels(ui, &painter, projector, content.planes);

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

    let faint = theme::egui_color(theme::TEXT_3);
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
