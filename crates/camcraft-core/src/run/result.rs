//! What a run produced: each operation's paths and reports, and the setup's
//! material, verified.

use std::sync::Arc;

use camcraft_geom::mesh::Aabb;
use camcraft_sim::stock::{PackedStock, PartSurface, SimReport, VerifyReport};

use crate::cutter::assembly::{Assembly, PieceKind};
use glam::DVec2;

use super::Planned;
use crate::job::{Coolant, HeightContext, SpindleDirection};
use crate::path::linker::LinkReport;
use crate::path::toolpath::Toolpath;
use crate::setup::frame::Frame;
use crate::strategy::adaptive::AdaptiveReport;
use crate::strategy::drilling::DrillReport;
use crate::strategy::flat::FlatReport;
use crate::strategy::morph::MorphedSpiralReport;
use crate::strategy::parallel::ParallelReport;
use crate::strategy::planar::PlanarReport;
use crate::strategy::pocket_clearing::ClearingReport;
use crate::strategy::projected::ProjectReport;
use crate::strategy::round::RoundReport;
use crate::strategy::scallop::ScallopReport;
use crate::strategy::waterline::WaterlineReport;

/// What a strategy reported, in its own terms.
///
/// Carried through rather than flattened into warnings, so that anything looking
/// at a result -- the app, the command line and the tests alike -- sees the same
/// numbers the run did.
#[derive(Debug, Clone, PartialEq)]
pub enum StrategyDetail {
    /// Raster finishing.
    Parallel(ParallelReport),
    /// Scallop finishing.
    Scallop(ScallopReport),
    /// Contour finishing: a waterline.
    Contour(WaterlineReport),
    /// Flat finishing.
    Flat(FlatReport),
    /// Spiral finishing.
    Spiral(ProjectReport),
    /// Radial finishing.
    Radial(ProjectReport),
    /// Morphed spiral finishing.
    MorphedSpiral(MorphedSpiralReport),
    /// Z-level clearing.
    PocketClearing(ClearingReport),
    /// Adaptive clearing.
    Adaptive(AdaptiveReport),
    /// A 2D profile.
    Profile(PlanarReport),
    /// A 2D pocket.
    Pocket(PlanarReport),
    /// Facing.
    Face(PlanarReport),
    /// Drilling.
    Drill(DrillReport),
    /// Bore.
    Bore(RoundReport),
    /// Circular.
    Circular(RoundReport),
}

/// How an operation's paths came to be in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Made {
    /// Worked out in this run.
    Generated,
    /// Its passes kept from an earlier run, and linked again over this run's
    /// material: the operation and the part as they were, and the material
    /// before it not.
    Relinked,
    /// Kept from an earlier run, as it was, and cut again into this run's
    /// material.
    Kept,
    /// Not worked out: no paths, and it cuts nothing.
    Skipped,
    /// Turned off in the job: no paths, cuts nothing, and is not posted.
    Disabled,
}

impl Made {
    /// Whether it has paths: worked out now, linked again, or kept from
    /// before.
    pub fn has_paths(self) -> bool {
        matches!(self, Made::Generated | Made::Relinked | Made::Kept)
    }
}

/// What a run is to do with one operation: the less there is to do, the more
/// of what an earlier run made is used.
#[derive(Debug, Clone)]
pub enum Plan {
    /// Work it out.
    Generate,
    /// Link again, over the material as this run leaves it, the passes an
    /// earlier run worked out ([`OperationResult::planned`]): what the
    /// operation, its tool and the part were then, they are now; and, for a
    /// strategy that clears what the operations before it left
    /// ([`Uses::rest`](crate::job::Uses::rest)), those operations too.
    Relink(Arc<Planned>),
    /// Keep what an earlier run made of it, unchanged since.
    Keep(Box<Kept>),
    /// Leave it out.
    Skip,
}

/// An operation an earlier run made, for a run to keep.
#[derive(Debug, Clone)]
pub struct Kept {
    /// What it made of it.
    pub result: OperationResult,
    /// The material as it left it, where it is known: put back as it was
    /// rather than cut again, where everything before it is kept too and the
    /// material is on the same grid.
    pub after: Option<Arc<PackedStock>>,
}

/// What one operation produced.
#[derive(Debug, Clone)]
pub struct OperationResult {
    /// The operation's name.
    pub name: String,
    /// Which strategy produced it, for the program's section comment.
    pub strategy: &'static str,
    /// The machinable path: shared, by what draws it, replays it and rests
    /// against it, rather than copied.
    pub path: Arc<Toolpath>,
    /// Tool number, for the post.
    pub tool_number: u32,
    /// Spindle speed.
    pub rpm: u32,
    /// Tool description.
    pub tool_comment: String,
    /// Which way the spindle turns.
    pub spindle: SpindleDirection,
    /// Coolant while cutting.
    pub coolant: Coolant,
    /// What the simulator saw while this operation ran.
    pub sim: SimReport,
    /// Cutting passes produced.
    pub passes: u64,
    /// Inter-pass travel removed by reordering, as a fraction.
    pub travel_saved: f64,
    /// What the strategy reported.
    pub detail: StrategyDetail,
    /// What linking did.
    pub links: LinkReport,
    /// What smoothing did, where it was asked for.
    pub smoothing: Option<crate::path::fit::FitReport>,
    /// What feed optimisation did, where it was asked for.
    pub corners: Option<crate::path::corners::CornerReport>,
    /// The clearance height, resolved: where the tool arrives and leaves. Between
    /// cuts it flies over the material by the safe distance (or at the retract
    /// height, with a full retraction), and never lower than this where the
    /// material is unknown.
    pub clearance_z: f64,
    /// Continuous cutting descents steeper than [`PLUNGE_RATIO`](super::PLUNGE_RATIO).
    pub steep_cut_descents: u64,
    /// Deepest of those, in millimetres.
    pub deepest_cut_descent_mm: f64,
    /// How deep a steep descent may be before it is a plunge, in millimetres: the
    /// operation's `max_cut_step`.
    pub max_cut_step: f64,
    /// Anything the caller should look at.
    pub warnings: Vec<String>,
    /// How its paths came to be in this run.
    pub made: Made,
    /// Its passes as worked out, before they were linked: for a later run to
    /// link again ([`Plan::Relink`]). `None` where it was not worked out.
    pub planned: Option<Arc<Planned>>,
    /// The whole tool it cut with, shank and holder or spindle nose and all,
    /// as it was checked: `None` where it was not cut.
    pub assembly: Option<Assembly>,
    /// Where the tool's shank, holder or the spindle's nose met the material
    /// ([decision 0021]): the moves, in order.
    ///
    /// [decision 0021]: ../../../../docs/decisions/0021-shank-and-holder.md
    pub contacts: Vec<Contact>,
}

/// A move where a part of the tool that does not cut met the material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    /// The move, by its place in the operation's path.
    pub at: usize,
    /// What met it.
    pub piece: PieceKind,
    /// How far into the material it would have gone, in millimetres: how much
    /// higher it would have had to be.
    pub depth: f64,
}

impl OperationResult {
    /// How far the tool would have to stick out of its holder, or the
    /// spindle's nose, for neither to meet the material, where either did:
    /// as it is, raised by the deepest they went in.
    pub fn stickout_needed(&self) -> Option<f64> {
        let a = self.assembly.as_ref()?;
        self.contacts
            .iter()
            .filter(|c| !c.piece.is_shank())
            .map(|c| c.depth)
            .fold(None, |m: Option<f64>, d| Some(m.map_or(d, |m| m.max(d))))
            .map(|d| a.stickout + d)
    }
}

impl OperationResult {
    /// Whether this operation is safe to run as it stands.
    ///
    /// A plunge at cutting feed is as unsafe as a rapid through material, so a
    /// steep descent deeper than the allowance fails the operation, not just warns.
    pub fn is_clean(&self) -> bool {
        self.sim.is_clean()
            && !self.path.has_unplanned_links()
            && self.deepest_cut_descent_mm <= self.max_cut_step
            && self.contacts.is_empty()
    }
}

/// What one setup produced. Everything in it is in the setup's frame.
#[derive(Debug, Clone)]
pub struct SetupResult {
    /// Which setup.
    pub setup: String,
    /// How model coordinates became the setup's.
    pub frame: Frame,
    /// Each operation, in order.
    pub operations: Vec<OperationResult>,
    /// The part the setup was supposed to produce, shared with whatever replays
    /// the run.
    pub part: Arc<PartSurface>,
    /// How the finished stock compares with the part.
    pub verify: VerifyReport,
    /// Tolerance the comparison allowed.
    pub verify_budget: f64,
    /// Each block of stock.
    pub stocks: Vec<Aabb>,
    /// Footprint of all the stock in XY, lower corner.
    pub stock_min: DVec2,
    /// Footprint of all the stock in XY, upper corner.
    pub stock_max: DVec2,
    /// The heights symbolic references resolved against.
    pub heights: HeightContext,
    /// The material before the first operation, then after each: one more than
    /// there are operations, the last being the finished material. The keyframes a replay
    /// starts from, packed (a full sheet's raw heights are hundreds of megabytes,
    /// and machined material packs small) and shared with it rather than copied.
    pub keyframes: Vec<Arc<PackedStock>>,
}

impl SetupResult {
    /// Whether the whole setup verified.
    pub fn is_clean(&self) -> bool {
        self.verify.is_clean() && self.operations.iter().all(OperationResult::is_clean)
    }

    /// Everything worth a person's attention, across every operation.
    pub fn warnings(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .operations
            .iter()
            .flat_map(|o| o.warnings.iter().map(|w| format!("{}: {w}", o.name)))
            .collect();
        if self.verify.gouged > 0 {
            out.push(format!(
                "finished stock is below the part in {} column(s), deepest {:.3} mm \
                 against a {:.3} mm budget",
                self.verify.gouged, self.verify.deepest_gouge_mm, self.verify_budget
            ));
        }
        out
    }
}

/// Where a run has got to, as it goes: what it has started on. Operations'
/// passes are worked out at once, so several may be under way together;
/// they are cut into the material one after another, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Laying the models and stock out in the setup's frame.
    Preparing,
    /// Working out operation `i`'s passes, from 0.
    Operation(usize),
    /// Linking operation `i`'s passes over the material the ones before it
    /// left, and cutting them into it.
    Cutting(usize),
    /// Measuring the finished material against the part.
    Verifying,
    /// Running the setup's checks on what it made.
    Checking,
}

/// What a run tells as it goes.
#[derive(Debug, Clone, Copy)]
pub enum Progress<'a> {
    /// It has started on a [`Stage`].
    Stage(Stage),
    /// The setup's material and part are laid out: a result with all a
    /// result holds but its operations, which come as each is made, and its
    /// verification, which comes at the end.
    Begun(&'a SetupResult),
    /// Operation `index` is done with, made, kept or left out, and this is
    /// the material after it.
    Made {
        /// Which operation, from 0.
        index: usize,
        /// What it is.
        op: &'a OperationResult,
        /// The material after it.
        after: &'a Arc<PackedStock>,
    },
}
