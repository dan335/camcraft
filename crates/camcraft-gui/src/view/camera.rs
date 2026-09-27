//! An orbit camera for a Z-up world measured in millimetres.
//!
//! The design is nodemangler's arcball camera (`view_window/arcball_camera.rs`):
//! yaw and pitch about a target, a distance, and a pan that keeps the dragged
//! point under the cursor. Two things differ, and both matter here. The world is
//! **Z-up**, which is the engine's convention, so reading the picture any other way
//! would make every reading of it wrong. And a part may be 25 mm or 400 mm across,
//! so the zoom and pan limits scale with the part rather than being fixed numbers.
//!
//! Looking straight at the part from the top, bottom, front, back, left or right,
//! the view is orthographic, as a drawing is; from anywhere else, perspective. The
//! projection follows from the angle alone ([`Camera::orthographic`]), so there is
//! no mode to set: the view cube turns to a straight view, and orbiting away from
//! it is perspective again.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use glam::camera::rh::{proj::directx, view::look_to_mat4};
use glam::{DVec3, Mat4, Vec2, Vec3};

const ROTATION_SPEED: f32 = 0.005;
const ZOOM_SPEED: f32 = 0.0015;
/// How far orbiting may go towards straight down or up: just short, so a drag is
/// always a perspective view. Only a turn to Top or Bottom looks straight along Z.
const MAX_ORBIT_PITCH: f32 = FRAC_PI_2 - 1e-3;
/// How near a straight view an angle must be to be one, in radians.
const STRAIGHT: f32 = 1e-4;

/// Where the camera is, relative to what it looks at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// The point orbited around.
    pub target: Vec3,
    /// Distance from the target to the eye.
    pub distance: f32,
    /// Angle about +Z, from +X towards +Y.
    pub yaw: f32,
    /// Elevation above the XY plane.
    pub pitch: f32,
    /// Vertical field of view, radians.
    pub fov_y: f32,
    /// The part's centre and radius, which the limits are scaled to.
    centre: Vec3,
    radius: f32,
}

impl Camera {
    /// A camera framing a sphere of `radius` around `centre`, from the front right
    /// and above, looking down on it: the view the part is first shown in.
    pub fn framing(centre: Vec3, radius: f32) -> Self {
        let radius = if radius.is_finite() && radius > 0.0 {
            radius
        } else {
            1.0
        };
        let fov_y = 40.0_f32.to_radians();
        Self {
            target: centre,
            // Far enough that the whole sphere fits the vertical field of view,
            // with a little room.
            distance: radius / (fov_y * 0.5).sin() * 1.15,
            yaw: (-1.7_f32).atan2(1.4),
            pitch: 0.5,
            fov_y,
            centre,
            radius,
        }
    }

    /// Look at `point` from where the view already is, close enough that a sphere
    /// of `radius` around it fills the view. The limits stay the part's.
    pub fn focus(&mut self, point: Vec3, radius: f32) {
        let radius = if radius.is_finite() && radius > 0.0 {
            radius
        } else {
            self.radius * 0.1
        };
        self.target = point;
        self.distance =
            (radius / (self.fov_y * 0.5).sin()).clamp(self.radius * 0.05, self.radius * 20.0);
    }

    /// Let the view reach a scene of `radius`: zoom out, pan and see that far,
    /// without moving it.
    pub fn cover(&mut self, radius: f32) {
        if radius.is_finite() && radius > self.radius {
            self.radius = radius;
        }
    }

    /// Move the camera, and what it frames, by `by`: for when everything drawn has
    /// moved by that much, so the view stays on the same part of it.
    pub fn shift(&mut self, by: Vec3) {
        if by.is_finite() {
            self.target += by;
            self.centre += by;
        }
    }

    /// Back to the framing view.
    pub fn reset(&mut self) {
        *self = Self::framing(self.centre, self.radius);
    }

    /// Where the eye is.
    pub fn eye(&self) -> Vec3 {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        self.target + Vec3::new(cp * cy, cp * sy, sp) * self.distance
    }

    /// Which way the eye looks, and the screen's right and up, in the world.
    ///
    /// Right comes from the yaw alone, so it stays defined looking straight down
    /// or up, where it is the yaw's own sideways: Top, at a yaw of -90°, has +X to
    /// the right and +Y up the screen.
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        let forward = -Vec3::new(cp * cy, cp * sy, sp);
        let right = Vec3::new(-sy, cy, 0.0);
        (forward, right, right.cross(forward))
    }

    /// Whether it looks straight at the part from the top, bottom, front, back,
    /// left or right: straight down or up, or level with a yaw a whole number of
    /// right angles round. Then the view is orthographic.
    pub fn orthographic(&self) -> bool {
        let pole = (self.pitch.abs() - FRAC_PI_2).abs() < STRAIGHT;
        let square = self.yaw.rem_euclid(FRAC_PI_2);
        let level =
            self.pitch.abs() < STRAIGHT && (square < STRAIGHT || FRAC_PI_2 - square < STRAIGHT);
        pole || level
    }

    /// Half the height of the view where the target is, in millimetres: the same
    /// in either projection, so switching between them keeps the part's size.
    fn half_height(&self) -> f32 {
        self.distance * (self.fov_y * 0.5).tan()
    }

    /// How far behind the target an orthographic view sees: past anything the
    /// target can be panned to, and the part round it.
    fn behind(&self) -> f32 {
        self.radius * 4.0
    }

    /// World to view.
    pub fn view(&self) -> Mat4 {
        let (forward, _, up) = self.basis();
        look_to_mat4(self.eye(), forward, up)
    }

    /// View to clip, with depth from 0 to 1 as wgpu expects (the DirectX convention).
    pub fn projection(&self, aspect: f32) -> Mat4 {
        let aspect = aspect.max(1e-3);
        // Far follows the distance, so the part never clips however far in or out
        // the view is zoomed.
        let far = self.distance + self.radius * 4.0;
        if self.orthographic() {
            // From behind the eye too: an orthographic eye stands anywhere along
            // its line, and what is between it and the target still shows.
            let h = self.half_height();
            return directx::orthographic(-h * aspect, h * aspect, -h, h, -self.behind(), far);
        }
        let near = (self.distance * 0.01).max(1e-3);
        directx::perspective(self.fov_y, aspect, near, far)
    }

    /// Where its lines of sight come from.
    pub fn sight(&self) -> Sight {
        if self.orthographic() {
            Sight::Along(self.basis().0.as_dvec3(), f64::from(self.sight_length()))
        } else {
            Sight::Eye(self.eye().as_dvec3())
        }
    }

    /// How far back from what it shows a line of sight starts, looking
    /// orthographically: from in front of the nearest thing drawn to past the
    /// farthest.
    pub fn sight_length(&self) -> f32 {
        self.distance + self.radius * 4.0 + self.behind()
    }

    /// This view turned to look along `-dir`, a direction from the target to the
    /// eye: the target and distance kept. A direction along an axis turns to the
    /// straight view exactly, so it is orthographic; straight down or up, the
    /// front is at the bottom of the screen.
    pub fn turned_to(&self, dir: Vec3) -> Self {
        let dir = dir.normalize_or_zero();
        let mut out = *self;
        if dir == Vec3::ZERO {
            return out;
        }
        let flat = dir.x.hypot(dir.y);
        if flat < 1e-6 {
            out.yaw = -FRAC_PI_2;
            out.pitch = FRAC_PI_2.copysign(dir.z);
        } else {
            out.yaw = dir.y.atan2(dir.x);
            out.pitch = if dir.z.abs() < 1e-6 {
                0.0
            } else {
                dir.z.atan2(flat)
            };
        }
        // The way round from where the view is that turns least.
        out.yaw = self.yaw + wrap(out.yaw - self.yaw);
        out
    }

    /// Part of the way from `a` to `b`, `t` from 0 to 1: `b` itself at 1.
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        if t >= 1.0 {
            return *b;
        }
        let t = t.max(0.0);
        let mix = |x: f32, y: f32| x + (y - x) * t;
        Self {
            target: a.target.lerp(b.target, t),
            distance: mix(a.distance, b.distance),
            yaw: a.yaw + wrap(b.yaw - a.yaw) * t,
            pitch: mix(a.pitch, b.pitch),
            fov_y: mix(a.fov_y, b.fov_y),
            ..*b
        }
    }

    /// The whole transform, world to clip.
    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    /// The ray from the eye through `ndc`, a point on the screen with x right and y
    /// up, each -1 to 1 across the view, in world coordinates: where it starts,
    /// and a unit direction.
    ///
    /// Built from the same basis [`view`](Self::view) uses, in `f64` from there on,
    /// so what it hits is what is drawn under that point.
    ///
    /// Orthographically, every ray runs the same way, from in front of everything
    /// drawn.
    pub fn ray(&self, aspect: f32, ndc: Vec2) -> (DVec3, DVec3) {
        let eye = self.eye();
        let (forward, right, up) = self.basis();
        if self.orthographic() {
            let h = self.half_height();
            let at = eye + right * (ndc.x * h * aspect.max(1e-3)) + up * (ndc.y * h)
                - forward * self.behind();
            return (at.as_dvec3(), forward.as_dvec3().normalize_or_zero());
        }
        let tan = (self.fov_y * 0.5).tan();
        let dir = forward + right * (ndc.x * tan * aspect.max(1e-3)) + up * (ndc.y * tan);
        (eye.as_dvec3(), dir.as_dvec3().normalize_or_zero())
    }

    /// Orbit by a drag, in points.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * ROTATION_SPEED;
        self.pitch = (self.pitch + dy * ROTATION_SPEED).clamp(-MAX_ORBIT_PITCH, MAX_ORBIT_PITCH);
    }

    /// Move the target in the screen plane so the dragged point follows the cursor.
    ///
    /// `height` is the viewport height in points, which sets how far one point of
    /// drag is at the target's depth.
    pub fn pan(&mut self, dx: f32, dy: f32, height: f32) {
        if height <= 0.0 {
            return;
        }
        let (_, right, up) = self.basis();
        let scale = self.half_height() * 2.0 / height;
        self.target += (-dx * right + dy * up) * scale;
        // Keep the part reachable: never further than two radii from its centre.
        let offset = self.target - self.centre;
        let limit = self.radius * 2.0;
        if offset.length() > limit {
            self.target = self.centre + offset.normalize_or_zero() * limit;
        }
    }

    /// Zoom by a scroll amount, in points.
    pub fn zoom(&mut self, scroll: f32) {
        let factor = (1.0 - scroll * ZOOM_SPEED).clamp(0.5, 2.0);
        self.distance = (self.distance * factor).clamp(self.radius * 0.05, self.radius * 20.0);
    }
}

/// Where a view's lines of sight come from: what can be seen is what nothing
/// stands in front of along one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sight {
    /// All from the eye, in perspective.
    Eye(DVec3),
    /// Looking orthographically: all the same way, the direction given, each from
    /// the length given back from what it sees, in front of everything drawn.
    Along(DVec3, f64),
}

impl Sight {
    /// Where the line of sight to `p` starts.
    pub fn to(self, p: DVec3) -> DVec3 {
        match self {
            Sight::Eye(eye) => eye,
            Sight::Along(dir, length) => p - dir * length,
        }
    }

    /// How far into the view `p` is: less is nearer.
    pub fn depth(self, p: DVec3) -> f64 {
        match self {
            Sight::Eye(eye) => p.distance(eye),
            Sight::Along(dir, _) => p.dot(dir),
        }
    }
}

/// An angle brought to within half a turn either way of zero.
fn wrap(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn cam() -> Camera {
        Camera::framing(Vec3::new(10.0, 20.0, 5.0), 50.0)
    }

    /// World +Z must point up on screen from any orbit. Getting this wrong would
    /// make every reading of the picture wrong.
    #[test]
    fn z_is_up_on_screen() {
        let mut c = cam();
        for _ in 0..40 {
            let v = c.view();
            let up_on_screen = v.transform_vector3(Vec3::Z);
            assert!(
                up_on_screen.y > 0.0,
                "Z points {up_on_screen:?} in view space"
            );
            c.orbit(37.0, 11.0);
        }
    }

    /// The framing view fits the part: every corner of its bounding cube lands on
    /// screen and in front of the eye.
    #[test]
    fn the_framing_view_shows_the_whole_part() {
        let c = cam();
        let m = c.view_projection(16.0 / 9.0);
        let r = 50.0 / 3f32.sqrt();
        for corner in 0..8 {
            let s = |bit: i32| if corner & bit == 0 { -r } else { r };
            let p = c.centre + Vec3::new(s(1), s(2), s(4));
            let clip = m * p.extend(1.0);
            assert!(clip.w > 0.0, "corner behind the eye");
            let ndc = clip.truncate() / clip.w;
            assert!(
                ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0,
                "corner off screen: {ndc:?}"
            );
            assert!(
                (0.0..=1.0).contains(&ndc.z),
                "corner outside depth range: {ndc:?}"
            );
        }
    }

    #[test]
    fn reset_returns_to_the_framing_view() {
        let mut c = cam();
        c.orbit(200.0, -80.0);
        c.pan(40.0, 12.0, 600.0);
        c.zoom(300.0);
        c.reset();
        assert_eq!(c, cam());
    }

    /// Limits scale with the part, so a 25 mm part and a 400 mm part both zoom and
    /// pan sensibly.
    /// Covering a larger scene lets the view zoom out to it and see all of it,
    /// and never shrinks what it can reach.
    #[test]
    fn covering_a_sheet_reaches_all_of_it() {
        let mut c = Camera::framing(Vec3::ZERO, 60.0);
        c.cover(2000.0);
        c.cover(10.0);
        for _ in 0..200 {
            c.zoom(-1000.0);
        }
        assert!((c.distance - 2000.0 * 20.0).abs() < 1.0, "{}", c.distance);
        assert_eq!(c.radius, 2000.0);
    }

    #[test]
    fn zoom_and_pan_limits_scale_with_the_part() {
        for radius in [12.5_f32, 200.0] {
            let mut c = Camera::framing(Vec3::ZERO, radius);
            for _ in 0..200 {
                c.zoom(1000.0);
            }
            assert!((c.distance - radius * 0.05).abs() < radius * 1e-4);
            for _ in 0..200 {
                c.pan(500.0, 0.0, 100.0);
            }
            assert!(c.target.length() <= radius * 2.0 + 1e-3);
        }
    }

    /// Focusing keeps the angle and puts the point dead centre.
    #[test]
    fn focus_centres_the_point_and_keeps_the_angle() {
        let mut c = cam();
        let (yaw, pitch) = (c.yaw, c.pitch);
        let p = Vec3::new(20.0, 25.0, 0.0);
        c.focus(p, 5.0);
        assert_eq!((c.yaw, c.pitch), (yaw, pitch));
        let clip = c.view_projection(1.5) * p.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        assert!(ndc.x.abs() < 1e-4 && ndc.y.abs() < 1e-4, "{ndc:?}");
    }

    /// A point drawn at some place on the screen lies on the ray through that
    /// place: picking hits what is drawn.
    #[test]
    fn the_ray_through_a_point_on_screen_passes_through_what_is_drawn_there() {
        let mut c = cam();
        let aspect = 1.6;
        for step in 0..12 {
            let m = c.view_projection(aspect);
            for p in [
                Vec3::new(10.0, 20.0, 5.0),
                Vec3::new(30.0, 0.0, -10.0),
                Vec3::new(-12.0, 41.0, 22.0),
            ] {
                let clip = m * p.extend(1.0);
                let ndc = clip.truncate() / clip.w;
                let (o, d) = c.ray(aspect, Vec2::new(ndc.x, ndc.y));
                let to = p.as_dvec3() - o;
                let off = (to - d * to.dot(d)).length();
                assert!(off < 1e-3 * to.length(), "step {step}: {off} off at {p}");
                assert!((d.length() - 1.0).abs() < 1e-9);
            }
            c.orbit(53.0, 17.0);
        }
    }

    /// The six straight views, as the view cube turns to them.
    fn straight() -> [(&'static str, Vec3); 6] {
        [
            ("top", Vec3::Z),
            ("bottom", -Vec3::Z),
            ("front", -Vec3::Y),
            ("back", Vec3::Y),
            ("left", -Vec3::X),
            ("right", Vec3::X),
        ]
    }

    /// Straight on is orthographic, and anywhere else, orbiting away included,
    /// perspective.
    #[test]
    fn straight_views_are_orthographic() {
        for (name, dir) in straight() {
            let c = cam().turned_to(dir);
            assert!(c.orthographic(), "{name}");
            let eye = (c.eye() - c.target).normalize();
            assert!(eye.distance(dir) < 1e-5, "{name}: the eye is along {eye}");
            let mut away = c;
            away.orbit(1.0, 0.0);
            assert!(!away.orthographic(), "{name}, orbited");
            let mut up = c;
            up.orbit(0.0, 1.0);
            assert!(!up.orthographic(), "{name}, orbited up");
        }
        assert!(!cam().orthographic());
        assert!(!cam().turned_to(Vec3::new(1.0, -1.0, 1.0)).orthographic());
        assert!(!cam().turned_to(Vec3::new(1.0, 0.0, 1.0)).orthographic());
    }

    /// Top shows +X to the right and +Y up the screen, the front at the bottom;
    /// Front shows +X to the right and +Z up.
    #[test]
    fn top_and_front_read_as_drawings_do() {
        let top = cam().turned_to(Vec3::Z).view();
        assert!(top.transform_vector3(Vec3::X).x > 0.99);
        assert!(top.transform_vector3(Vec3::Y).y > 0.99);
        let front = cam().turned_to(-Vec3::Y).view();
        assert!(front.transform_vector3(Vec3::X).x > 0.99);
        assert!(front.transform_vector3(Vec3::Z).y > 0.99);
    }

    /// Off the poles the screen's axes are what they always were, so nothing
    /// drawn moves.
    #[test]
    fn the_basis_is_the_look_at_one() {
        let mut c = cam();
        for _ in 0..20 {
            let (forward, right, up) = c.basis();
            let f = (c.target - c.eye()).normalize();
            let r = f.cross(Vec3::Z).normalize();
            assert!(forward.distance(f) < 1e-5);
            assert!(right.distance(r) < 1e-5);
            assert!(up.distance(r.cross(f)) < 1e-5);
            c.orbit(41.0, 13.0);
        }
    }

    /// Switching projection at the end of a turn keeps the part its size: a
    /// point on the target's plane is drawn at the same place in both.
    #[test]
    fn the_target_plane_is_the_same_size_in_both() {
        let ortho = cam().turned_to(-Vec3::Y);
        let mut persp = ortho;
        persp.yaw += 2.0 * STRAIGHT;
        assert!(ortho.orthographic() && !persp.orthographic());
        let p = ortho.target + Vec3::new(20.0, 0.0, 12.0);
        let at = |c: &Camera| {
            let clip = c.view_projection(1.5) * p.extend(1.0);
            clip.truncate() / clip.w
        };
        let (a, b) = (at(&ortho), at(&persp));
        assert!(
            (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3,
            "{a} {b}"
        );
    }

    /// Orthographically, the ray through a point drawn passes through it, runs
    /// the way the view looks, and starts in front of it.
    #[test]
    fn an_orthographic_ray_passes_through_what_is_drawn() {
        let aspect = 1.6;
        for (name, dir) in straight() {
            let c = cam().turned_to(dir);
            let m = c.view_projection(aspect);
            for p in [
                Vec3::new(10.0, 20.0, 5.0),
                Vec3::new(30.0, 0.0, -10.0),
                Vec3::new(-12.0, 41.0, 22.0),
            ] {
                let clip = m * p.extend(1.0);
                assert!((clip.w - 1.0).abs() < 1e-6);
                let ndc = clip.truncate() / clip.w;
                assert!(
                    (0.0..=1.0).contains(&ndc.z),
                    "{name}: {p} at depth {}",
                    ndc.z
                );
                let (o, d) = c.ray(aspect, Vec2::new(ndc.x, ndc.y));
                assert!(d.distance(-dir.as_dvec3()) < 1e-6, "{name}");
                let to = p.as_dvec3() - o;
                let off = (to - d * to.dot(d)).length();
                assert!(off < 1e-3, "{name}: {off} off at {p}");
                assert!(to.dot(d) > 0.0, "{name}: the ray starts past {p}");
                let from = c.sight().to(p.as_dvec3());
                let back = p.as_dvec3() - from;
                assert!(back.normalize().distance(-dir.as_dvec3()) < 1e-6);
                assert!(back.length() >= f64::from(c.distance));
            }
        }
    }

    /// A turn goes the short way round, and ends exactly where it was going.
    #[test]
    fn a_turn_goes_the_short_way_and_arrives() {
        let mut a = cam();
        a.yaw = 3.0;
        let b = a.turned_to(Vec3::new(-1.0, -0.2, 0.0));
        assert!((b.yaw - a.yaw).abs() < PI, "{} to {}", a.yaw, b.yaw);
        let mid = Camera::lerp(&a, &b, 0.5);
        assert!((mid.yaw - a.yaw).abs() < (b.yaw - a.yaw).abs());
        assert_eq!(Camera::lerp(&a, &b, 1.0), b);
        assert_eq!(Camera::lerp(&a, &b, 0.0), a);
        // Turned to a straight view, it arrives straight, so orthographic.
        let top = a.turned_to(Vec3::Z);
        assert!(Camera::lerp(&a, &top, 1.0).orthographic());
        assert!(!Camera::lerp(&a, &top, 0.9).orthographic());
    }

    #[test]
    fn a_degenerate_part_still_gives_a_usable_camera() {
        let c = Camera::framing(Vec3::ZERO, 0.0);
        assert!(c.distance.is_finite() && c.distance > 0.0);
        assert!(c.view_projection(1.0).is_finite());
    }
}
