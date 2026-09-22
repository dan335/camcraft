//! The job model: setup, tools, and an ordered list of operations sharing one stock.
//!
//! This is the contract between a person and the engine, and it is shaped the way
//! Fusion shapes an operation — **Geometry, Heights, Passes, Linking** — because that
//! factoring is proven and maps one-to-one onto the provisions this project was
//! built around. Adopting it means the parameter layer never needs restructuring as
//! strategies are added.
//!
//! # Heights are symbolic
//!
//! `clearance = { ref = "stock-top", offset = 10 }` rather than a bare number. A job
//! written against one block of material then works against a taller one, and the
//! meaning survives being read six months later. Absolute numbers are still
//! available, but they have to be asked for.
//!
//! # One stock, threaded
//!
//! Operations run in order against a single block. Each reads the material as it
//! currently stands and writes back what it removed, so a finishing pass never
//! re-cuts what roughing already cleared. Rest machining is not a feature here; it is
//! the consequence of that ordering.

use serde::Deserialize;

use crate::tool::{Ball, Bullnose, BullnoseFamily, Flat, ToolError};

/// A complete job.
#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    /// Chordal tolerance in millimetres. Drives sampling and grid resolution alike.
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
    /// The setup: what the stock is and where the work offset sits.
    pub setup: Setup,
    /// Cutters, by name.
    pub tools: std::collections::BTreeMap<String, ToolSpec>,
    /// Operations, in the order they run.
    #[serde(default, rename = "operation")]
    pub operations: Vec<Operation>,
}

fn default_tolerance() -> f64 {
    0.01
}

/// What the material is and where it sits.
#[derive(Debug, Clone, Deserialize)]
pub struct Setup {
    /// The block of material.
    pub stock: StockSpec,
    /// Work offset, 54 for `G54`.
    #[serde(default = "default_wcs")]
    pub wcs: u8,
}

fn default_wcs() -> u8 {
    54
}

/// How the block relates to the model.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StockSpec {
    /// A box around the model, grown by a margin on each axis.
    ///
    /// The Z margin applies upward only: material below the model is the part of the
    /// block the cutter never reaches, and growing downward would only make the
    /// simulation taller without changing anything.
    Box {
        /// Extra material on X, Y and Z, in millimetres.
        #[serde(default)]
        margin: [f64; 3],
    },
}

/// A cutter.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolSpec {
    /// Shape.
    pub kind: ToolKind,
    /// Diameter in millimetres.
    pub diameter: f64,
    /// Corner radius, for a bullnose.
    #[serde(default)]
    pub corner: f64,
    /// Tool number in the program.
    #[serde(default = "one_u32")]
    pub number: u32,
    /// Spindle speed.
    #[serde(default = "default_rpm")]
    pub rpm: u32,
    /// Description for the operator.
    #[serde(default)]
    pub comment: String,
}

fn one_u32() -> u32 {
    1
}

fn default_rpm() -> u32 {
    18000
}

/// Cutter shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolKind {
    /// Square end.
    Flat,
    /// Ball nose.
    Ball,
    /// Corner radius.
    Bullnose,
}

impl ToolSpec {
    /// The cutter this describes.
    pub fn build(&self) -> Result<Bullnose, ToolError> {
        let r = self.diameter / 2.0;
        match self.kind {
            ToolKind::Flat => Flat::new(r).map(|t| t.as_bullnose()),
            ToolKind::Ball => Ball::new(r).map(|t| t.as_bullnose()),
            ToolKind::Bullnose => Bullnose::new(r, self.corner),
        }
    }
}

/// One operation.
#[derive(Debug, Clone, Deserialize)]
pub struct Operation {
    /// Name, for comments and diagnostics.
    pub name: String,
    /// Which strategy, and its own parameters.
    #[serde(flatten)]
    pub strategy: StrategySpec,
    /// Which cutter, by name from the tool table.
    pub tool: String,
    /// What to machine.
    #[serde(default)]
    pub geometry: Geometry,
    /// Where the tool travels.
    #[serde(default)]
    pub heights: Heights,
    /// How the passes are laid out.
    #[serde(default)]
    pub passes: Passes,
    /// How the tool moves between cuts.
    #[serde(default)]
    pub linking: Linking,
}

/// Strategy and the parameters only it has.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "strategy", rename_all = "kebab-case")]
pub enum StrategySpec {
    /// Raster finishing.
    Parallel {
        /// Pass direction in degrees; zero runs along +X.
        #[serde(default)]
        angle: f64,
    },
    /// Z-level clearing.
    ZlevelRough {
        /// Depth of each slice, in millimetres.
        stepdown: f64,
        /// Rings shorter than this are not worth the entry.
        #[serde(default = "default_min_ring")]
        min_ring_length: f64,
    },
}

fn default_min_ring() -> f64 {
    3.0
}

/// What to machine.
#[derive(Debug, Clone, Deserialize)]
pub struct Geometry {
    /// Extra margin around the model's silhouette, in millimetres.
    #[serde(default)]
    pub margin: f64,
    /// Highest level to cut at.
    #[serde(default)]
    pub top: Option<Height>,
    /// Lowest level to cut at.
    #[serde(default)]
    pub bottom: Option<Height>,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            margin: 0.0,
            top: None,
            bottom: None,
        }
    }
}

/// Where the tool travels.
#[derive(Debug, Clone, Deserialize)]
pub struct Heights {
    /// Traverse height.
    #[serde(default = "default_clearance")]
    pub clearance: Height,
}

impl Default for Heights {
    fn default() -> Self {
        Self {
            clearance: default_clearance(),
        }
    }
}

fn default_clearance() -> Height {
    Height {
        reference: HeightRef::StockTop,
        offset: 10.0,
    }
}

/// A height, stated relative to something that can be measured.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Height {
    /// What the offset is measured from.
    #[serde(rename = "ref", default = "default_ref")]
    pub reference: HeightRef,
    /// Millimetres above the reference.
    #[serde(default)]
    pub offset: f64,
}

fn default_ref() -> HeightRef {
    HeightRef::StockTop
}

/// What a height is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HeightRef {
    /// Top of the block.
    StockTop,
    /// Bottom of the block.
    StockBottom,
    /// Highest point of the model.
    ModelTop,
    /// Lowest point of the model.
    ModelBottom,
    /// The offset is the height, in setup coordinates.
    Absolute,
}

/// What a height resolves against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeightContext {
    /// Top of the block.
    pub stock_top: f64,
    /// Bottom of the block.
    pub stock_bottom: f64,
    /// Highest point of the model.
    pub model_top: f64,
    /// Lowest point of the model.
    pub model_bottom: f64,
}

impl Height {
    /// Resolve to a height in setup coordinates.
    pub fn resolve(&self, ctx: &HeightContext) -> f64 {
        let base = match self.reference {
            HeightRef::StockTop => ctx.stock_top,
            HeightRef::StockBottom => ctx.stock_bottom,
            HeightRef::ModelTop => ctx.model_top,
            HeightRef::ModelBottom => ctx.model_bottom,
            HeightRef::Absolute => 0.0,
        };
        base + self.offset
    }
}

/// How the passes are laid out.
#[derive(Debug, Clone, Deserialize)]
pub struct Passes {
    /// Distance between neighbouring passes, in millimetres.
    #[serde(default = "default_stepover")]
    pub stepover: f64,
    /// Cutting feed, in mm/min.
    #[serde(default = "default_feed")]
    pub feed: f64,
    /// Material to leave behind.
    #[serde(default)]
    pub stock_to_leave: StockToLeave,
    /// Largest Z drop that may be traversed as a cut.
    #[serde(default = "default_max_cut_step")]
    pub max_cut_step: f64,
}

impl Default for Passes {
    fn default() -> Self {
        Self {
            stepover: default_stepover(),
            feed: default_feed(),
            stock_to_leave: StockToLeave::default(),
            max_cut_step: default_max_cut_step(),
        }
    }
}

fn default_stepover() -> f64 {
    1.0
}

fn default_feed() -> f64 {
    1200.0
}

fn default_max_cut_step() -> f64 {
    0.5
}

/// Material to leave, radial and axial.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub struct StockToLeave {
    /// Perpendicular to the tool axis.
    #[serde(default)]
    pub radial: f64,
    /// Along the tool axis.
    #[serde(default)]
    pub axial: f64,
}

/// How the tool moves between cuts.
#[derive(Debug, Clone, Deserialize)]
pub struct Linking {
    /// Feed for coming up out of a cut.
    #[serde(default = "default_retract_feed")]
    pub retract_feed: f64,
    /// Feed for descending into material.
    #[serde(default = "default_plunge_feed")]
    pub plunge_feed: f64,
    /// Whether passes may be reordered to shorten travel.
    #[serde(default = "yes")]
    pub reorder: bool,
    /// Whether an open pass may be reversed while reordering.
    ///
    /// Off by default: reversing flips climb for conventional, and that is a cutting
    /// decision rather than a travel one.
    #[serde(default)]
    pub allow_reverse: bool,
}

impl Default for Linking {
    fn default() -> Self {
        Self {
            retract_feed: default_retract_feed(),
            plunge_feed: default_plunge_feed(),
            reorder: true,
            allow_reverse: false,
        }
    }
}

fn default_retract_feed() -> f64 {
    1500.0
}

fn default_plunge_feed() -> f64 {
    300.0
}

fn yes() -> bool {
    true
}

/// A job that cannot be run as written.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum JobError {
    /// An operation named a tool the table does not define.
    #[error("operation '{op}' uses tool '{tool}', which is not in the tool table")]
    UnknownTool {
        /// Which operation.
        op: String,
        /// Which tool it asked for.
        tool: String,
    },
    /// A tool dimension was not physically meaningful.
    #[error("tool '{name}': {source}")]
    BadTool {
        /// Which tool.
        name: String,
        /// Why.
        source: ToolError,
    },
    /// A value was outside its usable range.
    #[error("{field} must be finite and greater than zero, got {value}")]
    BadValue {
        /// Which field.
        field: String,
        /// What it was.
        value: f64,
    },
    /// The job would do nothing.
    #[error("job has no operations")]
    NoOperations,
    /// A work offset outside the usual range.
    #[error("work offset {0} is not in the range 54..=59")]
    BadWorkOffset(u8),
}

impl Job {
    /// Check the job is runnable before anything is computed.
    ///
    /// Everything catchable is caught here, so a job fails on its own terms rather
    /// than part-way through an operation with a half-built path in hand.
    pub fn validate(&self) -> Result<(), JobError> {
        if self.operations.is_empty() {
            return Err(JobError::NoOperations);
        }
        if !(54..=59).contains(&self.setup.wcs) {
            return Err(JobError::BadWorkOffset(self.setup.wcs));
        }
        positive("tolerance", self.tolerance)?;

        for (name, spec) in &self.tools {
            spec.build().map_err(|source| JobError::BadTool {
                name: name.clone(),
                source,
            })?;
            if spec.rpm == 0 {
                return Err(JobError::BadValue {
                    field: format!("tools.{name}.rpm"),
                    value: 0.0,
                });
            }
        }

        for op in &self.operations {
            if !self.tools.contains_key(&op.tool) {
                return Err(JobError::UnknownTool {
                    op: op.name.clone(),
                    tool: op.tool.clone(),
                });
            }
            positive(&format!("{}.passes.stepover", op.name), op.passes.stepover)?;
            positive(&format!("{}.passes.feed", op.name), op.passes.feed)?;
            positive(
                &format!("{}.passes.max_cut_step", op.name),
                op.passes.max_cut_step,
            )?;
            positive(
                &format!("{}.linking.retract_feed", op.name),
                op.linking.retract_feed,
            )?;
            positive(
                &format!("{}.linking.plunge_feed", op.name),
                op.linking.plunge_feed,
            )?;
            if op.passes.stock_to_leave.radial < 0.0 || op.passes.stock_to_leave.axial < 0.0 {
                return Err(JobError::BadValue {
                    field: format!("{}.passes.stock_to_leave", op.name),
                    value: op
                        .passes
                        .stock_to_leave
                        .radial
                        .min(op.passes.stock_to_leave.axial),
                });
            }
            if let StrategySpec::ZlevelRough { stepdown, .. } = op.strategy {
                positive(&format!("{}.stepdown", op.name), stepdown)?;
            }
        }
        Ok(())
    }

    /// The tool an operation names.
    pub fn tool_for(&self, op: &Operation) -> Option<&ToolSpec> {
        self.tools.get(&op.tool)
    }
}

fn positive(field: &str, value: f64) -> Result<(), JobError> {
    (value.is_finite() && value > 0.0)
        .then_some(())
        .ok_or_else(|| JobError::BadValue {
            field: field.to_string(),
            value,
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use crate::tool::ToolProfile;

    const SAMPLE: &str = r#"
tolerance = 0.02

[setup]
stock = { kind = "box", margin = [2.0, 2.0, 1.0] }
wcs = 54

[tools.rough-6-flat]
kind = "flat"
diameter = 6.0
number = 1
rpm = 16000
comment = "6mm flat, carbide"

[tools.finish-4-ball]
kind = "ball"
diameter = 4.0
number = 2

[[operation]]
name = "rough"
strategy = "zlevel-rough"
tool = "rough-6-flat"
stepdown = 2.0
geometry = { margin = 1.0, bottom = { ref = "model-bottom" } }
heights = { clearance = { ref = "stock-top", offset = 8.0 } }
passes = { stepover = 3.0, feed = 1800.0, stock_to_leave = { radial = 0.5, axial = 0.2 } }
linking = { retract_feed = 1800.0, plunge_feed = 400.0 }

[[operation]]
name = "finish"
strategy = "parallel"
tool = "finish-4-ball"
angle = 90.0
passes = { stepover = 0.4, feed = 2400.0 }
"#;

    fn sample() -> Job {
        toml::from_str(SAMPLE).unwrap()
    }

    #[test]
    fn parses_a_complete_job() {
        let j = sample();
        assert_eq!(j.tolerance, 0.02);
        assert_eq!(j.tools.len(), 2);
        assert_eq!(j.operations.len(), 2);
        j.validate().unwrap();
    }

    #[test]
    fn defaults_fill_in_what_is_omitted() {
        let minimal = r#"
[setup]
stock = { kind = "box" }
[tools.t]
kind = "ball"
diameter = 6.0
[[operation]]
name = "finish"
strategy = "parallel"
tool = "t"
"#;
        let j: Job = toml::from_str(minimal).unwrap();
        j.validate().unwrap();
        assert_eq!(j.tolerance, 0.01);
        assert_eq!(j.setup.wcs, 54);
        let op = j.operations.first().unwrap();
        assert_eq!(op.passes.stepover, 1.0);
        assert!(op.linking.reorder);
        assert!(
            !op.linking.allow_reverse,
            "reversing flips climb for conventional and must be asked for"
        );
    }

    /// The reason heights are symbolic: the same job survives a different block.
    #[test]
    fn heights_resolve_against_the_setup() {
        let ctx = HeightContext {
            stock_top: 20.0,
            stock_bottom: -5.0,
            model_top: 18.0,
            model_bottom: 0.0,
        };
        let cases = [
            (HeightRef::StockTop, 8.0, 28.0),
            (HeightRef::StockBottom, 0.0, -5.0),
            (HeightRef::ModelTop, 2.0, 20.0),
            (HeightRef::ModelBottom, -1.0, -1.0),
            (HeightRef::Absolute, 12.5, 12.5),
        ];
        for (reference, offset, want) in cases {
            let h = Height { reference, offset };
            assert_eq!(h.resolve(&ctx), want, "{reference:?} + {offset}");
        }
    }

    #[test]
    fn a_taller_block_moves_the_clearance_with_it() {
        let h = Height {
            reference: HeightRef::StockTop,
            offset: 10.0,
        };
        let short = HeightContext {
            stock_top: 10.0,
            stock_bottom: 0.0,
            model_top: 8.0,
            model_bottom: 0.0,
        };
        let tall = HeightContext {
            stock_top: 50.0,
            ..short
        };
        assert_eq!(h.resolve(&short), 20.0);
        assert_eq!(h.resolve(&tall), 60.0);
    }

    #[test]
    fn tools_build_into_the_bullnose_family() {
        let j = sample();
        let flat = j.tools.get("rough-6-flat").unwrap().build().unwrap();
        assert_eq!(flat.max_radius(), 3.0);
        assert_eq!(flat.corner(), 0.0, "a flat mill has no corner");

        let ball = j.tools.get("finish-4-ball").unwrap().build().unwrap();
        assert_eq!(ball.corner(), 2.0, "a ball mill's corner fills its radius");
    }

    #[test]
    fn an_unknown_tool_is_caught_before_anything_runs() {
        let bad = SAMPLE.replace(r#"tool = "finish-4-ball""#, r#"tool = "nonesuch""#);
        let j: Job = toml::from_str(&bad).unwrap();
        assert_eq!(
            j.validate(),
            Err(JobError::UnknownTool {
                op: "finish".into(),
                tool: "nonesuch".into()
            })
        );
    }

    /// Catchable mistakes are caught up front, not part-way through an operation
    /// with a half-built path in hand.
    #[test]
    fn unusable_values_are_rejected_up_front() {
        for (from, to) in [
            ("stepover = 0.4", "stepover = 0.0"),
            ("feed = 2400.0", "feed = -1.0"),
            ("tolerance = 0.02", "tolerance = 0.0"),
            ("wcs = 54", "wcs = 99"),
            ("stepdown = 2.0", "stepdown = 0.0"),
        ] {
            let j: Job = toml::from_str(&SAMPLE.replace(from, to)).unwrap();
            assert!(j.validate().is_err(), "{to} should be rejected");
        }
    }

    #[test]
    fn a_job_with_no_operations_is_refused() {
        let j: Job = toml::from_str(
            r#"
[setup]
stock = { kind = "box" }
[tools.t]
kind = "ball"
diameter = 6.0
"#,
        )
        .unwrap();
        assert_eq!(j.validate(), Err(JobError::NoOperations));
    }

    #[test]
    fn strategy_parameters_belong_to_their_strategy() {
        let j = sample();
        match j.operations.first().unwrap().strategy {
            StrategySpec::ZlevelRough { stepdown, .. } => assert_eq!(stepdown, 2.0),
            StrategySpec::Parallel { .. } => panic!("first operation should be roughing"),
        }
        match j.operations.get(1).unwrap().strategy {
            StrategySpec::Parallel { angle } => assert_eq!(angle, 90.0),
            StrategySpec::ZlevelRough { .. } => panic!("second operation should be finishing"),
        }
    }
}
