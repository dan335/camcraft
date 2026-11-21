//! The 3D finishing strategies' paths, each from its own settings and where the
//! tool may go.

use super::*;

/// Raster finishing, with the tool's centre kept to `limit` as well as the
/// sweep, and off the steep ground, which rings round the walls finish
/// ([`steep_band_of`]).
pub(super) fn parallel_paths(
    work: &Work<'_>,
    angle: f64,
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    let (sweep_lo, sweep_hi) = work.sweep();
    let (_, slope_to) = work.slopes();
    let band = steep_band_of(work, limit, slope_to)?;
    let kept = match &band {
        Some(band) => Some(
            limit
                .cloned()
                .unwrap_or_else(|| Region::rectangle(sweep_lo, sweep_hi))
                .difference(band)
                .map_err(in_op(op))?,
        ),
        None => limit.cloned(),
    };
    let params = ParallelParams {
        watch: work.watch.part(BAND_SHARE, PASSES_SHARE),
        stepover: op.passes.stepover,
        angle: angle.to_radians(),
        tolerance: job.tolerance,
        feed: op.passes.feed,
        max_cut_step: op.max_cut_step(),
        ..Default::default()
    };
    let (mut path, mut report) =
        parallel_finish_within(&work.seat()?, sweep_lo, sweep_hi, kept.as_ref(), &params)
            .map_err(in_op(op))?;
    report.walls = with_walls(work, limit, band.as_ref(), slope_to, &mut path)?;
    report.passes += report.walls;
    sampling_warnings(&report, warnings);
    Ok((path, report.passes, StrategyDetail::Parallel(report)))
}

/// How much of a finish over the part finding its steep ground is, as it says
/// how far it has got; then its passes, to [`PASSES_SHARE`]; then the rings
/// round its walls.
const BAND_SHARE: f64 = 0.15;

/// See [`BAND_SHARE`].
const PASSES_SHARE: f64 = 0.75;

/// The ground passes laid over the part leave to rings round the walls
/// ([`steep_walls`]): where the surface the tool's centre
/// rides is steeper than a plunge, over where it touches the part
/// ([`steep_ground`]). A pass cut down such ground touches a wall only where
/// it crosses it, so a wall along the passes is left in steps a stepover deep;
/// rings round it, a stepover apart down it, finish it all. `None` where the
/// slope range asked for stops short of a plunge's: the passes keep to it, and
/// there are no walls.
fn steep_band_of(
    work: &Work<'_>,
    limit: Option<&Region>,
    slope_to: f64,
) -> Result<Option<Region>, RunError> {
    if slope_to <= PLUNGE_RATIO.atan() {
        return Ok(None);
    }
    let area = finish_area(work, limit, true)?;
    steep_ground(
        &work.seat()?,
        &area,
        work.op.passes.stepover,
        ScallopParams::default().max_nodes,
        &work.watch.part(0.0, BAND_SHARE),
    )
    .map(Some)
    .map_err(in_op(work.op))
}

/// Rings round the walls of `band` ([`steep_walls`]) after `path`, up to
/// `slope_to`; how many. None without a band.
fn with_walls(
    work: &Work<'_>,
    limit: Option<&Region>,
    band: Option<&Region>,
    slope_to: f64,
    path: &mut Toolpath,
) -> Result<u64, RunError> {
    let Some(band) = band else {
        return Ok(0);
    };
    let (walls, rings) = steep_walls(work, limit, band, (CutDirection::Climb, slope_to))?;
    path.append(walls.moves()).map_err(in_op(work.op))?;
    Ok(rings)
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
        area = area.intersection(&work.contact()?).map_err(in_op(op))?;
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
    // Up to a tolerance past where the tool leaves the part: its field finds
    // that line itself, and its last ring runs along it.
    let shadow = work.prepared.shadow().map_err(in_op(op))?;
    let past = work
        .surfaces
        .past_contact(shadow, work.tool, job.tolerance)
        .map_err(in_op(op))?;
    let area = finish_area(work, limit, false)?
        .intersection(&past)
        .map_err(in_op(op))?;
    let (slope_from, slope_to) = work.slopes();
    let (_, bottom) = op.levels_3d(work.heights);
    let params = ScallopParams {
        // The field most of the work, the rings below it the rest.
        watch: work.watch.part(0.0, PASSES_SHARE),
        // The linking's default, as the rings below it ramp: a scallop
        // takes no ramp setting (decision 0034).
        ramp: crate::job::Linking::default().ramp_angle.to_radians(),
        stepover: op.passes.stepover,
        tolerance: job.tolerance,
        feed: op.passes.feed,
        max_cut_step: op.max_cut_step(),
        order: order.engine(),
        direction: direction.engine(),
        climb_right: work.clockwise(),
        slope_from,
        slope_to,
        bottom,
        ..Default::default()
    };
    // Its passes go up and down walls as they go over the rest: on steep
    // ground each point is found round the wall at the height the field puts
    // it at. What the field cannot cover is where the tool is below where it
    // would rest from above: down the part's outside below where it last
    // touches it from above, which has no width seen from above, and under
    // what overhangs. There height is the only measure, and level rings a
    // stepover apart in it finish it.
    let (field, mut report) = scallop_finish(&work.seat()?, &area, &params).map_err(in_op(op))?;
    let (below, rings) = below_reach(
        work,
        limit,
        report.walls_from,
        direction,
        (slope_from, slope_to),
    )?;
    report.walls = rings;
    report.passes += rings;
    // The field's rings, from its first edge on, and then on down the wall
    // below them; from the boundary in, from the top of the wall down to it
    // and then in.
    let mut path = Toolpath::new();
    let parts = match order {
        PassOrder::InsideOut => [&field, &below],
        PassOrder::OutsideIn => [&below, &field],
    };
    for part in parts {
        path.append(part.moves()).map_err(in_op(op))?;
    }
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

/// Level rings a stepover apart down the walls a scallop's field cannot cover,
/// from a stepover below `from`, the scallop's
/// [`walls_from`](crate::strategy::scallop::ScallopReport::walls_from): the highest
/// point of where the tool leaves the part from above, the field's last ring,
/// or, where rings blend down from that line, half a stepover under its
/// lowest, so each goes all the way round; to the Bottom, within
/// the sweep and `limit`, as contour finishing finds them, each way as
/// `direction` asks, within the slopes asked: kept only where the tool on
/// them is more than the tolerance below where it would rest dropped from
/// above, or has nothing under it. Above that the field's rings run, on to
/// the line where the tool leaves the part: so nothing is cut twice, and
/// nothing between that line and the first ring below it is wider than a
/// stepover. Each level's last ring goes down along the wall onto the next,
/// at the linking's default ramp angle, as contour finishing's do. The
/// rings, and how many.
fn below_reach(
    work: &Work<'_>,
    limit: Option<&Region>,
    from: Option<f64>,
    direction: CutDirection,
    (slope_from, slope_to): (f64, f64),
) -> Result<(Toolpath, u64), RunError> {
    let (job, op) = (work.job, work.op);
    let (top, bottom) = op.levels_3d(work.heights);
    // Where the tool never leaves the part from above, it rests on it
    // everywhere, and the field covers it all.
    let Some(top) = from.map(|z| z.min(top)) else {
        return Ok((Toolpath::new(), 0));
    };
    if top <= bottom {
        return Ok((Toolpath::new(), 0));
    }
    // As contour finishing finds its walls: coarser bands are enough to find
    // them, since every point of a ring is pulled onto the part exactly.
    let coarse = (10.0 * job.tolerance)
        .min(work.tool.max_radius() / 4.0)
        .max(job.tolerance);
    let mut reach = work.reach(coarse)?;
    let seat = work.seat()?;
    let area = finish_area(work, limit, false)?;
    let params = WaterlineParams {
        watch: work.watch.part(PASSES_SHARE, 1.0),
        stepdown: op.passes.stepover,
        top,
        bottom,
        flats: Vec::new(),
        tolerance: job.tolerance,
        feed: op.passes.feed,
        direction: direction.engine(),
        climb_right: work.clockwise(),
        order: ContourOrder::ByDepth.engine(),
        slope_from,
        slope_to,
        ramp: Some(crate::job::Linking::default().ramp_angle.to_radians()),
    };
    let (rings, _) = waterline(&mut reach, &seat, &area, &params).map_err(in_op(op))?;
    let tolerance = job.tolerance;
    // Where the tool would rest from above within the tolerance of the ring,
    // it is on the surface the field's rings run over, down to where the tool
    // leaves the part: the field's.
    let covered = |p: DVec3| seat.drop_at(p.x, p.y).is_some_and(|z| z <= p.z + tolerance);
    // Looked along as finely as a pass is sampled where it is flat.
    let step = (seat.tool().max_radius() * 0.5).max(tolerance);
    // Where the wall's top is not level, its upper rings are arcs, each
    // level's in as many pieces as the wall has stretches below the field.
    // They stay level by level, from the top down, so no ring is cut under
    // material a ring above it has yet to take.
    let (kept, _) = cut_out(&rings, covered, step, tolerance).map_err(in_op(op))?;
    // Where the line the tool leaves the part along dips only just past a
    // ring, the ring is left a bit shorter than the tool's radius there: a
    // dimple between the field's last ring and the ring below, lifted to and
    // from. Left to those, as the field leaves its own.
    let mut long = Toolpath::new();
    let mut passes = 0;
    let mut at: Option<DVec3> = None;
    for pass in kept.passes() {
        let mut from = at;
        let mut cut = 0.0;
        for m in pass {
            if m.role == crate::path::toolpath::MoveRole::Cut
                && let Some(f) = from
            {
                cut += m.length(f);
            }
            from = Some(m.to);
        }
        at = from;
        if cut >= seat.tool().max_radius() {
            long.append(pass).map_err(in_op(op))?;
            passes += 1;
        }
    }
    Ok((long, passes))
}

/// Rings round the walls passes laid over the part leave out, up to `slope_to`: a
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
    let mut reach = work.reach(coarse)?;
    // The band the passes leave, grown to overlap them; and where the part
    // ends below an edge, which the passes stop at: within the tool's width
    // inside where it stops touching the part, and a little past, where the
    // lowest rings run. `touched` is drawn a tolerance short of that, its arcs
    // as chords inside them, so it is grown by two to be past it all along.
    let radius = work.widest();
    let overlap = band.offset(radius, job.tolerance);
    let touched = work.contact()?;
    let edge = touched
        .offset(2.0 * job.tolerance, 0.1 * job.tolerance)
        .difference(&touched.offset(-2.0 * radius, job.tolerance))
        .map_err(in_op(op))?;
    let area = finish_area(work, limit, false)?
        .intersection(&overlap.union(&edge).map_err(in_op(op))?)
        .map_err(in_op(op))?;
    let (top, bottom) = op.levels_3d(work.heights);
    let params = WaterlineParams {
        watch: work.watch.part(PASSES_SHARE, 1.0),
        stepdown: op.passes.stepover * steep.sin(),
        top,
        bottom,
        // A ring at each flat face as well, as contour finishing has: the
        // floor beside a wall is the rings' where the band takes it in, as in
        // a channel narrower than the tool's width twice over, and a ring a
        // level above it leaves a step on it.
        flats: work.flat_levels(Flats::All),
        tolerance: job.tolerance,
        feed: op.passes.feed,
        direction: direction.engine(),
        climb_right: work.clockwise(),
        order: ContourOrder::ByDepth.engine(),
        slope_from: 0.0,
        slope_to,
        ramp: None,
    };
    let (path, report) = waterline(&mut reach, &work.seat()?, &area, &params).map_err(in_op(op))?;
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
    let mut reach = work.reach(coarse)?;
    let area = finish_area(work, limit, false)?;
    let (top, bottom) = op.levels_3d(work.heights);
    let (slope_from, slope_to) = work.slopes();
    let params = WaterlineParams {
        watch: work.watch.clone(),
        // Checked with the job: it always has one.
        stepdown: op.stepdown().unwrap_or(f64::NAN),
        top,
        bottom,
        flats: work.flat_levels(Flats::All),
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
    let (path, report) = waterline(&mut reach, &work.seat()?, &area, &params).map_err(in_op(op))?;
    if report.passes == 0 {
        warnings.push(
            "found nothing to finish: no wall within the boundary, heights and slopes".into(),
        );
    }
    Ok((path, report.passes, StrategyDetail::Contour(report)))
}

/// Ramp finishing: each wall gone down without stopping, a stepdown each time
/// round, within the sweep and `limit`, from contour finishing's rings
/// ([decision 0027]).
///
/// [decision 0027]: ../../../../../docs/decisions/0027-ramp.md
pub(super) fn ramp_paths(
    work: &Work<'_>,
    (direction, flat_areas): (CutDirection, bool),
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    // Drawn at three tolerances, not contour finishing's ten: every point of
    // a ring is pulled onto the part either way, but which rings there are
    // says where a wall goes on down, and the coarser region closes gaps the
    // tool fits through, joining walls that are apart: on `ops_3d`, the
    // 0.11 mm between the dome's foot and the pocket's fillets at Z10
    // (decision 0027). At three, a ball's region has a third of the bands it
    // would at one, and overstates by about three tolerances.
    let fine = (3.0 * job.tolerance)
        .min(work.tool.max_radius() / 4.0)
        .max(job.tolerance);
    let mut reach = work.reach(fine)?;
    let area = finish_area(work, limit, false)?;
    let (top, bottom) = op.levels_3d(work.heights);
    let (slope_from, slope_to) = work.slopes();
    let params = RampParams {
        watch: work.watch.clone(),
        // Checked with the job: it always has one.
        stepdown: op.stepdown().unwrap_or(f64::NAN),
        top,
        bottom,
        flats: work.flat_levels(Flats::all_if(flat_areas)),
        tolerance: job.tolerance,
        feed: op.passes.feed,
        direction: direction.engine(),
        climb_right: work.clockwise(),
        slope_from,
        slope_to,
        // Fusion's Ramping Angle, on its Linking tab: the steepest the tool
        // goes down a wall, where a stepdown each time round would be
        // steeper.
        ramp: op.linking.ramp_angle.to_radians(),
    };
    let (path, report) = ramp(&mut reach, &work.seat()?, &area, &params).map_err(in_op(op))?;
    if report.passes == 0 {
        warnings.push(
            "found nothing to finish: no wall within the boundary, heights and slopes".into(),
        );
    }
    Ok((path, report.passes, StrategyDetail::Ramp(report)))
}

/// What a pencil is asked: which way its lines cut, its bitangency angle in
/// degrees, its overthickness and how many stepovers beside its lines.
pub(super) struct PencilAsked {
    pub(super) direction: CutDirection,
    pub(super) bitangency_angle: f64,
    pub(super) overthickness: f64,
    pub(super) stepovers: u32,
}

/// Pencil finishing: the tool run along where it touches the part in two
/// places at once, within the sweep, `limit` and where it touches the part
/// ([decision 0028]).
///
/// [decision 0028]: ../../../../../docs/decisions/0028-pencil.md
pub(super) fn pencil_paths(
    work: &Work<'_>,
    asked: &PencilAsked,
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    let area = finish_area(work, limit, true)?;
    // The lines are found for the tool grown all round by the overthickness,
    // on top of its stock to leave, and cut with the tool as it cuts.
    let stock = work.tool.allowance();
    let finder = EffectiveTool::of(
        work.tool.base(),
        Allowance::new(
            stock.radial() + asked.overthickness,
            stock.axial() + asked.overthickness,
        )?,
    )?;
    let finder = work.surfaces.under(&finder)?;
    let params = PencilParams {
        watch: work.watch.clone(),
        tolerance: job.tolerance,
        feed: op.passes.feed,
        bitangency: asked.bitangency_angle.to_radians(),
        direction: asked.direction.engine(),
        climb_right: work.clockwise(),
        stepovers: asked.stepovers,
        stepover: op.passes.stepover,
        max_cut_step: op.max_cut_step(),
        ..Default::default()
    };
    let (path, report) = pencil(&work.seat()?, &finder, &area, &params).map_err(in_op(op))?;
    if report.passes == 0 {
        warnings.push(
            "found nothing to finish: the tool touches the part in two places nowhere \
             within the boundary at the bitangency angle"
                .into(),
        );
    }
    sampling_warnings(&report.sweep, warnings);
    Ok((path, report.passes, StrategyDetail::Pencil(report)))
}

/// Flat finishing: each flat face it machines between the top and bottom
/// cleared at its height, within the sweep and `limit`.
pub(super) fn flat_paths(
    work: &Work<'_>,
    order: PassOrder,
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    let mut reach = work.reach(job.tolerance)?;
    let area = finish_area(work, limit, false)?;
    let (top, bottom) = op.levels_3d(work.heights);
    let slack = job.tolerance;
    let params = FlatParams {
        watch: work.watch.clone(),
        stepover: op.passes.stepover,
        tolerance: job.tolerance,
        feed: op.passes.feed,
        flats: work
            .flat_levels(Flats::Machined)
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
            "found nothing to finish: the part has no flat face it machines between the \
             top and bottom"
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
        max_cut_step: work.op.max_cut_step(),
        slope_from,
        slope_to,
        raise: 0.0,
    }
}

/// Project finishing: `curves`, picked on the part, each dropped onto it and
/// cut along where the tool rests there, raised by `axial_offset`, within the
/// sweep, `limit` and where the tool touches the part ([decision 0029]).
///
/// [decision 0029]: ../../../../../docs/decisions/0029-project.md
pub(super) fn project_paths(
    work: &Work<'_>,
    (direction, axial_offset): (ProjectDirection, f64),
    (limit, curves): (Option<&Region>, &[Picked]),
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let area = finish_area(work, limit, true)?;
    let params = ProjectCurvesParams {
        drop: ProjectParams {
            // A curve has no stepover: each is seeded every quarter of the
            // tool's width, as a link over the part is, so nothing that narrow
            // is passed over, and refined from there to the tolerance.
            stepover: (0.5 * work.tool.max_radius()).max(work.job.tolerance),
            raise: axial_offset,
            ..project_params(work)
        },
        way: direction.engine(),
    };
    let (path, report) =
        project_curves(&work.seat()?, &area, curves, &params).map_err(in_op(op))?;
    if report.passes == 0 {
        warnings.push(
            "found nothing to finish: no picked curve passes over the part within the boundary \
             and slopes"
                .into(),
        );
    }
    sampling_warnings(&report.sweep, warnings);
    Ok((path, report.passes, StrategyDetail::Project(report)))
}

/// Morph finishing: passes between the two `curves` picked on the part, each
/// a step of the way from the one to the other, dropped onto the part within
/// the sweep, `limit` and where the tool touches the part ([decision 0030]).
///
/// [decision 0030]: ../../../../../docs/decisions/0030-morph.md
pub(super) fn morph_paths(
    work: &Work<'_>,
    direction: ProjectDirection,
    (limit, curves): (Option<&Region>, &[Picked]),
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let area = finish_area(work, limit, true)?;
    let params = MorphCurvesParams {
        drop: project_params(work),
        way: direction.engine(),
    };
    let (path, report) = morph_curves(&work.seat()?, &area, curves, &params).map_err(in_op(op))?;
    if report.cut.passes == 0 {
        warnings.push(
            "found nothing to finish: no pass between the picked curves passes over the part \
             within the boundary and slopes"
                .into(),
        );
    }
    sampling_warnings(&report.cut.sweep, warnings);
    Ok((path, report.cut.passes, StrategyDetail::Morph(report)))
}

/// `curves` dropped onto the part within `area`, kept to `limit`, off the
/// steep ground, which rings round the walls finish ([`steep_band_of`]); and
/// what that found to say.
pub(super) fn projected(
    work: &Work<'_>,
    (area, limit): (&Region, Option<&Region>),
    curves: &[Curve],
    warnings: &mut Vec<String>,
) -> Result<(Toolpath, ProjectReport), RunError> {
    let op = work.op;
    let (_, slope_to) = work.slopes();
    let band = steep_band_of(work, limit, slope_to)?;
    let kept = match &band {
        Some(band) => area.difference(band).map_err(in_op(op))?,
        None => area.clone(),
    };
    let params = ProjectParams {
        watch: work.watch.part(BAND_SHARE, PASSES_SHARE),
        ..project_params(work)
    };
    let (mut path, mut report) =
        project(&work.seat()?, &kept, curves, &params).map_err(in_op(op))?;
    report.walls = with_walls(work, limit, band.as_ref(), slope_to, &mut path)?;
    report.passes += report.walls;
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
    let (path, report) = projected(work, (&area, limit), &curves, warnings)?;
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
    let (path, report) = projected(work, (&area, limit), &curves, warnings)?;
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
    let (path, cut) = projected(work, (&area, limit), &curves, warnings)?;
    Ok((
        path,
        cut.passes,
        StrategyDetail::MorphedSpiral(MorphedSpiralReport { pattern, cut }),
    ))
}
