use super::*;
use crate::fixture::JOB;
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

    let StrategyDetail::ZlevelRough(r) = rough.detail else {
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
    job.setups[0].orientation = crate::frame::Orientation::default().flipped();
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
    use crate::features::find_face;
    use crate::job::FeatureRef;
    let body = Body::derived(synthetic::step(5.0, 10.0));
    let mut job: Job = toml::from_str(JOB).unwrap();
    job.setups[0].operations.retain(|op| op.name == keep);
    let pick = FeatureRef {
        model: "dome".into(),
        index: 0,
        at: [-5.0, 0.0, 5.0],
    };
    let face = find_face(&body, &pick, 0.01).unwrap().index;
    let g = &mut job.setups[0].operations[0].geometry;
    g.faces.push(FeatureRef {
        index: face,
        ..pick
    });
    g.containment = containment;
    (job, body)
}

/// The plate's 8 mm hole, picked by its top rim and drilled through with an
/// 8 mm drill in pecks: one hole, drilled to the model's bottom, each peck
/// backing out, and the result verifies.
fn drilled(peck: Option<f64>) -> (SetupResult, Job) {
    use crate::features::find_edge;
    use crate::job::{FeatureRef, ToolSpec};
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
    op.geometry.edges.push(FeatureRef {
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
        .filter(|m| m.role == crate::toolpath::MoveRole::Retract)
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

fn cuts(result: &SetupResult) -> Vec<DVec2> {
    result.operations[0]
        .path
        .moves()
        .iter()
        .filter(|m| m.role == crate::toolpath::MoveRole::Cut)
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
    whole.setups[0].operations[0].geometry.faces.clear();
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
    use crate::job::{Boundary, Containment, StockSpec};
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

    let g = &mut job.setups[0].operations[0].geometry;
    g.boundary_kind = Some(Boundary::Silhouette);
    let inside = run_setup(&job, 0, &bodies).unwrap();
    for p in cuts(&inside) {
        assert!(
            p.x.abs() <= 10.01 && p.y.abs() <= 10.01,
            "a cut at {p} leaves the part"
        );
    }

    let g = &mut job.setups[0].operations[0].geometry;
    g.containment = Containment::Outside;
    g.margin = 6.0;
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
    job.setups[0].operations[0].geometry.faces[0].at = [40.0, 40.0, 40.0];
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
/// `strategy`.
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
        stepdown: 2.0,
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
        .filter(|m| m.role == crate::toolpath::MoveRole::Cut)
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

/// A pocket picked by its floor, the step's shelf: cleared to it within the
/// shelf's outline, the tool's side on the wall and nowhere past it, and the
/// result verifies. A tool too wide for it is refused.
#[test]
fn a_pocket_is_cleared_to_its_picked_floor() {
    use crate::features::find_face;
    use crate::job::FeatureRef;
    let body = Body::derived(synthetic::step(5.0, 10.0));
    let pick = FeatureRef {
        model: "dome".into(),
        index: 0,
        at: [5.0, 0.0, 0.0],
    };
    let face = find_face(&body, &pick, 0.01).unwrap().index;
    let pocket = StrategySpec::Pocket {
        stepdown: 2.0,
        conventional: false,
    };
    let (mut job, _) = in_a_sheet(pocket, &body);
    job.stocks[0].spec = crate::job::StockSpec::AroundModels {
        models: vec![],
        margin: [0.0, 0.0, 0.0],
        below: 0.0,
    };
    job.setups[0].operations[0].geometry.faces.push(FeatureRef {
        index: face,
        ..pick
    });
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
        .filter(|m| m.role == crate::toolpath::MoveRole::Cut)
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

    job.tools.get_mut("rough").unwrap().diameter = 25.0;
    let err = run_setup(&job, 0, &bodies).unwrap_err().to_string();
    assert!(err.contains("does not fit"), "{err}");
}
