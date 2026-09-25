//! An operation, shaped the way Fusion shapes one: its strategy, then Geometry,
//! Heights, Passes and Linking.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::validate::positive;
use super::{FeatureRef, JobError, Preset, ToolKind, ToolSpec, is_default};

/// What an operation is kept to, seen from above: Fusion's machining boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Boundary {
    /// The stock's footprint: the whole part, and everything around it.
    Stock,
    /// The rectangle around the setup's models.
    BoundingBox,
    /// The setup's models' outline: their shadow from straight above.
    Silhouette,
    /// The picked faces seen from above, and the loops the picked edges close.
    Selection,
}

/// Where the tool may go, against the machining boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Containment {
    /// The tool's centre stays inside the boundary, so its side reaches past it by
    /// its radius. Fusion's default.
    #[default]
    Center,
    /// The whole tool stays inside.
    Inside,
    /// The tool may go its whole width outside, so its side still reaches the
    /// boundary from beyond it.
    Outside,
}

/// One operation.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Operation {
    /// Name, for comments and diagnostics.
    pub name: String,
    /// Which strategy, and its own parameters.
    #[serde(flatten)]
    pub strategy: StrategySpec,
    /// Which cutter, by name from the job's tools. A new operation has none yet,
    /// and cannot run until it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The tool's preset its values were copied from, by name. Only a record: the
    /// operation cuts with its own values, whatever the preset now says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// What to machine.
    #[serde(default, skip_serializing_if = "is_default")]
    pub geometry: Geometry,
    /// Where the tool travels.
    #[serde(default, skip_serializing_if = "is_default")]
    pub heights: Heights,
    /// How the passes are laid out.
    #[serde(default, skip_serializing_if = "is_default")]
    pub passes: Passes,
    /// How the tool moves between cuts.
    #[serde(default, skip_serializing_if = "is_default")]
    pub linking: Linking,
    /// Whether it runs. One turned off stays in the job, but is not worked out,
    /// cut, checked or posted.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub enabled: bool,
}

/// Strategy and the parameters only it has.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
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
    /// Holes, picked by their rims, each drilled straight down.
    Drill {
        /// Depth of each peck, backing out between; all at once when not given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        peck: Option<f64>,
        /// Seconds to pause at the bottom, for a canned cycle that can.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dwell: Option<f64>,
    },
}

impl StrategySpec {
    /// The strategy's name as a job file writes it, which is also how a program's
    /// section comment names it.
    pub fn name(&self) -> &'static str {
        match self {
            StrategySpec::Parallel { .. } => "parallel",
            StrategySpec::ZlevelRough { .. } => "zlevel-rough",
            StrategySpec::Drill { .. } => "drill",
        }
    }
}

fn default_min_ring() -> f64 {
    3.0
}

/// What to machine.
///
/// Fusion's Geometry tab: a machining boundary seen from above, where the tool
/// may go against it, and how far past it. The stock's footprint, the default,
/// machines the whole part and everything around it; the models' bounding box or
/// silhouette keeps the tool near the part; picked faces and edges keep it to their
/// outline. What else picks mean is the strategy's to say, which is why they live
/// here and not with it: a pocket will clear a picked floor, a trace will follow
/// picked edges.
///
/// A job from before boundaries had no `boundary`: picks made the boundary, and
/// with none it was the stock's. [`Geometry::boundary`] reads it so.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Geometry {
    /// The boundary; `None` for what a job from before boundaries meant.
    #[serde(default, rename = "boundary", skip_serializing_if = "Option::is_none")]
    pub boundary_kind: Option<Boundary>,
    /// Fusion's additional offset, in millimetres: how far the boundary moves
    /// out, or in when the whole tool is kept inside it. Past the stock's
    /// footprint, with no boundary.
    #[serde(default)]
    pub margin: f64,
    /// Highest level to cut at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Height>,
    /// Lowest level to cut at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<Height>,
    /// Faces picked on the models.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FeatureRef>,
    /// Edges picked on the models.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<FeatureRef>,
    /// Where the tool may go against the boundary.
    #[serde(default, skip_serializing_if = "is_default")]
    pub containment: Containment,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            boundary_kind: None,
            margin: 0.0,
            top: None,
            bottom: None,
            faces: Vec::new(),
            edges: Vec::new(),
            containment: Containment::Center,
        }
    }
}

impl Geometry {
    /// Whether any face or edge is picked.
    pub fn picked(&self) -> bool {
        !self.faces.is_empty() || !self.edges.is_empty()
    }

    /// The machining boundary. One never chosen is what it was before there was a
    /// choice: the picks', when there are any, or the stock's.
    pub fn boundary(&self) -> Boundary {
        self.boundary_kind.unwrap_or(self.legacy_boundary())
    }

    /// Whether a build from before boundaries would keep the tool where this
    /// does: the same boundary, and an offset that means the same.
    pub fn reads_as_before(&self) -> bool {
        self.boundary() == self.legacy_boundary()
            && (self.containment != Containment::Inside || self.margin == 0.0)
    }

    fn legacy_boundary(&self) -> Boundary {
        if self.picked() {
            Boundary::Selection
        } else {
            Boundary::Stock
        }
    }
}

/// Where the tool travels.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
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

impl Operation {
    /// Why this operation cannot cut with `tool`, if it cannot.
    ///
    /// A cone slices badly into Z levels (its reach widens all the way up, so the
    /// levels need far too many offsets) and cannot leave stock (grown by a ball it
    /// is not a cone); it finishes, engraves and chamfers.
    pub fn cannot_use(&self, tool: &ToolSpec) -> Option<&'static str> {
        let drilling = matches!(self.strategy, StrategySpec::Drill { .. });
        if tool.kind == ToolKind::Drill && !drilling {
            return Some("a drill only drills; use a drilling operation");
        }
        if drilling && !matches!(tool.kind, ToolKind::Drill | ToolKind::Flat) {
            return Some("drill holes with a drill, or a flat end mill that plunges");
        }
        let leave = self.passes.stock_to_leave;
        let leaves = leave.radial != 0.0 || leave.axial != 0.0;
        if drilling && leaves {
            return Some("a hole is drilled to size: set stock to leave to zero");
        }
        if tool.kind != ToolKind::Cone {
            return None;
        }
        if matches!(self.strategy, StrategySpec::ZlevelRough { .. }) {
            return Some("a V-bit or chamfer mill cannot rough in Z levels; use a parallel pass");
        }
        if leaves {
            return Some("a V-bit or chamfer mill cuts to size: set stock to leave to zero");
        }
        None
    }

    /// Check this operation of setup `setup` can run with the job's `tools`: it
    /// names one the job has, the tool can do what it asks, and its values are
    /// usable. Its picks are checked with the setup's.
    pub(super) fn check(
        &self,
        setup: &str,
        tools: &BTreeMap<String, ToolSpec>,
    ) -> Result<(), JobError> {
        let at = format!("{setup}/{}", self.name);
        let Some(tool) = &self.tool else {
            return Err(JobError::NoTool { op: at });
        };
        let Some(spec) = tools.get(tool) else {
            return Err(JobError::UnknownTool {
                op: at,
                tool: tool.clone(),
            });
        };
        if let Some(why) = self.cannot_use(spec) {
            return Err(JobError::WrongTool {
                op: at,
                tool: tool.clone(),
                why: why.into(),
            });
        }
        positive(&format!("{at}.passes.stepover"), self.passes.stepover)?;
        positive(&format!("{at}.passes.feed"), self.passes.feed)?;
        if self.passes.rpm == Some(0) {
            return Err(JobError::BadValue {
                field: format!("{at}.passes.rpm"),
                value: 0.0,
            });
        }
        positive(
            &format!("{at}.passes.max_cut_step"),
            self.passes.max_cut_step,
        )?;
        positive(
            &format!("{at}.linking.retract_feed"),
            self.linking.retract_feed,
        )?;
        positive(
            &format!("{at}.linking.plunge_feed"),
            self.linking.plunge_feed,
        )?;
        let leave = self.passes.stock_to_leave;
        if leave.radial < 0.0 || leave.axial < 0.0 {
            return Err(JobError::BadValue {
                field: format!("{at}.passes.stock_to_leave"),
                value: leave.radial.min(leave.axial),
            });
        }
        if let StrategySpec::ZlevelRough { stepdown, .. } = self.strategy {
            positive(&format!("{at}.stepdown"), stepdown)?;
        }
        if let StrategySpec::Drill { peck, dwell } = self.strategy {
            if let Some(p) = peck {
                positive(&format!("{at}.peck"), p)?;
            }
            if let Some(d) = dwell
                && !(d.is_finite() && d >= 0.0)
            {
                return Err(JobError::BadValue {
                    field: format!("{at}.dwell"),
                    value: d,
                });
            }
        }
        Ok(())
    }

    /// The stepover Fusion gives this operation with `tool`, at `tolerance`: for
    /// parallel finishing, a quarter of the tool's diameter with a square end, and
    /// with a round one the spacing that leaves a cusp no taller than the
    /// tolerance, `2·√(r² − (r − tol)²) / √2`; for Z-level clearing, 0.6 of the
    /// diameter, its 3D pocket's. `None` for drilling, which has none.
    pub fn default_stepover(&self, tool: &ToolSpec, tolerance: f64) -> Option<f64> {
        let d = tool.diameter;
        match self.strategy {
            StrategySpec::Parallel { .. } => {
                let r = match tool.kind {
                    ToolKind::Ball => d / 2.0,
                    ToolKind::Bullnose => tool.corner,
                    ToolKind::Flat | ToolKind::Cone | ToolKind::Drill => 0.0,
                };
                let over = if r > tolerance && r > 0.0 {
                    2.0 * (r * r - (r - tolerance).powi(2)).sqrt() / 2f64.sqrt()
                } else {
                    d / 4.0
                };
                // To the micron, as Fusion rounds it.
                Some((over * 1000.0).round() / 1000.0)
            }
            StrategySpec::ZlevelRough { .. } => Some(0.6 * d),
            StrategySpec::Drill { .. } => None,
        }
    }

    /// Copy the values `preset` has into this operation, leaving the rest, and
    /// record where they came from.
    pub fn apply_preset(&mut self, preset: &Preset) {
        if let Some(rpm) = preset.rpm {
            self.passes.rpm = Some(rpm);
        }
        if let Some(feed) = preset.feed {
            self.passes.feed = feed;
        }
        if let Some(plunge) = preset.plunge_feed {
            self.linking.plunge_feed = plunge;
        }
        if let Some(stepover) = preset.stepover {
            self.passes.stepover = stepover;
        }
        match (preset.stepdown, &mut self.strategy) {
            (Some(d), StrategySpec::ZlevelRough { stepdown, .. }) => *stepdown = d,
            (Some(d), StrategySpec::Drill { peck, .. }) => *peck = Some(d),
            _ => {}
        }
        if let Some(s) = preset.stock_to_leave {
            self.passes.stock_to_leave = s;
        }
        self.preset = Some(preset.name.clone());
    }

    /// This operation's speeds and feeds as a preset called `name`: what
    /// [`apply_preset`](Self::apply_preset) would set them back to.
    #[cfg(test)]
    pub(crate) fn as_preset(&self, name: &str) -> Preset {
        Preset {
            name: name.to_string(),
            comment: String::new(),
            rpm: self.passes.rpm,
            feed: Some(self.passes.feed),
            plunge_feed: Some(self.linking.plunge_feed),
            ramp_feed: None,
            stepdown: match self.strategy {
                StrategySpec::ZlevelRough { stepdown, .. } => Some(stepdown),
                StrategySpec::Drill { peck, .. } => peck,
                StrategySpec::Parallel { .. } => None,
            },
            stepover: Some(self.passes.stepover),
            stock_to_leave: Some(self.passes.stock_to_leave),
        }
    }

    /// The values `preset` has that this operation no longer matches, by the name
    /// a person reads: what has been changed since it was applied.
    #[cfg(test)]
    pub(crate) fn differs_from(&self, preset: &Preset) -> Vec<&'static str> {
        let same = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
        let mut out = Vec::new();
        if preset.rpm.is_some() && preset.rpm != self.passes.rpm {
            out.push("spindle speed");
        }
        if preset.feed.is_some_and(|f| !same(f, self.passes.feed)) {
            out.push("feed");
        }
        if preset
            .plunge_feed
            .is_some_and(|f| !same(f, self.linking.plunge_feed))
        {
            out.push("plunge feed");
        }
        if preset
            .stepover
            .is_some_and(|s| !same(s, self.passes.stepover))
        {
            out.push("stepover");
        }
        match (preset.stepdown, &self.strategy) {
            (Some(d), StrategySpec::ZlevelRough { stepdown, .. }) if !same(d, *stepdown) => {
                out.push("stepdown");
            }
            (Some(d), StrategySpec::Drill { peck, .. }) if !peck.is_some_and(|p| same(d, p)) => {
                out.push("peck");
            }
            _ => {}
        }
        if let Some(s) = preset.stock_to_leave {
            let now = self.passes.stock_to_leave;
            if !same(s.radial, now.radial) || !same(s.axial, now.axial) {
                out.push("stock to leave");
            }
        }
        out
    }
}

/// How the passes are laid out.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
    /// Spindle speed; the tool's own when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpm: Option<u32>,
}

impl Default for Passes {
    fn default() -> Self {
        Self {
            stepover: default_stepover(),
            feed: default_feed(),
            stock_to_leave: StockToLeave::default(),
            max_cut_step: default_max_cut_step(),
            rpm: None,
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, Serialize)]
pub struct StockToLeave {
    /// Perpendicular to the tool axis.
    #[serde(default)]
    pub radial: f64,
    /// Along the tool axis.
    #[serde(default)]
    pub axial: f64,
}

/// How the tool moves between cuts.
///
/// What each setting does, and why its default is Fusion's, is in
/// `docs/decisions/0004-linking-and-entries.md`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
    /// How far above the material the tool stops rapiding down and feeds, and up to
    /// which it feeds on the way out, in millimetres.
    #[serde(
        default = "default_safe_distance",
        skip_serializing_if = "is_default_safe_distance"
    )]
    pub safe_distance: f64,
    /// The longest link made without lifting clear, in millimetres: the
    /// strategy's own when not given (50 mm for parallel, 5 × the tool's diameter
    /// for Z-level), zero to always lift.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stay_down: Option<f64>,
    /// The radius of the vertical arcs a parallel pass is entered and left along,
    /// in millimetres: a tenth of the tool's diameter when not given, zero for
    /// none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_radius: Option<f64>,
    /// How steeply Z-level roughing ramps into material, in degrees from level.
    #[serde(
        default = "default_ramp_angle",
        skip_serializing_if = "is_default_ramp_angle"
    )]
    pub ramp_angle: f64,
    /// Feed along a ramp, in mm/min: a third of the cutting feed when not given,
    /// as Fusion's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_feed: Option<f64>,
}

impl Default for Linking {
    fn default() -> Self {
        Self {
            retract_feed: default_retract_feed(),
            plunge_feed: default_plunge_feed(),
            reorder: true,
            allow_reverse: false,
            safe_distance: default_safe_distance(),
            stay_down: None,
            lead_radius: None,
            ramp_angle: default_ramp_angle(),
            ramp_feed: None,
        }
    }
}

fn default_safe_distance() -> f64 {
    2.0
}

// Serde's `skip_serializing_if` hands the field by reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_safe_distance(v: &f64) -> bool {
    v.to_bits() == default_safe_distance().to_bits()
}

fn default_ramp_angle() -> f64 {
    2.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_ramp_angle(v: &f64) -> bool {
    v.to_bits() == default_ramp_angle().to_bits()
}

fn default_retract_feed() -> f64 {
    1500.0
}

fn default_plunge_feed() -> f64 {
    300.0
}

// Serde's `skip_serializing_if` hands the field by reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_yes(b: &bool) -> bool {
    *b
}

fn yes() -> bool {
    true
}
