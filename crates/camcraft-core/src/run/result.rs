//! What a run produced: each operation's paths and reports, and the setup's
//! material, verified.

use std::sync::Arc;

use camcraft_geom::mesh::Aabb;
use camcraft_sim::stock::{PackedStock, PartSurface, SimReport, VerifyReport};
use glam::DVec2;

use crate::drilling::DrillReport;
use crate::frame::Frame;
use crate::job::{Coolant, HeightContext, SpindleDirection};
use crate::linker::LinkReport;
use crate::parallel::ParallelReport;
use crate::roughing::RoughingReport;
use crate::toolpath::Toolpath;

/// What a strategy reported, in its own terms.
///
/// Carried through rather than flattened into warnings, so that anything looking
/// at a result -- the app, the command line and the tests alike -- sees the same
/// numbers the run did.
#[derive(Debug, Clone, PartialEq)]
pub enum StrategyDetail {
    /// Raster finishing.
    Parallel(ParallelReport),
    /// Z-level clearing.
    ZlevelRough(RoughingReport),
    /// Drilling.
    Drill(DrillReport),
}

/// How an operation's paths came to be in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Made {
    /// Worked out in this run.
    Generated,
    /// Kept from an earlier run, as it was, and cut again into this run's
    /// material.
    Kept,
    /// Not worked out: no paths, and it cuts nothing.
    Skipped,
    /// Turned off in the job: no paths, cuts nothing, and is not posted.
    Disabled,
}

impl Made {
    /// Whether it has paths: worked out now or kept from before.
    pub fn has_paths(self) -> bool {
        matches!(self, Made::Generated | Made::Kept)
    }
}

/// What a run is to do with one operation.
#[derive(Debug, Clone)]
pub enum Plan {
    /// Work it out.
    Generate,
    /// Keep what an earlier run made of it, unchanged since.
    Keep(Box<OperationResult>),
    /// Leave it out.
    Skip,
}

/// What one operation produced.
#[derive(Debug, Clone)]
pub struct OperationResult {
    /// The operation's name.
    pub name: String,
    /// Which strategy produced it, for the program's section comment.
    pub strategy: &'static str,
    /// The machinable path.
    pub path: Toolpath,
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
    /// The clearance height, resolved. Traverses fly over the material by the
    /// clearance offset, and never lower than this where the material is unknown.
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

/// Where a run has got to, as it goes: what it is doing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Laying the models and stock out in the setup's frame.
    Preparing,
    /// Running operation `i`, from 0: its paths, then cutting them.
    Operation(usize),
    /// Measuring the finished material against the part.
    Verifying,
    /// Running the setup's checks on what it made.
    Checking,
}
