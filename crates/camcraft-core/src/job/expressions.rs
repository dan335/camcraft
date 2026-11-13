//! Settings worked out from expressions, as Fusion's parameter expressions
//! are: an operation's `expressions` name a setting by its place in the job
//! file ([`super::settings`]) and give the expression it is worked out from
//! ([`crate::job::expr`]), reading the tool, the job's tolerance and the
//! operation's own settings by Fusion's names ([`Operation::expression_value`]).
//! The setting itself still holds the number, as a job file without
//! expressions has it, so everything that reads a job reads it as before.

use toml::{Table, Value as Toml};

use super::settings::{Number, at, number_kind, put};
use super::{Job, Operation, ToolKind, ToolSpec};
use crate::job::expr::{self, Value};

/// An expression that could not be worked out, or not put where it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionFault {
    /// Which setup, by name.
    pub setup: String,
    /// Which of its operations, from zero.
    pub operation: usize,
    /// The setting the expression is for, by its place (`passes.stepover`).
    pub setting: String,
    /// Why.
    pub message: String,
}

/// How many times an operation's expressions are worked out in turn, at
/// most, so one that reads a setting another sets reads it set: as deep as
/// anyone chains them, and never forever when two read each other.
const ROUNDS: usize = 8;

/// Every name an expression may read, and what it is, as the panels list them.
pub const NAMES: [(&str, &str); 21] = [
    ("tool_diameter", "the tool's diameter"),
    ("tool_cornerRadius", "the tool's corner radius"),
    (
        "tool_fluteLength",
        "the tool's flute length, two diameters if it gives none",
    ),
    ("tool_shoulderLength", "the tool's shoulder length"),
    ("tool_shaftDiameter", "the tool's shank diameter"),
    ("tool_overallLength", "the tool's overall length"),
    (
        "tool_taperAngle",
        "a cone's taper from its axis: half its included angle",
    ),
    ("tool_numberOfFlutes", "how many flutes the tool has"),
    ("tool_threadPitch", "a thread mill's pitch"),
    (
        "tool_threadProfileAngle",
        "a thread mill's tooth's profile angle, 60 if it gives none",
    ),
    (
        "tool_type",
        "'flat end mill', 'ball end mill', 'bull nose end mill', 'chamfer mill', 'drill' or \
         'thread mill'",
    ),
    ("tool_spindleSpeed", "the spindle speed, in rev/min"),
    ("tool_feedCutting", "the cutting feed"),
    ("tool_feedPlunge", "the plunge feed"),
    ("tolerance", "the job's tolerance"),
    ("stepover", "the stepover"),
    ("optimalLoad", "adaptive's optimal load, its stepover"),
    ("maximumStepdown", "the stepdown"),
    ("stockToLeave", "the stock left on walls"),
    ("axialStockToLeave", "the stock left on floors"),
    ("safeDistance", "the safe distance"),
];

impl Operation {
    /// What `name` reads in one of this operation's expressions, with `tool`
    /// its tool and `tolerance` the job's: Fusion's names ([`NAMES`]). `None`
    /// for a name with no such value, as a tool's overall length it does not
    /// give.
    pub fn expression_value(
        &self,
        name: &str,
        tool: Option<&ToolSpec>,
        tolerance: f64,
    ) -> Option<Value> {
        let n = Value::Number;
        Some(match name {
            "tolerance" => n(tolerance),
            "stepover" | "optimalLoad" => n(self.passes.stepover),
            "maximumStepdown" => n(self.passes.stepdown?),
            "stockToLeave" => n(self.passes.stock_to_leave.radial),
            "axialStockToLeave" => n(self.passes.stock_to_leave.axial),
            "safeDistance" => n(self.linking.safe_distance),
            "tool_feedCutting" => n(self.passes.feed),
            "tool_feedPlunge" => n(self.linking.plunge_feed),
            "tool_spindleSpeed" => n(f64::from(self.passes.rpm.or(tool.map(|t| t.rpm))?)),
            _ => {
                let t = tool?;
                match name {
                    "tool_diameter" => n(t.diameter),
                    "tool_cornerRadius" => n(t.corner_radius()),
                    "tool_fluteLength" => n(t.flute_length()),
                    "tool_shoulderLength" => n(t.shoulder_length?),
                    "tool_shaftDiameter" => n(t.shank_diameter.unwrap_or(t.diameter)),
                    "tool_overallLength" => n(t.overall_length?),
                    // Fusion measures a taper from the axis.
                    "tool_taperAngle" => n(t.angle? / 2.0),
                    "tool_numberOfFlutes" => n(f64::from(t.flutes?)),
                    // A thread mill's alone, as Fusion's are.
                    "tool_threadPitch" if t.kind == ToolKind::ThreadMill => n(t.pitch?),
                    "tool_threadProfileAngle" if t.kind == ToolKind::ThreadMill => {
                        n(t.thread_profile())
                    }
                    "tool_type" => Value::Text(t.kind.fusion_type().into()),
                    _ => return None,
                }
            }
        })
    }

    /// Work out each of its expressions, with `tool` its tool and `tolerance`
    /// the job's, and put what each comes to in its setting: a whole number
    /// where the setting is one. In turns until nothing changes, so one that
    /// reads a setting another sets (a ramp feed the plunge feed) reads it
    /// set. Those that cannot be leave their settings as they were, and say
    /// why.
    pub fn work_out_expressions(&mut self, tool: Option<&ToolSpec>, tolerance: f64) -> Worked {
        let mut worked = Worked::default();
        for _ in 0..ROUNDS {
            let turn = self.work_out_once(tool, tolerance);
            worked.faults = turn.faults;
            worked.changed |= turn.changed;
            if !turn.changed {
                break;
            }
        }
        worked
    }

    /// One turn of [`Operation::work_out_expressions`]: each expression
    /// reading the settings as they were before any was worked out.
    fn work_out_once(&mut self, tool: Option<&ToolSpec>, tolerance: f64) -> Worked {
        let mut worked = Worked::default();
        // Without a tool, what they read of one is not there yet: the settings
        // keep their numbers until one is chosen.
        if self.expressions.is_empty() || tool.is_none() {
            return worked;
        }
        let Ok(mut table) = Table::try_from(&*self) else {
            return worked;
        };
        let faults = &mut worked.faults;
        let mut wrote = false;
        let read = |name: &str| self.expression_value(name, tool, tolerance);
        for (setting, text) in &self.expressions {
            let value = match expr::evaluate(text, &read) {
                // Clear of what arithmetic leaves past the ninth place, as
                // 6.35 * 0.4 does: kept as it is written.
                Ok(v) => (v * 1e9).round() / 1e9,
                Err(e) => {
                    faults.push((setting.clone(), e.to_string()));
                    continue;
                }
            };
            let put_as = match number_kind::<Self>(&table, setting, self.strategy.name()) {
                Some(Number::Real) => Toml::Float(value),
                // A count or a speed, to the nearest.
                #[allow(clippy::cast_possible_truncation)]
                Some(Number::Whole) if value >= 0.0 => Toml::Integer(value.round() as i64),
                Some(Number::Whole) => {
                    faults.push((setting.clone(), "it comes to less than zero".into()));
                    continue;
                }
                None => {
                    faults.push((setting.clone(), "there is no number setting there".into()));
                    continue;
                }
            };
            if at(&table, setting) != Some(&put_as) {
                put(&mut table, setting, Some(put_as));
                wrote = true;
            }
        }
        if wrote {
            match Toml::Table(table).try_into::<Self>() {
                Ok(new) => {
                    worked.changed = new != *self;
                    *self = new;
                }
                Err(e) => worked.faults.push((
                    String::new(),
                    format!("what they come to does not fit: {e}"),
                )),
            }
        }
        worked
    }
}

/// What working out an operation's expressions did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Worked {
    /// Whether any setting changed.
    pub changed: bool,
    /// Those that could not be worked out, by setting, with why.
    pub faults: Vec<(String, String)>,
}

impl Job {
    /// Work out every operation's expressions ([`Operation::work_out_expressions`])
    /// into their settings: each reads only its own operation, its tool and
    /// the tolerance. Returns those that could not be.
    pub fn work_out_expressions(&mut self) -> Vec<ExpressionFault> {
        let tolerance = self.tolerance;
        let mut faults = Vec::new();
        for setup in &mut self.setups {
            for (i, op) in setup.operations.iter_mut().enumerate() {
                let tool = op.tool.as_ref().and_then(|t| self.tools.get(t));
                for (setting, message) in op.work_out_expressions(tool, tolerance).faults {
                    faults.push(ExpressionFault {
                        setup: setup.name.clone(),
                        operation: i,
                        setting,
                        message,
                    });
                }
            }
        }
        faults
    }

    /// Whether any operation has an expression: when there is nothing to
    /// work out.
    pub fn has_expressions(&self) -> bool {
        self.setups
            .iter()
            .flat_map(|s| &s.operations)
            .any(|o| !o.expressions.is_empty())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::job::{Setup, StrategySpec};

    fn tool() -> ToolSpec {
        toml::from_str(
            "kind = \"flat\"\ndiameter = 6.0\nnumber = 1\nrpm = 18000\nflute_length = 20.0",
        )
        .unwrap()
    }

    /// An expression is worked out into its setting, reading the tool, and
    /// again when the tool changes; one that cannot be leaves the setting as
    /// it was and says why.
    #[test]
    fn a_setting_follows_its_expression() {
        let mut job = Job::default();
        job.tools.insert("t".into(), tool());
        let mut op = Operation::new("rough", StrategySpec::named("adaptive").unwrap());
        op.tool = Some("t".into());
        let mut say = |setting: &str, text: &str| {
            op.expressions.insert(setting.into(), text.into());
        };
        say("passes.stepover", "tool_diameter * 0.4");
        say(
            "passes.stepdown",
            "Math.min(tool_fluteLength * 0.75; tool_diameter * 2.5)",
        );
        // Reads the stepover once it is set: worked out in a later turn.
        say("linking.safe_distance", "stepover * 2");
        job.setups.push(Setup {
            operations: vec![op],
            ..Setup::new("top")
        });

        assert!(job.work_out_expressions().is_empty());
        let op = &job.setups[0].operations[0];
        assert!((op.passes.stepover - 2.4).abs() < 1e-12);
        assert_eq!(op.passes.stepdown, Some(15.0));
        assert!((op.linking.safe_distance - 4.8).abs() < 1e-12);

        job.tools.get_mut("t").unwrap().diameter = 10.0;
        assert!(job.work_out_expressions().is_empty());
        let op = &job.setups[0].operations[0];
        assert!((op.passes.stepover - 4.0).abs() < 1e-12);
        assert_eq!(op.passes.stepdown, Some(15.0), "the flutes now the less");

        let op = &mut job.setups[0].operations[0];
        op.expressions
            .insert("passes.stepover".into(), "tool_diameter * nonsense".into());
        op.expressions.insert("name".into(), "1".into());
        let faults = job.work_out_expressions();
        assert_eq!(faults.len(), 2, "{faults:?}");
        assert!(
            faults
                .iter()
                .any(|f| f.setting == "passes.stepover" && f.message.contains("nonsense"))
        );
        assert!((job.setups[0].operations[0].passes.stepover - 4.0).abs() < 1e-12);
    }
}
