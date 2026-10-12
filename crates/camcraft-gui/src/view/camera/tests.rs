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

/// Limits scale with the part, so a 25 mm part and a 400 mm part both zoom and
/// pan sensibly.
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

/// Where a point is drawn, and the ray through that place, agree.
#[test]
fn the_projector_and_the_ray_agree() {
    use eframe::egui::vec2;
    let mut c = Camera::framing(Vec3::new(0.0, 0.0, 2.5), 40.0);
    c.pitch = 1.5;
    c.yaw = -FRAC_PI_2;
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
    let aspect = 800.0 / 600.0;
    let proj = Projector::new(c.view_projection(aspect), rect);
    for p in [DVec3::new(10.0, 10.0, 5.0), DVec3::new(-15.0, 3.0, 0.0)] {
        let s = proj.screen(p).unwrap();
        let (o, d) = c.ray(aspect, proj.ndc(s));
        let to = p - o;
        assert!((to - d * to.dot(d)).length() < 1e-3 * to.length());
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
