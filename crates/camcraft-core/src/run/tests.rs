use super::pitch::{CELLS_PER_TOOL, FINEST_PITCH, MAX_SIM_CELLS};
use super::*;
use crate::fixture::JOB;
use crate::job::{Boundary, Height, HeightRef, Outline, StrategySpec};
use crate::setup::PrepareError;
use camcraft_geom::synthetic;
use camcraft_geom::topology::Body;
use glam::{DVec2, DVec3, dvec2};

/// A small stock is simulated finely, a large one fine enough for its
/// narrowest tool, and nothing beyond the memory cap.
#[test]
fn the_pitch_follows_the_stock_and_its_tools() {
    let inch = 25.4;
    let sheet = dvec2(5.0 * 12.0 * inch, 12.0 * 12.0 * inch);
    // The lego brick's block: about 0.053 mm, two million cells, whatever the tool.
    let lego = sim_pitch(0.05, dvec2(104.6, 53.8), Some(inch / 4.0));
    assert!((lego - 0.053).abs() < 0.001, "{lego}");
    // A tiny part stops at the finest pitch.
    assert_eq!(sim_pitch(0.05, dvec2(10.0, 10.0), None), FINEST_PITCH);
    // A full sheet with a quarter-inch cutter: eight tolerances.
    assert_eq!(sim_pitch(0.05, sheet, Some(inch / 4.0)), 0.4);
    // With a sixteenth, as fine as memory allows.
    let fine = sim_pitch(0.05, sheet, Some(inch / 16.0));
    assert!(fine < 0.4 && fine > inch / 16.0 / CELLS_PER_TOOL, "{fine}");
    assert!(((sheet.x * sheet.y) / (fine * fine) - MAX_SIM_CELLS).abs() < 1.0);
    // A thirty-second on a smaller stock gets its eight cells across.
    let small = sim_pitch(0.05, dvec2(300.0, 300.0), Some(inch / 32.0));
    assert!(
        (small - inch / 32.0 / CELLS_PER_TOOL).abs() < 1e-12,
        "{small}"
    );
    // Nothing sensible to go on: eight tolerances.
    assert_eq!(sim_pitch(0.05, dvec2(f64::NAN, 1.0), None), 0.4);
    for (size, tool) in [(sheet, Some(1.0)), (dvec2(900.0, 900.0), Some(0.5))] {
        let p = sim_pitch(0.05, size, tool);
        assert!(size.x * size.y / (p * p) <= MAX_SIM_CELLS * 1.0001);
    }
}

fn dome() -> camcraft_geom::mesh::TriMesh {
    synthetic::uv_sphere(8.0, 100, 140)
}

fn run_job(job: &Job, mesh: &camcraft_geom::mesh::TriMesh) -> Result<SetupResult, RunError> {
    let body = Body::derived(mesh.clone());
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    run_setup(job, 0, &bodies)
}

#[test]
fn runs_both_operations_in_order() {
    let job: Job = toml::from_str(JOB).unwrap();
    let result = run_job(&job, &dome()).unwrap();

    assert_eq!(result.operations.len(), 2);
    let names: Vec<&str> = result.operations.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, vec!["rough", "finish"], "order must be preserved");
    for o in &result.operations {
        assert!(
            !o.path.has_unplanned_links(),
            "{} is not machinable",
            o.name
        );
        assert!(o.passes > 0, "{} produced no passes", o.name);
    }
}

/// Whatever looks at a result sees the run's own reports, not a recomputation.
/// A plunge at cutting feed is refused like a rapid through material, not
/// merely warned about. The strategy breaks passes before it gets here; this is
/// the backstop, so it is tested directly.
#[test]
fn a_descent_past_the_allowance_fails_the_operation() {
    let job: Job = toml::from_str(JOB).unwrap();
    let result = run_job(&job, &dome()).unwrap();
    let mut finish = result.operations[1].clone();
    assert!(finish.is_clean(), "{:?}", finish.warnings);
    finish.deepest_cut_descent_mm = finish.max_cut_step + 0.01;
    assert!(!finish.is_clean());
}

#[test]
fn results_carry_what_the_run_reported() {
    let job: Job = toml::from_str(JOB).unwrap();
    let result = run_job(&job, &dome()).unwrap();
    let [rough, finish] = result.operations.as_slice() else {
        panic!("expected two operations");
    };

    let StrategyDetail::PocketClearing(r) = rough.detail else {
        panic!("rough reported {:?}", rough.detail);
    };
    assert_eq!(r.rings, rough.passes);
    let StrategyDetail::Parallel(p) = finish.detail else {
        panic!("finish reported {:?}", finish.detail);
    };
    assert_eq!(p.passes, finish.passes);

    // Clearance resolved against this job's block: stock-top plus 6.
    assert!((result.heights.stock_top - (8.0 + 2.0)).abs() < 1e-6);
    for o in &result.operations {
        assert!((o.clearance_z - (result.heights.stock_top + 6.0)).abs() < 1e-9);
        assert!(o.links.links_resolved > 0, "{} resolved no links", o.name);
    }
    assert!(result.stock_min.x < -8.0 && result.stock_max.x > 8.0);
}

/// The point of threading: the second operation sees what the first removed.
#[test]
fn roughing_removes_material_before_finishing_runs() {
    let job: Job = toml::from_str(JOB).unwrap();
    let result = run_job(&job, &dome()).unwrap();
    let rough = result.operations.first().unwrap();
    assert!(
        rough.sim.removed_mm3 > 0.0,
        "roughing should remove material"
    );
}

/// Every operation is simulated as it runs, and nothing may traverse material.
#[test]
fn no_operation_rapids_through_stock() {
    let job: Job = toml::from_str(JOB).unwrap();
    let result = run_job(&job, &dome()).unwrap();
    for o in &result.operations {
        assert!(
            o.sim.is_clean(),
            "{} put {} rapid(s) through material, deepest {:.3} mm",
            o.name,
            o.sim.rapid_contacts,
            o.sim.deepest_rapid_contact_mm
        );
    }
}

/// The whole job must land on the part.
#[test]
fn the_finished_result_is_the_part() {
    let job: Job = toml::from_str(JOB).unwrap();
    let result = run_job(&job, &dome()).unwrap();
    assert!(result.verify.compared > 500);
    assert!(
        result.is_clean(),
        "job did not verify: {:?}",
        result.warnings()
    );
}

/// A V-bit finishes the dome after roughing: its path runs, sweeps through the
/// simulator as a cone, and the result verifies against the part.
#[test]
fn a_vbit_finishes_and_verifies() {
    let vbit = JOB.replace(
        "[tools.finish]\nkind = \"ball\"\ndiameter = 4.0\n",
        "[tools.finish]\nkind = \"cone\"\ndiameter = 6.0\nangle = 90.0\n",
    );
    assert_ne!(vbit, JOB, "the edit must land");
    let job: Job = toml::from_str(&vbit).unwrap();
    let result = run_job(&job, &dome()).unwrap();
    let finish = &result.operations[1];
    assert!(finish.passes > 0);
    assert!(finish.sim.is_clean(), "{:?}", finish.warnings);
    assert!(
        result.is_clean(),
        "job did not verify: {:?}",
        result.warnings()
    );
}

/// A validation failure must stop the job before anything is computed.
#[test]
fn an_invalid_job_never_starts() {
    let bad = JOB.replace("stepdown = 2.5", "stepdown = 0.0");
    let job: Job = toml::from_str(&bad).unwrap();
    assert!(matches!(run_job(&job, &dome()), Err(RunError::Job(_))));
}

#[test]
fn an_empty_model_is_refused() {
    let job: Job = toml::from_str(JOB).unwrap();
    let empty = camcraft_geom::mesh::TriMesh::default();
    assert!(matches!(
        run_job(&job, &empty),
        Err(RunError::Prepare(PrepareError::EmptyModel(_)))
    ));
}

/// Turned over, the setup machines the other side: a dome standing 11 mm up
/// and hanging 5 mm down shows its 5 mm side on top, and still verifies.
#[test]
fn a_flipped_setup_machines_the_underside() {
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.setups[0].orientation = crate::setup::frame::Orientation::default().flipped();
    let dome = dome().mapped(|p| p + glam::dvec3(0.0, 0.0, 3.0)).unwrap();
    let result = run_job(&job, &dome).unwrap();
    assert!((result.heights.model_top - 5.0).abs() < 1e-6);
    assert!((result.heights.model_bottom + 11.0).abs() < 1e-6);
    assert!(result.is_clean(), "{:?}", result.warnings());
}

/// Two setups start from their own stock: the second's first stage is a whole
/// block, not what the first left.
#[test]
fn each_setup_starts_from_its_own_stock() {
    let mut job: Job = toml::from_str(JOB).unwrap();
    let mut second = job.setups[0].clone();
    second.name = "again".into();
    job.setups.push(second);
    let body = Body::derived(dome());
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let first = run_setup(&job, 0, &bodies).unwrap();
    let again = run_setup(&job, 1, &bodies).unwrap();
    assert_eq!(first.keyframes[0], again.keyframes[0]);
    assert_eq!(again.setup, "again");
}

/// Two blocks side by side make one stock: the taller one sets stock-top, and
/// both are cut.
#[test]
fn two_blocks_are_one_stock() {
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.stocks = vec![
        crate::job::StockDef {
            name: "left".into(),
            spec: crate::job::StockSpec::FixedBox(crate::job::FixedBox::at(
                [-10.0, -10.0, -8.0],
                [10.0, 20.0, 17.0],
            )),
        },
        crate::job::StockDef {
            name: "right".into(),
            spec: crate::job::StockSpec::FixedBox(crate::job::FixedBox::at(
                [0.0, -10.0, -8.0],
                [10.0, 20.0, 18.0],
            )),
        },
    ];
    job.setups[0].stocks = vec!["left".into(), "right".into()];
    let result = run_job(&job, &dome()).unwrap();
    assert_eq!(result.stocks.len(), 2);
    assert_eq!(result.heights.stock_top, 10.0);
    assert_eq!(result.stock_min, dvec2(-10.0, -10.0));
    assert_eq!(result.stock_max, dvec2(10.0, 10.0));
    assert!(result.is_clean(), "{:?}", result.warnings());
}

/// The step as the "dome" of `JOB`, with one operation left: `finish` or
/// `rough`, kept to the plateau.
fn kept_to_plateau(keep: &str, containment: crate::job::Containment) -> (Job, Body) {
    use crate::job::FeatureRef;
    use crate::setup::features::find_face;
    let body = Body::derived(synthetic::step(5.0, 10.0));
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.setups[0].operations.retain(|op| op.name == keep);
    let pick = FeatureRef {
        model: "dome".into(),
        index: 0,
        at: [-5.0, 0.0, 5.0],
    };
    let face = find_face(&body, &pick, 0.01).unwrap().index;
    let op = &mut job.setups[0].operations[0];
    op.selection.faces.push(FeatureRef {
        index: face,
        ..pick
    });
    op.boundary.containment = containment;
    op.boundary.kind = Boundary::Selection;
    (job, body)
}

/// The plate's 8 mm hole, picked by its top rim and drilled through with an
/// 8 mm drill in pecks: one hole, drilled to the model's bottom, each peck
/// backing out, and the result verifies.
fn drilled(peck: Option<f64>) -> (SetupResult, Job) {
    use crate::job::{FeatureRef, ToolSpec};
    use crate::setup::features::find_edge;
    let body = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 48));
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.tools.insert(
        "drill".into(),
        toml::from_str::<ToolSpec>("kind = \"drill\"\ndiameter = 8.0\nnumber = 3\n").unwrap(),
    );
    job.setups[0].operations.retain(|op| op.name == "finish");
    let op = &mut job.setups[0].operations[0];
    op.name = "holes".into();
    op.tool = Some("drill".into());
    op.strategy = StrategySpec::Drill { peck, dwell: None };
    let pick = FeatureRef {
        model: "dome".into(),
        index: 0,
        at: [4.0, 0.0, 5.0],
    };
    let edge = find_edge(&body, &pick, 0.01).unwrap().index;
    op.selection.edges.push(FeatureRef {
        index: edge,
        ..pick
    });
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    (run_setup(&job, 0, &bodies).unwrap(), job)
}

#[test]
fn a_hole_is_drilled_through_in_pecks() {
    let (result, _) = drilled(Some(2.0));
    let op = &result.operations[0];
    let StrategyDetail::Drill(report) = &op.detail else {
        panic!("reported {:?}", op.detail);
    };
    assert_eq!(report.holes.len(), 1);
    let hole = report.holes[0];
    assert!(hole.at.length() < 1e-6, "{hole:?}");
    assert!((hole.top - hole.bottom - 5.0).abs() < 1e-6, "{hole:?}");
    assert_eq!(op.strategy, "drill");
    assert!(op.warnings.is_empty(), "{:?}", op.warnings);
    // Down to the bottom in three pecks, backing out between.
    let deepest = op
        .path
        .moves()
        .iter()
        .map(|m| m.to.z)
        .fold(f64::MAX, f64::min);
    assert!((deepest - hole.bottom).abs() < 1e-9);
    let retracts = op
        .path
        .moves()
        .iter()
        .filter(|m| m.role == crate::path::toolpath::MoveRole::Retract)
        .count();
    assert!(retracts >= 3, "{retracts} retracts");
    assert!(result.is_clean(), "{:?}", result.warnings());
    assert!(result.verify.gouged == 0);
}

/// A drill too small for its hole is drilled, and said.
#[test]
fn a_drill_the_wrong_size_is_said() {
    let (_, mut job) = drilled(None);
    job.tools.get_mut("drill").unwrap().diameter = 6.0;
    let body = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 48));
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let w = &result.operations[0].warnings;
    assert!(w.iter().any(|w| w.contains("8.000 mm across")), "{w:?}");
}

/// The plate's 8 mm hole, picked by its wall and cut with a 6 mm flat end mill
/// by `strategy`, a new operation of it.
fn round_wall_cut(strategy: &str) -> (SetupResult, Job) {
    use crate::job::{FeatureRef, Operation};
    use crate::setup::features::find_face;
    let body = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 48));
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.setups[0].operations.clear();
    let mut op = Operation::new("hole", StrategySpec::named(strategy).unwrap());
    op.tool = Some("rough".into());
    op.passes.stepover = 5.7;
    let pick = FeatureRef {
        model: "dome".into(),
        index: 0,
        at: [4.0, 0.0, 2.5],
    };
    let face = find_face(&body, &pick, 0.01).unwrap().index;
    op.selection.faces.push(FeatureRef {
        index: face,
        ..pick
    });
    job.setups[0].operations.push(op);
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    (run_setup(&job, 0, &bodies).unwrap(), job)
}

/// A bore and a circular each cut the plate's hole from its top to its own
/// bottom, through the plate, and leave nothing in it: every cut an arc about
/// its axis, the result verified clean.
#[test]
fn a_hole_is_bored_and_milled_round() {
    for strategy in ["bore", "circular"] {
        let (result, job) = round_wall_cut(strategy);
        let op = &result.operations[0];
        let report = match &op.detail {
            StrategyDetail::Bore(r) | StrategyDetail::Circular(r) => *r,
            other => panic!("reported {other:?}"),
        };
        assert_eq!((report.holes, report.bosses, report.cores), (1, 0, 0));
        assert_eq!(op.strategy, strategy);
        assert!(result.is_clean(), "{strategy}: {:?}", result.warnings());
        assert_eq!(result.verify.gouged, 0, "{strategy}");
        assert_eq!(op.sim.rapid_contacts, 0, "{strategy}");
        // Down to the plate's bottom, its hole's own, and no further.
        let lowest = op
            .path
            .moves()
            .iter()
            .map(|m| m.to.z)
            .fold(f64::MAX, f64::min);
        assert!(lowest.abs() < 1e-9, "{strategy}: down to {lowest}");
        // A circular ramps down into each level round its circle, as
        // Fusion's ramps on; a bore's helix is its own.
        let ramps = op
            .path
            .moves()
            .iter()
            .filter(|m| m.role == crate::path::toolpath::MoveRole::Ramp)
            .collect::<Vec<_>>();
        if strategy == "circular" {
            assert!(op.links.ramps > 0);
            assert!(
                ramps
                    .iter()
                    .all(|m| matches!(m.kind, crate::path::toolpath::MoveKind::Arc(_)))
            );
        } else {
            assert!(ramps.is_empty());
        }
        // Every cut an arc round the hole, the tool's radius in from its wall.
        let mut at = DVec3::ZERO;
        for m in op.path.moves() {
            if m.role == crate::path::toolpath::MoveRole::Cut {
                let crate::path::toolpath::MoveKind::Arc(a) = m.kind else {
                    panic!("{strategy}: a straight cut to {}", m.to);
                };
                assert!(a.centre.truncate().length() < 1e-6);
                assert!((a.radius(at) - 1.0).abs() < 1e-6, "{}", a.radius(at));
            }
            at = m.to;
        }
        // Nothing is left in the hole.
        let stock = result.keyframes.last().unwrap().unpack().unwrap();
        let field = stock.field();
        let mut inside = 0;
        for iy in 0..field.ny() {
            for ix in 0..field.nx() {
                let p = field.position(ix, iy).unwrap();
                if p.length() < 4.0 - 2.0 * field.pitch() {
                    inside += 1;
                    let h = field.get(ix, iy).unwrap();
                    assert!(h <= job.tolerance, "{strategy}: {h} left at {p}");
                }
            }
        }
        assert!(inside > 100);
    }
}

/// A block 40 by 20 by 10 mm about the origin with a blind slot along X in its
/// top, its ends' centres at (-8, 0) and (8, 0), 6 mm wide, its floor at 6 mm:
/// faces named as a file would name them (top, bottom, outside, floor, the
/// two straight walls and the two round ends), the round ends' edges circles.
fn slotted_block() -> Body {
    use camcraft_geom::mesh::TriMesh;
    use std::f64::consts::TAU;
    let (half, r, floor, top) = (8.0, 3.0, 6.0, 10.0);
    let (wx, wy) = (20.0, 10.0);
    // Where a ray from the middle at `a` leaves the slot, and the block.
    let slot_at = |a: f64| {
        let u = DVec2::from_angle(a);
        if u.y.abs() > 1e-12 && (r / u.y.abs() * u.x).abs() <= half + 1e-12 {
            return u * (r / u.y.abs());
        }
        let c = dvec2(half.copysign(u.x), 0.0);
        let b = u.dot(c);
        u * (b + (b * b - c.length_squared() + r * r).sqrt())
    };
    let block_at = |a: f64| {
        let u = DVec2::from_angle(a);
        u * (wx / u.x.abs()).min(wy / u.y.abs())
    };
    // Rays through every corner of either, and evenly between.
    let mut angles: Vec<f64> = (0..96).map(|i| TAU * f64::from(i) / 96.0).collect();
    for (x, y) in [(half, r), (wx, wy)] {
        for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
            angles.push((sy * y).atan2(sx * x).rem_euclid(TAU));
        }
    }
    angles.sort_by(f64::total_cmp);
    angles.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let n = u32::try_from(angles.len()).unwrap();
    let mut v = Vec::new();
    // Rings: slot at the top, slot at the floor, block at the top and bottom.
    for (z, slot) in [(top, true), (floor, true), (top, false), (0.0, false)] {
        for &a in &angles {
            let p = if slot { slot_at(a) } else { block_at(a) };
            v.push(p.extend(z));
        }
    }
    v.push(DVec3::new(0.0, 0.0, floor));
    v.push(DVec3::new(0.0, 0.0, 0.0));
    let (floor_mid, bottom_mid) = (4 * n, 4 * n + 1);
    let at = |ring: u32, i: u32| ring * n + i % n;
    let mut t = Vec::new();
    let mut labels = Vec::new();
    for i in 0..n {
        let j = i + 1;
        let (st0, st1, sf0, sf1) = (at(0, i), at(0, j), at(1, i), at(1, j));
        let (ot0, ot1, ob0, ob1) = (at(2, i), at(2, j), at(3, i), at(3, j));
        let a0 = angles[i as usize];
        let a1 = if j == n {
            angles[0] + TAU
        } else {
            angles[j as usize]
        };
        let m = slot_at((a0 + a1) / 2.0);
        let wall = if m.x.abs() <= half + 1e-9 {
            if m.y > 0.0 { 4 } else { 5 }
        } else if m.x > 0.0 {
            6
        } else {
            7
        };
        for (tri, face) in [
            ([st0, ot0, ot1], 0),
            ([st0, ot1, st1], 0),
            ([ob0, bottom_mid, ob1], 1),
            ([ob0, ob1, ot1], 2),
            ([ob0, ot1, ot0], 2),
            ([floor_mid, sf0, sf1], 3),
            ([sf0, st1, sf1], wall),
            ([sf0, st0, st1], wall),
        ] {
            t.push(tri);
            labels.push(face);
        }
    }
    Body::with_faces(TriMesh::new(v, t).unwrap(), labels, 8).unwrap()
}

/// `body`'s floor-of-the-slot edges or face, picked where they are.
fn slot_pick(body: &Body, face: bool) -> crate::job::Selection {
    use crate::job::{FeatureRef, Selection};
    use crate::setup::features::{find_edge, find_face};
    let pick = |at: [f64; 3]| FeatureRef {
        model: "dome".into(),
        index: 0,
        at,
    };
    let mut s = Selection::default();
    if face {
        let f = find_face(body, &pick([0.0, 0.0, 6.0]), 0.01).unwrap().index;
        s.faces.push(FeatureRef {
            index: f,
            ..pick([0.0, 0.0, 6.0])
        });
    } else {
        for at in [
            [0.0, 3.0, 6.0],
            [11.0, 0.0, 6.0],
            [0.0, -3.0, 6.0],
            [-11.0, 0.0, 6.0],
        ] {
            let e = find_edge(body, &pick(at), 0.01).unwrap().index;
            s.edges.push(FeatureRef {
                index: e,
                ..pick(at)
            });
        }
    }
    s
}

/// The block's slot cut by `op`, a new operation of `strategy` with tool
/// `tool` of the job's, changed by `change`.
fn on_slotted_block(
    strategy: &str,
    tool: (&str, crate::job::ToolSpec),
    change: impl FnOnce(&mut crate::job::Operation),
) -> (SetupResult, Job) {
    use crate::job::Operation;
    let body = slotted_block();
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.tools.insert(tool.0.into(), tool.1);
    job.setups[0].operations.clear();
    let mut op = Operation::new("along", StrategySpec::named(strategy).unwrap());
    op.tool = Some(tool.0.into());
    op.passes.feed = 1200.0;
    change(&mut op);
    job.setups[0].operations.push(op);
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    (run_setup(&job, 0, &bodies).unwrap(), job)
}

/// The heights the last of `result`'s material stands at over the points
/// `inside` picks, each with how high the part is there: every one at most
/// the tolerance above it, and how many there were.
fn left_over(result: &SetupResult, job: &Job, inside: impl Fn(DVec2) -> bool, floor: f64) -> usize {
    let stock = result.keyframes.last().unwrap().unpack().unwrap();
    let field = stock.field();
    let mut count = 0;
    for iy in 0..field.ny() {
        for ix in 0..field.nx() {
            let p = field.position(ix, iy).unwrap();
            if inside(p) {
                count += 1;
                let h = field.get(ix, iy).unwrap();
                assert!(h <= floor + job.tolerance, "{h} left at {p}");
            }
        }
    }
    count
}

/// Fusion's Slot, a new operation picked by the edges round the slot's floor
/// or by its floor, with a tool as wide as the slot, ramps back and forth down
/// to the floor along the middle and cuts along it: clean, verified, every
/// cut on the middle, and nothing left in the slot. In levels, with a
/// narrower tool, it goes round the walls too, and still leaves nothing.
#[test]
fn a_slot_is_cut_along_its_middle() {
    use crate::path::toolpath::MoveRole;
    let flat = |d: f64| crate::job::ToolSpec {
        diameter: d,
        ..toml::from_str::<Job>(JOB).unwrap().tools["rough"].clone()
    };
    for (tool, levels, face) in [(6.0, None, false), (4.0, Some(2.0), true)] {
        let (result, job) = on_slotted_block("slot", ("slotter", flat(tool)), |op| {
            op.selection = slot_pick(&slotted_block(), face);
            op.passes.stepdown = levels;
        });
        let op = &result.operations[0];
        let StrategyDetail::Slot(report) = op.detail else {
            panic!("reported {:?}", op.detail);
        };
        assert_eq!(report.slots, 1);
        assert_eq!(report.walls, u64::from(tool < 6.0));
        assert!(result.is_clean(), "{tool}: {:?}", result.warnings());
        assert_eq!(result.verify.gouged, 0);
        assert_eq!(op.sim.rapid_contacts, 0);
        assert!(op.links.ramps > 0, "{tool}: ramped down");
        let ramps = op.path.moves().iter().filter(|m| m.role == MoveRole::Ramp);
        // Back and forth along the middle, no further than the slot's ends, or
        // round the walls, as far from it as the tool goes round them.
        let reach = 3.0 - tool / 2.0;
        for m in ramps {
            let off = dvec2(m.to.x.clamp(-8.0, 8.0), 0.0).distance(m.to.truncate());
            assert!(off <= reach + 1e-9, "{tool}: {}", m.to);
        }
        let lowest = op
            .path
            .moves()
            .iter()
            .map(|m| m.to.z)
            .fold(f64::MAX, f64::min);
        assert!((lowest - 6.0).abs() < 1e-9, "down to {lowest}");
        if tool >= 6.0 {
            for m in op.path.moves().iter().filter(|m| m.role == MoveRole::Cut) {
                assert!(m.to.y.abs() < 1e-9, "{}", m.to);
            }
        }
        let pitch = 2.0 * result.keyframes[0].unpack().unwrap().field().pitch();
        let inside = |p: DVec2| {
            let along = dvec2(p.x.clamp(-8.0, 8.0), 0.0);
            p.distance(along) < 3.0 - pitch
        };
        assert!(left_over(&result, &job, inside, 6.0) > 100);
    }
}

/// A slot's refusals: a tool wider than the slot, a pick that is not one,
/// and nothing picked.
#[test]
fn a_slot_refuses_what_it_cannot_cut() {
    use crate::job::{FeatureRef, Operation};
    let body = slotted_block();
    let wide = crate::job::ToolSpec {
        diameter: 8.0,
        ..toml::from_str::<Job>(JOB).unwrap().tools["rough"].clone()
    };
    let run = |op: Operation| {
        let mut job: Job = toml::from_str(JOB).unwrap();
        job.tools.insert("wide".into(), wide.clone());
        job.setups[0].operations = vec![op];
        let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
        run_setup(&job, 0, &bodies).unwrap_err().to_string()
    };
    let mut op = Operation::new("slot", StrategySpec::named("slot").unwrap());
    op.tool = Some("rough".into());
    let nothing = run(op.clone());
    assert!(
        nothing.contains("pick the edges round the floor"),
        "{nothing}"
    );
    op.selection = slot_pick(&body, true);
    op.tool = Some("wide".into());
    let wider = run(op.clone());
    assert!(wider.contains("narrower than the 8.000 mm tool"), "{wider}");
    // The block's top: a rectangle with a slot in it, not a slot.
    op.tool = Some("rough".into());
    let top = crate::setup::features::find_face(
        &body,
        &FeatureRef {
            model: "dome".into(),
            index: 0,
            at: [15.0, 0.0, 10.0],
        },
        0.01,
    )
    .unwrap()
    .index;
    op.selection.faces = vec![FeatureRef {
        model: "dome".into(),
        index: top,
        at: [15.0, 0.0, 10.0],
    }];
    let not = run(op);
    assert!(not.contains("is not a slot"), "{not}");
}

/// Fusion's Trace, picked by the edges round the slot's floor, its tool kept
/// to their left, inside the loop: each cut its radius from the loop at the
/// floor, the round ends as arcs, and the band it cuts down to the floor,
/// clean and verified.
#[test]
fn a_trace_follows_the_picked_edges() {
    use crate::job::Compensation;
    use crate::path::toolpath::MoveRole;
    // Long enough to reach the floor, 6 mm down, with its flutes.
    let small = crate::job::ToolSpec {
        diameter: 2.0,
        flute_length: Some(8.0),
        ..toml::from_str::<Job>(JOB).unwrap().tools["rough"].clone()
    };
    let (result, job) = on_slotted_block("trace", ("small", small), |op| {
        op.selection = slot_pick(&slotted_block(), false);
        if let StrategySpec::Trace { compensation, .. } = &mut op.strategy {
            *compensation = Compensation::Left;
        }
    });
    let op = &result.operations[0];
    let StrategyDetail::Trace(report) = op.detail else {
        panic!("reported {:?}", op.detail);
    };
    assert_eq!((report.curves, report.closed, report.passes), (1, 1, 1));
    assert!(result.is_clean(), "{:?}", result.warnings());
    assert_eq!(result.verify.gouged, 0);
    assert_eq!(op.sim.rapid_contacts, 0);
    let middle = |p: DVec2| dvec2(p.x.clamp(-8.0, 8.0), 0.0).distance(p);
    let mut arcs = 0;
    for m in op.path.moves().iter().filter(|m| m.role == MoveRole::Cut) {
        assert!((m.to.z - 6.0).abs() < 1e-9);
        assert!((middle(m.to.truncate()) - 2.0).abs() < 1e-6, "{}", m.to);
        arcs += usize::from(matches!(m.kind, crate::path::toolpath::MoveKind::Arc(_)));
    }
    assert!(arcs >= 2, "the round ends as arcs");
    let pitch = 2.0 * result.keyframes[0].unpack().unwrap().field().pitch();
    let band = |p: DVec2| (1.0 + pitch..3.0 - pitch).contains(&middle(p));
    assert!(left_over(&result, &job, band, 6.0) > 100);
}

/// A 6 mm 90° V-bit, as Fusion's Engrave was compared with, its flutes long
/// enough to stand through the 2 mm of stock over the block.
fn vee() -> crate::job::ToolSpec {
    crate::job::ToolSpec {
        kind: crate::job::ToolKind::Cone,
        diameter: 6.0,
        angle: Some(90.0),
        flute_length: Some(10.0),
        ..toml::from_str::<Job>(JOB).unwrap().tools["rough"].clone()
    }
}

/// The slotted block's edges or faces at `at`, as picks.
fn picks_at(body: &Body, faces: &[[f64; 3]], edges: &[[f64; 3]]) -> crate::job::Selection {
    use crate::job::{FeatureRef, Selection};
    use crate::setup::features::{find_edge, find_face};
    let pick = |at: [f64; 3]| FeatureRef {
        model: "dome".into(),
        index: 0,
        at,
    };
    Selection {
        faces: faces
            .iter()
            .map(|&at| FeatureRef {
                index: find_face(body, &pick(at), 0.01).unwrap().index,
                ..pick(at)
            })
            .collect(),
        edges: edges
            .iter()
            .map(|&at| FeatureRef {
                index: find_edge(body, &pick(at), 0.01).unwrap().index,
                ..pick(at)
            })
            .collect(),
    }
}

/// Fusion's Engrave, a new operation picked by the slot's rim or by its floor:
/// the slot is 6 mm wide, wider than the V reaches at its depth, 2.85 mm, so
/// the V runs round the rim 2.85 mm in at that depth, round the ends as arcs;
/// the slot is sunk 4 mm, deeper than that, so it is clean and verified, and
/// cuts only inside the slot. The floor picked engraves the same, from the top
/// of the walls round it.
#[test]
fn a_slot_is_engraved_from_its_rim() {
    use crate::path::toolpath::{MoveKind, MoveRole};
    let rim = [
        [0.0, 3.0, 10.0],
        [11.0, 0.0, 10.0],
        [0.0, -3.0, 10.0],
        [-11.0, 0.0, 10.0],
    ];
    let mut paths = Vec::new();
    for (faces, edges) in [(&[][..], &rim[..]), (&[[0.0, 0.0, 6.0]][..], &[][..])] {
        let (result, _) = on_slotted_block("engrave", ("vee", vee()), |op| {
            op.selection = picks_at(&slotted_block(), faces, edges);
        });
        let op = &result.operations[0];
        let StrategyDetail::Engrave(report) = op.detail else {
            panic!("reported {:?}", op.detail);
        };
        assert_eq!((report.shapes, report.loops, report.passes), (1, 1, 1));
        assert!((report.deepest - 2.85).abs() < 1e-9);
        assert_eq!(report.into_part, None);
        assert!(result.is_clean(), "{:?}", result.warnings());
        assert_eq!(result.verify.gouged, 0);
        assert_eq!(op.sim.rapid_contacts, 0);
        let middle = |p: DVec2| dvec2(p.x.clamp(-8.0, 8.0), 0.0).distance(p);
        let mut arcs = 0;
        for m in op.path.moves().iter().filter(|m| m.role == MoveRole::Cut) {
            // As deep as the rim is far, no deeper than 2.85 mm, and inside.
            let far = 3.0 - middle(m.to.truncate());
            assert!((10.0 - m.to.z - far.min(2.85)).abs() < 1e-6, "{}", m.to);
            arcs += usize::from(matches!(m.kind, MoveKind::Arc(_)));
        }
        assert!(arcs >= 2, "the round ends as arcs");
        paths.push(op.path.clone());
    }
    assert_eq!(paths[0], paths[1], "the floor engraves as the rim does");
}

/// A face with nothing sunk into it, the block's top, is engraved into: the
/// run says where and how far, and the material fails against the part.
/// Anything but a cone is refused.
#[test]
fn an_engraving_into_the_part_is_said() {
    let (result, _) = on_slotted_block("engrave", ("vee", vee()), |op| {
        op.selection = picks_at(&slotted_block(), &[[15.0, 0.0, 10.0]], &[]);
    });
    let op = &result.operations[0];
    let StrategyDetail::Engrave(report) = op.detail else {
        panic!("reported {:?}", op.detail);
    };
    // The top and the slot in it: the slot is an island of the top.
    assert_eq!((report.shapes, report.loops), (1, 2));
    let (_, deep) = report.into_part.unwrap();
    assert!((deep - 2.85).abs() < 0.05, "{deep}");
    assert!(op.warnings.iter().any(|w| w.contains("into the part")));
    assert!(result.verify.gouged > 0);

    let mut job: Job = toml::from_str(JOB).unwrap();
    job.setups[0].operations.clear();
    let mut op = crate::job::Operation::new("engrave", StrategySpec::named("engrave").unwrap());
    op.tool = Some("rough".into());
    op.selection = picks_at(&slotted_block(), &[[0.0, 0.0, 6.0]], &[]);
    job.setups[0].operations.push(op);
    let body = slotted_block();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let refused = run_setup(&job, 0, &bodies).unwrap_err().to_string();
    assert!(refused.contains("cone"), "{refused}");
}

fn cuts(result: &SetupResult) -> Vec<DVec2> {
    result.operations[0]
        .path
        .moves()
        .iter()
        .filter(|m| m.role == crate::path::toolpath::MoveRole::Cut)
        .map(|m| m.to.truncate())
        .collect()
}

/// Finishing kept to the plateau: every cut has the tool's centre over it, and
/// with the whole tool inside, a radius in from its edges.
#[test]
fn parallel_keeps_to_a_picked_face() {
    use crate::job::Containment;
    for (containment, inset) in [(Containment::Center, 0.0), (Containment::Inside, 2.0)] {
        let (job, body) = kept_to_plateau("finish", containment);
        let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
        let result = run_setup(&job, 0, &bodies).unwrap();
        let cuts = cuts(&result);
        assert!(cuts.len() > 20, "{containment:?}: only {} cuts", cuts.len());
        let slack = 1e-6 + 0.01;
        for p in &cuts {
            assert!(
                p.x <= -inset + slack
                    && p.x >= -10.0 + inset - slack
                    && p.y.abs() <= 10.0 - inset + slack,
                "{containment:?}: a cut at {p} leaves the plateau"
            );
        }
        // And the passes reach across it, not a stepover short.
        let reach = cuts.iter().map(|p| p.x).fold(f64::MIN, f64::max);
        assert!(
            reach > -inset - 0.1,
            "{containment:?}: stops at x = {reach}"
        );
    }
    // Outside, the tool's side reaches the plateau from past its edge.
    let (job, body) = kept_to_plateau("finish", Containment::Outside);
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let far = cuts(&result).iter().map(|p| p.x).fold(f64::MIN, f64::max);
    assert!(far > 1.5, "outside stops at x = {far}");
}

/// Roughing kept to the plateau clears only around it, and less than the whole
/// block.
#[test]
fn roughing_keeps_to_a_picked_face() {
    use crate::job::Containment;
    let (job, body) = kept_to_plateau("rough", Containment::Center);
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let kept = run_setup(&job, 0, &bodies).unwrap();
    for p in cuts(&kept) {
        assert!(
            p.x <= 0.01 && p.x >= -10.01 && p.y.abs() <= 10.01,
            "a roughing cut at {p} leaves the plateau"
        );
    }
    let mut whole = job.clone();
    whole.setups[0].operations[0].selection.faces.clear();
    whole.setups[0].operations[0].boundary.kind = Boundary::Stock;
    let all = run_setup(&whole, 0, &bodies).unwrap();
    assert!(
        kept.operations[0].sim.removed_mm3 < all.operations[0].sim.removed_mm3,
        "kept {} vs whole {}",
        kept.operations[0].sim.removed_mm3,
        all.operations[0].sim.removed_mm3
    );
}

/// Roughing kept to the part's silhouette leaves the block around it alone: with
/// the tool's centre on it, nothing is cut outside the part; reaching in from
/// outside with an offset, a channel that wide is cut round it and no further.
#[test]
fn roughing_keeps_to_the_silhouette() {
    use crate::job::{Containment, StockSpec};
    let body = Body::derived(synthetic::step(5.0, 10.0));
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.setups[0].operations.retain(|op| op.name == "rough");
    job.stocks[0].spec = StockSpec::AroundModels {
        models: vec![],
        margin: [20.0, 20.0, 2.0],
        below: 0.0,
    };
    let all = run_setup(&job, 0, &bodies).unwrap();

    job.setups[0].operations[0].boundary.kind = Boundary::Silhouette;
    let inside = run_setup(&job, 0, &bodies).unwrap();
    for p in cuts(&inside) {
        assert!(
            p.x.abs() <= 10.01 && p.y.abs() <= 10.01,
            "a cut at {p} leaves the part"
        );
    }

    let b = &mut job.setups[0].operations[0].boundary;
    b.containment = Containment::Outside;
    b.margin = 6.0;
    let channel = run_setup(&job, 0, &bodies).unwrap();
    let cuts = cuts(&channel);
    // The step is 20 square; the tool's centre may go its radius and the offset
    // past it.
    let reach = 10.0 + 3.0 + 6.0 + 0.05;
    for p in &cuts {
        assert!(
            p.x.abs() <= reach && p.y.abs() <= reach,
            "a cut at {p} is past the channel"
        );
    }
    assert!(
        cuts.iter().any(|p| p.x.abs() > 12.0 || p.y.abs() > 12.0),
        "no channel is cut round the part"
    );
    let removed = |r: &SetupResult| r.operations[0].sim.removed_mm3;
    assert!(removed(&inside) < removed(&channel));
    assert!(
        removed(&channel) < removed(&all),
        "channel {} vs the whole block {}",
        removed(&channel),
        removed(&all)
    );
}

/// A pick on nothing stops the run and names the operation.
#[test]
fn a_gone_pick_stops_the_run() {
    let (mut job, body) = kept_to_plateau("finish", crate::job::Containment::Center);
    job.setups[0].operations[0].selection.faces[0].at = [40.0, 40.0, 40.0];
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let err = run_setup(&job, 0, &bodies).unwrap_err();
    assert!(
        matches!(err, RunError::Feature { ref op, .. } if op == "finish"),
        "{err}"
    );
}

/// Kept to a boundary, the same job gives the same moves on any number of
/// threads.
#[test]
fn bounded_runs_are_deterministic() {
    let (job, body) = kept_to_plateau("rough", crate::job::Containment::Center);
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let on = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| run_setup(&job, 0, &bodies).unwrap())
    };
    let (one, eight) = (on(1), on(8));
    assert_eq!(one.operations[0].path, eight.operations[0].path);
}

/// The plate with an 8 mm hole, standing in a sheet 10 mm bigger all round and no
/// thicker, and one operation with the 6 mm flat and no stock to leave, made
/// `strategy`, cutting in levels 2 mm apart where it can.
fn in_a_sheet(strategy: StrategySpec, body: &Body) -> (Job, SetupResult) {
    use crate::job::StockSpec;
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.setups[0].operations.retain(|op| op.name == "rough");
    job.stocks[0].spec = StockSpec::AroundModels {
        models: vec![],
        margin: [10.0, 10.0, 0.0],
        below: 0.0,
    };
    let op = &mut job.setups[0].operations[0];
    op.strategy = strategy;
    op.passes.stepdown = (strategy.uses().depths != crate::job::Depths::None).then_some(2.0);
    op.passes.stock_to_leave = Default::default();
    let bodies: Bodies<'_> = [("dome", body)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    (job, result)
}

/// A part profiled out of a sheet with nothing picked: round its silhouette, the
/// hole before the outline, each all the way through in even levels, and the
/// result verifies.
#[test]
fn a_part_is_profiled_out_hole_first() {
    use crate::job::ProfileSide;
    let body = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 48));
    let profile = StrategySpec::Profile {
        side: ProfileSide::Outside,
        conventional: false,
    };
    let (_, result) = in_a_sheet(profile, &body);
    let op = &result.operations[0];
    let StrategyDetail::Profile(report) = &op.detail else {
        panic!("reported {:?}", op.detail);
    };
    assert_eq!((report.areas, report.rings, report.levels), (1, 2, 3));
    assert_eq!(op.strategy, "profile");
    let cuts: Vec<DVec3> = op
        .path
        .moves()
        .iter()
        .filter(|m| m.role == crate::path::toolpath::MoveRole::Cut)
        .map(|m| m.to)
        .collect();
    let deepest = cuts.iter().map(|p| p.z).fold(f64::MAX, f64::min);
    assert!(deepest.abs() < 1e-9, "down to {deepest}");
    // The hole's ring, a tool's radius inside it, then the outline's, one outside.
    let first = cuts.first().unwrap().truncate().length();
    assert!(first < 1.1, "starts {first} from the hole's middle");
    let last = cuts.last().unwrap().truncate();
    assert!(last.x.abs().max(last.y.abs()) > 22.9, "ends at {last}");
    assert!(result.is_clean(), "{:?}", result.warnings());
    assert_eq!(result.verify.gouged, 0);
}

/// With finishing passes asked for, the same pocket is cleared short of its
/// walls and then finished round them, at the finishing feed, on and off along
/// leads in what is cleared; it still reaches the walls and verifies.
#[test]
fn a_pocket_with_finishing_passes_verifies() {
    use crate::job::{FeatureRef, Finishing};
    use crate::path::toolpath::MoveRole;
    use crate::setup::features::find_face;
    let body = Body::derived(synthetic::step(5.0, 10.0));
    let pick = FeatureRef {
        model: "dome".into(),
        index: 0,
        at: [5.0, 0.0, 0.0],
    };
    let face = find_face(&body, &pick, 0.01).unwrap().index;
    let pocket = StrategySpec::Pocket {
        conventional: false,
    };
    let (mut job, _) = in_a_sheet(pocket, &body);
    job.stocks[0].spec = crate::job::StockSpec::AroundModels {
        models: vec![],
        margin: [0.0, 0.0, 0.0],
        below: 0.0,
    };
    let op = &mut job.setups[0].operations[0];
    op.selection.faces.push(FeatureRef {
        index: face,
        ..pick
    });
    op.outline = Outline::Selection;
    op.passes.finishing = Some(Finishing {
        passes: Some(2),
        feed: Some(333.0),
        ..Finishing::default()
    });
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let op = &result.operations[0];
    let moves = op.path.moves();
    assert!(
        moves
            .iter()
            .any(|m| m.feed == Some(333.0) && m.role == MoveRole::Cut)
    );
    assert!(moves.iter().any(|m| m.role == MoveRole::Lead));
    // Still to the wall, and no further.
    let cuts: Vec<DVec3> = moves
        .iter()
        .filter(|m| m.role == MoveRole::Cut)
        .map(|m| m.to)
        .collect();
    assert!(cuts.iter().all(|p| p.x >= 3.0 - 1e-6 && p.x <= 7.0 + 1e-6));
    assert!(
        cuts.iter()
            .any(|p| (p.x - 3.0).abs() < 0.03 && p.z.abs() < 1e-9)
    );
    assert!(op.sim.removed_mm3 > 950.0, "{}", op.sim.removed_mm3);
    assert!(result.is_clean(), "{:?}", result.warnings());
    assert_eq!(result.verify.gouged, 0);
}

/// A pocket picked by its floor, the step's shelf: cleared to it within the
/// shelf's outline, the tool's side on the wall and nowhere past it, and the
/// result verifies. A tool too wide for it is refused.
#[test]
fn a_pocket_is_cleared_to_its_picked_floor() {
    use crate::job::FeatureRef;
    use crate::setup::features::find_face;
    let body = Body::derived(synthetic::step(5.0, 10.0));
    let pick = FeatureRef {
        model: "dome".into(),
        index: 0,
        at: [5.0, 0.0, 0.0],
    };
    let face = find_face(&body, &pick, 0.01).unwrap().index;
    let pocket = StrategySpec::Pocket {
        conventional: false,
    };
    let (mut job, _) = in_a_sheet(pocket, &body);
    job.stocks[0].spec = crate::job::StockSpec::AroundModels {
        models: vec![],
        margin: [0.0, 0.0, 0.0],
        below: 0.0,
    };
    job.setups[0].operations[0]
        .selection
        .faces
        .push(FeatureRef {
            index: face,
            ..pick
        });
    job.setups[0].operations[0].outline = Outline::Selection;
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let op = &result.operations[0];
    let StrategyDetail::Pocket(report) = &op.detail else {
        panic!("reported {:?}", op.detail);
    };
    assert_eq!((report.areas, report.levels), (1, 3));
    let cuts: Vec<DVec3> = op
        .path
        .moves()
        .iter()
        .filter(|m| m.role == crate::path::toolpath::MoveRole::Cut)
        .map(|m| m.to)
        .collect();
    for p in &cuts {
        assert!(
            p.x >= 3.0 - 1e-6 && p.x <= 7.0 + 1e-6 && p.y.abs() <= 7.0 + 1e-6,
            "a cut at {p} leaves the shelf, less the tool's radius"
        );
    }
    assert!(
        cuts.iter()
            .any(|p| (p.x - 3.0).abs() < 0.03 && p.z.abs() < 1e-9)
    );
    // All of the shelf's block but its corners, which a round tool leaves.
    assert!(op.sim.removed_mm3 > 950.0, "{}", op.sim.removed_mm3);
    assert!(result.is_clean(), "{:?}", result.warnings());
    assert_eq!(result.verify.gouged, 0);

    // Measured from the picked floor: from 3 mm above it to half a millimetre
    // under it, in two even levels.
    let from_picks = |offset| {
        Some(Height {
            reference: HeightRef::Selection,
            offset,
        })
    };
    let h = &mut job.setups[0].operations[0].heights;
    h.top = from_picks(3.0);
    h.bottom = from_picks(-0.5);
    let result = run_setup(&job, 0, &bodies).unwrap();
    let op = &result.operations[0];
    let StrategyDetail::Pocket(report) = &op.detail else {
        panic!("reported {:?}", op.detail);
    };
    assert_eq!(report.levels, 2);
    let zs: Vec<f64> = op
        .path
        .moves()
        .iter()
        .filter(|m| m.role == crate::path::toolpath::MoveRole::Cut)
        .map(|m| m.to.z)
        .collect();
    let deepest = zs.iter().copied().fold(f64::MAX, f64::min);
    let highest = zs.iter().copied().fold(f64::MIN, f64::max);
    assert!((deepest + 0.5).abs() < 1e-9, "down to {deepest}");
    assert!((highest - 1.25).abs() < 1e-9, "first level at {highest}");
    let h = &mut job.setups[0].operations[0].heights;
    (h.top, h.bottom) = (None, None);

    job.tools.get_mut("rough").unwrap().diameter = 25.0;
    let err = run_setup(&job, 0, &bodies).unwrap_err().to_string();
    assert!(err.contains("does not fit"), "{err}");
}

/// The radial stock to leave moves a 2D cut, and an additional offset, a 3D
/// machining boundary's, does not: a profile out by it, a pocket in. The pocket
/// still goes down to the floor picked on the boundary itself.
#[test]
fn stock_to_leave_moves_a_2d_cut_and_an_offset_does_not() {
    use crate::job::{FeatureRef, ProfileSide};
    use crate::setup::features::find_face;
    let cuts = |result: &SetupResult| -> Vec<DVec3> {
        result.operations[0]
            .path
            .moves()
            .iter()
            .filter(|m| m.role == crate::path::toolpath::MoveRole::Cut)
            .map(|m| m.to)
            .collect()
    };

    let plate = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 48));
    let profile = StrategySpec::Profile {
        side: ProfileSide::Outside,
        conventional: false,
    };
    let (mut job, _) = in_a_sheet(profile, &plate);
    let op = &mut job.setups[0].operations[0];
    op.passes.stock_to_leave.radial = 2.0;
    op.boundary.margin = 5.0;
    let bodies: Bodies<'_> = [("dome", &plate)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    // The outline's ring a tool's radius and 2 mm outside it; the hole's, a
    // radius inside the hole less 2 mm, nearer its middle.
    let reach = cuts(&result)
        .iter()
        .map(|p| p.x.abs().max(p.y.abs()))
        .fold(0.0, f64::max);
    assert!((reach - 25.0).abs() < 0.05, "reaches {reach}");

    let step = Body::derived(synthetic::step(5.0, 10.0));
    let pick = FeatureRef {
        model: "dome".into(),
        index: 0,
        at: [5.0, 0.0, 0.0],
    };
    let face = find_face(&step, &pick, 0.01).unwrap().index;
    let pocket = StrategySpec::Pocket {
        conventional: false,
    };
    let (mut job, _) = in_a_sheet(pocket, &step);
    job.stocks[0].spec = crate::job::StockSpec::AroundModels {
        models: vec![],
        margin: [0.0, 0.0, 0.0],
        below: 0.0,
    };
    let op = &mut job.setups[0].operations[0];
    op.selection.faces.push(FeatureRef {
        index: face,
        ..pick
    });
    op.outline = Outline::Selection;
    job.setups[0].operations[0].passes.stock_to_leave.radial = 1.0;
    let bodies: Bodies<'_> = [("dome", &step)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let cuts = cuts(&result);
    for p in &cuts {
        assert!(
            p.x >= 4.0 - 1e-6 && p.x <= 6.0 + 1e-6 && p.y.abs() <= 6.0 + 1e-6,
            "a cut at {p} is not a millimetre further in"
        );
    }
    assert!(
        cuts.iter().any(|p| p.z.abs() < 1e-9),
        "not down to the floor"
    );
}

/// Tabs hold a part profiled out of a sheet: the deepest cut rides over them at
/// their top, less is cut, nothing rapids into what they leave, and the result
/// verifies.
#[test]
fn a_profiled_part_is_held_by_tabs() {
    use crate::job::{ProfileSide, TabShape, Tabs};
    let plate = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 48));
    let profile = StrategySpec::Profile {
        side: ProfileSide::Outside,
        conventional: false,
    };
    let (mut job, without) = in_a_sheet(profile, &plate);
    let bodies: Bodies<'_> = [("dome", &plate)].into_iter().collect();
    for shape in [TabShape::Rectangular, TabShape::Triangular] {
        job.setups[0].operations[0].passes.tabs = Some(Tabs {
            shape,
            width: 4.0,
            height: 2.0,
            spacing: 60.0,
        });
        let with = run_setup(&job, 0, &bodies).unwrap();
        let op = &with.operations[0];
        assert!(with.is_clean(), "{shape:?}: {:?}", with.warnings());
        assert_eq!(with.verify.gouged, 0);
        assert_eq!(op.sim.rapid_contacts, 0);
        // The deepest level is 0; the tabs' top 2 mm up, and cut along.
        let on_top = op
            .path
            .moves()
            .iter()
            .filter(|m| {
                m.role == crate::path::toolpath::MoveRole::Cut && (m.to.z - 2.0).abs() < 1e-9
            })
            .count();
        assert!(on_top > 0, "{shape:?}: nothing rides over a tab");
        let (a, b) = (op.sim.removed_mm3, without.operations[0].sim.removed_mm3);
        assert!(
            a < b - 10.0,
            "{shape:?}: removed {a} against {b} without tabs"
        );
    }
}

/// A profile with leads enters each ring along an arc and leaves along another,
/// with tabs on it still, and the result verifies: the arcs stay in the waste.
#[test]
fn a_profile_leads_on_and_off_each_ring() {
    use crate::job::{Leads, ProfileSide, Tabs};
    use crate::path::toolpath::MoveRole;
    let plate = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 48));
    let profile = StrategySpec::Profile {
        side: ProfileSide::Outside,
        conventional: false,
    };
    let (mut job, _) = in_a_sheet(profile, &plate);
    let op = &mut job.setups[0].operations[0];
    op.linking.leads = Some(Leads::default());
    op.passes.tabs = Some(Tabs {
        width: 4.0,
        height: 2.0,
        spacing: 60.0,
        ..Default::default()
    });
    let bodies: Bodies<'_> = [("dome", &plate)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let op = &result.operations[0];
    assert!(result.is_clean(), "{:?}", result.warnings());
    assert_eq!(result.verify.gouged, 0);
    assert_eq!(op.sim.rapid_contacts, 0);
    let roles: Vec<MoveRole> = op.path.moves().iter().map(|m| m.role).collect();
    // Every run of cuts is entered along a lead and left along one. A tab's
    // step down is part of the run.
    let mut runs = 0;
    for (i, w) in roles.windows(2).enumerate() {
        if w == [MoveRole::Lead, MoveRole::Cut] {
            runs += 1;
            let after = roles[i + 1..]
                .iter()
                .position(|r| !matches!(r, MoveRole::Cut | MoveRole::Plunge));
            assert_eq!(
                after.map(|k| roles[i + 1 + k]),
                Some(MoveRole::Lead),
                "a ring left without a lead"
            );
        }
    }
    // Two rings, three levels each.
    assert_eq!(runs, 6, "{roles:?}");
    // Tabs still ride over the deepest.
    assert!(
        op.path
            .moves()
            .iter()
            .any(|m| m.role == MoveRole::Cut && (m.to.z - 2.0).abs() < 1e-9)
    );
}

/// Stock to leave beside moves a profile out by it, and negative stock in by it,
/// as Fusion's does; what is cut past the part on purpose is no gouge.
#[test]
fn a_profile_leaves_stock_beside_or_cuts_past_the_part() {
    use crate::job::{ProfileSide, StockToLeave};
    let plate = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 48));
    let profile = StrategySpec::Profile {
        side: ProfileSide::Outside,
        conventional: false,
    };
    let (mut job, _) = in_a_sheet(profile, &plate);
    let bodies: Bodies<'_> = [("dome", &plate)].into_iter().collect();
    for radial in [0.5, -0.5] {
        job.setups[0].operations[0].passes.stock_to_leave = StockToLeave { radial, axial: 0.0 };
        let result = run_setup(&job, 0, &bodies).unwrap();
        let reach = result.operations[0]
            .path
            .moves()
            .iter()
            .filter(|m| m.role == crate::path::toolpath::MoveRole::Cut)
            .map(|m| m.to.x.abs().max(m.to.y.abs()))
            .fold(0.0, f64::max);
        assert!(
            (reach - (23.0 + radial)).abs() < 0.05,
            "{radial}: reaches {reach}"
        );
        assert_eq!(result.verify.gouged, 0, "{radial}");
    }
}

/// Stock left beside a pocket's walls stays in the material: the pocket's cut
/// keeps off them by it, and less is taken out.
#[test]
fn a_pocket_leaves_stock_on_its_walls() {
    use crate::job::{FeatureRef, StockToLeave};
    use crate::setup::features::find_face;
    let body = Body::derived(synthetic::step(5.0, 10.0));
    let pick = FeatureRef {
        model: "dome".into(),
        index: 0,
        at: [5.0, 0.0, 0.0],
    };
    let face = find_face(&body, &pick, 0.01).unwrap().index;
    let pocket = StrategySpec::Pocket {
        conventional: false,
    };
    let (mut job, _) = in_a_sheet(pocket, &body);
    job.stocks[0].spec = crate::job::StockSpec::AroundModels {
        models: vec![],
        margin: [0.0, 0.0, 0.0],
        below: 0.0,
    };
    job.setups[0].operations[0]
        .selection
        .faces
        .push(FeatureRef {
            index: face,
            ..pick
        });
    job.setups[0].operations[0].outline = Outline::Selection;
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let removed = |job: &Job| {
        let result = run_setup(job, 0, &bodies).unwrap();
        assert_eq!(result.verify.gouged, 0);
        let op = &result.operations[0];
        for m in op.path.moves() {
            if m.role == crate::path::toolpath::MoveRole::Cut {
                let leave = job.setups[0].operations[0].passes.stock_to_leave.radial;
                assert!(
                    m.to.x >= 3.0 + leave - 1e-6 && m.to.x <= 7.0 - leave + 1e-6,
                    "a cut at {} is not {leave} off the walls",
                    m.to
                );
            }
        }
        op.sim.removed_mm3
    };
    let bare = removed(&job);
    job.setups[0].operations[0].passes.stock_to_leave = StockToLeave {
        radial: 0.5,
        axial: 0.0,
    };
    let leaving = removed(&job);
    assert!(
        leaving < bare - 50.0,
        "removed {leaving} leaving stock, {bare} not"
    );
}

/// Roughing the same again, leaving the same stock, finds nothing to cut: the
/// stock left on the walls is the part's to keep, not material for a ring.
#[test]
fn roughing_twice_leaves_nothing_for_the_second() {
    let mut job: Job = toml::from_str(JOB).unwrap();
    let rough = job.setups[0].operations[0].clone();
    job.setups[0].operations = vec![
        rough.clone(),
        Operation {
            name: "again".into(),
            ..rough
        },
    ];
    let result = run_job(&job, &dome()).unwrap();
    let cut = |o: &OperationResult| -> f64 {
        let moves = o.path.moves();
        moves
            .windows(2)
            .filter(|w| w[1].role == crate::path::toolpath::MoveRole::Cut)
            .map(|w| w[0].to.distance(w[1].to))
            .sum()
    };
    let (first, again) = (&result.operations[0], &result.operations[1]);
    assert!(
        again.sim.removed_mm3 < 1.0,
        "removed {}",
        again.sim.removed_mm3
    );
    assert!(
        cut(again) < 0.02 * cut(first),
        "cut {} mm again against {} mm",
        cut(again),
        cut(first)
    );
}

/// Facing takes the stock above the part down to its top, all of it and past
/// its edges, off the stock at both ends of every pass, and nothing of the part:
/// the result verifies.
#[test]
fn facing_takes_the_stock_down_to_the_part() {
    use crate::job::{FaceDirection, StockSpec};
    let plate = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 48));
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.setups[0].operations.retain(|op| op.name == "rough");
    job.stocks[0].spec = StockSpec::AroundModels {
        models: vec![],
        margin: [10.0, 10.0, 3.0],
        below: 0.0,
    };
    let op = &mut job.setups[0].operations[0];
    let diameter = job.tools[op.tool.as_ref().unwrap()].diameter;
    // Fusion's meaning: to the tool's centre, here a millimetre past clear.
    op.strategy = StrategySpec::Face {
        angle: 0.0,
        direction: FaceDirection::BothWays,
        pass_extension: Some(diameter / 2.0 + 1.0),
        stock_offset: 0.0,
    };
    op.passes.stepdown = Some(2.0);
    op.passes.stock_to_leave = Default::default();
    op.passes.stepover = 0.9 * diameter;
    job.validate().unwrap();
    let bodies: Bodies<'_> = [("dome", &plate)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let op = &result.operations[0];
    let StrategyDetail::Face(report) = &op.detail else {
        panic!("reported {:?}", op.detail);
    };
    assert_eq!(report.levels, 2);
    assert_eq!(op.strategy, "face");
    let top = result.heights.model_top;
    let cuts: Vec<DVec3> = op
        .path
        .moves()
        .iter()
        .filter(|m| m.role == crate::path::toolpath::MoveRole::Cut)
        .map(|m| m.to)
        .collect();
    let deepest = cuts.iter().map(|p| p.z).fold(f64::MAX, f64::min);
    assert!((deepest - top).abs() < 1e-9, "down to {deepest}, not {top}");
    // Every pass starts and ends off the stock, its centre the extension past
    // it: the tool's radius and a millimetre.
    let reach = diameter / 2.0 + 1.0;
    for p in &cuts {
        assert!(p.y.abs() <= 30.0 + 1e-9, "{p}");
    }
    assert!(
        cuts.iter()
            .all(|p| (p.x.abs() - (30.0 + reach)).abs() < 1e-9)
    );
    // 60 by 60 by 3, less the grid's edges.
    let taken = op.sim.removed_mm3;
    assert!((taken - 10_800.0).abs() < 400.0, "took {taken}");
    assert!(result.is_clean(), "{:?}", result.warnings());
    assert_eq!(result.verify.gouged, 0);
}

/// The shared job with its roughing made adaptive: a tenth of the 6 mm flat's
/// width, the same levels.
fn adaptive_job() -> Job {
    use crate::job::CutDirection;
    let mut job: Job = toml::from_str(JOB).unwrap();
    let rough = &mut job.setups[0].operations[0];
    rough.strategy = StrategySpec::named("adaptive").unwrap();
    assert!(matches!(
        rough.strategy,
        StrategySpec::Adaptive {
            direction: CutDirection::Climb,
            flat_areas: true,
            ..
        }
    ));
    rough.passes.stepover = 0.6;
    job
}

/// Adaptive clearing roughs the dome, the ball finishes it, and the result is
/// the part: nothing gouged, no rapid through material, every link resolved.
#[test]
fn adaptive_roughing_is_finished_to_the_part() {
    let job = adaptive_job();
    let result = run_job(&job, &dome()).unwrap();
    let rough = &result.operations[0];
    let StrategyDetail::Adaptive(r) = rough.detail else {
        panic!("rough reported {:?}", rough.detail);
    };
    assert!(r.passes > 0 && r.levels > 0, "{r:?}");
    assert_eq!(r.no_room, 0, "{r:?}");
    assert!(r.left_mm2 < 1.0, "{r:?}");
    assert!(!rough.path.has_unplanned_links());
    assert!(rough.sim.is_clean(), "{:?}", rough.warnings);
    assert!(
        result.is_clean(),
        "job did not verify: {:?}",
        result.warnings()
    );
}

/// Held to a tenth of the tool, it still takes off what Z-level roughing does,
/// and more, as its flat levels take the terraces over flats.
#[test]
fn adaptive_clears_what_zlevel_roughing_does() {
    use crate::job::StockSpec;
    let body = Body::derived(synthetic::step(5.0, 10.0));
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let removed = |mut job: Job| {
        job.setups[0].operations.retain(|op| op.name == "rough");
        job.stocks[0].spec = StockSpec::AroundModels {
            models: vec![],
            margin: [8.0, 8.0, 2.0],
            below: 0.0,
        };
        let result = run_setup(&job, 0, &bodies).unwrap();
        assert!(result.is_clean(), "{:?}", result.warnings());
        result.operations[0].sim.removed_mm3
    };
    let zlevel = removed(toml::from_str(JOB).unwrap());
    let adaptive = removed(adaptive_job());
    assert!(
        adaptive > 0.99 * zlevel,
        "adaptive removed {adaptive} mm3 against {zlevel}"
    );
}

/// Kept inside the part's plateau, there is no way in from the side: it goes
/// down a helix, and the result still verifies.
#[test]
fn adaptive_goes_down_a_helix_where_it_cannot_come_from_the_side() {
    let (mut job, body) = kept_to_plateau("rough", crate::job::Containment::Center);
    let op = &mut job.setups[0].operations[0];
    op.strategy = StrategySpec::named("adaptive").unwrap();
    op.passes.stepover = 0.6;
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let rough = &result.operations[0];
    assert!(rough.links.helixes > 0, "{:?}", rough.links);
    assert!(rough.sim.is_clean(), "{:?}", rough.warnings);
    assert!(result.is_clean(), "{:?}", result.warnings());
}

#[test]
fn adaptive_runs_are_deterministic() {
    let job = adaptive_job();
    let body = Body::derived(dome());
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let on = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| run_setup(&job, 0, &bodies).unwrap())
    };
    let (one, eight) = (on(1), on(8));
    assert_eq!(one.operations[0].path, eight.operations[0].path);
}

/// The dome roughed adaptive 2.5 mm a level, then again 1 mm a level to take
/// the terraces down: with rest machining, the second clears only what the
/// first left, in far fewer passes than starting from the block, taking about
/// as much, and the result verifies.
#[test]
fn rest_machining_clears_only_what_the_operations_before_left() {
    let second = |rest: bool| {
        let mut job = adaptive_job();
        job.setups[0].operations.retain(|op| op.name == "rough");
        let mut again = job.setups[0].operations[0].clone();
        again.name = "rest".into();
        again.passes.stepdown = Some(1.0);
        if let StrategySpec::Adaptive { rest_machining, .. } = &mut again.strategy {
            *rest_machining = rest;
        }
        assert_eq!(again.strategy.uses().rest, rest);
        job.setups[0].operations.push(again);
        let result = run_job(&job, &dome()).unwrap();
        assert!(result.is_clean(), "{:?}", result.warnings());
        let op = result.operations[1].clone();
        assert!(op.sim.is_clean(), "{:?}", op.warnings);
        op
    };
    let (rest, all) = (second(true), second(false));
    let StrategyDetail::Adaptive(r) = rest.detail else {
        panic!("{:?}", rest.detail);
    };
    assert!(r.passes > 0, "{r:?}");
    assert!(
        rest.passes * 2 < all.passes,
        "rest machining made {} passes, from the block {}",
        rest.passes,
        all.passes
    );
    assert!(
        rest.sim.removed_mm3 > 0.9 * all.sim.removed_mm3,
        "rest machining took {} mm3, from the block {}",
        rest.sim.removed_mm3,
        all.sim.removed_mm3
    );
}

/// An operation turned off or skipped cuts nothing, so the material after it
/// is the material before it: the same keyframe, not another copy of it.
#[test]
fn an_operation_left_out_keeps_the_keyframe_before_it() {
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.setups[0].operations[0].enabled = false;
    let body = Body::derived(dome());
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let prepared = crate::setup::prepare(&job, 0, &bodies).unwrap();
    let plans = [Plan::Generate, Plan::Skip];
    let result = run_prepared_planned(
        &job,
        0,
        &prepared,
        &bodies,
        &plans,
        &crate::control::Control::new(plans.len()),
        &mut |_| {},
    )
    .unwrap();
    let k = &result.keyframes;
    assert_eq!(k.len(), 3);
    assert!(Arc::ptr_eq(&k[0], &k[1]) && Arc::ptr_eq(&k[1], &k[2]));
    assert_eq!(result.operations[0].made, Made::Disabled);
    assert_eq!(result.operations[1].made, Made::Skipped);
}

/// What a run tells, in a form to compare: each stage started, the setup
/// begun, and each operation made by its place.
#[derive(Debug, Clone, PartialEq)]
enum Told {
    Stage(Stage),
    Begun,
    Made(usize, Made),
}

/// Setup 0 of `job` on the dome, as `control` lets it, and what it told.
fn told(job: &Job, control: &Control) -> (Result<SetupResult, RunError>, Vec<Told>) {
    let body = Body::derived(dome());
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let prepared = crate::setup::prepare(job, 0, &bodies).unwrap();
    let mut said = Vec::new();
    let result = run_prepared_planned(job, 0, &prepared, &bodies, &[], control, &mut |p| {
        said.push(match p {
            Progress::Stage(s) => Told::Stage(s),
            Progress::Begun(r) => {
                assert!(r.operations.is_empty() && r.keyframes.len() == 1);
                Told::Begun
            }
            Progress::Made { index, op, .. } => Told::Made(index, op.made),
        });
    });
    (result, said)
}

/// The finish's passes, worked out from the part alone, are worked out from
/// the start, while the roughing, which clears what is left, waits its turn;
/// each is cut in order and told as soon as it is made, before the material
/// is verified; and each has got all the way, as it says.
#[test]
fn every_operation_is_worked_out_as_soon_as_it_can_and_told_as_it_is_made() {
    let job: Job = toml::from_str(JOB).unwrap();
    let control = Control::new(2);
    let (result, said) = told(&job, &control);
    let result = result.unwrap();
    assert_eq!(
        said,
        vec![
            Told::Stage(Stage::Operation(1)),
            Told::Begun,
            Told::Stage(Stage::Operation(0)),
            Told::Stage(Stage::Cutting(0)),
            Told::Made(0, Made::Generated),
            Told::Stage(Stage::Cutting(1)),
            Told::Made(1, Made::Generated),
            Told::Stage(Stage::Verifying),
        ]
    );
    assert_eq!(result.operations.len(), 2);
    assert!((control.done(0) - 1.0).abs() < 1e-9 && (control.done(1) - 1.0).abs() < 1e-9);
}

/// Stopped from its second operation on, a run still makes its first, tells
/// it, and ends there: stopped, not failed.
#[test]
fn a_run_stopped_from_an_operation_makes_the_ones_before_it() {
    let job: Job = toml::from_str(JOB).unwrap();
    let control = Control::new(2);
    control.stop_from(1);
    let (result, said) = told(&job, &control);
    assert!(matches!(result, Err(RunError::Stopped)), "{result:?}");
    assert!(said.contains(&Told::Made(0, Made::Generated)), "{said:?}");
    assert!(
        !said.iter().any(|t| matches!(t, Told::Made(1, _))),
        "{said:?}"
    );
}

/// Stopped from the start, nothing is made.
#[test]
fn a_run_stopped_from_the_start_makes_nothing() {
    let job: Job = toml::from_str(JOB).unwrap();
    let control = Control::new(2);
    control.stop();
    let (result, said) = told(&job, &control);
    assert!(matches!(result, Err(RunError::Stopped)), "{result:?}");
    assert!(
        !said.iter().any(|t| matches!(t, Told::Made(..))),
        "{said:?}"
    );
}

/// A box with walls straight down, roughed and then finished by each strategy
/// that lays passes over the part: their passes leave the walls, steeper than
/// a plunge, to rings round them, a stepover apart down them, and the job
/// still lands on the part. A pass down such a wall would touch it only where
/// it crossed it.
#[test]
fn a_finish_over_the_part_leaves_its_walls_to_rings_round_them() {
    for strategy in ["scallop", "parallel", "spiral", "radial", "morphed-spiral"] {
        let text = JOB
            .replace("margin = [1.0, 1.0, 2.0]", "margin = [6.0, 6.0, 2.0]")
            .replace(
                "strategy = \"parallel\"",
                &format!("strategy = \"{strategy}\""),
            );
        assert!(
            text.contains("margin = [6.0, 6.0, 2.0]"),
            "the edits must land"
        );
        let job: Job = toml::from_str(&text).unwrap();
        let block = synthetic::cuboid(DVec3::new(-6.0, -6.0, -4.0), DVec3::new(6.0, 6.0, 4.0));
        let result = run_job(&job, &block).unwrap();
        let finish = &result.operations[1];
        let walls = match &finish.detail {
            StrategyDetail::Scallop(r) => r.walls,
            StrategyDetail::Parallel(r) => r.walls,
            StrategyDetail::Spiral(r) | StrategyDetail::Radial(r) => r.walls,
            StrategyDetail::MorphedSpiral(r) => r.cut.walls,
            other => panic!("{strategy} reported {other:?}"),
        };
        assert!(walls > 0, "{strategy}: {:?}", finish.detail);
        // Rings beside the walls, the ball's radius out from them, at levels
        // a stepover or less apart all the way down: from the bottom to where
        // the ball's centre is level with the box's top edge, 2 below it,
        // above which the ball rolls over the edge and no ring is that far
        // out.
        let mut levels: Vec<f64> = finish
            .path
            .moves()
            .iter()
            .filter(|m| m.role == crate::path::toolpath::MoveRole::Cut)
            .filter(|m| (m.to.x.abs().max(m.to.y.abs()) - 8.0).abs() < 0.05)
            .map(|m| m.to.z)
            .collect();
        levels.sort_by(f64::total_cmp);
        levels.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        let (lowest, highest) = (levels.first().copied(), levels.last().copied());
        assert!(
            lowest.is_some_and(|z| z <= -4.0 + 0.8),
            "{strategy}: {levels:?}"
        );
        assert!(highest.is_some_and(|z| z >= 2.0), "{strategy}: {levels:?}");
        assert!(
            levels.windows(2).all(|w| w[1] - w[0] <= 0.8 + 1e-6),
            "{strategy}: {levels:?}"
        );
        assert!(result.is_clean(), "{strategy}: {:?}", result.warnings());
    }
}

/// The dome roughed adaptive with Fusion's other settings on: levels added
/// in its shallow areas and on its peak, those cutting too little left out,
/// links lifted and kept off the material, staying down round what stands
/// between only half as far: the result still verifies.
#[test]
fn adaptive_with_shallow_areas_and_lifted_links_is_finished_to_the_part() {
    let mut job = adaptive_job();
    let rough = &mut job.setups[0].operations[0];
    if let StrategySpec::Adaptive {
        shallow_areas,
        min_axial_engagement,
        slot_clearing,
        stay_down_level,
        stay_down_clearance,
        lift_height,
        ..
    } = &mut rough.strategy
    {
        *shallow_areas = true;
        *min_axial_engagement = 0.3;
        *slot_clearing = true;
        *stay_down_level = 50.0;
        *stay_down_clearance = 0.5;
        *lift_height = 1.0;
    }
    job.validate().unwrap();
    let result = run_job(&job, &dome()).unwrap();
    let rough = &result.operations[0];
    let StrategyDetail::Adaptive(r) = rough.detail else {
        panic!("rough reported {:?}", rough.detail);
    };
    assert!(r.shallow_levels > 0, "{r:?}");
    assert!(r.left_mm2 < 1.0, "{r:?}");
    assert!(!rough.path.has_unplanned_links());
    assert!(rough.sim.is_clean(), "{:?}", rough.warnings);
    assert!(
        result.is_clean(),
        "job did not verify: {:?}",
        result.warnings()
    );
}

/// Smoothed, a profile round a plate and its hole, with leads and tabs: its
/// rings, already arcs and lines from the offsetting, are fewer moves, the
/// leads still go onto and off each ring, the tabs still stand, and the result
/// verifies.
#[test]
fn a_smoothed_profile_is_arcs_and_still_verifies() {
    use crate::job::{Leads, ProfileSide, Tabs};
    use crate::path::toolpath::{MoveKind, MoveRole};
    let plate = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 48));
    let profile = StrategySpec::Profile {
        side: ProfileSide::Outside,
        conventional: false,
    };
    let (mut job, _) = in_a_sheet(profile, &plate);
    let op = &mut job.setups[0].operations[0];
    op.linking.leads = Some(Leads::default());
    op.passes.tabs = Some(Tabs {
        width: 4.0,
        height: 2.0,
        spacing: 60.0,
        ..Default::default()
    });
    op.passes.smoothing = true;
    job.validate().unwrap();
    let bodies: Bodies<'_> = [("dome", &plate)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let op = &result.operations[0];
    assert!(result.is_clean(), "{:?}", result.warnings());
    assert_eq!(result.verify.gouged, 0);
    assert_eq!(op.sim.rapid_contacts, 0);
    let fit = op.smoothing.unwrap();
    assert!(fit.after < fit.before, "{fit:?}");
    assert!(
        op.path
            .moves()
            .iter()
            .any(|m| m.role == MoveRole::Cut && matches!(m.kind, MoveKind::Arc(_)))
    );
    assert!(
        op.path
            .moves()
            .iter()
            .any(|m| m.role == MoveRole::Cut && (m.to.z - 2.0).abs() < 1e-9)
    );
    let leads = op
        .path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Lead)
        .count();
    assert!(leads >= 12, "{leads} leads");
}

/// A pocket's rings slowed at its corners: each slowed cut is at the reduced
/// feed and goes where it went, so the pocket is cleared as it was, and it
/// still verifies.
#[test]
fn a_pockets_corners_are_slowed_and_it_still_verifies() {
    use crate::job::{FeatureRef, FeedOptimization};
    use crate::path::toolpath::MoveRole;
    use crate::setup::features::find_face;
    let body = Body::derived(synthetic::step(5.0, 10.0));
    let pick = FeatureRef {
        model: "dome".into(),
        index: 0,
        at: [5.0, 0.0, 0.0],
    };
    let face = find_face(&body, &pick, 0.01).unwrap().index;
    let pocket = StrategySpec::Pocket {
        conventional: false,
    };
    let (mut job, _) = in_a_sheet(pocket, &body);
    job.stocks[0].spec = crate::job::StockSpec::AroundModels {
        models: vec![],
        margin: [0.0, 0.0, 0.0],
        below: 0.0,
    };
    let op = &mut job.setups[0].operations[0];
    op.selection.faces.push(FeatureRef {
        index: face,
        ..pick
    });
    op.outline = Outline::Selection;
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let plain = run_setup(&job, 0, &bodies).unwrap();
    job.setups[0].operations[0].passes.feed_optimization = Some(FeedOptimization {
        reduced_feed: Some(250.0),
        ..FeedOptimization::default()
    });
    job.validate().unwrap();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let op = &result.operations[0];
    let corners = op.corners.unwrap();
    assert!(corners.corners > 0 && corners.slowed > 0.0, "{corners:?}");
    let slowed = op
        .path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut && m.feed == Some(250.0))
        .count();
    assert!(slowed > 0);
    let before = plain.operations[0].sim.removed_mm3;
    assert!((op.sim.removed_mm3 - before).abs() < 1e-6 * before);
    assert!(result.is_clean(), "{:?}", result.warnings());
    assert_eq!(result.verify.gouged, 0);
    assert!(plain.operations[0].corners.is_none());
}

/// The rest of the tool is watched as it cuts ([decision 0021]): flutes long
/// enough for each level meet nothing; flutes shorter than a level bring the
/// shank into what stands above them; and a tool held short in a spindle
/// whose nose is wide brings the nose down onto the block, and says how far
/// out the tool would have to be.
///
/// [decision 0021]: ../../../../docs/decisions/0021-shank-and-holder.md
#[test]
fn the_shank_and_the_spindle_nose_are_checked() {
    use crate::cutter::assembly::PieceKind;
    use crate::job::{HolderSegment, Machine};
    let job: Job = toml::from_str(JOB).unwrap();
    let rough = |job: &Job| run_job(job, &dome()).unwrap().operations.remove(0);

    let fine = rough(&job);
    assert!(fine.contacts.is_empty(), "{:?}", fine.contacts.first());
    assert!(fine.assembly.is_some());

    let mut short = job.clone();
    short.tools.get_mut("rough").unwrap().flute_length = Some(1.0);
    let short = rough(&short);
    assert!(
        !short.contacts.is_empty(),
        "levels 2.5 mm apart, flutes 1 mm long"
    );
    assert!(short.contacts.iter().all(|c| c.piece == PieceKind::Shank));
    assert!(!short.is_clean());
    // Each contact is at a move of the path, and that move is below the top.
    let top = short
        .path
        .moves()
        .iter()
        .map(|m| m.to.z)
        .fold(f64::MIN, f64::max);
    for c in &short.contacts {
        assert!(short.path.moves()[c.at].to.z < top);
    }

    let mut held = job.clone();
    let tool = held.tools.get_mut("rough").unwrap();
    tool.flute_length = Some(3.0);
    tool.stickout = Some(4.0);
    held.machines.insert(
        "router".into(),
        Machine {
            spindle: "ER20".into(),
            nose: vec![HolderSegment::cylinder(35.0, 20.0)],
            ..Machine::default()
        },
    );
    held.setups[0].machine = Some("router".into());
    let held = rough(&held);
    assert!(
        held.contacts.iter().any(|c| c.piece == PieceKind::Nose),
        "{:?}",
        held.contacts.first()
    );
    let needed = held.stickout_needed().unwrap();
    assert!(needed > 4.0, "{needed}");
}

/// Finishing low round the dome with a wide holder close above the tip
/// brings the holder down onto the dome's top: the check finds it, pulling
/// away and trimming keep it clear, and failing refuses ([decision 0021]).
///
/// [decision 0021]: ../../../../docs/decisions/0021-shank-and-holder.md
#[test]
fn a_finish_keeps_the_holder_off_the_part_as_it_is_asked() {
    use crate::job::{Holder, HolderMode, HolderSegment};
    let mut job: Job = toml::from_str(JOB).unwrap();
    let tool = job.tools.get_mut("finish").unwrap();
    tool.flute_length = Some(3.0);
    tool.stickout = Some(4.0);
    tool.holder = Some(Holder {
        name: "wide".into(),
        segments: vec![HolderSegment::cylinder(30.0, 30.0)],
    });
    let finish = |mode: HolderMode| {
        let mut job = job.clone();
        job.setups[0].operations[1].passes.holder.mode = mode;
        run_job(&job, &dome()).map(|r| r.operations[1].clone())
    };

    let off = finish(HolderMode::Off).unwrap();
    assert!(
        !off.contacts.is_empty(),
        "the holder reaches the dome's top"
    );
    for mode in [HolderMode::PullAway, HolderMode::Trim] {
        let kept = finish(mode).unwrap();
        assert!(
            kept.contacts.is_empty(),
            "{mode:?}: {:?}",
            kept.contacts.first()
        );
        assert!(
            kept.warnings.iter().any(|w| w.contains("shank or holder")),
            "{mode:?}: {:?}",
            kept.warnings
        );
        // Kept clear of the top by its clearance: nothing below 8 - 4 + 1.
        let lowest = kept
            .path
            .moves()
            .iter()
            .filter(|m| m.role == crate::path::toolpath::MoveRole::Cut)
            .map(|m| m.to.z)
            .fold(f64::MAX, f64::min);
        assert!(lowest > 4.9, "{mode:?}: cuts down to {lowest}");
    }
    assert!(matches!(
        finish(HolderMode::Fail),
        Err(RunError::Held { .. })
    ));
}

/// Clearing levels down round the dome with a wide holder close above the
/// tip: trimmed, each level clears only where the holder keeps off the part
/// above it, and nothing meets the material ([decision 0021]).
///
/// [decision 0021]: ../../../../docs/decisions/0021-shank-and-holder.md
#[test]
fn clearing_keeps_the_holder_off_the_part_by_the_region_it_clears() {
    use crate::job::{Holder, HolderMode, HolderSegment};
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.setups[0].operations.truncate(1);
    let tool = job.tools.get_mut("rough").unwrap();
    tool.flute_length = Some(3.0);
    tool.stickout = Some(4.0);
    tool.holder = Some(Holder {
        name: "wide".into(),
        segments: vec![HolderSegment::cylinder(30.0, 30.0)],
    });
    let rough = |mode: HolderMode| {
        let mut job = job.clone();
        job.setups[0].operations[0].passes.holder.mode = mode;
        run_job(&job, &dome()).unwrap().operations.remove(0)
    };
    let off = rough(HolderMode::Off);
    assert!(!off.contacts.is_empty(), "the holder reaches the dome");
    let trimmed = rough(HolderMode::Trim);
    assert!(
        trimmed.contacts.is_empty(),
        "{:?}",
        trimmed.contacts.first()
    );
    assert!(trimmed.passes > 0 && trimmed.sim.removed_mm3 < off.sim.removed_mm3);
}

/// What a test does to a new operation.
type Change = Box<dyn FnOnce(&mut crate::job::Operation)>;

/// A 90° V-bit 6 mm across: what a 2D chamfer cuts with.
fn vbit() -> crate::job::ToolSpec {
    crate::job::ToolSpec {
        kind: crate::job::ToolKind::Cone,
        angle: Some(90.0),
        diameter: 6.0,
        number: 3,
        ..toml::from_str::<Job>(JOB).unwrap().tools["rough"].clone()
    }
}

/// The edges of `body` nearest each of `at`, picked.
fn edges_at(body: &Body, at: &[[f64; 3]]) -> crate::job::Selection {
    use crate::job::{FeatureRef, Selection};
    use crate::setup::features::find_edge;
    let mut s = Selection::default();
    for &at in at {
        let pick = FeatureRef {
            model: "dome".into(),
            index: 0,
            at,
        };
        let e = find_edge(body, &pick, 0.01).unwrap().index;
        s.edges.push(FeatureRef { index: e, ..pick });
    }
    s
}

/// The slotted block's top outline, 40 by 20 at 10 mm, by a point on each
/// side.
const OUTLINE: [[f64; 3]; 4] = [
    [0.0, 10.0, 10.0],
    [20.0, 0.0, 10.0],
    [0.0, -10.0, 10.0],
    [-20.0, 0.0, 10.0],
];

/// The slotted block's slot's top rim, by a point on each of its four edges.
const RIM: [[f64; 3]; 4] = [
    [0.0, 3.0, 10.0],
    [11.0, 0.0, 10.0],
    [0.0, -3.0, 10.0],
    [-11.0, 0.0, 10.0],
];

/// `operations`, each a new one of its strategy made by its function, run on
/// `body` in a block exactly its size, every one with the job's V-bit.
fn chamfered(body: &Body, operations: Vec<(&str, Change)>) -> (SetupResult, Job) {
    use crate::job::{Operation, StockSpec};
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.tools.insert("vbit".into(), vbit());
    job.stocks[0].spec = StockSpec::AroundModels {
        models: vec![],
        margin: [0.0, 0.0, 0.0],
        below: 0.0,
    };
    job.setups[0].operations.clear();
    for (i, (strategy, change)) in operations.into_iter().enumerate() {
        let mut op = Operation::new(format!("op{i}"), StrategySpec::named(strategy).unwrap());
        op.tool = Some("vbit".into());
        op.passes.feed = 1000.0;
        change(&mut op);
        job.setups[0].operations.push(op);
    }
    job.validate().unwrap();
    let bodies: Bodies<'_> = [("dome", body)].into_iter().collect();
    (run_setup(&job, 0, &bodies).unwrap(), job)
}

/// A chamfer `width` across, its tip offset `tip`, round the slotted block's
/// top outline.
fn outline_chamfer(width: f64, tip: f64) -> Change {
    Box::new(move |op: &mut crate::job::Operation| {
        op.selection = edges_at(&slotted_block(), &OUTLINE);
        if let StrategySpec::Chamfer2d {
            width: w,
            tip_offset,
            clearance,
            ..
        } = &mut op.strategy
        {
            *w = width;
            *tip_offset = tip;
            *clearance = 0.6;
        }
    })
}

/// A V-bit traced along `edges`, its tip `down` below them.
fn traced(edges: &'static [[f64; 3]], down: f64) -> Change {
    Box::new(move |op: &mut crate::job::Operation| {
        op.selection = edges_at(&slotted_block(), edges);
        if let StrategySpec::Trace { axial_offset, .. } = &mut op.strategy {
            *axial_offset = -down;
        }
    })
}

/// Fusion's 2D Chamfer, a 90° V-bit run round the slotted block's sharp top
/// outline, 1 mm wide with its tip the default millimetre below: every check
/// passes, and the stock along the edge is the chamfer asked, to the job's
/// tolerance: 1 mm down at the edge, rising at 45° to the top 1 mm in, and
/// the top untouched past it. Cut into the part on purpose, it is no gouge.
#[test]
fn a_chamfer_is_cut_to_the_width_asked_and_verifies() {
    let body = slotted_block();
    let (result, job) = chamfered(&body, vec![("chamfer-2d", outline_chamfer(1.0, 1.0))]);
    let op = &result.operations[0];
    let StrategyDetail::Chamfer2d(report) = &op.detail else {
        panic!("reported {:?}", op.detail);
    };
    assert_eq!((report.edges, report.closed, report.passes), (1, 1, 1));
    assert_eq!(report.lifted, 0, "nothing else within the clearance");
    assert!((report.depth - 2.0).abs() < 1e-9);
    assert!(result.is_clean(), "{:?}", result.warnings());
    assert_eq!(result.verify.gouged, 0);
    assert_eq!(op.sim.rapid_contacts, 0);
    let stock = result.keyframes.last().unwrap().unpack().unwrap();
    let field = stock.field();
    let pitch = field.pitch();
    let allowed = job.tolerance + 2.0 * pitch;
    let (mut on, mut past) = (0, 0);
    for iy in 0..field.ny() {
        for ix in 0..field.nx() {
            let p = field.position(ix, iy).unwrap();
            // In from the long sides, away from the ends and the slot.
            if p.x.abs() > 14.0 {
                continue;
            }
            let d = 10.0 - p.y.abs();
            let h = field.get(ix, iy).unwrap();
            if d > pitch && d < 1.0 - pitch {
                on += 1;
                let meant = 9.0 + d;
                assert!(
                    (h - meant).abs() <= allowed,
                    "{h:.4} at {p}, meant {meant:.4}"
                );
            } else if d > 1.0 + pitch && d < 2.0 {
                past += 1;
                assert!((h - 10.0).abs() <= allowed, "{h:.4} at {p}");
            }
        }
    }
    assert!(
        on > 100 && past > 100,
        "{on} on the chamfer, {past} past it"
    );
}

/// A chamfer is no gouge only as it was asked: a trace after it along the same
/// edges, its V-bit half a millimetre below the chamfer, gouges; and so does
/// one along the slot's rim, which nothing chamfered.
#[test]
fn a_cut_past_the_chamfer_asked_is_still_a_gouge() {
    let body = slotted_block();
    let (deeper, _) = chamfered(
        &body,
        vec![
            ("chamfer-2d", outline_chamfer(1.0, 1.0)),
            ("trace", traced(&OUTLINE, 1.5)),
        ],
    );
    assert!(deeper.verify.gouged > 0, "{:?}", deeper.verify);
    assert!(
        (deeper.verify.deepest_gouge_mm - 0.5).abs() < 0.1,
        "{:?}",
        deeper.verify
    );
    let (elsewhere, _) = chamfered(
        &body,
        vec![
            ("chamfer-2d", outline_chamfer(1.0, 1.0)),
            ("trace", traced(&RIM, 0.5)),
        ],
    );
    assert!(elsewhere.verify.gouged > 0, "{:?}", elsewhere.verify);
    // The trace along the outline no deeper than the chamfer: none.
    let (within, _) = chamfered(
        &body,
        vec![
            ("chamfer-2d", outline_chamfer(1.0, 1.0)),
            ("trace", traced(&OUTLINE, 0.8)),
        ],
    );
    assert_eq!(within.verify.gouged, 0, "{:?}", within.verify);
}

/// A block 40 by 20 by 10 mm about the origin in X and Y, its top outline
/// chamfered 1 mm at 45°: the top face 38 by 18 at 10 mm, a chamfer face down
/// each side to the walls at 9 mm.
fn chamfered_block() -> Body {
    use camcraft_geom::mesh::TriMesh;
    let ring = |hx: f64, hy: f64, z: f64| {
        [
            DVec3::new(-hx, -hy, z),
            DVec3::new(hx, -hy, z),
            DVec3::new(hx, hy, z),
            DVec3::new(-hx, hy, z),
        ]
    };
    let mut v: Vec<DVec3> = Vec::new();
    v.extend(ring(19.0, 9.0, 10.0));
    v.extend(ring(20.0, 10.0, 9.0));
    v.extend(ring(20.0, 10.0, 0.0));
    let (top, mid, bottom) = (0u32, 4u32, 8u32);
    let mut t: Vec<[u32; 3]> = vec![[top, top + 1, top + 2], [top, top + 2, top + 3]];
    let mut labels = vec![0, 0];
    for i in 0..4 {
        let j = (i + 1) % 4;
        // The chamfer face, then the wall below it.
        t.push([mid + i, mid + j, top + j]);
        t.push([mid + i, top + j, top + i]);
        labels.extend([1 + i, 1 + i]);
        t.push([bottom + i, bottom + j, mid + j]);
        t.push([bottom + i, mid + j, mid + i]);
        labels.extend([5 + i, 5 + i]);
    }
    t.push([bottom, bottom + 2, bottom + 1]);
    t.push([bottom, bottom + 3, bottom + 2]);
    labels.extend([9, 9]);
    Body::with_faces(TriMesh::new(v, t).unwrap(), labels, 10).unwrap()
}

/// A chamfer the model has, picked by its lower edges, followed with no width:
/// the V-bit's flank lies along it, the stock is left as the model's chamfer,
/// and nothing is a gouge. With a clearance, the top face beyond the chamfer
/// is one the edges do not join, and the tool is lifted off it, as Fusion
/// says to set no clearance for a chamfer the model has.
#[test]
fn a_chamfer_the_model_has_is_followed() {
    let body = chamfered_block();
    let lower = [
        [0.0, 10.0, 9.0],
        [20.0, 0.0, 9.0],
        [0.0, -10.0, 9.0],
        [-20.0, 0.0, 9.0],
    ];
    let follow = |clearance: f64| -> Change {
        Box::new(move |op: &mut crate::job::Operation| {
            op.selection = edges_at(&chamfered_block(), &lower);
            if let StrategySpec::Chamfer2d {
                width,
                clearance: c,
                ..
            } = &mut op.strategy
            {
                *width = 0.0;
                *c = clearance;
            }
        })
    };
    let (result, job) = chamfered(&body, vec![("chamfer-2d", follow(0.0))]);
    let op = &result.operations[0];
    let StrategyDetail::Chamfer2d(report) = &op.detail else {
        panic!("reported {:?}", op.detail);
    };
    assert_eq!((report.edges, report.lifted), (1, 0));
    assert!(result.is_clean(), "{:?}", result.warnings());
    assert_eq!(result.verify.gouged, 0);
    let stock = result.keyframes.last().unwrap().unpack().unwrap();
    let field = stock.field();
    let pitch = field.pitch();
    let mut on = 0;
    for iy in 0..field.ny() {
        for ix in 0..field.nx() {
            let p = field.position(ix, iy).unwrap();
            let d = 10.0 - p.y.abs();
            if p.x.abs() < 14.0 && d > pitch && d < 1.0 - pitch {
                on += 1;
                let h = field.get(ix, iy).unwrap();
                let meant = 9.0 + d;
                assert!(
                    (h - meant).abs() <= job.tolerance + 2.0 * pitch,
                    "{h:.4} at {p}, meant {meant:.4}"
                );
            }
        }
    }
    assert!(on > 100, "{on} on the chamfer");

    let (cleared, _) = chamfered(&body, vec![("chamfer-2d", follow(0.6))]);
    let op = &cleared.operations[0];
    let StrategyDetail::Chamfer2d(report) = &op.detail else {
        panic!("reported {:?}", op.detail);
    };
    assert!(report.lifted > 0);
    assert!(op.warnings.iter().any(|w| w.contains("narrower")));
    assert_eq!(cleared.verify.gouged, 0);
}

/// A 2D chamfer's refusals: a tool that is not a cone, a chamfer wider than
/// its flank reaches, nothing picked, and an edge with no lower side.
#[test]
fn a_chamfer_refuses_what_it_cannot_cut() {
    use crate::job::{JobError, Operation};
    let body = slotted_block();
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.tools.insert("vbit".into(), vbit());
    let mut op = Operation::new("chamfer", StrategySpec::named("chamfer-2d").unwrap());
    op.tool = Some("rough".into());
    op.selection = edges_at(&body, &OUTLINE);
    job.setups[0].operations = vec![op.clone()];
    let err = job.validate().unwrap_err();
    assert!(
        matches!(&err, JobError::WrongTool { why, .. } if why.contains("chamfer mill or V-bit")),
        "{err}"
    );
    op.tool = Some("vbit".into());
    if let StrategySpec::Chamfer2d { width, .. } = &mut op.strategy {
        *width = 2.5;
    }
    job.setups[0].operations = vec![op.clone()];
    let err = job.validate().unwrap_err();
    assert!(
        err.to_string().contains("wider than the tool's flank"),
        "{err}"
    );
    if let StrategySpec::Chamfer2d { width, .. } = &mut op.strategy {
        *width = 1.0;
    }
    op.selection = Default::default();
    job.setups[0].operations = vec![op];
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let err = run_setup(&job, 0, &bodies).unwrap_err().to_string();
    assert!(err.contains("pick the edges to chamfer"), "{err}");
}

/// A block of boxes on a grid: `xs` and `ys` the grid's lines, and each cell's
/// height and the face its top is, from `cell`; walls between, down to the
/// floor at nothing, each split at every height there is so that the mesh
/// meets itself edge to edge. Faces named: each top as `cell` says, the
/// bottom, and a wall for each plane and span.
fn terraced(xs: &[f64], ys: &[f64], cell: impl Fn(f64, f64) -> (f64, u32)) -> Body {
    use camcraft_geom::mesh::TriMesh;
    use std::collections::BTreeMap;
    let (nx, ny) = (xs.len() - 1, ys.len() - 1);
    let at = |i: usize, j: usize| cell((xs[i] + xs[i + 1]) / 2.0, (ys[j] + ys[j + 1]) / 2.0);
    let mut heights: Vec<f64> = vec![0.0];
    for i in 0..nx {
        for j in 0..ny {
            heights.push(at(i, j).0);
        }
    }
    heights.sort_by(f64::total_cmp);
    heights.dedup();
    let level = |h: f64| heights.iter().position(|&z| z == h).unwrap();
    let tops = (0..nx)
        .flat_map(|i| (0..ny).map(move |j| (i, j)))
        .map(|(i, j)| at(i, j).1)
        .max()
        .unwrap();
    let bottom = tops + 1;
    let mut vertices: Vec<DVec3> = Vec::new();
    let mut index: BTreeMap<(usize, usize, usize), u32> = BTreeMap::new();
    let mut vertex = |i: usize, j: usize, k: usize| {
        *index.entry((i, j, k)).or_insert_with(|| {
            vertices.push(DVec3::new(xs[i], ys[j], heights[k]));
            u32::try_from(vertices.len() - 1).unwrap()
        })
    };
    let mut t: Vec<[u32; 3]> = Vec::new();
    let mut labels: Vec<u32> = Vec::new();
    let mut walls: BTreeMap<(bool, usize, usize, usize), u32> = BTreeMap::new();
    let quad = |q: [u32; 4], face: u32, t: &mut Vec<[u32; 3]>, labels: &mut Vec<u32>| {
        t.push([q[0], q[1], q[2]]);
        t.push([q[0], q[2], q[3]]);
        labels.extend([face, face]);
    };
    for i in 0..nx {
        for j in 0..ny {
            let (h, face) = at(i, j);
            let k = level(h);
            let top = [
                vertex(i, j, k),
                vertex(i + 1, j, k),
                vertex(i + 1, j + 1, k),
                vertex(i, j + 1, k),
            ];
            quad(top, face, &mut t, &mut labels);
            let under = [
                vertex(i, j, 0),
                vertex(i, j + 1, 0),
                vertex(i + 1, j + 1, 0),
                vertex(i + 1, j, 0),
            ];
            quad(under, bottom, &mut t, &mut labels);
        }
    }
    // Walls between each cell and the one past each of its sides, or nothing.
    let height_of = |i: isize, j: isize| -> f64 {
        if i < 0 || j < 0 || i as usize >= nx || j as usize >= ny {
            0.0
        } else {
            at(i as usize, j as usize).0
        }
    };
    let mut next = bottom + 1;
    for line in 0..=nx {
        for j in 0..ny {
            let (a, b) = (
                height_of(line as isize - 1, j as isize),
                height_of(line as isize, j as isize),
            );
            if a == b {
                continue;
            }
            let (lo, hi) = (level(a.min(b)), level(a.max(b)));
            let face = *walls.entry((true, line, lo, hi)).or_insert_with(|| {
                next += 1;
                next - 1
            });
            for k in lo..hi {
                let q = [
                    vertex(line, j, k),
                    vertex(line, j + 1, k),
                    vertex(line, j + 1, k + 1),
                    vertex(line, j, k + 1),
                ];
                quad(q, face, &mut t, &mut labels);
            }
        }
    }
    for line in 0..=ny {
        for i in 0..nx {
            let (a, b) = (
                height_of(i as isize, line as isize - 1),
                height_of(i as isize, line as isize),
            );
            if a == b {
                continue;
            }
            let (lo, hi) = (level(a.min(b)), level(a.max(b)));
            let face = *walls.entry((false, line, lo, hi)).or_insert_with(|| {
                next += 1;
                next - 1
            });
            for k in lo..hi {
                let q = [
                    vertex(i, line, k),
                    vertex(i + 1, line, k),
                    vertex(i + 1, line, k + 1),
                    vertex(i, line, k + 1),
                ];
                quad(q, face, &mut t, &mut labels);
            }
        }
    }
    Body::with_faces(TriMesh::new(vertices, t).unwrap(), labels, next).unwrap()
}

/// A block 60 by 40 by 10 mm about the origin with two pockets in its top: a
/// closed one, 24 by 32 mm, 5 mm deep about (-14, 0), with an island 6 by 10
/// standing to the top in it; and one 22 by 16, 4 mm deep, open to the
/// block's +X side.
fn pocketed_block() -> Body {
    let xs = [-30.0, -26.0, -17.0, -11.0, -2.0, 8.0, 30.0];
    let ys = [-20.0, -16.0, -8.0, -5.0, 5.0, 8.0, 16.0, 20.0];
    terraced(&xs, &ys, |x, y| {
        if (-17.0..-11.0).contains(&x) && y.abs() < 5.0 {
            (10.0, 2)
        } else if (-26.0..-2.0).contains(&x) && y.abs() < 16.0 {
            (5.0, 1)
        } else if x > 8.0 && y.abs() < 8.0 {
            (6.0, 3)
        } else {
            (10.0, 0)
        }
    })
}

/// Fusion's 2D Adaptive Clearing, a new operation picked by the floors of a
/// pocket with an island and of one open to the block's side, cut to size: a
/// helix down into the closed one only, the open one entered from outside the
/// stock, and both cleared to their floors round the island, every check
/// passing. Each pocket's own floor, as a 2D pocket's.
#[test]
fn adaptive_2d_clears_a_pocket_round_an_island_and_one_open_to_the_side() {
    use crate::job::{FeatureRef, Operation, StockSpec};
    use crate::setup::features::find_face;
    let body = pocketed_block();
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.stocks[0].spec = StockSpec::AroundModels {
        models: vec![],
        margin: [0.0, 0.0, 0.0],
        below: 0.0,
    };
    let mut op = Operation::new("adaptive", StrategySpec::named("adaptive-2d").unwrap());
    op.tool = Some("rough".into());
    op.passes.feed = 1500.0;
    op.passes.stepover = 2.4;
    op.passes.stock_to_leave = Default::default();
    op.outline = Outline::Selection;
    for at in [[-22.0, 0.0, 5.0], [20.0, 0.0, 6.0]] {
        let pick = FeatureRef {
            model: "dome".into(),
            index: 0,
            at,
        };
        let face = find_face(&body, &pick, 0.01).unwrap().index;
        op.selection.faces.push(FeatureRef {
            index: face,
            ..pick
        });
    }
    job.setups[0].operations = vec![op];
    job.validate().unwrap();
    let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let op = &result.operations[0];
    let StrategyDetail::Adaptive2d(report) = &op.detail else {
        panic!("reported {:?}", op.detail);
    };
    assert!(result.is_clean(), "{:?}", result.warnings());
    assert_eq!(result.verify.gouged, 0);
    assert_eq!(op.sim.rapid_contacts, 0);
    let ramps: Vec<_> = op
        .path
        .moves()
        .iter()
        .filter(|m| m.role == crate::path::toolpath::MoveRole::Ramp)
        .map(|m| m.to)
        .collect();
    assert_eq!(
        (report.levels, report.entries, report.helixes),
        (2, 1, 1),
        "{ramps:?}"
    );
    // Every pass at a pocket's floor.
    for m in op.path.moves() {
        if m.role == crate::path::toolpath::MoveRole::Cut {
            assert!(
                (m.to.z - 5.0).abs() < 1e-9 || (m.to.z - 6.0).abs() < 1e-9,
                "{}",
                m.to
            );
        }
    }
    let stock = result.keyframes.last().unwrap().unpack().unwrap();
    let field = stock.field();
    let e = 2.0 * field.pitch() + job.tolerance;
    let (mut closed, mut open) = (0, 0);
    for iy in 0..field.ny() {
        for ix in 0..field.nx() {
            let p = field.position(ix, iy).unwrap();
            let h = field.get(ix, iy).unwrap();
            // A round tool leaves the inside corners: away from them.
            let cornered = |xs: [f64; 2], ys: [f64; 2]| {
                xs.iter().any(|x| (p.x - x).abs() < 3.0) && ys.iter().any(|y| (p.y - y).abs() < 3.0)
            };
            let in_closed = p.x > -26.0 + e
                && p.x < -2.0 - e
                && p.y.abs() < 16.0 - e
                && !cornered([-26.0, -2.0], [-16.0, 16.0]);
            let on_island = p.x > -17.0 - e && p.x < -11.0 + e && p.y.abs() < 5.0 + e;
            if in_closed && !on_island {
                closed += 1;
                assert!(h <= 5.0 + job.tolerance, "{h:.4} left at {p}");
            }
            if p.x > 8.0 + e && p.y.abs() < 8.0 - e && !cornered([8.0, 8.0], [-8.0, 8.0]) {
                open += 1;
                assert!(h <= 6.0 + job.tolerance, "{h:.4} left at {p}");
            }
            if p.x > -17.0 + e && p.x < -11.0 - e && p.y.abs() < 5.0 - e {
                assert!(h >= 10.0 - job.tolerance, "the island cut at {p}");
            }
        }
    }
    assert!(closed > 100 && open > 100, "{closed}, {open}");
}

/// 2D adaptive with multiple depths: levels a whole stepdown apart from the
/// top, the last what is left, or evenly where asked; each pocket to its own
/// floor; ordered by area, each pocket all the way down before the next. The
/// same result either way, and a cone refused.
#[test]
fn adaptive_2d_cuts_in_levels_as_asked() {
    use crate::job::{ContourOrder, FeatureRef, Operation, StockSpec};
    use crate::setup::features::find_face;
    let body = pocketed_block();
    let run = |even: bool, by_area: bool| {
        let mut job: Job = toml::from_str(JOB).unwrap();
        job.stocks[0].spec = StockSpec::AroundModels {
            models: vec![],
            margin: [0.0, 0.0, 0.0],
            below: 0.0,
        };
        let mut op = Operation::new("adaptive", StrategySpec::named("adaptive-2d").unwrap());
        op.tool = Some("rough".into());
        op.passes.feed = 1500.0;
        op.passes.stepover = 2.4;
        op.passes.stepdown = Some(2.0);
        op.outline = Outline::Selection;
        if let StrategySpec::Adaptive2d {
            even_stepdowns,
            order,
            ..
        } = &mut op.strategy
        {
            *even_stepdowns = even;
            *order = if by_area {
                ContourOrder::ByArea
            } else {
                ContourOrder::ByDepth
            };
        }
        let pick = FeatureRef {
            model: "dome".into(),
            index: 0,
            at: [-22.0, 0.0, 5.0],
        };
        let face = find_face(&body, &pick, 0.01).unwrap().index;
        op.selection.faces.push(FeatureRef {
            index: face,
            ..pick
        });
        job.setups[0].operations = vec![op];
        let bodies: Bodies<'_> = [("dome", &body)].into_iter().collect();
        run_setup(&job, 0, &bodies).unwrap()
    };
    let levels = |r: &SetupResult| {
        let mut zs: Vec<f64> = r.operations[0]
            .path
            .moves()
            .iter()
            .filter(|m| m.role == crate::path::toolpath::MoveRole::Cut)
            .map(|m| (m.to.z * 1000.0).round() / 1000.0)
            .collect();
        zs.dedup();
        zs
    };
    // From the stock's top, 10 mm, to the floor at 5 mm with 0.5 mm left on
    // it: whole stepdowns, the last what is left, or evenly.
    let whole = run(false, false);
    assert!(whole.is_clean(), "{:?}", whole.warnings());
    assert_eq!(levels(&whole), [8.0, 6.0, 5.5]);
    let even = run(true, false);
    assert!(even.is_clean(), "{:?}", even.warnings());
    let e = levels(&even);
    assert_eq!(e.len(), 3);
    assert!((e[0] - (10.0 - 4.5 / 3.0)).abs() < 1e-3, "{e:?}");
    let by_area = run(false, true);
    assert_eq!(levels(&by_area), [8.0, 6.0, 5.5]);
    assert_eq!(by_area.operations[0].path, whole.operations[0].path);

    // A cone cannot clear a floor.
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.tools.insert("vbit".into(), vbit());
    let mut op = Operation::new("adaptive", StrategySpec::named("adaptive-2d").unwrap());
    op.tool = Some("vbit".into());
    op.passes.stepdown = Some(2.0);
    job.setups[0].operations = vec![op];
    let err = job.validate().unwrap_err().to_string();
    assert!(err.contains("V-bit or chamfer mill"), "{err}");
}
