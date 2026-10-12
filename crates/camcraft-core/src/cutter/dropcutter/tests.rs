use super::*;
use camcraft_geom::synthetic;
use proptest::prelude::*;

const R: f64 = 3.0;

#[test]
fn flat_on_a_plane_is_exact() {
    let mesh = synthetic::plane(2.5, 20.0);
    let dc = DropCutter::new(&mesh);
    let tool = Flat::new(R).unwrap();
    for (x, y) in [(0.0, 0.0), (5.0, -7.0), (-12.0, 3.0)] {
        let z = dc.drop_at(&tool, x, y).unwrap();
        assert!((z - 2.5).abs() < 1e-12, "at ({x},{y}) got {z}, want 2.5");
    }
}

#[test]
fn ball_on_a_plane_is_exact() {
    let mesh = synthetic::plane(-1.25, 20.0);
    let dc = DropCutter::new(&mesh);
    let tool = Ball::new(R).unwrap();
    for (x, y) in [(0.0, 0.0), (4.0, 4.0), (-9.0, 2.0)] {
        let z = dc.drop_at(&tool, x, y).unwrap();
        assert!((z + 1.25).abs() < 1e-12, "at ({x},{y}) got {z}, want -1.25");
    }
}

/// The fixture that a grid-dilation implementation cannot pass: the step must be
/// full height right up to `x = R`, and the floor immediately past it.
#[test]
fn flat_on_a_sharp_step_does_not_round_the_edge() {
    let h = 4.0;
    let mesh = synthetic::step(h, 20.0);
    let dc = DropCutter::new(&mesh);
    let tool = Flat::new(R).unwrap();

    for x in [-10.0, -1.0, 0.0, R - 1e-6] {
        let z = dc.drop_at(&tool, x, 0.0).unwrap();
        assert!(
            (z - h).abs() < 1e-9,
            "at x={x} the disc still overlaps the plateau; got {z}, want {h}"
        );
    }
    for x in [R + 1e-6, R + 0.5, 10.0] {
        let z = dc.drop_at(&tool, x, 0.0).unwrap();
        assert!(
            z.abs() < 1e-9,
            "at x={x} the disc has cleared the plateau; got {z}, want 0"
        );
    }
}

/// The closed form, with the tip-versus-centre offset that the fixture exists to
/// protect. Compared within the tessellation sag, so a genuinely wrong answer —
/// notably one off by a whole ball radius — still fails.
#[test]
fn ball_on_a_sphere_matches_the_closed_form() {
    let (sphere_r, lat, lon) = (10.0, 200, 300);
    let mesh = synthetic::uv_sphere(sphere_r, lat, lon);
    let sag = synthetic::uv_sphere_max_sag(sphere_r, lat, lon);
    let dc = DropCutter::new(&mesh);
    let br = 2.0;
    let tool = Ball::new(br).unwrap();

    for (x, y) in [(0.0, 0.0), (2.0, 0.0), (0.0, -3.0), (4.0, 3.0)] {
        let got = dc.drop_at(&tool, x, y).unwrap();
        let sum = sphere_r + br;
        let want = (sum * sum - x * x - y * y).sqrt() - br;
        assert!(
            (got - want).abs() <= sag + 1e-9,
            "at ({x},{y}) got {got}, want {want}, sag budget {sag}"
        );
        assert!(
            got <= want + 1e-9,
            "a chord mesh may sit below the ideal surface, never above"
        );
    }
}

/// Exercises the facet rim. On a slope a flat mill rests on the *uphill edge* of
/// its disc, `R * |grad|` above the plane beneath its centre. The horizontal
/// fixtures cannot catch a missing rim term, because there the rim and the centre
/// sit at the same height.
#[test]
fn flat_on_an_incline_rests_on_the_uphill_rim() {
    let (a, b, z0) = (0.35, -0.2, 1.5);
    let mesh = synthetic::inclined_plane(a, b, z0, 40.0);
    let dc = DropCutter::new(&mesh);
    let tool = Flat::new(R).unwrap();
    let lift = R * (a * a + b * b).sqrt();
    assert!(lift > 0.1, "fixture must actually be sloped");

    for (x, y) in [(0.0, 0.0), (6.0, -4.0), (-8.0, 5.0)] {
        let got = dc.drop_at(&tool, x, y).unwrap();
        let want = z0 + a * x + b * y + lift;
        assert!(
            (got - want).abs() < 1e-9,
            "at ({x},{y}) got {got}, want {want} (rim lift {lift})"
        );
    }
}

/// The ball's facet contact on a slope: the centre sits `r / n_z` above the
/// plane, so the tip sits `r * (sqrt(a^2 + b^2 + 1) - 1)` above it.
#[test]
fn ball_on_an_incline_matches_the_closed_form() {
    let (a, b, z0) = (0.4, 0.25, -2.0);
    let mesh = synthetic::inclined_plane(a, b, z0, 40.0);
    let dc = DropCutter::new(&mesh);
    let br = 2.5;
    let tool = Ball::new(br).unwrap();
    let lift = br * ((a * a + b * b + 1.0).sqrt() - 1.0);
    assert!(lift > 0.1, "fixture must actually be sloped");

    for (x, y) in [(0.0, 0.0), (5.0, 5.0), (-7.0, 2.0)] {
        let got = dc.drop_at(&tool, x, y).unwrap();
        let want = z0 + a * x + b * y + lift;
        assert!(
            (got - want).abs() < 1e-9,
            "at ({x},{y}) got {got}, want {want} (lift {lift})"
        );
    }
}

#[test]
fn reports_nothing_where_there_is_no_geometry() {
    let mesh = synthetic::plane(0.0, 1.0);
    let dc = DropCutter::new(&mesh);
    assert_eq!(dc.drop_at(&Flat::new(0.5).unwrap(), 50.0, 50.0), None);
}

#[test]
fn survives_degenerate_input() {
    let mesh = TriMesh::new(
        vec![dvec3(0.0, 0.0, 1.0), dvec3(1.0, 0.0, 1.0)],
        vec![[0, 0, 0], [0, 1, 1]],
    )
    .unwrap();
    let dc = DropCutter::new(&mesh);
    // Zero-area triangles must contribute their vertices and nothing worse.
    let z = dc.drop_at(&Flat::new(2.0).unwrap(), 0.5, 0.0).unwrap();
    assert!((z - 1.0).abs() < 1e-12);
}

/// The invariant the whole system exists to hold: the tool never ends up below
/// the surface it is cutting.
///
/// Sampled across whole triangles rather than at vertices alone. A vertex-only
/// check is far too weak — it cannot see the tool slicing through the middle of
/// an edge or a face, which is exactly what a missing edge or facet test does.
fn assert_no_penetration<T: DropTest>(
    mesh: &TriMesh,
    dc: &DropCutter<'_>,
    tool: &T,
    x: f64,
    y: f64,
) -> Result<(), TestCaseError> {
    let Some(z) = dc.drop_at(tool, x, y) else {
        return Ok(());
    };
    let r = tool.max_radius();
    const N: u32 = 8;

    for i in 0..mesh.len() {
        let Some(tri) = mesh.triangle(i) else {
            continue;
        };
        for u in 0..=N {
            for v in 0..=(N - u) {
                let (bu, bv) = (f64::from(u) / f64::from(N), f64::from(v) / f64::from(N));
                let p = tri[0] + (tri[1] - tri[0]) * bu + (tri[2] - tri[0]) * bv;
                let d = (p.truncate() - dvec2(x, y)).length();
                if d > r {
                    continue;
                }
                let Some(h) = tool.height_at(d) else { continue };
                prop_assert!(
                    z + h >= p.z - 1e-7,
                    "tool at z={z} cuts through {p:?}: surface {} above tool {}",
                    p.z,
                    z + h
                );
            }
        }
    }
    Ok(())
}

/// Cross-validation. `Bullnose` is a general, partly numerical implementation;
/// `Flat` and `Ball` are dedicated analytic ones. At `c = 0` and `c = R` they
/// describe the identical solid, so they must produce identical answers. Two
/// independent implementations agreeing is far stronger evidence than either
/// passing its own tests.
fn cross_check<A: DropTest, B: DropTest>(mesh: &TriMesh, a: &A, b: &B, tol: f64) {
    let dc = DropCutter::new(mesh);
    let mut compared = 0;
    for i in -9..=9 {
        for j in -9..=9 {
            let (x, y) = (f64::from(i) * 0.83, f64::from(j) * 0.77);
            match (dc.drop_at(a, x, y), dc.drop_at(b, x, y)) {
                (Some(za), Some(zb)) => {
                    assert!(
                        (za - zb).abs() <= tol,
                        "at ({x},{y}) the two implementations disagree: {za} vs {zb}"
                    );
                    compared += 1;
                }
                (None, None) => {}
                (za, zb) => panic!("at ({x},{y}) coverage differs: {za:?} vs {zb:?}"),
            }
        }
    }
    assert!(compared > 100, "expected real overlap, compared {compared}");
}

#[test]
fn bullnose_with_no_corner_is_a_flat_mill() {
    let tol = 1e-9;
    for mesh in [
        synthetic::step(4.0, 12.0),
        synthetic::inclined_plane(0.3, -0.2, 1.0, 12.0),
        synthetic::uv_sphere(9.0, 40, 60),
    ] {
        cross_check(
            &mesh,
            &Bullnose::new(R, 0.0).unwrap(),
            &Flat::new(R).unwrap(),
            tol,
        );
    }
}

#[test]
fn bullnose_with_a_full_corner_is_a_ball_mill() {
    let tol = 1e-7;
    for mesh in [
        synthetic::step(4.0, 12.0),
        synthetic::inclined_plane(0.3, -0.2, 1.0, 12.0),
        synthetic::uv_sphere(9.0, 40, 60),
    ] {
        cross_check(
            &mesh,
            &Bullnose::new(R, R).unwrap(),
            &Ball::new(R).unwrap(),
            tol,
        );
    }
}

#[test]
fn bullnose_on_a_plane_is_exact() {
    let mesh = synthetic::plane(1.75, 20.0);
    let dc = DropCutter::new(&mesh);
    let tool = Bullnose::new(4.0, 1.5).unwrap();
    for (x, y) in [(0.0, 0.0), (5.0, -6.0)] {
        let z = dc.drop_at(&tool, x, y).unwrap();
        assert!((z - 1.75).abs() < 1e-9, "at ({x},{y}) got {z}");
    }
}

/// On a slope the tool rests on its corner torus, between the flat mill's rim
/// lift and the ball's. Bracketing it is enough to show the corner is modelled
/// rather than ignored in either direction.
#[test]
fn bullnose_on_a_slope_sits_between_flat_and_ball() {
    let (a, b, z0) = (0.45, 0.3, 0.0);
    let mesh = synthetic::inclined_plane(a, b, z0, 40.0);
    let dc = DropCutter::new(&mesh);
    let (x, y) = (2.0, -1.0);

    let flat = dc.drop_at(&Flat::new(R).unwrap(), x, y).unwrap();
    let ball = dc.drop_at(&Ball::new(R).unwrap(), x, y).unwrap();
    let bull = dc.drop_at(&Bullnose::new(R, 1.0).unwrap(), x, y).unwrap();

    assert!(
        ball < flat,
        "a ball must sit lower than a flat mill on a slope"
    );
    assert!(
        bull > ball && bull < flat,
        "bullnose {bull} should lie strictly between ball {ball} and flat {flat}"
    );
}

fn vbit(angle: f64, tip: f64) -> Cone {
    Cone::new(R, tip, (angle / 2.0).to_radians()).unwrap()
}

#[test]
fn cone_on_a_plane_is_exact() {
    let mesh = synthetic::plane(1.25, 20.0);
    let dc = DropCutter::new(&mesh);
    for tool in [vbit(90.0, 0.0), vbit(60.0, 0.4)] {
        let z = dc.drop_at(&tool, 3.0, -2.0).unwrap();
        assert!((z - 1.25).abs() < 1e-12, "got {z}");
    }
}

/// On a slope `s` a cone rests on its tip's rim when the plane is shallower
/// than the flank, at `plane + s * tip`, and on its full radius when steeper,
/// at `plane + s * R - (R - tip) * k`.
#[test]
fn cone_on_an_incline_matches_the_closed_form() {
    let (x, y) = (1.5, -2.0);
    for (a, b) in [(0.2, 0.1), (1.6, 0.9)] {
        let mesh = synthetic::inclined_plane(a, b, 0.5, 40.0);
        let dc = DropCutter::new(&mesh);
        let plane = 0.5 + a * x + b * y;
        let s = f64::hypot(a, b);
        for tool in [vbit(90.0, 0.5), vbit(60.0, 0.0), vbit(120.0, 1.0)] {
            let (tip, k) = (tool.tip(), tool.rise());
            let want = if s < k {
                plane + s * tip
            } else {
                plane + s * R - (R - tip) * k
            };
            let z = dc.drop_at(&tool, x, y).unwrap();
            assert!(
                (z - want).abs() < 1e-9,
                "slope {s}, k {k}: got {z}, want {want}"
            );
        }
    }
}

/// Beside a step, a V-bit's flank meets the edge: the tip hangs `h(d)` below
/// the top, `d` out from the wall, and sits on the floor once past its reach.
#[test]
fn a_vbit_meets_a_step_edge_with_its_flank() {
    let mesh = synthetic::step(4.0, 12.0);
    let dc = DropCutter::new(&mesh);
    let tool = vbit(90.0, 0.0);
    for d in [0.5, 1.0, 2.5] {
        let z = dc.drop_at(&tool, d, 0.0).unwrap();
        assert!((z - (4.0 - d)).abs() < 1e-9, "at {d} got {z}");
    }
    let z = dc.drop_at(&tool, R + 0.5, 0.0).unwrap();
    assert!(z.abs() < 1e-9, "past its reach got {z}");
}

/// A cone whose tip is its whole width is a flat mill, and must answer as one.
#[test]
fn a_cone_all_tip_is_a_flat_mill() {
    let near_flat = Cone::new(R, R * (1.0 - 1e-12), 0.7).unwrap();
    for mesh in [
        synthetic::step(4.0, 12.0),
        synthetic::inclined_plane(0.3, -0.2, 1.0, 12.0),
        synthetic::uv_sphere(9.0, 40, 60),
    ] {
        cross_check(&mesh, &near_flat, &Flat::new(R).unwrap(), 1e-6);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn cone_never_penetrates_the_sphere(
        x in -8.0f64..8.0,
        y in -8.0f64..8.0,
        angle in 20.0f64..160.0,
        tip in 0.0f64..2.0,
    ) {
        let mesh = synthetic::uv_sphere(10.0, 40, 60);
        let dc = DropCutter::new(&mesh);
        assert_no_penetration(&mesh, &dc, &vbit(angle, tip), x, y)?;
    }

    #[test]
    fn cone_never_penetrates_the_step(
        x in -8.0f64..8.0,
        y in -8.0f64..8.0,
        angle in 20.0f64..160.0,
        tip in 0.0f64..2.0,
    ) {
        let mesh = synthetic::step(4.0, 10.0);
        let dc = DropCutter::new(&mesh);
        assert_no_penetration(&mesh, &dc, &vbit(angle, tip), x, y)?;
    }

    #[test]
    fn never_penetrates_the_sphere(x in -8.0f64..8.0, y in -8.0f64..8.0, br in 0.5f64..4.0) {
        let mesh = synthetic::uv_sphere(10.0, 40, 60);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(br).unwrap();
        assert_no_penetration(&mesh, &dc, &tool, x, y)?;
    }

    #[test]
    fn never_penetrates_the_step(x in -8.0f64..8.0, y in -8.0f64..8.0, fr in 0.5f64..4.0) {
        let mesh = synthetic::step(4.0, 10.0);
        let dc = DropCutter::new(&mesh);
        let tool = Flat::new(fr).unwrap();
        assert_no_penetration(&mesh, &dc, &tool, x, y)?;
    }

    #[test]
    fn bullnose_never_penetrates(x in -8.0f64..8.0, y in -8.0f64..8.0, c in 0.0f64..2.0) {
        let mesh = synthetic::uv_sphere(10.0, 40, 60);
        let dc = DropCutter::new(&mesh);
        let tool = Bullnose::new(2.5, c).unwrap();
        assert_no_penetration(&mesh, &dc, &tool, x, y)?;
    }

    /// A larger tool cannot reach deeper than a smaller one.
    #[test]
    fn monotonic_in_tool_radius(x in -8.0f64..8.0, y in -8.0f64..8.0) {
        let mesh = synthetic::uv_sphere(10.0, 40, 60);
        let dc = DropCutter::new(&mesh);
        let small = dc.drop_at(&Ball::new(1.0).unwrap(), x, y);
        let large = dc.drop_at(&Ball::new(3.0).unwrap(), x, y);
        if let (Some(s), Some(l)) = (small, large) {
            prop_assert!(l >= s - 1e-7, "radius 3 gave {l}, radius 1 gave {s}");
        }
    }

    /// Contact is a maximum over candidates, so visiting order cannot matter.
    #[test]
    fn is_deterministic(x in -8.0f64..8.0, y in -8.0f64..8.0) {
        let mesh = synthetic::uv_sphere(10.0, 30, 40);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(2.0).unwrap();
        let first = dc.drop_at(&tool, x, y);
        for _ in 0..4 {
            prop_assert_eq!(dc.drop_at(&tool, x, y), first);
        }
    }
}
