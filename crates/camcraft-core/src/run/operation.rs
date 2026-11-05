//! Running one operation: its strategy's paths, ordered, linked over the material
//! as it stands, and cut into it.

use std::sync::Arc;

use camcraft_geom::contour::Region;
use camcraft_geom::topology::EdgeKind;
use camcraft_sim::stock::{SimReport, Stock};
use glam::{DVec2, DVec3, dvec2};

use super::error::in_op;
use super::{Made, OperationResult, PLUNGE_RATIO, RunError, StrategyDetail};
use crate::control::Watch;
use crate::cutter::allowance::{Allowance, EffectiveTool};
use crate::cutter::dropcutter::DropCutter;
use crate::cutter::tool::ToolProfile;
use crate::job::{
    ContourOrder, CutDirection, Depths, Entry, FaceDirection, HeightContext, Job, Links, Operation,
    PassOrder, Picks, RadialDirection, Retraction, SpindleDirection, StrategySpec, ToolSpec,
};
use crate::path::corners::{CornerParams, CornerReport, slow_corners};
use crate::path::fit::{FitReport, fit_arcs};
use crate::path::linker::{
    ClearanceField, EntryKind, LinkParams, LinkReport, Material, Ramp, StayDown, Surface,
    highest_under, link_watched,
};
use crate::path::ordering::order_passes;
use crate::path::simadapt::sim_moves_of;
use crate::path::tabs::{TabParams, add_tabs};
use crate::path::toolpath::{Move, Toolpath};
use crate::path::trim::within_heights;
use crate::setup::features::{Chain, FeatureError, Picking, picked_chains};
use crate::setup::{Bodies, Prepared};
use crate::strategy::adaptive::rest::Earlier;
use crate::strategy::adaptive::{AdaptiveParams, Shallow, adaptive_rough, peak_heights};
use crate::strategy::drilling::{DrillParams, DrillReport, Hole, drill, holes, nearest_first};
use crate::strategy::face::{FaceParams, LevelsAsked, face};
use crate::strategy::flat::{FlatParams, flat_finish};
use crate::strategy::morph::{MorphParams, MorphedSpiralReport, morphed_spiral};
use crate::strategy::parallel::{ParallelParams, ParallelReport, follow, parallel_finish_within};
use crate::strategy::patterns::{Centred, radial, reaching, spiral};
use crate::strategy::planar::{
    Area, LeadArc, PlanarParams, PlanarReport, Spiral, WallFinish, areas, centre_offset, pocket,
    profile,
};
use crate::strategy::pocket_clearing::{ClearingParams, pocket_clearing};
use crate::strategy::projected::{Curve, ProjectParams, ProjectReport, project};
use crate::strategy::reach::LevelReach;
use crate::strategy::scallop::{ScallopParams, contact_region, scallop_finish_banded};
use crate::strategy::slope::steep_ground;
use crate::strategy::waterline::{WaterlineParams, waterline};

mod clearing;
mod finishing;
mod link;

use clearing::{
    AdaptiveAsked, adaptive_paths, drill_paths, face_paths, planar_paths, pocket_clearing_paths,
};
pub(super) use clearing::{drill_holes, heights_of, planar_areas};
use finishing::{
    contour_paths, flat_paths, morphed_spiral_paths, parallel_paths, radial_paths, scallop_paths,
    spiral_paths,
};
use link::{Cutting, OnPart, link_params};

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
    /// The setup, laid out: its part, and what is found from it once.
    pub(super) prepared: &'a Prepared,
    /// The drop-cutter over the part.
    pub(super) cutter: &'a DropCutter<'a>,
    /// The stock's footprint in XY, lower and upper corner.
    pub(super) bounds: (DVec2, DVec2),
    /// The blocks of material the setup starts from, each its footprint's lower
    /// and upper corner and its top.
    pub(super) blocks: &'a [(DVec2, DVec2, f64)],
    /// The heights of the part's flat faces, found for the setup when an
    /// operation first needs them.
    /// The operations before it in the setup, as cut, for a strategy that
    /// clears what they left ([`Uses::rest`](crate::job::Uses::rest)); none
    /// for any other.
    pub(super) before: &'a [Earlier],
    /// What its work tells how far it has got, and asks whether to stop.
    pub(super) watch: Watch,
}

impl Work<'_> {
    /// Whether its tool's spindle turns clockwise, seen from above: a pass
    /// climbs with what it cuts on its right.
    fn clockwise(&self) -> bool {
        self.spec.spindle == SpindleDirection::Clockwise
    }

    /// Whether the material being cut is on the tool's right: climbing with a
    /// clockwise spindle, or conventional with the other.
    fn kept_on_right(&self) -> bool {
        self.clockwise() != self.op.strategy.conventional()
    }

    /// The heights of the part's flat faces, raised by the stock to leave down,
    /// where flat area detection adds a level; none unless `on`.
    fn flat_levels(&self, on: bool) -> Vec<f64> {
        if !on {
            return Vec::new();
        }
        let axial = self.op.stock_to_leave().axial;
        self.prepared
            .flat_heights(self.job.tolerance)
            .iter()
            .map(|z| z + axial)
            .collect()
    }

    /// The range of slopes its strategy keeps to, in radians from level: every
    /// slope where it has none.
    fn slopes(&self) -> (f64, f64) {
        let (from, to) = self.op.strategy.slopes().unwrap_or((0.0, 90.0));
        (from.to_radians(), to.to_radians())
    }

    /// Which way a spiral cut in `order` goes: outward or not, and turning
    /// anticlockwise or not. Going outward, what is still to finish is outside:
    /// on the tool's right going anticlockwise.
    fn turning(&self, order: PassOrder) -> (bool, bool) {
        let outward = order == PassOrder::InsideOut;
        (outward, outward == self.kept_on_right())
    }

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

/// An operation's passes, worked out and put in order, its tabs on them: ready
/// to be linked over the material and cut into it, and kept with its result,
/// for a later run to link again over other material
/// ([`Plan::Relink`](super::Plan::Relink)).
#[derive(Debug)]
pub struct Planned {
    /// The passes, with the links between them still to make.
    ordered: Toolpath,
    /// How many passes the strategy made.
    passes: u64,
    /// What the strategy reported.
    detail: StrategyDetail,
    /// What is worth a person's attention so far.
    warnings: Vec<String>,
    /// How much shorter ordering made the travel between passes, as a share.
    travel_saved: f64,
    /// What smoothing did, where it was asked for.
    smoothing: Option<FitReport>,
    /// What feed optimisation did, where it was asked for.
    corners: Option<CornerReport>,
}

/// Work out one operation's passes, to `target`, and put them in order. Only a
/// strategy that clears what the operations before it left
/// ([`Uses::rest`](crate::job::Uses::rest)) reads `stock`, the material as it
/// stands; every other works from the part alone, so may be worked out at once
/// with the operations before it.
pub(super) fn plan_operation(
    work: &Work<'_>,
    target: &Target,
    stock: &Stock,
) -> Result<Planned, RunError> {
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
        StrategySpec::Scallop {
            direction, order, ..
        } => scallop_paths(work, (direction, order), limit, &mut warnings)?,
        StrategySpec::Contour {
            direction, order, ..
        } => contour_paths(work, (direction, order), limit, &mut warnings)?,
        StrategySpec::Flat { order, .. } => flat_paths(work, order, limit, &mut warnings)?,
        StrategySpec::Spiral {
            centre,
            inner_radius,
            outer_radius,
            order,
            ..
        } => spiral_paths(
            work,
            (centre, inner_radius, outer_radius),
            order,
            limit,
            &mut warnings,
        )?,
        StrategySpec::Radial {
            centre,
            inner_radius,
            outer_radius,
            angle_from,
            angle_to,
            direction,
            ..
        } => radial_paths(
            work,
            (centre, inner_radius, outer_radius),
            (angle_from, angle_to, direction),
            limit,
            &mut warnings,
        )?,
        StrategySpec::MorphedSpiral { centre, order, .. } => {
            morphed_spiral_paths(work, centre, order, limit, &mut warnings)?
        }
        StrategySpec::PocketClearing {
            min_ring_length,
            flat_areas,
            ..
        } => pocket_clearing_paths(
            work,
            min_ring_length,
            flat_areas,
            limit,
            stock,
            &mut warnings,
        )?,
        StrategySpec::Adaptive {
            direction,
            flat_areas,
            machine_cavities,
            min_cutting_radius,
            order,
            rest_machining,
            ignore_stock_less_than,
            min_flat_stepdown,
            min_axial_engagement,
            shallow_areas,
            min_shallow_stepdown,
            max_shallow_stepover,
            slot_clearing,
            slot_width,
            stay_down_level,
            stay_down_clearance,
            lift_height,
        } => adaptive_paths(
            work,
            AdaptiveAsked {
                direction,
                flat_areas,
                machine_cavities,
                min_cutting_radius,
                order,
                rest_machining,
                ignore_stock_less_than,
                min_flat_stepdown,
                min_axial_engagement,
                shallow_areas,
                min_shallow_stepdown,
                max_shallow_stepover,
                slot_clearing,
                slot_width,
                stay_down_level,
                stay_down_clearance,
                lift_height,
            },
            limit,
            &mut warnings,
        )?,
        StrategySpec::Profile { .. } | StrategySpec::Pocket { .. } => {
            planar_paths(work, found, &mut warnings)?
        }
        StrategySpec::Face {
            angle,
            direction,
            pass_extension,
            stock_offset,
        } => face_paths(
            work,
            (angle, direction),
            (pass_extension, stock_offset),
            &mut warnings,
        )?,
        StrategySpec::Drill { peck, dwell } => {
            drill_paths(work, peck, dwell, holes, &mut warnings)?
        }
    };

    // A finishing path over the part keeps between the Top and Bottom it is
    // given, as Fusion's does: what goes past either is trimmed away, and what
    // is left is passes for the linker to join. Without them, all of it.
    let (raw, passes) = if op.strategy.uses().links == Links::OverPart
        && (op.heights.top.is_some() || op.heights.bottom.is_some())
    {
        let (top, bottom) = op.levels_3d(ctx);
        let (trimmed, report) = within_heights(
            &raw,
            op.heights.top.map(|_| top),
            op.heights.bottom.map(|_| bottom),
            work.job.tolerance,
        )
        .map_err(in_op(op))?;
        if report.after == 0 && report.before > 0 {
            warnings.push("found nothing to finish between the Top and Bottom".into());
        }
        (trimmed, report.after)
    } else {
        (raw, passes)
    };

    // Ordering is a Toolpath -> Toolpath transform, applied before linking so the
    // retracts it needs are planned around the order actually used. Holes are
    // already in order, and a hole reversed would be drilled from the bottom up.
    // A 2D operation's order is its own: holes before the outline that frees a
    // part, a pocket's middle before its walls.
    let (ordered, order) = if op.linking.reorder && op.strategy.uses().reorder {
        let by_level = op.strategy.uses().depths != Depths::None;
        order_passes(&raw, op.linking.allow_reverse, by_level).map_err(in_op(op))?
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
            let (tabbed, report) = add_tabs(&ordered, &params).map_err(in_op(op))?;
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

    // Smoothing, last, on the passes as they will be cut: the linker joins
    // them, and the simulator cuts and checks them, as they are fitted, so
    // what is verified is the program.
    let (ordered, smoothing) = match op.smoothing(work.job.tolerance) {
        Some(tolerance) => {
            let (fitted, report) = fit_arcs(&ordered, tolerance).map_err(in_op(op))?;
            (fitted, Some(report))
        }
        None => (ordered, None),
    };

    // Feed optimisation, on the fitted passes, whose arcs say how tight they
    // turn: the corners the program has are the ones slowed.
    let (ordered, corners) = match op.feed_optimization(work.tool.cutter_radius()) {
        Some(f) => {
            let params = CornerParams {
                most_turn: f.max_directional_change.to_radians(),
                radius: f.radius,
                distance: f.distance,
                feed: f.feed,
                only_inner: f.only_inner_corners,
                material_on_right: op.strategy.one_way().then(|| work.kept_on_right()),
            };
            let (slowed, report) = slow_corners(&ordered, &params).map_err(in_op(op))?;
            (slowed, Some(report))
        }
        None => (ordered, None),
    };
    Ok(Planned {
        ordered,
        passes,
        detail,
        warnings,
        travel_saved: order.saved(),
        smoothing,
        corners,
    })
}

/// Link `planned`, operation `work`'s passes, over `stock`, the material as the
/// operations before it left it, and cut them into it as they are linked,
/// telling `watch` how far it has got.
pub(super) fn cut_operation(
    work: &Work<'_>,
    planned: &Arc<Planned>,
    stock: &mut Stock,
    watch: &Watch,
) -> Result<OperationResult, RunError> {
    let op = work.op;
    let ctx = work.heights;
    let Planned {
        ordered,
        passes,
        detail,
        warnings,
        travel_saved,
        smoothing,
        corners,
    } = &**planned;
    let (passes, travel_saved, smoothing, corners) = (*passes, *travel_saved, *smoothing, *corners);
    let detail = detail.clone();
    let mut warnings = warnings.clone();

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
    let (path, mut links) = link_watched(
        ordered,
        &clearance,
        &mut cutting,
        &surface,
        &link_params(work, clearance_z),
        watch,
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
    if deepest > op.max_cut_step() {
        warnings.push(format!(
            "a steep cutting descent of {deepest:.2} mm exceeds the {:.2} mm \
             allowance: a plunge at cutting feed ({steep} steep descent(s))",
            op.max_cut_step()
        ));
    }

    Ok(OperationResult {
        path: Arc::new(path),
        sim,
        passes,
        travel_saved,
        detail,
        links,
        clearance_z,
        steep_cut_descents: steep,
        deepest_cut_descent_mm: deepest,
        smoothing,
        corners,
        warnings,
        planned: Some(Arc::clone(planned)),
        // The rest is the operation's own: its name, strategy and tool.
        ..not_made(op, Some(work.spec), Made::Generated)
    })
}

/// What stands for an operation left out of a run: no paths, nothing cut. `spec`
/// is its tool, if it names one the job has.
///
/// Its name, strategy and tool are what any result of the operation carries, so
/// a result worked out takes them from here too.
pub(super) fn not_made(op: &Operation, spec: Option<&ToolSpec>, made: Made) -> OperationResult {
    let detail = match op.strategy {
        StrategySpec::Parallel { .. } => StrategyDetail::Parallel(Default::default()),
        StrategySpec::Scallop { .. } => StrategyDetail::Scallop(Default::default()),
        StrategySpec::Contour { .. } => StrategyDetail::Contour(Default::default()),
        StrategySpec::Flat { .. } => StrategyDetail::Flat(Default::default()),
        StrategySpec::Spiral { .. } => StrategyDetail::Spiral(Default::default()),
        StrategySpec::Radial { .. } => StrategyDetail::Radial(Default::default()),
        StrategySpec::MorphedSpiral { .. } => StrategyDetail::MorphedSpiral(Default::default()),
        StrategySpec::PocketClearing { .. } => StrategyDetail::PocketClearing(Default::default()),
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
        path: Arc::new(Toolpath::new()),
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
        smoothing: None,
        corners: None,
        clearance_z: 0.0,
        steep_cut_descents: 0,
        deepest_cut_descent_mm: 0.0,
        max_cut_step: op.max_cut_step(),
        warnings: Vec::new(),
        made,
        planned: None,
    }
}
