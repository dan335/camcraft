//! An operation, shaped the way Fusion shapes one: its strategy, then what it
//! machines (Fusion's Geometry tab: a selection, a 3D boundary or a 2D outline),
//! Heights, Passes and Linking.

use serde::{Deserialize, Serialize};

use super::{
    Depths, Heights, Linking, MachiningBoundary, Outline, Passes, Preset, ProfileSide, Selection,
    StrategySpec, ToolKind, ToolSpec, is_default, is_yes, yes,
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
}

impl Operation {
    /// A new operation called `name` of `strategy`, with no tool yet, and
    /// everything else as Fusion starts it: the strategy's own multiple depths
    /// and stock to leave ([`StrategySpec::default_stepdown`],
    /// [`StrategySpec::default_stock_to_leave`]), and the rest as a job file that
    /// gives none has it.
    pub fn new(name: impl Into<String>, strategy: StrategySpec) -> Self {
        Self {
            name: name.into(),
            strategy,
            tool: None,
            preset: None,
            selection: Selection::default(),
            boundary: MachiningBoundary::default(),
            outline: Outline::default(),
            heights: Heights::default(),
            passes: Passes {
                stepdown: strategy.default_stepdown(),
                stock_to_leave: strategy.default_stock_to_leave(),
                ..Passes::default()
            },
            linking: Linking::default(),
            enabled: true,
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
        let drilling = uses.holes;
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
        if tool.kind != ToolKind::Cone {
            return None;
        }
        match self.strategy {
            StrategySpec::PocketClearing { .. } | StrategySpec::Adaptive { .. } => {
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
            StrategySpec::Profile { side, .. } if side != ProfileSide::On => {
                return Some(
                    "a V-bit or chamfer mill profiles only on the line: its width changes \
                     with depth",
                );
            }
            _ => {}
        }
        if leaves {
            return Some("a V-bit or chamfer mill cuts to size: set stock to leave to zero");
        }
        None
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
