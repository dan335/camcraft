use super::*;
use crate::cutter::dropcutter::DropCutter;
use crate::cutter::tool::{Ball, Flat};
use camcraft_geom::synthetic;

#[test]
fn rejects_unusable_stock() {
    assert!(
        Allowance::new(-0.1, 0.0).is_ok(),
        "negative cuts past the part"
    );
    assert!(matches!(
        Allowance::new(0.0, f64::NAN),
        Err(AllowanceError::NotANumber { .. })
    ));
    assert!(matches!(
        Allowance::new(f64::INFINITY, 0.0),
        Err(AllowanceError::NotANumber { .. })
    ));
    assert!(Allowance::new(0.0, 0.0).is_ok());
    assert!(Allowance::none().is_none());
}

/// A flat mill leaving stock becomes a bullnose. Getting this wrong -- treating
/// stock as the same tool raised -- would cut through the corner it exists to
/// protect.
#[test]
fn isotropic_stock_rounds_a_flat_mill() {
    let e =
        EffectiveTool::new(&Flat::new(5.0).unwrap(), Allowance::isotropic(0.5).unwrap()).unwrap();
    assert_eq!(e.shape().max_radius(), 5.5);
    assert_eq!(e.shape().bullnose().unwrap().corner(), 0.5);
    assert_eq!(e.tip_offset(), 0.5);
}

#[test]
fn isotropic_stock_keeps_a_ball_a_ball() {
    let e =
        EffectiveTool::new(&Ball::new(3.0).unwrap(), Allowance::isotropic(0.5).unwrap()).unwrap();
    // corner == radius is exactly a ball.
    assert_eq!(e.shape().max_radius(), 3.5);
    assert_eq!(e.shape().bullnose().unwrap().corner(), 3.5);
}

#[test]
fn pure_axial_widens_nothing_and_pure_radial_raises_nothing() {
    let flat = Flat::new(4.0).unwrap();

    let axial = EffectiveTool::new(&flat, Allowance::new(0.0, 0.7).unwrap()).unwrap();
    assert_eq!(
        axial.shape().max_radius(),
        4.0,
        "axial stock must not widen"
    );
    assert_eq!(
        axial.shape().bullnose().unwrap().corner(),
        0.0,
        "axial stock must not round"
    );
    assert_eq!(axial.tip_offset(), 0.7);

    let radial = EffectiveTool::new(&flat, Allowance::new(0.7, 0.0).unwrap()).unwrap();
    assert_eq!(radial.shape().max_radius(), 4.7, "radial stock must widen");
    assert_eq!(radial.tip_offset(), 0.0, "radial stock must not raise");
}

/// Zero stock must be indistinguishable from the bare cutter, or the seam has
/// introduced an error at the very point it should be transparent.
#[test]
fn zero_stock_matches_the_bare_tool_exactly() {
    let mesh = synthetic::uv_sphere(9.0, 40, 60);
    let dc = DropCutter::new(&mesh);
    let ball = Ball::new(2.0).unwrap();
    let effective = EffectiveTool::new(&ball, Allowance::none()).unwrap();

    let mut compared = 0;
    for i in -7..=7 {
        for j in -7..=7 {
            let (x, y) = (f64::from(i) * 0.9, f64::from(j) * 0.8);
            match (dc.drop_at(&ball, x, y), dc.drop_at(&effective, x, y)) {
                (Some(a), Some(b)) => {
                    assert!((a - b).abs() < 1e-7, "at ({x},{y}) {a} vs {b}");
                    compared += 1;
                }
                (None, None) => {}
                (a, b) => panic!("coverage differs at ({x},{y}): {a:?} vs {b:?}"),
            }
        }
    }
    assert!(compared > 80, "expected real overlap, compared {compared}");
}

/// On a level floor the tool must end up exactly the axial stock above it,
/// whatever the radial amount does.
#[test]
fn leaves_exactly_the_axial_stock_on_a_floor() {
    let mesh = synthetic::plane(2.0, 30.0);
    let dc = DropCutter::new(&mesh);
    for (radial, axial) in [(0.0, 0.5), (0.5, 0.5), (1.0, 0.25), (0.8, 0.0)] {
        let stock = Allowance::new(radial, axial).unwrap();
        let tool = EffectiveTool::new(&Flat::new(4.0).unwrap(), stock).unwrap();
        let z = dc.drop_at(&tool, 0.0, 0.0).unwrap();
        assert!(
            (z - (2.0 + axial)).abs() < 1e-9,
            "radial {radial}, axial {axial}: got {z}, want {}",
            2.0 + axial
        );
    }
}

/// Negative stock shrinks a ball to a smaller ball and puts its tip that far
/// below the contact: the same sum, taken away.
#[test]
fn negative_stock_shrinks_a_ball_to_a_ball() {
    let e = EffectiveTool::new(
        &Ball::new(3.0).unwrap(),
        Allowance::isotropic(-0.5).unwrap(),
    )
    .unwrap();
    assert_eq!(e.shape().max_radius(), 2.5);
    assert_eq!(e.shape().bullnose().unwrap().corner(), 2.5);
    assert_eq!(e.tip_offset(), -0.5);
    assert_eq!(e.cutter_radius(), 3.0, "the real tool is unchanged");
}

/// A flat mill's corner is already sharp: shrunk, it stays flat, and cuts a
/// floor exactly the axial amount below it.
#[test]
fn negative_stock_keeps_a_flat_mill_flat() {
    let stock = Allowance::isotropic(-0.3).unwrap();
    let e = EffectiveTool::new(&Flat::new(4.0).unwrap(), stock).unwrap();
    assert_eq!(e.shape().max_radius(), 3.7);
    assert_eq!(e.shape().bullnose().unwrap().corner(), 0.0);
    let mesh = synthetic::plane(2.0, 30.0);
    let z = DropCutter::new(&mesh).drop_at(&e, 0.0, 0.0).unwrap();
    assert!((z - 1.7).abs() < 1e-9, "got {z}");
}

/// A tool cannot shrink past nothing beside.
#[test]
fn negative_stock_cannot_take_the_whole_radius() {
    let flat = Flat::new(2.0).unwrap();
    assert!(EffectiveTool::new(&flat, Allowance::new(-2.0, 0.0).unwrap()).is_err());
    assert!(EffectiveTool::new(&flat, Allowance::new(-1.9, 0.0).unwrap()).is_ok());
}

/// The material is cut by the real tool at the points the grown one found,
/// so the stock left on a wall stays in the material.
#[test]
fn the_real_tool_is_kept() {
    let ball = Ball::new(3.0).unwrap();
    let e = EffectiveTool::new(&ball, Allowance::isotropic(0.5).unwrap()).unwrap();
    assert_eq!(e.base().max_radius(), 3.0);
    assert_eq!(e.base().bullnose().unwrap().corner(), 3.0);
}

/// More stock can never put the tool lower. Monotonicity is the cheap sanity
/// check that catches a sign error anywhere in the dilation.
#[test]
fn more_stock_never_cuts_deeper() {
    let mesh = synthetic::uv_sphere(10.0, 50, 70);
    let dc = DropCutter::new(&mesh);
    let base = Ball::new(2.0).unwrap();

    for (x, y) in [(0.0, 0.0), (3.0, 2.0), (-5.0, 1.0), (6.0, -4.0)] {
        let mut previous = f64::NEG_INFINITY;
        for step in 0..5 {
            let s = f64::from(step) * 0.25;
            let tool = EffectiveTool::new(&base, Allowance::isotropic(s).unwrap()).unwrap();
            if let Some(z) = dc.drop_at(&tool, x, y) {
                assert!(
                    z >= previous - 1e-9,
                    "at ({x},{y}) stock {s} gave {z}, below the previous {previous}"
                );
                previous = z;
            }
        }
    }
}
