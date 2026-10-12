//! The paths of the strategies that cut level by level or straight down:
//! clearing, 2D cutting, facing and drilling, and what they are cut to.

use super::*;

/// Pocket clearing of the material as it stands, a level each stepdown and, with
/// flat areas, one on each flat face, within the sweep and `limit`, dropping rings
/// shorter than `min_ring_length`.
pub(super) fn pocket_clearing_paths(
    work: &Work<'_>,
    min_ring_length: f64,
    flat_areas: bool,
    limit: Option<&Region>,
    stock: &Stock,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    let mut reach =
        LevelReach::new(&work.prepared.part, work.tool, job.tolerance).map_err(in_op(op))?;
    let (top, bottom) = op.levels_3d(work.heights);
    let params = ClearingParams {
        // Checked with the job: it always has one.
        stepdown: op.stepdown().unwrap_or(f64::NAN),
        stepover: op.passes.stepover,
        feed: op.passes.feed,
        top,
        bottom,
        min_ring_length,
        kept_on_right: work.kept_on_right(),
        flats: work.flat_levels(flat_areas),
    };
    let (sweep_lo, sweep_hi) = work.sweep();
    let sweep = Region::rectangle(sweep_lo, sweep_hi);
    let base = *work.tool.base();
    let (path, report) = pocket_clearing(&mut reach, &sweep, limit, stock.field(), &base, &params)
        .map_err(in_op(op))?;
    if report.rings == 0 {
        warnings.push(
            "found nothing to clear: the stock may already be below this \
             operation's top"
                .into(),
        );
    }
    Ok((path, report.rings, StrategyDetail::PocketClearing(report)))
}

/// Adaptive clearing of the stock's blocks, a level each stepdown and one on
/// each flat face, within the sweep and `limit`.
pub(super) fn adaptive_paths(
    work: &Work<'_>,
    direction: CutDirection,
    flat_areas: bool,
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    let mut reach =
        LevelReach::new(&work.prepared.part, work.tool, job.tolerance).map_err(in_op(op))?;
    let diameter = work.spec.diameter;
    let (top, bottom) = op.levels_3d(work.heights);
    let flats = work.flat_levels(flat_areas);
    // Into a closed area it goes down a helix, or straight: it offers no ramp.
    let helix_radius = match op.entry() {
        Some(Entry::Plunge) => 0.0,
        _ => op.helix_diameter(diameter, work.spec.corner_radius()) / 2.0,
    };
    let params = AdaptiveParams {
        // Checked with the job: it always has one.
        stepdown: op.stepdown().unwrap_or(f64::NAN),
        optimal_load: op.passes.stepover,
        feed: op.passes.feed,
        top,
        bottom,
        tolerance: job.tolerance,
        climb_right: work.clockwise(),
        direction: direction.engine(),
        helix_radius,
        stay_down: op.stay_down(diameter),
        ramp_angle: op.linking.ramp_angle.to_radians(),
        ramp_feed: op.ramp_feed(),
        ramp_stepdown: Some(op.ramp_stepdown(diameter)),
        safe_distance: op.linking.safe_distance,
        flats,
    };
    // Room past the stock's sides for the whole tool, so a pass can start
    // beside the block rather than going down into it.
    let (lo, hi) = work.bounds;
    let room = work.tool.max_radius()
        + work.tool.cutter_radius()
        + op.boundary.margin.max(0.0)
        + job.tolerance;
    let sweep = Region::rectangle(lo - DVec2::splat(room), hi + DVec2::splat(room));
    let area = match limit {
        Some(l) => sweep.intersection(l).map_err(in_op(op))?,
        None => sweep,
    };
    let base = *work.tool.base();
    let (path, report) =
        adaptive_rough(&mut reach, &area, work.blocks, &base, &params).map_err(in_op(op))?;
    if report.passes == 0 {
        warnings.push(
            "found nothing to clear: the stock may already be below this \
             operation's top"
                .into(),
        );
    }
    if report.no_room > 0 {
        warnings.push(format!(
            "{} area(s) have no room for the tool to go down into, and are left",
            report.no_room
        ));
    }
    if report.left_mm2 > 1.0 {
        warnings.push(format!(
            "{:.0} mm² the tool could reach was left uncleared",
            report.left_mm2
        ));
    }
    Ok((path, report.passes, StrategyDetail::Adaptive(report)))
}

/// A 2D profile or pocket of `areas`.
pub(super) fn planar_paths(
    work: &Work<'_>,
    areas: &[Area],
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let diameter = work.spec.diameter;
    let params = PlanarParams {
        stepdown: op.stepdown(),
        stepover: op.passes.stepover,
        feed: op.passes.feed,
        arc: work.job.tolerance * 0.5,
        radius: work.tool.cutter_radius(),
        kept_on_right: work.kept_on_right(),
        // A profile's leads go onto every ring; a pocket's, as Fusion's 2D
        // pocket's, onto its first finishing pass and off its last.
        leads: match op.strategy {
            StrategySpec::Pocket { .. } => op
                .finishing(diameter)
                .and(op.linking.leads)
                .map(|l| l.sizes(diameter)),
            _ => op.leads(diameter),
        }
        .map(|l| LeadArc {
            radius: l.radius,
            sweep: l.sweep.to_radians(),
            straight: l.straight,
        }),
        // Ramped into, a profile goes down each ring in one spiral, as Fusion's;
        // a pocket ramps along its rings as the linker enters them.
        spiral: (matches!(op.strategy, StrategySpec::Profile { .. })
            && op.entry() == Some(Entry::Ramp))
        .then(|| Spiral {
            angle: op.linking.ramp_angle.to_radians(),
            feed: op.ramp_feed(),
            most: Some(op.ramp_stepdown(diameter)),
            above: op.linking.safe_distance,
        }),
        finishing: op.finishing(diameter).map(|f| WallFinish {
            passes: f.passes,
            stepover: f.stepover,
            feed: f.feed,
            repeat: f.repeat,
        }),
    };
    let (path, report, detail): (Toolpath, PlanarReport, fn(PlanarReport) -> StrategyDetail) =
        match op.strategy {
            StrategySpec::Profile { side, .. } => {
                let (path, report) = profile(areas, side, &params).map_err(in_op(op))?;
                (path, report, StrategyDetail::Profile)
            }
            _ => {
                let (path, report) = pocket(areas, &params).map_err(in_op(op))?;
                (path, report, StrategyDetail::Pocket)
            }
        };
    if report.no_room_to_lead > 0 {
        warnings.push(format!(
            "{} ring(s) have no room for a lead in the waste beside them, and are \
             entered and left without",
            report.no_room_to_lead
        ));
    }
    if report.too_shallow > 0 {
        warnings.push(format!(
            "{} area(s) have their floor at or above their top, and are not cut",
            report.too_shallow
        ));
    }
    Ok((path, report.passes, detail(report)))
}

/// The stock's top faced: its footprint and the stock offset, from the top down
/// to the bottom, the model's top unless another is asked for, raised by the
/// stock to leave down; the passes at `angle`, cut the way `direction` says, and
/// run on `pass_extension` past the stock.
pub(super) fn face_paths(
    work: &Work<'_>,
    (angle, direction): (f64, FaceDirection),
    (pass_extension, stock_offset): (f64, f64),
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let (lo, hi) = work.bounds;
    let grow = DVec2::splat(stock_offset);
    let (top, bottom) = op.levels_3d(work.heights);
    let params = FaceParams {
        area: (lo - grow, hi + grow),
        angle: angle.to_radians(),
        stepover: op.passes.stepover,
        extension: pass_extension,
        radius: work.tool.cutter_radius(),
        levels: LevelsAsked {
            top,
            bottom: bottom + op.stock_to_leave().axial,
            stepdown: op.stepdown(),
        },
        direction,
        clockwise: work.clockwise(),
        feed: op.passes.feed,
    };
    let (path, report) = face(&params).map_err(in_op(op))?;
    if report.too_shallow > 0 {
        warnings.push(format!(
            "the bottom is not below the top, {:.3} mm, so nothing is faced",
            params.levels.top
        ));
    }
    Ok((path, report.passes, StrategyDetail::Face(report)))
}

/// A 2D operation's areas: its boundary, which the tool's centre follows its
/// radius and the stock to leave beside, or inside for a pocket, split into its
/// pieces, each with its top and floor ([`Operation::levels_2d`]); the floor
/// raised by the stock to leave down.
pub(in crate::run) fn planar_areas(
    job: &Job,
    op: &Operation,
    picking: &Picking<'_>,
    bodies: &Bodies<'_>,
    ctx: &HeightContext,
    tool: &EffectiveTool,
) -> Result<Vec<Area>, RunError> {
    let prepared = picking.prepared();
    let (shape, picked) = picking.outline(op).map_err(in_op(op))?;
    let shape = shape.unwrap_or_else(|| {
        let b = prepared.stock_bounds();
        Region::rectangle(b.min.truncate(), b.max.truncate())
    });
    let leave = op.stock_to_leave();
    let offset = centre_offset(op, tool.cutter_radius());
    let levels = |picks: Option<Picks>| op.levels_2d(ctx, picks);
    let circles = level_circles(prepared, bodies);
    let mut found = areas(
        &shape,
        &circles,
        offset,
        job.tolerance * 0.5,
        &picked,
        &levels,
    )
    .map_err(in_op(op))?;
    if found.is_empty() {
        return Err(in_op(op)(FeatureError::TooWide {
            diameter: 2.0 * tool.cutter_radius(),
        }));
    }
    for a in &mut found {
        a.bottom += leave.axial;
    }
    Ok(found)
}

/// What `op`'s heights resolve against: the setup's `ctx`, and how high its
/// picks, `picking`, go when a height is measured from them.
pub(in crate::run) fn heights_of(
    op: &Operation,
    picking: &Picking<'_>,
    ctx: &HeightContext,
) -> Result<HeightContext, RunError> {
    if !op.heights.measured_from_selection() {
        return Ok(*ctx);
    }
    Ok(HeightContext {
        picked: picking.picks().map_err(in_op(op))?,
        ..*ctx
    })
}

/// Each of `holes` drilled straight down, pecking by `peck` if given; a hole the
/// tool is not the size of is drilled, and said.
pub(super) fn drill_paths(
    work: &Work<'_>,
    peck: Option<f64>,
    dwell: Option<f64>,
    holes: &[Hole],
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    let params = DrillParams {
        peck,
        feed: op.linking.plunge_feed,
        retract_feed: op.linking.retract_feed,
    };
    let path = drill(holes, &params).map_err(in_op(op))?;
    let width = work.spec.diameter;
    for h in holes {
        if (2.0 * h.radius - width).abs() > job.tolerance.max(0.01) {
            warnings.push(format!(
                "the hole at ({:.3}, {:.3}) is {:.3} mm across but the tool is {:.3} mm",
                h.at.x,
                h.at.y,
                2.0 * h.radius,
                width
            ));
        }
    }
    let report = DrillReport {
        holes: holes.to_vec(),
        peck,
        dwell,
        feed: params.feed,
    };
    Ok((path, holes.len() as u64, StrategyDetail::Drill(report)))
}

/// The setup's models' round edges that lie level as the part sits, seen from
/// above: each a centre and a radius, the model's own, for a 2D cut to follow
/// exactly.
pub(super) fn level_circles(prepared: &Prepared, bodies: &Bodies<'_>) -> Vec<(DVec2, f64)> {
    let mut out: Vec<(DVec2, f64)> = Vec::new();
    for piece in &prepared.pieces {
        let Some(body) = bodies.get(piece.model.as_str()) else {
            continue;
        };
        for edge in body.topology().edges() {
            let EdgeKind::Circle(c) = edge.kind else {
                continue;
            };
            if prepared.frame.orientation.apply(c.normal).z.abs() < 1.0 - 1e-6 {
                continue;
            }
            let centre = prepared.frame.to_setup(c.center).truncate();
            // The top and bottom of a round wall are the same circle from above.
            let known = out
                .iter()
                .any(|&(p, r)| p.distance(centre) < 1e-6 && (r - c.radius).abs() < 1e-6);
            if !known {
                out.push((centre, c.radius));
            }
        }
    }
    out
}

/// The holes a drilling operation's picked rims make, in the setup's frame, in the
/// order they are drilled.
pub(in crate::run) fn drill_holes(
    job: &Job,
    op: &Operation,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    ctx: &HeightContext,
) -> Result<Vec<Hole>, RunError> {
    let chains =
        picked_chains(&op.selection, prepared, bodies, job.tolerance).map_err(in_op(op))?;
    let circle = |c: &Chain| {
        let body = bodies.get(c.model.as_str())?;
        match body.topology().edge(c.edge)?.kind {
            EdgeKind::Circle(circle) => {
                Some((prepared.frame.to_setup(circle.center), circle.radius))
            }
            _ => None,
        }
    };
    let (_, bottom) = op.levels_3d(ctx);
    let found = holes(&chains, circle, bottom, job.tolerance.max(0.01)).map_err(in_op(op))?;
    Ok(nearest_first(&found))
}
