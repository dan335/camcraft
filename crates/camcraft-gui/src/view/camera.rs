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

use eframe::egui::{Pos2, Rect, pos2};
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

/// Where points in the setup's frame are drawn on the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projector {
    view_proj: Mat4,
    rect: Rect,
}

impl Projector {
    /// For a view drawn with `view_proj` into `rect`.
    pub fn new(view_proj: Mat4, rect: Rect) -> Self {
        Self { view_proj, rect }
    }

    /// Where `p` is drawn, or `None` if it is behind the eye.
    pub fn screen(&self, p: DVec3) -> Option<Pos2> {
        let clip = self.view_proj * p.as_vec3().extend(1.0);
        if clip.w <= 1e-6 {
            return None;
        }
        let (x, y) = (clip.x / clip.w, clip.y / clip.w);
        let c = self.rect.center();
        Some(pos2(
            c.x + x * self.rect.width() * 0.5,
            c.y - y * self.rect.height() * 0.5,
        ))
    }

    /// The point on the screen as the camera measures it: x right and y up, -1 to
    /// 1 across the view.
    pub fn ndc(&self, p: Pos2) -> Vec2 {
        let c = self.rect.center();
        Vec2::new(
            (p.x - c.x) / (self.rect.width() * 0.5).max(1e-3),
            (c.y - p.y) / (self.rect.height() * 0.5).max(1e-3),
        )
    }
}

/// An angle brought to within half a turn either way of zero.
fn wrap(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::unwrap_used)]
mod tests;
