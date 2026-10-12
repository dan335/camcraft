use super::*;
use approx::assert_abs_diff_eq;
use proptest::prelude::*;

const EPS: f64 = 1e-9;

#[test]
fn rejects_unphysical_radii() {
    assert_eq!(Flat::new(0.0), Err(ToolError::BadRadius(0.0)));
    assert_eq!(Ball::new(-1.0), Err(ToolError::BadRadius(-1.0)));
    assert!(matches!(Ball::new(f64::NAN), Err(ToolError::BadRadius(_))));
    assert!(Flat::new(3.175).is_ok());
}

#[test]
fn flat_is_a_cylinder() {
    let t = Flat::new(3.0).unwrap();
    assert_eq!(t.radius_at(0.0), 3.0);
    assert_eq!(t.radius_at(50.0), 3.0);
    assert_eq!(t.radius_at(-1.0), 0.0);
    // The entire cutting face lies in one plane.
    assert_eq!(t.height_at(0.0), Some(0.0));
    assert_eq!(t.height_at(3.0), Some(0.0));
    assert_eq!(t.height_at(3.000_001), None);
}

#[test]
fn ball_matches_closed_form() {
    let r = 3.0;
    let t = Ball::new(r).unwrap();

    // Tip.
    assert_abs_diff_eq!(t.radius_at(0.0), 0.0, epsilon = EPS);
    assert_abs_diff_eq!(t.height_at(0.0).unwrap(), 0.0, epsilon = EPS);

    // Equator: at z = R the ball reaches full radius.
    assert_abs_diff_eq!(t.radius_at(r), r, epsilon = EPS);
    assert_abs_diff_eq!(t.height_at(r).unwrap(), r, epsilon = EPS);

    // 45 degrees around the ball: d = R/sqrt(2) sits at z = R - R/sqrt(2).
    let d = r / 2.0_f64.sqrt();
    assert_abs_diff_eq!(t.height_at(d).unwrap(), r - d, epsilon = EPS);

    // Above the ball it is a plain shank.
    assert_abs_diff_eq!(t.radius_at(10.0), r, epsilon = EPS);
}

/// The duality the whole drop-cutter rests on.
fn check_inverse<T: ToolProfile>(t: &T, d: f64) -> Result<(), TestCaseError> {
    let Some(z) = t.height_at(d) else {
        return Ok(());
    };
    prop_assert!(z >= 0.0, "height must be at or above the tip, got {z}");
    // h(d) = inf { z : r(z) >= d }, so at h(d) the tool has reached radius d...
    prop_assert!(
        t.radius_at(z) >= d - 1e-7,
        "r(h(d)) = {} should be >= d = {d}",
        t.radius_at(z)
    );
    // ...and just below it, it has not.
    if z > 1e-6 {
        prop_assert!(
            t.radius_at(z - 1e-6) <= d + 1e-7,
            "r just below h(d) should not yet have reached d"
        );
    }
    Ok(())
}

proptest! {
    #[test]
    fn ball_inverse_holds(r in 0.1f64..25.0, frac in 0.0f64..=1.0) {
        let t = Ball::new(r).unwrap();
        check_inverse(&t, frac * r)?;
    }

    #[test]
    fn flat_inverse_holds(r in 0.1f64..25.0, frac in 0.0f64..=1.0) {
        let t = Flat::new(r).unwrap();
        check_inverse(&t, frac * r)?;
    }

    /// `r(z)` must be monotonically non-decreasing for flat and ball mills. That is
    /// what makes `height_at` well defined, and it is exactly the assumption an
    /// undercutting tool will later break.
    #[test]
    fn radius_is_monotonic(r in 0.1f64..25.0, a in 0.0f64..50.0, b in 0.0f64..50.0) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let ball = Ball::new(r).unwrap();
        let flat = Flat::new(r).unwrap();
        prop_assert!(ball.radius_at(lo) <= ball.radius_at(hi) + 1e-12);
        prop_assert!(flat.radius_at(lo) <= flat.radius_at(hi) + 1e-12);
        prop_assert!(ball.is_monotonic() && flat.is_monotonic());
    }

    /// Nothing may exceed the declared maximum radius; the drop-cutter bounds
    /// its search footprint with it, so an understated value means gouges.
    #[test]
    fn never_exceeds_max_radius(r in 0.1f64..25.0, z in -5.0f64..100.0) {
        let ball = Ball::new(r).unwrap();
        let flat = Flat::new(r).unwrap();
        prop_assert!(ball.radius_at(z) <= ball.max_radius() + 1e-12);
        prop_assert!(flat.radius_at(z) <= flat.max_radius() + 1e-12);
    }

    #[test]
    fn cone_inverse_holds(
        r in 0.1f64..25.0,
        tip in 0.0f64..0.9,
        half in 0.05f64..1.5,
        frac in 0.0f64..=1.0,
    ) {
        let t = Cone::new(r, tip * r, half).unwrap();
        check_inverse(&t, frac * r)?;
    }

    #[test]
    fn cone_is_monotonic_and_bounded(
        r in 0.1f64..25.0,
        tip in 0.0f64..0.9,
        half in 0.05f64..1.5,
        a in -5.0f64..100.0,
        b in -5.0f64..100.0,
    ) {
        let t = Cone::new(r, tip * r, half).unwrap();
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        prop_assert!(t.radius_at(lo) <= t.radius_at(hi) + 1e-12);
        prop_assert!(t.radius_at(hi) <= t.max_radius() + 1e-12);
    }
}

/// A 90° V-bit rises one unit for each unit out; a 60° one, √3.
#[test]
fn a_cone_matches_its_angle() {
    let v90 = Cone::new(3.0, 0.0, 45f64.to_radians()).unwrap();
    assert_abs_diff_eq!(v90.height_at(2.0).unwrap(), 2.0, epsilon = EPS);
    assert_abs_diff_eq!(v90.flank_height(), 3.0, epsilon = EPS);
    assert_abs_diff_eq!(v90.radius_at(1.5), 1.5, epsilon = EPS);
    assert_abs_diff_eq!(v90.radius_at(10.0), 3.0, epsilon = EPS);
    let v60 = Cone::new(3.0, 0.5, 30f64.to_radians()).unwrap();
    assert_eq!(v60.height_at(0.5), Some(0.0));
    assert_abs_diff_eq!(v60.height_at(1.5).unwrap(), 3f64.sqrt(), epsilon = EPS);
    assert_eq!(v60.height_at(3.01), None);
}

#[test]
fn a_cone_must_be_a_cone() {
    assert!(matches!(
        Cone::new(3.0, 0.0, 0.0),
        Err(ToolError::BadAngle(_))
    ));
    assert!(matches!(
        Cone::new(3.0, 0.0, std::f64::consts::FRAC_PI_2),
        Err(ToolError::BadAngle(_))
    ));
    assert!(matches!(
        Cone::new(3.0, 3.0, 0.5),
        Err(ToolError::BadTip { .. })
    ));
    assert!(matches!(
        Cone::new(0.0, 0.0, 0.5),
        Err(ToolError::BadRadius(_))
    ));
}
