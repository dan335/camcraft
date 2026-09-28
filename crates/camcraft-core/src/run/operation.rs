//! Running one operation: its strategy's paths, ordered, linked over the material
//! as it stands, and cut into it.

use camcraft_geom::contour::Region;
use camcraft_geom::mesh::{Aabb, TriMesh};
use camcraft_geom::topology::EdgeKind;
use camcraft_sim::stock::{SimReport, Stock};
use glam::{DVec2, DVec3, dvec2};

use super::error::{OperationError, in_op};
use super::{Made, OperationResult, PLUNGE_RATIO, RunError, StrategyDetail};
use crate::adaptive::{AdaptiveParams, Direction, adaptive_rough, flat_heights};
use crate::allowance::{Allowance, EffectiveTool};
use crate::drilling::{DrillParams, DrillReport, Hole, drill, holes, nearest_first};
use crate::dropcutter::DropCutter;
use crate::face::{FaceParams, LevelsAsked, face};
use crate::features::{Chain, FeatureError, outline, picked_chains, picked_points};
use crate::job::{
    AdaptiveDirection, Entry, HeightContext, Job, Operation, Picks, Retraction, SpindleDirection,
    StrategySpec, ToolSpec,
};
use crate::linker::{
    ClearanceField, EntryKind, LinkParams, LinkReport, Material, Ramp, StayDown, Surface,
    highest_under, link,
};
use crate::ordering::order_passes;
use crate::parallel::{ParallelParams, follow, parallel_finish_within};
use crate::planar::{
    Area, LeadArc, PlanarParams, PlanarReport, Spiral, areas, centre_offset, pocket, profile,
};
use crate::reach::LevelReach;
use crate::roughing::{RoughingParams, zlevel_rough};
use crate::setup::{Bodies, Prepared};
use crate::simadapt::sim_moves_of;
use crate::strategy::StrategyError;
use crate::tabs::{TabParams, add_tabs};
use crate::tool::ToolProfile;
use crate::toolpath::{Move, Toolpath};

/// One operation, and everything its strategy works from.
pub(super) struct Work<'a> {
    /// The job it is in.
    pub(super) job: &'a Job,
    /// The operation.
    pub(super) op: &'a Operation,
    /// Its tool, as the job describes it.
    pub(super) spec: &'a ToolSpec,
    /// Its tool as it cuts: grown by the stock to leave.
    pub(super) tool: &'a EffectiveTool,
    /// What its heights resolve against.
    pub(super) heights: &'a HeightContext,
    /// The setup's part.
    pub(super) mesh: &'a TriMesh,
    /// The drop-cutter over the part.
    pub(super) cutter: &'a DropCutter<'a>,
    /// The stock's footprint in XY, lower and upper corner.
    pub(super) bounds: (DVec2, DVec2),
    /// The blocks of material the setup starts from.
    pub(super) stocks: &'a [Aabb],
}

impl Work<'_> {
    /// Where the tool's centre may sweep: the stock's footprint, plus the tool,
    /// plus whatever extra was asked for.
    fn sweep(&self) -> (DVec2, DVec2) {
        let (lo, hi) = self.bounds;
        let reach = self.tool.max_radius() + self.op.boundary.margin;
        (
            dvec2(lo.x - reach, lo.y - reach),
            dvec2(hi.x + reach, hi.y + reach),
        )
    }
}

/// A strategy's paths, before ordering and linking: the path, how many passes it
/// has, and what the strategy reported.
type Paths = (Toolpath, u64, StrategyDetail);

/// What an operation is cut to, found before its strategy runs.
pub(super) enum Target {
    /// The machining boundary its tool's centre is kept to, if any.
    Limit(Option<Region>),
    /// A drilling operation's holes, in order.
    Holes(Vec<Hole>),
    /// A 2D operation's areas, each with its floor.
    Areas(Vec<Area>),
}

/// Work out one operation's paths, link them over the material as it stands, and
/// cut them into it, to `target`.
pub(super) fn run_operation(
    work: &Work<'_>,
    target: &Target,
    stock: &mut Stock,
) -> Result<OperationResult, RunError> {
    let op = work.op;
    let ctx = work.heights;
    let mut warnings = Vec::new();

    let (limit, holes, found) = match target {
        Target::Limit(limit) => (limit.as_ref(), &[][..], &[][..]),
        Target::Holes(holes) => (None, holes.as_slice(), &[][..]),
        Target::Areas(areas) => (None, &[][..], areas.as_slice()),
    };
    let (raw, passes, detail) = match op.strategy {
        StrategySpec::Parallel { angle } => parallel_paths(work, angle, limit, &mut warnings)?,
        StrategySpec::ZlevelRough { min_ring_length } => {
            zlevel_paths(work, min_ring_length, limit, stock, &mut warnings)?
        }
        StrategySpec::Adaptive {
            direction,
            flat_areas,
        } => adaptive_paths(work, direction, flat_areas, limit, &mut warnings)?,
        StrategySpec::Profile { .. } | StrategySpec::Pocket { .. } => {
            planar_paths(work, found, &mut warnings)?
        }
        StrategySpec::Face { .. } => face_paths(work, &mut warnings)?,
        StrategySpec::Drill { peck, dwell } => {
            drill_paths(work, peck, dwell, holes, &mut warnings)?
        }
    };

    // Ordering is a Toolpath -> Toolpath transform, applied before linking so the
    // retracts it needs are planned around the order actually used. Holes are
    // already in order, and a hole reversed would be drilled from the bottom up.
    // A 2D operation's order is its own: holes before the outline that frees a
    // part, a pocket's middle before its walls.
    let (ordered, order) = if op.linking.reorder && op.strategy.uses().reorder {
        order_passes(&raw, op.linking.allow_reverse)?
    } else {
        (raw, Default::default())
    };

    // Tabs are another Toolpath -> Toolpath transform, on the order used, before
    // linking: the linker cuts the passes into the material as it links them,
    // so it plans round the tabs left standing. Only a strategy that cuts loops
    // has them.
    let ordered = match op.passes.tabs {
        Some(tabs) if op.strategy.uses().tabs => {
            let params = TabParams {
                tabs,
                tool_radius: work.tool.cutter_radius(),
                plunge_feed: op.linking.plunge_feed,
            };
            let (tabbed, report) =
                add_tabs(&ordered, &params).map_err(|e| in_op(op)(StrategyError::from(e)))?;
            if report.tabs == 0 {
                warnings
                    .push("no tabs: no closed loop at the deepest level has room for one".into());
            } else if report.too_short > 0 {
                warnings.push(format!(
                    "{} loop(s) at the deepest level too short for a tab",
                    report.too_short
                ));
            }
            tabbed
        }
        _ => ordered,
    };

    // Traverses clear the material as it stood before the operation: never lower
    // than it will stand, so they are safe whatever the operation cuts first.
    // Between cuts, the safe distance over it, as Fusion's minimum retraction;
    // with a full one, the retract height or higher. The clearance height is
    // where the tool arrives and leaves.
    let clearance_z = op.heights.clearance.resolve(ctx);
    let retract_z = op.heights.retract.resolve(ctx);
    let clearance =
        ClearanceField::new(stock.field().clone(), op.linking.safe_distance, clearance_z)
            .map_err(in_op(op))?;
    let clearance = match op.linking.retraction {
        Retraction::Minimum => clearance,
        Retraction::Full => clearance.at_least(retract_z),
    };

    // Linked over the material as each pass leaves it, and cut into it as linked.
    let before = stock.volume();
    let surface = OnPart::new(work.cutter, work.tool, work.job.tolerance);
    let mut cutting = Cutting {
        stock,
        tool: work.tool,
        rapid_contacts: 0,
        deepest: 0.0,
    };
    let (path, mut links) = link(
        &ordered,
        &clearance,
        &mut cutting,
        &surface,
        &link_params(work, clearance_z),
    )
    .map_err(in_op(op))?;
    // Adaptive clearing makes its own links at the level, and its own helixes:
    // they are the operation's as much as the linker's are.
    if let StrategyDetail::Adaptive(r) = &detail {
        links.links_resolved += r.stayed_down;
        links.stayed_down += r.stayed_down;
        links.helixes += r.helixes;
    }
    let sim = SimReport {
        removed_mm3: (before - cutting.stock.volume()).max(0.0),
        rapid_contacts: cutting.rapid_contacts,
        deepest_rapid_contact_mm: cutting.deepest,
    };
    if links.suspicious_plunges > 0 {
        warnings.push(format!(
            "{} plunge(s) do not start above the surface",
            links.suspicious_plunges
        ));
    }
    if path.has_unplanned_links() {
        warnings.push("path still contains unresolved links and is NOT machinable".into());
    }
    if sim.rapid_contacts > 0 {
        warnings.push(format!(
            "{} rapid move(s) pass through material, deepest {:.2} mm",
            sim.rapid_contacts, sim.deepest_rapid_contact_mm
        ));
    }
    let start = path.moves().first().map_or(glam::DVec3::ZERO, |m| m.to);
    let (steep, deepest) = path.steep_cut_descents(start, PLUNGE_RATIO);
    if deepest > op.passes.max_cut_step {
        warnings.push(format!(
            "a steep cutting descent of {deepest:.2} mm exceeds the {:.2} mm \
             allowance: a plunge at cutting feed ({steep} steep descent(s))",
            op.passes.max_cut_step
        ));
    }

    Ok(OperationResult {
        path,
        sim,
        passes,
        travel_saved: order.saved(),
        detail,
        links,
        clearance_z,
        steep_cut_descents: steep,
        deepest_cut_descent_mm: deepest,
        warnings,
        // The rest is the operation's own: its name, strategy and tool.
        ..not_made(op, Some(work.spec), Made::Generated)
    })
}

/// How `work`'s operation links: its own settings, with the strategy's defaults
/// where it gives none, as Fusion's.
fn link_params(work: &Work<'_>, clearance_z: f64) -> LinkParams {
    let (op, linking) = (work.op, &work.op.linking);
    let diameter = work.spec.diameter;
    // A profile's vertical lead is part of its leads; a finishing pass's is its
    // own setting.
    let leads = linking
        .leads
        .filter(|_| op.strategy.uses().leads)
        .map(|l| l.sizes(diameter));
    let (stay_down, lead_radius, ramp) = match op.strategy {
        StrategySpec::Parallel { .. } => (
            StayDown::OverPart {
                max: linking.stay_down.unwrap_or(50.0),
            },
            linking.lead_radius.unwrap_or(0.1 * diameter),
            None,
        ),
        StrategySpec::ZlevelRough { .. }
        | StrategySpec::Profile { .. }
        | StrategySpec::Pocket { .. }
        | StrategySpec::Face { .. } => (
            StayDown::Level {
                max: linking.stay_down.unwrap_or(5.0 * diameter),
            },
            leads.map_or(0.0, |l| l.vertical),
            Some(Ramp {
                angle: linking.ramp_angle.to_radians(),
                // A ramp moves mostly sideways, so it goes nearer the cutting
                // feed than a straight plunge's.
                feed: op.ramp_feed(),
            }),
        ),
        // Adaptive clearing stays down itself, at the level, only where it has
        // cleared the way, and goes down its own helixes: a link it leaves is
        // one to lift for, into what it has cleared, straight down.
        StrategySpec::Adaptive { .. } => (StayDown::Never, 0.0, None),
        StrategySpec::Drill { .. } => (StayDown::Never, 0.0, None),
    };
    let entry = if op.strategy.uses().entry {
        match linking.entry.unwrap_or_default() {
            Entry::Helix => EntryKind::Helix,
            Entry::Plunge => EntryKind::Plunge,
            // A profile's ramp is its own path, a spiral down each ring
            // (`planar_paths`): the linker only brings the tool onto its lead.
            Entry::Ramp => EntryKind::Plunge,
        }
    } else {
        EntryKind::Ramp
    };
    let conventional = match op.strategy {
        StrategySpec::Profile { conventional, .. } | StrategySpec::Pocket { conventional, .. } => {
            conventional
        }
        StrategySpec::Adaptive { direction, .. } => direction == AdaptiveDirection::Conventional,
        _ => false,
    };
    let clockwise = work.spec.spindle == SpindleDirection::Clockwise;
    LinkParams {
        retract_feed: linking.retract_feed,
        plunge_feed: linking.plunge_feed,
        tool_radius: work.tool.cutter_radius(),
        safe_distance: linking.safe_distance,
        stay_down,
        lead_radius,
        ramp,
        entry,
        helix_radius: linking.helix_diameter.unwrap_or(0.9 * diameter) / 2.0,
        // A helix climbs as the cut does.
        helix_ccw: clockwise != conventional,
        ramp_stepdown: linking.ramp_stepdown.or(op.stepdown()).or(Some(diameter)),
        lead_in_material: leads.is_some(),
        max_cut_step: op.passes.max_cut_step,
        tolerance: work.job.tolerance,
        end_z: Some(clearance_z),
        start_z: Some(clearance_z),
    }
}

/// The operation's stock, cut as its moves are linked: what every link decision
/// is measured against.
struct Cutting<'a> {
    stock: &'a mut Stock,
    tool: &'a EffectiveTool,
    rapid_contacts: u64,
    deepest: f64,
}

impl Material for Cutting<'_> {
    fn highest(&self, from: DVec2, to: DVec2, radius: f64) -> Option<f64> {
        // The simulator's own disc, down to its margin, so a rapid stopped above
        // this is one the simulator agrees touches nothing.
        highest_under(self.stock.field(), from, to, radius * (1.0 + 1e-9)).top
    }

    fn cut(&mut self, moves: &[Move]) {
        let swept = self.stock.sweep(&sim_moves_of(moves), self.tool);
        self.rapid_contacts += swept.rapid_contacts;
        self.deepest = self.deepest.max(swept.deepest_rapid_contact_mm);
    }
}

/// The part, as the operation's tool meets it.
struct OnPart<'a> {
    cutter: &'a DropCutter<'a>,
    tool: &'a EffectiveTool,
    /// The tool a tolerance narrower: where one only grazing the part rests.
    slim: EffectiveTool,
    tolerance: f64,
}

impl<'a> OnPart<'a> {
    fn new(cutter: &'a DropCutter<'a>, tool: &'a EffectiveTool, tolerance: f64) -> Self {
        let a = tool.allowance();
        let slim = Allowance::new(a.radial() - tolerance, a.axial())
            .ok()
            .and_then(|a| EffectiveTool::of(tool.base(), a).ok())
            .unwrap_or(*tool);
        Self {
            cutter,
            tool,
            slim,
            tolerance,
        }
    }
}

impl Surface for OnPart<'_> {
    fn at(&self, p: DVec2) -> Option<f64> {
        self.cutter.drop_at(self.tool, p.x, p.y)
    }

    fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
        // Seeded at a quarter of the tool's width: nothing that narrow is passed over.
        let seed = (self.tool.max_radius() * 0.5).max(self.tolerance);
        follow(self.cutter, self.tool, from, to, self.tolerance, seed)
    }

    fn grazing_at(&self, p: DVec2) -> Option<f64> {
        self.cutter.drop_at(&self.slim, p.x, p.y)
    }

    fn grazing_along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
        let seed = (self.slim.max_radius() * 0.5).max(self.tolerance);
        follow(self.cutter, &self.slim, from, to, self.tolerance, seed)
    }
}

/// Raster finishing, with the tool's centre kept to `limit` as well as the sweep.
fn parallel_paths(
    work: &Work<'_>,
    angle: f64,
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op) = (work.job, work.op);
    let (sweep_lo, sweep_hi) = work.sweep();
    let params = ParallelParams {
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
    if !report.is_clean() {
        warnings.push(format!(
            "{} span(s) hit the refinement limit; the path is coarser than the \
             requested tolerance",
            report.refinement_limited
        ));
    }
    Ok((path, report.passes, StrategyDetail::Parallel(report)))
}

/// Z-level clearing of the material as it stands, a level each stepdown, within
/// the sweep and `limit`.
fn zlevel_paths(
    work: &Work<'_>,
    min_ring_length: f64,
    limit: Option<&Region>,
    stock: &Stock,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op, ctx) = (work.job, work.op, work.heights);
    let mut reach = LevelReach::new(work.mesh, work.tool, job.tolerance).map_err(in_op(op))?;
    let params = RoughingParams {
        // Checked with the job: it always has one.
        stepdown: op.stepdown().unwrap_or(f64::NAN),
        stepover: op.passes.stepover,
        feed: op.passes.feed,
        top: op.heights.top.map_or(ctx.stock_top, |h| h.resolve_top(ctx)),
        bottom: op
            .heights
            .bottom
            .map_or(ctx.model_bottom, |h| h.resolve_bottom(ctx)),
        min_ring_length,
    };
    let (sweep_lo, sweep_hi) = work.sweep();
    let sweep = Region::rectangle(sweep_lo, sweep_hi);
    let area = match limit {
        Some(l) => sweep
            .intersection(l)
            .map_err(|e| StrategyError::Region(e.to_string()).named(op.name.clone()))?,
        None => sweep,
    };
    let (path, report) =
        zlevel_rough(&mut reach, &area, stock.field(), &params).map_err(in_op(op))?;
    if report.rings == 0 {
        warnings.push(
            "found nothing to clear: the stock may already be below this \
             operation's top"
                .into(),
        );
    }
    Ok((path, report.rings, StrategyDetail::ZlevelRough(report)))
}

/// Adaptive clearing of the stock's blocks, a level each stepdown and one on
/// each flat face, within the sweep and `limit`.
fn adaptive_paths(
    work: &Work<'_>,
    direction: AdaptiveDirection,
    flat_areas: bool,
    limit: Option<&Region>,
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let (job, op, ctx) = (work.job, work.op, work.heights);
    let mut reach = LevelReach::new(work.mesh, work.tool, job.tolerance).map_err(in_op(op))?;
    let diameter = work.spec.diameter;
    let flats = if flat_areas {
        let axial = op.stock_to_leave().axial;
        flat_heights(work.mesh, job.tolerance)
            .into_iter()
            .map(|z| z + axial)
            .collect()
    } else {
        Vec::new()
    };
    // A ramp along a pass would cut what it takes as cleared: into a closed
    // area it goes down a helix, or straight.
    let helix_radius = match op.linking.entry.unwrap_or_default() {
        Entry::Plunge => 0.0,
        Entry::Helix | Entry::Ramp => op.linking.helix_diameter.unwrap_or(0.9 * diameter) / 2.0,
    };
    let params = AdaptiveParams {
        // Checked with the job: it always has one.
        stepdown: op.stepdown().unwrap_or(f64::NAN),
        optimal_load: op.passes.stepover,
        feed: op.passes.feed,
        top: op.heights.top.map_or(ctx.stock_top, |h| h.resolve_top(ctx)),
        bottom: op
            .heights
            .bottom
            .map_or(ctx.model_bottom, |h| h.resolve_bottom(ctx)),
        tolerance: job.tolerance,
        climb_right: work.spec.spindle == SpindleDirection::Clockwise,
        direction: match direction {
            AdaptiveDirection::Climb => Direction::Climb,
            AdaptiveDirection::Conventional => Direction::Conventional,
            AdaptiveDirection::BothWays => Direction::BothWays,
        },
        helix_radius,
        stay_down: op.linking.stay_down.unwrap_or(10.0 * diameter),
        ramp_angle: op.linking.ramp_angle.to_radians(),
        ramp_feed: op.ramp_feed(),
        ramp_stepdown: op.linking.ramp_stepdown.or(op.stepdown()),
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
        Some(l) => sweep
            .intersection(l)
            .map_err(|e| StrategyError::Region(e.to_string()).named(op.name.clone()))?,
        None => sweep,
    };
    let stocks: Vec<(DVec2, DVec2, f64)> = work
        .stocks
        .iter()
        .map(|b| (b.min.truncate(), b.max.truncate(), b.max.z))
        .collect();
    let base = *work.tool.base();
    let (path, report) =
        adaptive_rough(&mut reach, &area, &stocks, &base, &params).map_err(in_op(op))?;
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
fn planar_paths(
    work: &Work<'_>,
    areas: &[Area],
    warnings: &mut Vec<String>,
) -> Result<Paths, RunError> {
    let op = work.op;
    let conventional = match op.strategy {
        StrategySpec::Profile { conventional, .. } | StrategySpec::Pocket { conventional, .. } => {
            conventional
        }
        _ => false,
    };
    let clockwise = work.spec.spindle == SpindleDirection::Clockwise;
    let params = PlanarParams {
        stepdown: op.stepdown(),
        stepover: op.passes.stepover,
        feed: op.passes.feed,
        arc: work.job.tolerance * 0.5,
        radius: work.tool.cutter_radius(),
        kept_on_right: clockwise != conventional,
        leads: op
            .linking
            .leads
            .filter(|_| op.strategy.uses().leads)
            .map(|l| {
                let l = l.sizes(work.spec.diameter);
                LeadArc {
                    radius: l.radius,
                    sweep: l.sweep.to_radians(),
                    straight: l.straight,
                }
            }),
        // Ramped into, a profile goes down each ring in one spiral, as Fusion's.
        spiral: matches!(op.strategy, StrategySpec::Profile { .. })
            .then_some(op.linking.entry)
            .flatten()
            .filter(|e| *e == Entry::Ramp)
            .map(|_| Spiral {
                angle: op.linking.ramp_angle.to_radians(),
                feed: op.ramp_feed(),
                most: op
                    .linking
                    .ramp_stepdown
                    .or(op.stepdown())
                    .or(Some(work.spec.diameter)),
                above: op.linking.safe_distance,
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
/// stock to leave down.
fn face_paths(work: &Work<'_>, warnings: &mut Vec<String>) -> Result<Paths, RunError> {
    let (op, ctx) = (work.op, work.heights);
    let StrategySpec::Face {
        angle,
        direction,
        pass_extension,
        stock_offset,
    } = op.strategy
    else {
        return Ok((Toolpath::new(), 0, StrategyDetail::Face(Default::default())));
    };
    let (lo, hi) = work.bounds;
    let grow = DVec2::splat(stock_offset);
    let params = FaceParams {
        area: (lo - grow, hi + grow),
        angle: angle.to_radians(),
        stepover: op.passes.stepover,
        extension: pass_extension,
        radius: work.tool.cutter_radius(),
        levels: LevelsAsked {
            top: op.heights.top.map_or(ctx.stock_top, |h| h.resolve_top(ctx)),
            bottom: op
                .heights
                .bottom
                .map_or(ctx.model_top, |h| h.resolve_bottom(ctx))
                + op.stock_to_leave().axial,
            stepdown: op.stepdown(),
        },
        direction,
        clockwise: work.spec.spindle == SpindleDirection::Clockwise,
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
pub(super) fn planar_areas(
    job: &Job,
    op: &Operation,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    ctx: &HeightContext,
    tool: &EffectiveTool,
) -> Result<Vec<Area>, RunError> {
    let (shape, picked) = outline(op, prepared, bodies, job.tolerance).map_err(in_op(op))?;
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

/// What `op`'s heights resolve against: the setup's `ctx`, and how high its picks
/// go when a height is measured from them.
pub(super) fn heights_of(
    job: &Job,
    op: &Operation,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    ctx: &HeightContext,
) -> Result<HeightContext, RunError> {
    if !op.heights.measured_from_selection() {
        return Ok(*ctx);
    }
    let points =
        picked_points(&op.selection, prepared, bodies, job.tolerance).map_err(in_op(op))?;
    Ok(HeightContext {
        picked: Picks::of(points.iter().map(|p| p.z)),
        ..*ctx
    })
}

/// Each of `holes` drilled straight down, pecking by `peck` if given; a hole the
/// tool is not the size of is drilled, and said.
fn drill_paths(
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
fn level_circles(prepared: &Prepared, bodies: &Bodies<'_>) -> Vec<(DVec2, f64)> {
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
pub(super) fn drill_holes(
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
    let bottom = op
        .heights
        .bottom
        .map_or(ctx.model_bottom, |h| h.resolve_bottom(ctx));
    let found = holes(&chains, circle, bottom, job.tolerance.max(0.01)).map_err(in_op(op))?;
    Ok(nearest_first(&found))
}

/// What stands for an operation left out of a run: no paths, nothing cut. `spec`
/// is its tool, if it names one the job has.
///
/// Its name, strategy and tool are what any result of the operation carries, so
/// a result worked out takes them from here too.
pub(super) fn not_made(op: &Operation, spec: Option<&ToolSpec>, made: Made) -> OperationResult {
    let detail = match op.strategy {
        StrategySpec::Parallel { .. } => StrategyDetail::Parallel(Default::default()),
        StrategySpec::ZlevelRough { .. } => StrategyDetail::ZlevelRough(Default::default()),
        StrategySpec::Adaptive { .. } => StrategyDetail::Adaptive(Default::default()),
        StrategySpec::Profile { .. } => StrategyDetail::Profile(Default::default()),
        StrategySpec::Pocket { .. } => StrategyDetail::Pocket(Default::default()),
        StrategySpec::Face { .. } => StrategyDetail::Face(Default::default()),
        StrategySpec::Drill { peck, dwell } => StrategyDetail::Drill(DrillReport {
            holes: Vec::new(),
            peck,
            dwell,
            feed: op.linking.plunge_feed,
        }),
    };
    OperationResult {
        name: op.name.clone(),
        strategy: op.strategy.name(),
        path: Toolpath::new(),
        tool_number: spec.map_or(0, |t| t.number),
        rpm: op.passes.rpm.or(spec.map(|t| t.rpm)).unwrap_or(0),
        tool_comment: spec.map(|t| t.comment.clone()).unwrap_or_default(),
        spindle: spec.map(|t| t.spindle).unwrap_or_default(),
        coolant: spec.map(|t| t.coolant).unwrap_or_default(),
        sim: SimReport::default(),
        passes: 0,
        travel_saved: 0.0,
        detail,
        links: LinkReport::default(),
        clearance_z: 0.0,
        steep_cut_descents: 0,
        deepest_cut_descent_mm: 0.0,
        max_cut_step: op.passes.max_cut_step,
        warnings: Vec::new(),
        made,
    }
}
