//! An orbit camera for a Z-up world measured in millimetres.
//!
//! The design is nodemangler's arcball camera (`view_window/arcball_camera.rs`):
//! yaw and pitch about a target, a distance, and a pan that keeps the dragged
//! point under the cursor. Two things differ, and both matter here. The world is
//! **Z-up**, which is the engine's convention, so reading the picture any other way
//! would make every reading of it wrong. And a part may be 25 mm or 400 mm across,
//! so the zoom and pan limits scale with the part rather than being fixed numbers.

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{DVec3, Mat4, Vec2, Vec3};

const ROTATION_SPEED: f32 = 0.005;
const ZOOM_SPEED: f32 = 0.0015;
/// Just short of straight down or up, where yaw would lose its meaning.
const MAX_PITCH: f32 = 1.55;

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

    /// World to view, with +Z up.
    pub fn view(&self) -> Mat4 {
        look_at_mat4(self.eye(), self.target, Vec3::Z)
    }

    /// View to clip, with depth from 0 to 1 as wgpu expects (the DirectX convention).
    pub fn projection(&self, aspect: f32) -> Mat4 {
        // Near and far follow the distance, so the part never clips however far
        // in or out the view is zoomed.
        let near = (self.distance * 0.01).max(1e-3);
        let far = self.distance + self.radius * 4.0;
        directx::perspective(self.fov_y, aspect.max(1e-3), near, far)
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
    pub fn ray(&self, aspect: f32, ndc: Vec2) -> (DVec3, DVec3) {
        let eye = self.eye();
        let forward = (self.target - eye).normalize_or_zero();
        let right = forward.cross(Vec3::Z).normalize_or_zero();
        let up = right.cross(forward);
        let tan = (self.fov_y * 0.5).tan();
        let dir = forward + right * (ndc.x * tan * aspect.max(1e-3)) + up * (ndc.y * tan);
        (eye.as_dvec3(), dir.as_dvec3().normalize_or_zero())
    }

    /// Orbit by a drag, in points.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * ROTATION_SPEED;
        self.pitch = (self.pitch + dy * ROTATION_SPEED).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Move the target in the screen plane so the dragged point follows the cursor.
    ///
    /// `height` is the viewport height in points, which sets how far one point of
    /// drag is at the target's depth.
    pub fn pan(&mut self, dx: f32, dy: f32, height: f32) {
        if height <= 0.0 {
            return;
        }
        let forward = (self.target - self.eye()).normalize_or_zero();
        let right = forward.cross(Vec3::Z).normalize_or_zero();
        let up = right.cross(forward);
        let scale = self.distance * (self.fov_y * 0.5).tan() * 2.0 / height;
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

    #[test]
    fn a_degenerate_part_still_gives_a_usable_camera() {
        let c = Camera::framing(Vec3::ZERO, 0.0);
        assert!(c.distance.is_finite() && c.distance > 0.0);
        assert!(c.view_projection(1.0).is_finite());
    }
}
