//! An operation, shaped the way Fusion shapes one: its strategy, then what it
//! machines (Fusion's Geometry tab: a selection, a 3D boundary or a 2D outline),
//! Heights, Passes and Linking.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    Boundary, Compensation, Depths, Heights, Linking, MachiningBoundary, Outline, Passes, Preset,
    ProfileSide, Selection, StrategySpec, ToolKind, ToolSpec, is_default, is_yes, yes,
};

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
    /// Faces and edges picked on the models, for whatever of it reads them: a
    /// boundary or outline of the selection, a height measured from it, the holes
    /// a drill drills.
    #[serde(default, skip_serializing_if = "is_default")]
    pub selection: Selection,
    /// Where a 3D operation's tool may go
    /// ([`Uses::boundary`](super::Uses::boundary)).
    #[serde(default, skip_serializing_if = "is_default")]
    pub boundary: MachiningBoundary,
    /// What a 2D operation cuts round or clears
    /// ([`Uses::outline`](super::Uses::outline)).
    #[serde(default, skip_serializing_if = "is_default")]
    pub outline: Outline,
    /// Where the tool travels, and the levels it cuts between.
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
    /// Settings worked out from expressions, as Fusion's parameter
    /// expressions are, each by its place in the job file
    /// (`passes.stepover = "tool_diameter * 0.4"`): worked out again whenever
    /// what they read changes ([`Job::work_out_expressions`](super::Job::work_out_expressions)).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub expressions: BTreeMap<String, String>,
}

impl Operation {
    /// A new operation called `name` of `strategy`, with no tool yet, and
    /// everything else as Fusion starts it: the strategy's own multiple depths
    /// and stock to leave ([`StrategySpec::default_stepdown`],
    /// [`StrategySpec::default_stock_to_leave`]), its stepover and stepdown
    /// worked out from its tool once it has one
    /// ([`StrategySpec::default_expressions`]), and the rest as a job file that
    /// gives none has it.
    pub fn new(name: impl Into<String>, strategy: StrategySpec) -> Self {
        let strategy = strategy.fresh();
        let stepdown = strategy.default_stepdown();
        Self {
            name: name.into(),
            strategy,
            tool: None,
            preset: None,
            selection: Selection::default(),
            boundary: MachiningBoundary::default(),
            outline: Outline::default(),
            heights: Heights::fresh(),
            passes: Passes {
                stepdown,
                stock_to_leave: strategy.default_stock_to_leave(),
                ..Passes::default()
            },
            linking: Linking {
                entry: strategy.fresh_entry(),
                retraction: strategy.fresh_retraction(),
                ..Linking::default()
            },
            enabled: true,
            // The stepdown's expression comes with the levels: with none, it
            // would put them back.
            expressions: strategy
                .default_expressions()
                .iter()
                .filter(|(setting, _)| *setting != "passes.stepdown" || stepdown.is_some())
                .map(|(setting, text)| ((*setting).to_string(), (*text).to_string()))
                .collect(),
        }
    }

    /// Why this operation cannot cut with `tool`, if it cannot.
    ///
    /// A cone slices badly into Z levels (its reach widens all the way up, so the
    /// levels need far too many offsets) and cannot leave stock (grown by a ball it
    /// is not a cone); it finishes, engraves and chamfers.
    ///
    /// Negative stock shrinks the tool, as Fusion's does, so it can take no more
    /// than the tool's radius beside; and in 3D, where a round end has to stay
    /// round, no more than its corner radius down.
    pub fn cannot_use(&self, tool: &ToolSpec) -> Option<&'static str> {
        let uses = self.strategy.uses();
        let drilling = matches!(self.strategy, StrategySpec::Drill { .. });
        if tool.kind == ToolKind::Drill && !drilling {
            return Some("a drill only drills; use a drilling operation");
        }
        if drilling && !matches!(tool.kind, ToolKind::Drill | ToolKind::Flat) {
            return Some("drill holes with a drill, or a flat end mill that plunges");
        }
        if matches!(self.strategy, StrategySpec::Face { .. }) && tool.kind == ToolKind::Ball {
            return Some("a ball end mill cannot face flat; use a flat or bull-nose end mill");
        }
        if uses.stepover && self.passes.stepover > tool.diameter {
            return Some(
                "the stepover is wider than the tool: it would leave ridges between passes",
            );
        }
        let leave = self.stock_to_leave();
        let leaves = leave.radial != 0.0 || leave.axial != 0.0;
        let least = self.least_stock_to_leave(tool);
        if leave.radial <= least.radial {
            return Some("negative stock to leave beside must be less than the tool's radius");
        }
        if leave.axial < least.axial {
            return Some(
                "negative stock to leave down must be no more than the tool's corner radius",
            );
        }
        let chamfering = matches!(self.strategy, StrategySpec::Chamfer2d { .. });
        if chamfering && tool.kind != ToolKind::Cone {
            return Some(
                "a 2D chamfer cuts with a cone: use a chamfer mill or V-bit, whose flank \
                 makes the chamfer",
            );
        }
        let engraving = matches!(self.strategy, StrategySpec::Engrave { .. });
        if engraving && tool.kind != ToolKind::Cone {
            return Some(
                "an engraving cuts with a cone: use a V-bit or chamfer mill, whose flank \
                 touches the outline",
            );
        }
        if tool.kind != ToolKind::Cone {
            return None;
        }
        match self.strategy {
            StrategySpec::PocketClearing { .. }
            | StrategySpec::Adaptive { .. }
            | StrategySpec::Adaptive2d { .. } => {
                return Some(
                    "a V-bit or chamfer mill cannot rough in Z levels; use a parallel pass",
                );
            }
            StrategySpec::Contour { .. } => {
                return Some(
                    "a V-bit or chamfer mill cannot finish in Z levels; use a parallel pass",
                );
            }
            StrategySpec::Flat { .. } => {
                return Some("a V-bit or chamfer mill cannot finish a flat");
            }
            StrategySpec::Pocket { .. } => {
                return Some("a V-bit or chamfer mill cannot clear a pocket's floor flat");
            }
            StrategySpec::Face { .. } => {
                return Some("a V-bit or chamfer mill cannot face flat");
            }
            StrategySpec::Bore { .. } | StrategySpec::Circular { .. } => {
                return Some(
                    "a V-bit or chamfer mill cannot cut a round wall straight: its width \
                     changes with depth",
                );
            }
            StrategySpec::Profile { side, .. } if side != ProfileSide::On => {
                return Some(
                    "a V-bit or chamfer mill profiles only on the line: its width changes \
                     with depth",
                );
            }
            StrategySpec::Slot { .. } => {
                return Some(
                    "a V-bit or chamfer mill cannot cut a slot's walls straight: its width \
                     changes with depth",
                );
            }
            StrategySpec::Trace { compensation, .. } if compensation != Compensation::Center => {
                return Some(
                    "a V-bit or chamfer mill traces only on the curve: its width changes \
                     with depth, so it has no one radius to keep beside it",
                );
            }
            StrategySpec::Chamfer2d {
                width, tip_offset, ..
            } if chamfer_reach(tool, width, tip_offset) > tool.diameter / 2.0 + 1e-9 => {
                return Some(
                    "the chamfer is wider than the tool's flank reaches: use a wider tool, \
                     or a smaller chamfer width or tip offset",
                );
            }
            StrategySpec::Engrave {
                depth: Some(depth), ..
            } if engrave_flank(tool).is_some_and(|most| depth > most + 1e-9) => {
                return Some(
                    "the engraving is deeper than the tool's flank reaches: use a wider \
                     tool, or a smaller depth",
                );
            }
            _ => {}
        }
        if leaves {
            return Some("a V-bit or chamfer mill cuts to size: set stock to leave to zero");
        }
        None
    }

    /// A new operation called `name` of `strategy`, cutting with the job's tool
    /// `tool`, described by `spec`, and its preset `preset`: as
    /// [`Operation::new`] starts one, with the preset's values, on the part's
    /// silhouette as Fusion starts it ([`StrategySpec::starting_containment`]),
    /// and with room round the part for the tool ([`Operation::fit_channel`]).
    pub fn fresh(
        name: impl Into<String>,
        strategy: StrategySpec,
        (tool, spec, preset): (Option<String>, Option<&ToolSpec>, Option<&Preset>),
    ) -> Self {
        let mut op = Self {
            tool,
            ..Self::new(name, strategy)
        };
        if let Some(p) = preset {
            op.apply_preset(p);
        }
        if let Some(containment) = strategy.starting_containment() {
            op.boundary.kind = Boundary::Silhouette;
            op.boundary.containment = containment;
        }
        op.fit_channel(spec);
        op
    }

    /// Room for `tool` round the part, as the Geometry tab gives it: a 3D
    /// boundary that [`MachiningBoundary::needs_room`] is offset by the tool's
    /// diameter, a channel two tools wide. Set once, as a number, so changing
    /// the tool later does not move it.
    pub fn fit_channel(&mut self, tool: Option<&ToolSpec>) {
        if self.strategy.uses().boundary
            && self.boundary.needs_room()
            && let Some(t) = tool
        {
            self.boundary.margin = t.diameter;
        }
    }

    /// Copy the values `preset` has into this operation, leaving the rest, and
    /// record where they came from.
    pub fn apply_preset(&mut self, preset: &Preset) {
        // What the preset gives is the setting's, not worked out over it.
        let mut given = |setting: &str| {
            self.expressions.remove(setting);
        };
        for (setting, is) in [
            ("passes.rpm", preset.rpm.is_some()),
            ("passes.feed", preset.feed.is_some()),
            ("linking.plunge_feed", preset.plunge_feed.is_some()),
            ("linking.ramp_feed", preset.ramp_feed.is_some()),
            ("passes.stepdown", preset.stepdown.is_some()),
            (
                "passes.stock_to_leave.radial",
                preset.stock_to_leave.is_some(),
            ),
            (
                "passes.stock_to_leave.axial",
                preset.stock_to_leave.is_some(),
            ),
            (
                "passes.stepover",
                preset.stepover.is_some() && !self.strategy.uses().load,
            ),
        ] {
            if is {
                given(setting);
            }
        }
        if let Some(rpm) = preset.rpm {
            self.passes.rpm = Some(rpm);
        }
        if let Some(feed) = preset.feed {
            self.passes.feed = feed;
        }
        if let Some(plunge) = preset.plunge_feed {
            self.linking.plunge_feed = plunge;
        }
        if let Some(ramp) = preset.ramp_feed {
            self.linking.ramp_feed = Some(ramp);
        }
        // A preset's stepover is a slotting or finishing one: an adaptive
        // operation's is its load, a fraction of it.
        if let Some(stepover) = preset.stepover
            && !self.strategy.uses().load
        {
            self.passes.stepover = stepover;
        }
        if let Some(d) = preset.stepdown {
            // Multiple depths stay as they were chosen, on or off.
            let depths = self.strategy.uses().depths;
            match (&mut self.strategy, depths) {
                (StrategySpec::Drill { peck, .. }, _) => *peck = Some(d),
                (_, Depths::Required) => self.passes.stepdown = Some(d),
                (_, Depths::Optional) if self.passes.stepdown.is_some() => {
                    self.passes.stepdown = Some(d)
                }
                _ => {}
            }
        }
        if let Some(s) = preset.stock_to_leave {
            self.passes.stock_to_leave = s;
        }
        self.preset = Some(preset.name.clone());
    }
}

/// How deep `tool`'s flank reaches, from its tip to where it is its full
/// width: what an engraving with it may go down to at most. `None` for a tool
/// with no angle.
pub fn engrave_flank(tool: &ToolSpec) -> Option<f64> {
    let half = tool.angle?.to_radians() / 2.0;
    (half > 0.0 && half < std::f64::consts::FRAC_PI_2)
        .then(|| (tool.diameter - tool.tip_diameter) / 2.0 / half.tan())
}

/// How deep an engraving with `tool` goes when its depth is not given: 0.95
/// of how deep its flank reaches, a new Engrave of Fusion's Bottom offset,
/// `0.95*((tool_diameter-tool_tipDiameter)/2)*tan(90 - tool_taperAngle)`.
pub fn engrave_depth(tool: &ToolSpec) -> Option<f64> {
    engrave_flank(tool).map(|f| 0.95 * f)
}

/// How far out from a 2D chamfer's tool's axis the top of a chamfer `width`
/// across reaches, its tip `tip_offset` below the chamfer, with `tool`'s cone:
/// its tip's radius, what the tip offset moves it out, and the width. More
/// than the tool's radius, its flank does not reach the chamfer's top. A tool
/// with no angle reaches nowhere.
pub fn chamfer_reach(tool: &ToolSpec, width: f64, tip_offset: f64) -> f64 {
    let half = tool.angle.unwrap_or(0.0).to_radians() / 2.0;
    if !(half > 0.0 && half < std::f64::consts::FRAC_PI_2) {
        return f64::INFINITY;
    }
    tool.tip_diameter / 2.0 + tip_offset.max(0.0) * half.tan() + width.max(0.0)
}
