//! The view cube, after Fusion's ViewCube: a small cube in the view's corner,
//! turned as the view is, that turns the view to where it is clicked.
//!
//! Each face is cut three by three. Its middle is the face, and looks straight at
//! it: Top, Front, Right and the rest, which are orthographic. Its sides are the
//! twelve edges, and its corners the eight corners, each looking at the part
//! from between them in perspective. Dragging the cube orbits the view, as
//! dragging the view does, and the house beside it goes back to the first view.
//!
//! The geometry (where each face and cell is drawn, and what is under a point) is
//! kept apart from the painting, so it is tested without a window.

use eframe::egui::{self, Pos2, Rect, Vec2, pos2, vec2};
use glam::{IVec3, Mat3, Vec3};

use crate::ui::theme;
use crate::view::camera::Camera;

/// The cube's size on the screen, in points: the square it is drawn in.
pub const SIZE: f32 = 104.0;
/// Where a face's middle ends and its edges begin, out of 1 from its centre.
const MIDDLE: f32 = 0.55;
/// The home button's size, in points.
const HOME: f32 = 22.0;

/// A place on the cube to turn to: a face, an edge or a corner, as the sum of the
/// faces it is on, each an axis with a sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region(pub IVec3);

impl Region {
    /// The direction from the part to the eye that looks at it.
    pub fn direction(self) -> Vec3 {
        self.0.as_vec3().normalize_or_zero()
    }
}

/// The six faces, each as its outward axis, its name, and which way is up on it,
/// so each reads upright in its own straight view.
const FACES: [(IVec3, &str, IVec3); 6] = [
    (IVec3::Z, "TOP", IVec3::Y),
    (IVec3::NEG_Z, "BOTTOM", IVec3::NEG_Y),
    (IVec3::NEG_Y, "FRONT", IVec3::Z),
    (IVec3::Y, "BACK", IVec3::Z),
    (IVec3::NEG_X, "LEFT", IVec3::Z),
    (IVec3::X, "RIGHT", IVec3::Z),
];

/// The cube as the view shows it: turned as the camera is, drawn about `centre`
/// with `scale` points to half its width.
pub struct Cube {
    centre: Pos2,
    scale: f32,
    /// World to view, turning only.
    turn: Mat3,
}

/// One face as drawn: which it is, its corners on the screen, and how squarely
/// it faces the eye, 0 to 1.
pub struct Face {
    /// Its outward axis.
    pub normal: IVec3,
    /// Its name.
    pub name: &'static str,
    /// Up on it, for its name.
    pub up: IVec3,
    /// Its corners, round it.
    pub corners: [Pos2; 4],
    /// How squarely it faces the eye.
    pub facing: f32,
}

impl Cube {
    /// The cube for `camera`, drawn in `rect`.
    pub fn new(camera: &Camera, rect: Rect) -> Self {
        Self {
            centre: rect.center(),
            // The cube's corners are √3 out: seen corner on, it just fills the
            // square.
            scale: rect.width().min(rect.height()) * 0.5 / 1.75,
            turn: Mat3::from_mat4(camera.view()),
        }
    }

    /// A point on the cube, from -1 to 1 along each axis, on the screen, and how
    /// near the eye it is (more is nearer).
    fn at(&self, p: Vec3) -> (Pos2, f32) {
        let v = self.turn * p;
        (self.centre + vec2(v.x, -v.y) * self.scale, v.z)
    }

    /// The faces the eye sees, furthest first, as they are painted.
    pub fn faces(&self) -> Vec<Face> {
        let mut out: Vec<(f32, Face)> = FACES
            .iter()
            .filter_map(|&(normal, name, up)| {
                let facing = (self.turn * normal.as_vec3()).z;
                (facing > 1e-3).then(|| {
                    let corners = self.cell(normal, up, [-1.0, 1.0], [-1.0, 1.0]);
                    (
                        facing,
                        Face {
                            normal,
                            name,
                            up,
                            corners,
                            facing,
                        },
                    )
                })
            })
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.into_iter().map(|(_, f)| f).collect()
    }

    /// The part of face `normal` between `u` and `v` across it, from -1 to 1,
    /// `up` being its v, on the screen.
    fn cell(&self, normal: IVec3, up: IVec3, u: [f32; 2], v: [f32; 2]) -> [Pos2; 4] {
        let (n, up) = (normal.as_vec3(), up.as_vec3());
        // Right on the face, seen from outside it.
        let right = up.cross(n);
        let p = |a: f32, b: f32| self.at(n + right * a + up * b).0;
        [p(u[0], v[0]), p(u[1], v[0]), p(u[1], v[1]), p(u[0], v[1])]
    }

    /// The nine cells of a face, each with the place it turns to.
    pub fn cells(&self, face: &Face) -> Vec<(Region, [Pos2; 4])> {
        let bands = [
            (-1, [-1.0, -MIDDLE]),
            (0, [-MIDDLE, MIDDLE]),
            (1, [MIDDLE, 1.0]),
        ];
        let (n, up) = (face.normal, face.up);
        let right = up.cross(n);
        let mut out = Vec::with_capacity(9);
        for &(i, u) in &bands {
            for &(j, v) in &bands {
                out.push((Region(n + right * i + up * j), self.cell(n, up, u, v)));
            }
        }
        out
    }

    /// What is under `p`, if anything: the nearest face's cell there.
    pub fn hit(&self, p: Pos2) -> Option<Region> {
        self.faces()
            .iter()
            .rev()
            .flat_map(|f| self.cells(f))
            .find(|(_, quad)| inside(quad, p))
            .map(|(r, _)| r)
    }

    /// Where each axis ends, drawn from the cube's corner nearest the origin of
    /// its world, and how near the eye that end is.
    fn axes(&self) -> [(Pos2, Pos2, f32); 3] {
        let o = Vec3::splat(-1.0);
        let (from, _) = self.at(o);
        [Vec3::X, Vec3::Y, Vec3::Z].map(|a| {
            let (to, near) = self.at(o + a * 2.45);
            (from, to, near)
        })
    }
}

/// Whether `p` is inside the convex quad `q`, whichever way round it winds.
fn inside(q: &[Pos2; 4], p: Pos2) -> bool {
    let (mut left, mut right) = (false, false);
    for i in 0..4 {
        let (Some(a), Some(b)) = (q.get(i), q.get((i + 1) % 4)) else {
            return false;
        };
        let c = (*b - *a).x * (p - *a).y - (*b - *a).y * (p - *a).x;
        left |= c > 1e-6;
        right |= c < -1e-6;
    }
    left != right
}

/// What the cube was asked to do this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// Turn to look at the part from this direction.
    Turn(Vec3),
    /// Orbit by a drag, in points.
    Orbit(Vec2),
    /// Back to the first view.
    Home,
}

/// Where the cube is drawn in `view`: its top right corner.
pub fn place(view: Rect) -> Rect {
    let inset = theme::SPACE_4;
    Rect::from_min_size(
        pos2(view.right() - inset - SIZE, view.top() + inset),
        Vec2::splat(SIZE),
    )
}

/// Where on the screen the cube in `view` turns to `region`, for `camera`: the
/// middle of its cell, if one is seen. Only the snapshot harness asks.
#[cfg(all(test, feature = "ui-snapshots"))]
pub fn point_of(view: Rect, camera: &Camera, region: Region) -> Option<Pos2> {
    let cube = Cube::new(camera, place(view));
    cube.faces()
        .iter()
        .flat_map(|f| cube.cells(f))
        .filter(|(r, _)| *r == region)
        .map(|(_, q)| (q.iter().fold(Vec2::ZERO, |s, p| s + p.to_vec2()) / 4.0).to_pos2())
        .find(|p| cube.hit(*p) == Some(region))
}

/// Draw the cube for `camera` in the top right of `view`, with the home button
/// beside it, and say what it was asked to do.
pub fn show(ui: &mut egui::Ui, view: Rect, camera: &Camera) -> Option<Action> {
    let rect = place(view);
    if !view.contains_rect(rect) {
        return None;
    }
    // Beside the cube, clear of it however it is turned.
    let home = Rect::from_min_size(
        rect.left_top() - vec2(HOME + theme::SPACE_2, 0.0),
        Vec2::splat(HOME),
    );
    let id = ui.id().with("view cube");
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    let home_response = ui
        .interact(home, id.with("home"), egui::Sense::click())
        .on_hover_text("home view (R)");

    let cube = Cube::new(camera, rect);
    let under = response
        .hover_pos()
        .filter(|_| !response.dragged() && !home_response.hovered())
        .and_then(|p| cube.hit(p));
    if under.is_some() || home_response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // The axes and their names reach past the cube's square, so it is painted
    // within the whole view.
    let painter = ui.painter_at(view);
    paint(ui, &painter, &cube, under);
    paint_home(&painter, home, home_response.hovered());

    if home_response.clicked() {
        return Some(Action::Home);
    }
    if response.dragged() {
        let d = response.drag_delta();
        return (d != Vec2::ZERO).then_some(Action::Orbit(d));
    }
    if response.clicked()
        && let Some(r) = response.interact_pointer_pos().and_then(|p| cube.hit(p))
    {
        return Some(Action::Turn(r.direction()));
    }
    None
}

/// The cube: the axes behind it, its faces, furthest first, the cell under the
/// pointer, and each face's name where it faces the eye enough to read.
fn paint(ui: &egui::Ui, painter: &egui::Painter, cube: &Cube, under: Option<Region>) {
    let axes = cube.axes();
    let colors = [theme::AXIS_X, theme::AXIS_Y, theme::AXIS_Z];
    let names = ["X", "Y", "Z"];
    let font = theme::caption(ui.style());
    // An axis going away is drawn under the cube, one coming nearer over it.
    let axis = |near_side: bool| {
        for ((from, to, near), (color, name)) in axes.iter().zip(colors.iter().zip(names)) {
            if (*near > 0.0) != near_side {
                continue;
            }
            let c = theme::egui_color(*color);
            painter.line_segment([*from, *to], egui::Stroke::new(1.5, c));
            let away = (*to - *from).normalized() * 7.0;
            painter.text(
                *to + away,
                egui::Align2::CENTER_CENTER,
                name,
                font.clone(),
                c,
            );
        }
    };
    axis(false);

    let border = egui::Stroke::new(1.0, theme::egui_color(theme::BORDER_STRONG));
    for face in cube.faces() {
        // Lighter the more squarely it faces the eye, so the cube reads as solid.
        let base = theme::HOVER_OVERLAY.over(theme::SURFACE_3);
        let lit = theme::Tint {
            rgb: theme::SURFACE_RAISED,
            alpha: face.facing,
        }
        .over(base);
        painter.add(egui::Shape::convex_polygon(
            face.corners.to_vec(),
            theme::egui_color(lit),
            border,
        ));
        if let Some(r) = under
            && let Some((_, quad)) = cube.cells(&face).into_iter().find(|(c, _)| *c == r)
        {
            painter.add(egui::Shape::convex_polygon(
                quad.to_vec(),
                theme::SELECTION.color(),
                egui::Stroke::new(1.0, theme::egui_color(theme::ACCENT)),
            ));
        }
        label(ui, painter, cube, &face);
    }
    axis(true);
}

/// A face's name across its middle, turned with it, where it faces the eye enough
/// to read.
fn label(ui: &egui::Ui, painter: &egui::Painter, cube: &Cube, face: &Face) {
    if face.facing < 0.35 {
        return;
    }
    let n = face.normal.as_vec3();
    // Seen from outside, the face is never mirrored, so its name only turns:
    // along the face's own right.
    let (centre, _) = cube.at(n);
    let (right, _) = cube.at(n + face.up.as_vec3().cross(n));
    let across = right - centre;
    let angle = across.y.atan2(across.x);
    let color = theme::egui_color(theme::TEXT_2);
    let galley =
        ui.painter()
            .layout_no_wrap(face.name.to_string(), theme::caption(ui.style()), color);
    let size = galley.size();
    let rot = egui::emath::Rot2::from_angle(angle);
    let pos = centre - rot * (size * 0.5);
    painter.add(egui::epaint::TextShape::new(pos, galley, color).with_angle(angle));
}

/// The home button: a small house, lit when the pointer is over it.
fn paint_home(painter: &egui::Painter, rect: Rect, hovered: bool) {
    let color = theme::egui_color(if hovered {
        theme::ACCENT
    } else {
        theme::TEXT_3
    });
    if hovered {
        painter.rect_filled(rect, theme::RADIUS_SM, theme::HOVER_OVERLAY.color());
    }
    let c = rect.center();
    let s = rect.width() * 0.26;
    let stroke = egui::Stroke::new(1.5, color);
    // The roof, then the walls and floor.
    painter.line_segment([c + vec2(-s * 1.2, 0.0), c + vec2(0.0, -s * 1.1)], stroke);
    painter.line_segment([c + vec2(0.0, -s * 1.1), c + vec2(s * 1.2, 0.0)], stroke);
    let walls = [
        c + vec2(-s * 0.8, -s * 0.3),
        c + vec2(-s * 0.8, s),
        c + vec2(s * 0.8, s),
        c + vec2(s * 0.8, -s * 0.3),
    ];
    painter.add(egui::Shape::line(walls.to_vec(), stroke));
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), Vec2::splat(SIZE))
    }

    fn framing() -> Camera {
        Camera::framing(Vec3::ZERO, 50.0)
    }

    /// Every one of the 26 places turns the view to look from it: orthographic
    /// straight at a face, perspective from an edge or a corner.
    #[test]
    fn every_place_turns_to_look_from_it() {
        let mut seen = Vec::new();
        for (normal, ..) in FACES {
            // Looking straight at the face, so all nine of its cells are drawn.
            let cube = Cube::new(&framing().turned_to(normal.as_vec3()), rect());
            let face = cube
                .faces()
                .into_iter()
                .find(|f| f.normal == normal)
                .unwrap();
            for (r, _) in cube.cells(&face) {
                if !seen.contains(&r) {
                    seen.push(r);
                }
            }
        }
        assert_eq!(seen.len(), 26);
        for r in seen {
            let c = framing().turned_to(r.direction());
            let eye = (c.eye() - c.target).normalize();
            assert!(eye.distance(r.direction()) < 1e-4, "{r:?}");
            let straight = r.0.abs().element_sum() == 1;
            assert_eq!(c.orthographic(), straight, "{r:?}");
        }
    }

    /// The first view sees the top, front and right, and nothing else.
    #[test]
    fn the_first_view_sees_top_front_and_right() {
        let cube = Cube::new(&framing(), rect());
        let mut names: Vec<&str> = cube.faces().iter().map(|f| f.name).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["FRONT", "RIGHT", "TOP"]);
    }

    /// The middle of the face seen hits that face, from each straight view and
    /// the first one; its corner cells hit corners.
    #[test]
    fn a_face_is_hit_where_it_is_drawn() {
        for c in [
            framing(),
            framing().turned_to(Vec3::Z),
            framing().turned_to(-Vec3::Y),
            framing().turned_to(Vec3::X),
        ] {
            let cube = Cube::new(&c, rect());
            for face in cube.faces() {
                for (r, quad) in cube.cells(&face) {
                    let middle = quad.iter().fold(Vec2::ZERO, |s, p| s + p.to_vec2()) / 4.0;
                    let hit = cube.hit(middle.to_pos2());
                    // Where two faces show, the nearer takes the point.
                    if face.facing > 0.9 {
                        assert_eq!(hit, Some(r), "{} {r:?}", face.name);
                    }
                }
            }
        }
        // Straight at the front, its middle is Front, and off the cube is nothing.
        let cube = Cube::new(&framing().turned_to(-Vec3::Y), rect());
        assert_eq!(cube.hit(rect().center()), Some(Region(IVec3::NEG_Y)));
        assert_eq!(cube.hit(pos2(1.0, 1.0)), None);
    }

    /// Straight from the top, TOP reads upright with Front below it.
    #[test]
    fn top_reads_upright_from_the_top() {
        let cube = Cube::new(&framing().turned_to(Vec3::Z), rect());
        let faces = cube.faces();
        let top = faces.iter().find(|f| f.name == "TOP").unwrap();
        let (centre, _) = cube.at(Vec3::Z);
        let (up, _) = cube.at(Vec3::Z + Vec3::Y);
        assert!(up.y < centre.y, "up on the face is up the screen");
        assert!(top.facing > 0.999);
        let (front, _) = cube.at(Vec3::Z - Vec3::Y);
        assert!(front.y > centre.y, "front at the bottom");
    }
}
