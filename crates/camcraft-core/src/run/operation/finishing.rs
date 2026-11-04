//! The 3D finishing strategies' paths, each from its own settings and where the
//! tool may go.

use super::*;

/// Raster finishing, with the tool's centre kept to `limit` as well as the sweep.
pub(super) fn parallel_paths(
    work: &Work<'_>,
    angle: f64,
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    let (sweep_lo, sweep_hi) = work.sweep();
    let params = ParallelParams {
        watch: work.watch.clone(),
        stepover: op.passes.stepover,
        angle: angle.to_radians(),
        tolerance: job.tolerance,
        feed: op.passes.feed,
        max_cut_step: op.passes.max_cut_step,
        ..Default::default()
    };
    let (path, report) =
        parallel_finish_within(work.cutter, work.tool, sweep_lo, sweep_hi, limit, &params)
            .map_err(in_op(op))?;
    sampling_warnings(&report, warnings);
    Ok((path, report.passes, StrategyDetail::Parallel(report)))
}

/// What sampling passes on the part found that the path should say.
pub(super) fn sampling_warnings(report: &ParallelReport, warnings: &mut Vec<String>) {
    if !report.is_clean() {
        warnings.push(format!(
            "{} span(s) hit the refinement limit; the path is coarser than the \
             requested tolerance",
            report.refinement_limited
        ));
    }
}

/// Where a 3D finishing strategy may put the tool's centre: the sweep, kept to
/// `limit`, and, with `contact`, to where the tool touches the part.
pub(super) fn finish_area(
    work: &Work<'_>,
    limit: Option<&Region>,
    contact: bool,
) -> Result<Region, RunError> {
    let op = work.op;
    let (sweep_lo, sweep_hi) = work.sweep();
    let mut area = Region::rectangle(sweep_lo, sweep_hi);
    if contact {
        let shadow = work.prepared.shadow().map_err(in_op(op))?;
        let touched = contact_region(shadow, work.tool.max_radius(), work.job.tolerance)
            .map_err(in_op(op))?;
        area = area.intersection(&touched).map_err(in_op(op))?;
    }
    if let Some(l) = limit {
        area = area.intersection(l).map_err(in_op(op))?;
    }
    Ok(area)
}

/// Scallop finishing within the sweep, `limit` and where the tool touches the
/// part.
pub(super) fn scallop_paths(
    work: &Work<'_>,
    (direction, order): (CutDirection, PassOrder),
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    let area = finish_area(work, limit, true)?;
    let (slope_from, slope_to) = work.slopes();
    let params = ScallopParams {
        // The passes most of the work, the rings round the walls the rest.
        watch: work.watch.part(0.0, 0.75),
        stepover: op.passes.stepover,
        tolerance: job.tolerance,
        feed: op.passes.feed,
        max_cut_step: op.passes.max_cut_step,
        order: order.engine(),
        direction: direction.engine(),
        climb_right: work.clockwise(),
        slope_from,
        slope_to,
        ..Default::default()
    };
    let (mut path, mut report, band) =
        scallop_finish_banded(work.cutter, work.tool, &area, &params).map_err(in_op(op))?;
    // Its passes are laid on the surface as the drop-cutter sees it, which
    // goes from a wall's top to its foot at once: they leave out what is
    // steeper than a plunge where they cross a wall, and stop at its top where
    // the part ends below it. There, a stepover apart along the surface is a
    // stepover apart in height: rings round every such wall, a stepover apart
    // down it, each going no deeper than the last by more than that, as
    // Fusion's scallop finishes them.
    let (walls, rings) = steep_walls(work, limit, &band, (direction, slope_to))?;
    for m in walls.moves() {
        path.push(*m)
            .map_err(|e| in_op(op)(StrategyError::from(e)))?;
    }
    report.passes += rings;
    report.walls = rings;
    if report.passes == 0 {
        warnings.push("found nothing to finish: nothing within the boundary and slopes".into());
    }
    sampling_warnings(&report.sweep, warnings);
    if report.unreached > 0 {
        warnings.push(format!(
            "{} point(s) of the area were not reached from its boundary, and are not \
             finished",
            report.unreached
        ));
    }
    Ok((path, report.passes, StrategyDetail::Scallop(report)))
}

/// Rings round the walls a scallop's passes leave out, up to `slope_to`: a
/// level each stepover down them, times the sine of that slope, so the rings
/// are no further apart along the surface than the passes, from the top to the
/// bottom. Over `band`, where the surface the tool's centre rides is steeper
/// than a plunge, grown by the tool's radius so they overlap the passes either
/// side, and along where the part ends below an edge, at every slope there:
/// the band is the passes' own judgement of what they leave, gaps it closed
/// included, so the rings cut all of that and the passes none of it. Kept to the sweep and
/// `limit` as contour finishing is, not to where the tool touches the part,
/// whose edge a ring beside a wall runs right along. The rings, and how many.
fn steep_walls(
    work: &Work<'_>,
    limit: Option<&Region>,
    band: &Region,
    (direction, slope_to): (CutDirection, f64),
) -> Result<(Toolpath, u64), RunError> {
    let (job, op) = (work.job, work.op);
    let steep = PLUNGE_RATIO.atan();
    if slope_to <= steep {
        return Ok((Toolpath::new(), 0));
    }
    // As contour finishing finds its walls: coarser bands are enough to find
    // them, since every point of a ring is pulled onto the part exactly.
    let coarse = (10.0 * job.tolerance)
        .min(work.tool.max_radius() / 4.0)
        .max(job.tolerance);
    let mut reach = LevelReach::new(&work.prepared.part, work.tool, coarse).map_err(in_op(op))?;
    // The band the passes leave, grown to overlap them; and where the part
    // ends below an edge, which the passes stop at: within the tool's width
    // inside where it stops touching the part, and a little past, where the
    // lowest rings run. `touched` is drawn a tolerance short of that, its arcs
    // as chords inside them, so it is grown by two to be past it all along.
    let radius = work.tool.max_radius();
    let overlap = band.offset(radius, job.tolerance);
    let shadow = work.prepared.shadow().map_err(in_op(op))?;
    let touched = contact_region(shadow, radius, job.tolerance).map_err(in_op(op))?;
    let edge = touched
        .offset(2.0 * job.tolerance, 0.1 * job.tolerance)
        .difference(&touched.offset(-2.0 * radius, job.tolerance))
        .map_err(in_op(op))?;
    let area = finish_area(work, limit, false)?
        .intersection(&overlap.union(&edge).map_err(in_op(op))?)
        .map_err(in_op(op))?;
    let (top, bottom) = op.levels_3d(work.heights);
    let params = WaterlineParams {
        watch: work.watch.part(0.75, 1.0),
        stepdown: op.passes.stepover * steep.sin(),
        top,
        bottom,
        flats: Vec::new(),
        tolerance: job.tolerance,
        feed: op.passes.feed,
        direction: direction.engine(),
        climb_right: work.clockwise(),
        order: ContourOrder::ByDepth.engine(),
        slope_from: 0.0,
        slope_to,
        ramp: None,
    };
    let (path, report) =
        waterline(&mut reach, work.cutter, work.tool, &area, &params).map_err(in_op(op))?;
    Ok((path, report.passes))
}

/// Contour finishing: a waterline round the part at each level, within the
/// sweep and `limit`.
pub(super) fn contour_paths(
    work: &Work<'_>,
    (direction, order): (CutDirection, ContourOrder),
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    // The region need only find the walls: every point of a pass is pulled onto
    // the part exactly. Coarser bands, ten tolerances or a quarter of the tool,
    // make it an order of magnitude cheaper, and miss only gaps narrower than
    // that the tool would have squeezed into.
    let coarse = (10.0 * job.tolerance)
        .min(work.tool.max_radius() / 4.0)
        .max(job.tolerance);
    let mut reach = LevelReach::new(&work.prepared.part, work.tool, coarse).map_err(in_op(op))?;
    let area = finish_area(work, limit, false)?;
    let (top, bottom) = op.levels_3d(work.heights);
    let (slope_from, slope_to) = work.slopes();
    let params = WaterlineParams {
        watch: work.watch.clone(),
        // Checked with the job: it always has one.
        stepdown: op.stepdown().unwrap_or(f64::NAN),
        top,
        bottom,
        flats: work.flat_levels(true),
        tolerance: job.tolerance,
        feed: op.passes.feed,
        direction: direction.engine(),
        climb_right: work.clockwise(),
        order: order.engine(),
        slope_from,
        slope_to,
        // Down the wall from level to level, as Fusion's smooth transition,
        // where it may stay down at all.
        ramp: (op.stay_down(work.spec.diameter) > 0.0).then(|| op.linking.ramp_angle.to_radians()),
    };
    let (path, report) =
        waterline(&mut reach, work.cutter, work.tool, &area, &params).map_err(in_op(op))?;
    if report.passes == 0 {
        warnings.push(
            "found nothing to finish: no wall within the boundary, heights and slopes".into(),
        );
    }
    Ok((path, report.passes, StrategyDetail::Contour(report)))
}

/// Flat finishing: each flat face between the top and bottom cleared at its
/// height, within the sweep and `limit`.
pub(super) fn flat_paths(
    work: &Work<'_>,
    order: PassOrder,
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    let mut reach =
        LevelReach::new(&work.prepared.part, work.tool, job.tolerance).map_err(in_op(op))?;
    let area = finish_area(work, limit, false)?;
    let (top, bottom) = op.levels_3d(work.heights);
    let slack = job.tolerance;
    let params = FlatParams {
        watch: work.watch.clone(),
        stepover: op.passes.stepover,
        tolerance: job.tolerance,
        feed: op.passes.feed,
        flats: work
            .flat_levels(true)
            .into_iter()
            .filter(|z| (bottom - slack..=top + slack).contains(z))
            .collect(),
        kept_on_right: work.kept_on_right(),
        order: order.engine(),
        min_ring_length: 2.0 * job.tolerance,
    };
    let (path, report) = flat_finish(&mut reach, &area, &params).map_err(in_op(op))?;
    if report.passes == 0 {
        warnings.push(if report.flats == 0 {
            "found nothing to finish: the part has no flat face between the top and \
             bottom"
                .into()
        } else {
            "found nothing to finish: no flat within the boundary has room for the tool".into()
        });
    }
    Ok((path, report.passes, StrategyDetail::Flat(report)))
}

/// A pattern about a centre's centre, and its inner and outer radii: as given,
/// or the middle of `area` and out to all of it.
pub(super) fn centred(
    area: &Region,
    (centre, inner, outer): (Option<[f64; 2]>, f64, f64),
) -> Option<Centred> {
    let bounds = area.bounds()?;
    let centre = centre.map_or((bounds.0 + bounds.1) / 2.0, |[x, y]| dvec2(x, y));
    let outer = if outer > 0.0 {
        outer
    } else {
        reaching(centre, bounds)
    };
    (outer > inner).then_some(Centred {
        centre,
        inner,
        outer,
    })
}

/// How a pattern is dropped onto the part, for `work`'s operation.
pub(super) fn project_params(work: &Work<'_>) -> ProjectParams {
    let (slope_from, slope_to) = work.slopes();
    ProjectParams {
        watch: work.watch.clone(),
        stepover: work.op.passes.stepover,
        tolerance: work.job.tolerance,
        feed: work.op.passes.feed,
        max_cut_step: work.op.passes.max_cut_step,
        slope_from,
        slope_to,
    }
}

/// `curves` dropped onto the part within `area`, and what that found to say.
pub(super) fn projected(
    work: &Work<'_>,
    area: &Region,
    curves: &[Curve],
    warnings: &mut Vec<String>,
) -> Result<(Toolpath, ProjectReport), RunError> {
    let (path, report) = project(work.cutter, work.tool, area, curves, &project_params(work))
        .map_err(in_op(work.op))?;
    if report.passes == 0 {
        warnings.push("found nothing to finish: nothing within the boundary and slopes".into());
    }
    sampling_warnings(&report.sweep, warnings);
    Ok((path, report))
}

/// Spiral finishing within the sweep, `limit` and where the tool touches the
/// part.
pub(super) fn spiral_paths(
    work: &Work<'_>,
    circle: (Option<[f64; 2]>, f64, f64),
    order: PassOrder,
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let area = finish_area(work, limit, true)?;
    let curves = match centred(&area, circle) {
        Some(on) => {
            let (outward, ccw) = work.turning(order);
            spiral(&on, op.passes.stepover, work.job.tolerance, outward, ccw).map_err(in_op(op))?
        }
        None => Vec::new(),
    };
    let (path, report) = projected(work, &area, &curves, warnings)?;
    Ok((path, report.passes, StrategyDetail::Spiral(report)))
}

/// Radial finishing within the sweep, `limit` and where the tool touches the
/// part.
pub(super) fn radial_paths(
    work: &Work<'_>,
    circle: (Option<[f64; 2]>, f64, f64),
    (angle_from, angle_to, direction): (f64, f64, RadialDirection),
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let area = finish_area(work, limit, true)?;
    let curves = match centred(&area, circle) {
        Some(on) => radial(
            &on,
            op.passes.stepover,
            (angle_from.to_radians(), angle_to.to_radians()),
            direction.engine(),
        )
        .map_err(in_op(op))?,
        None => Vec::new(),
    };
    let (path, report) = projected(work, &area, &curves, warnings)?;
    Ok((path, report.passes, StrategyDetail::Radial(report)))
}

/// Morphed spiral finishing within the sweep, `limit` and where the tool
/// touches the part.
pub(super) fn morphed_spiral_paths(
    work: &Work<'_>,
    centre: Option<[f64; 2]>,
    order: PassOrder,
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let area = finish_area(work, limit, true)?;
    let (outward, ccw) = work.turning(order);
    let params = MorphParams {
        stepover: op.passes.stepover,
        centre: centre.map(|[x, y]| dvec2(x, y)),
        outward,
        ccw,
        ..Default::default()
    };
    let (curves, pattern) = morphed_spiral(&area, &params).map_err(in_op(op))?;
    if pattern.uneven() {
        warnings.push(format!(
            "the laps are up to {:.3} mm apart, more than the stepover: the boundary \
             is too far from its centre in places for a spiral to reach evenly",
            pattern.widest
        ));
    }
    let (path, cut) = projected(work, &area, &curves, warnings)?;
    Ok((
        path,
        cut.passes,
        StrategyDetail::MorphedSpiral(MorphedSpiralReport { pattern, cut }),
    ))
}
