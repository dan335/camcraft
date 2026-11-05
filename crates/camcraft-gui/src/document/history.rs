//! Undo and redo: what each step is called, and where it changed.
//!
//! A step keeps the whole job as it was ([decision 0019]), so nothing an
//! edit touches can be missed. What it is called and where it is are worked
//! out from the two jobs, compared as job files setting by setting, the way
//! the panels tell a changed setting ([`camcraft_core::job::settings`]).
//!
//! [decision 0019]: ../../../../docs/decisions/0019-undo.md

use std::collections::BTreeMap;

use camcraft_core::job::settings::put;
use camcraft_core::job::{Job, Operation, Setup};
use serde::Serialize;
use serde::de::DeserializeOwned;
use toml::{Table, Value};

use super::Selection;

/// A job as it was before an edit, or after one undone, and the edit.
#[derive(Debug, Clone)]
pub struct Step {
    pub job: Job,
    pub change: Change,
}

/// An edit, as undoing it names it, and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// What it did, as the menu says it: "stepover of rough".
    pub what: String,
    /// Where it is, if it is in one place.
    pub at: Option<Place>,
}

/// Where an edit is: what to select to see it, and the setting, by its place
/// in an operation (`passes.stepover`), where it was one setting of one.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub selection: Selection,
    pub setting: Option<String>,
}

/// The edit that took the job from `before` to `after`.
pub fn change(before: &Job, after: &Job) -> Change {
    let mut found: Vec<Change> = Vec::new();
    found.extend(named_list(
        "setup",
        &names(&before.setups, |s| &s.name),
        &names(&after.setups, |s| &s.name),
        |name| Some(Selection::Setup(name.to_string())),
    ));
    for (b, a) in before.setups.iter().zip(&after.setups) {
        if b.name == a.name {
            found.extend(setup_change(b, a));
        }
    }
    found.extend(named_list(
        "tool",
        &before.tools.keys().map(String::as_str).collect::<Vec<_>>(),
        &after.tools.keys().map(String::as_str).collect::<Vec<_>>(),
        |_| None,
    ));
    for (name, a) in &after.tools {
        if let Some(b) = before.tools.get(name) {
            found.extend(settings_change(b, a, &format!("tool {name}"), None, |_| {
                false
            }));
        }
    }
    found.extend(named_list(
        "model",
        &names(&before.models, |m| &m.name),
        &names(&after.models, |m| &m.name),
        |name| Some(Selection::Model(name.to_string())),
    ));
    found.extend(named_list(
        "stock",
        &names(&before.stocks, |s| &s.name),
        &names(&after.stocks, |s| &s.name),
        |name| Some(Selection::Stock(name.to_string())),
    ));
    // Any change to it, however small, is an edit.
    if before.tolerance.to_bits() != after.tolerance.to_bits() {
        found.push(Change {
            what: "tolerance".into(),
            at: Some(Place {
                selection: Selection::Job,
                setting: None,
            }),
        });
    }
    // Models and stocks edited in place: named as such, without the detail.
    for (what, b, a) in [
        (
            "models",
            leaves_of(&before.models),
            leaves_of(&after.models),
        ),
        (
            "stocks",
            leaves_of(&before.stocks),
            leaves_of(&after.stocks),
        ),
    ] {
        if found.is_empty() && b != a && !b.is_empty() && b.len() == a.len() {
            found.push(Change {
                what: format!("edit {what}"),
                at: None,
            });
        }
    }
    match found.len() {
        0 => Change {
            what: "edit".into(),
            at: None,
        },
        1 => found.swap_remove(0),
        _ => Change {
            what: "several edits".into(),
            at: None,
        },
    }
}

fn names<T>(items: &[T], name: impl Fn(&T) -> &String) -> Vec<&str> {
    items.iter().map(|i| name(i).as_str()).collect()
}

/// A list of named things added to, taken from, renamed in or reordered:
/// `kind` names them, and `place` says where one is to be seen.
fn named_list(
    kind: &str,
    before: &[&str],
    after: &[&str],
    place: impl Fn(&str) -> Option<Selection>,
) -> Option<Change> {
    let at = |name: &str| {
        place(name).map(|selection| Place {
            selection,
            setting: None,
        })
    };
    let added: Vec<&str> = after
        .iter()
        .copied()
        .filter(|n| !before.contains(n))
        .collect();
    let gone: Vec<&str> = before
        .iter()
        .copied()
        .filter(|n| !after.contains(n))
        .collect();
    match (added.as_slice(), gone.as_slice()) {
        ([], []) if before == after => None,
        ([], []) => Some(Change {
            what: format!("reorder {kind}s"),
            at: None,
        }),
        ([new], [old]) if before.len() == after.len() => Some(Change {
            what: format!("rename {kind} {old} to {new}"),
            at: at(new),
        }),
        ([new], []) => Some(Change {
            what: format!("add {kind} {new}"),
            at: at(new),
        }),
        ([], [old]) => Some(Change {
            what: format!("delete {kind} {old}"),
            at: None,
        }),
        _ => Some(Change {
            what: format!("change {kind}s"),
            at: None,
        }),
    }
}

/// What changed in setup `b`, now `a`: its operations, or its own settings.
fn setup_change(b: &Setup, a: &Setup) -> Vec<Change> {
    let setup = &a.name;
    let mut found = Vec::new();
    found.extend(named_list(
        "operation",
        &names(&b.operations, |o| &o.name),
        &names(&a.operations, |o| &o.name),
        |name| {
            let i = a.operations.iter().position(|o| o.name == name)?;
            Some(Selection::Operation(setup.clone(), i))
        },
    ));
    for (i, op) in a.operations.iter().enumerate() {
        if let Some(was) = b.operations.iter().find(|o| o.name == op.name) {
            found.extend(operation_change(was, op, setup, i));
        }
    }
    let own = |s: &Setup| Setup {
        operations: Vec::new(),
        ..s.clone()
    };
    found.extend(settings_change(
        &own(b),
        &own(a),
        &format!("setup {setup}"),
        Some(Selection::Setup(setup.clone())),
        |_| false,
    ));
    found
}

/// What changed in operation `b`, now `a`, the `i`th of `setup`. A setting
/// worked out from an expression is not what was edited: the expression, or
/// what it reads, was.
fn operation_change(b: &Operation, a: &Operation, setup: &str, i: usize) -> Option<Change> {
    let worked_out = |path: &str| a.expressions.contains_key(path);
    settings_change(
        b,
        a,
        &a.name,
        Some(Selection::Operation(setup.to_string(), i)),
        worked_out,
    )
}

/// What changed between `b` and `a`, `owner` naming them: the one setting,
/// as "stepover of rough", or "edit rough" for several. `selection` shows
/// them; `skip` leaves out settings that changed only because others did.
fn settings_change<T: Serialize + DeserializeOwned>(
    b: &T,
    a: &T,
    owner: &str,
    selection: Option<Selection>,
    skip: impl Fn(&str) -> bool,
) -> Option<Change> {
    let (before, after) = (leaves_of(b), leaves_of(a));
    // A setting written on one side only is changed unless it is that side's
    // default, written only because another beside it in its section changed.
    let changed = |path: &str| match (before.get(path), after.get(path)) {
        (Some(x), Some(y)) => x != y,
        (Some(x), None) => !is_default::<T>(a, path, x),
        (None, Some(y)) => !is_default::<T>(b, path, y),
        (None, None) => false,
    };
    let mut paths: Vec<&String> = before
        .keys()
        .chain(after.keys())
        .filter(|p| changed(p) && !skip(p))
        .collect();
    paths.sort();
    paths.dedup();
    let place = |setting: Option<String>| {
        selection
            .clone()
            .map(|selection| Place { selection, setting })
    };
    match paths.as_slice() {
        [] => None,
        [one] => {
            // An expression is named for its setting.
            let (setting, how) = match one.strip_prefix("expressions.") {
                Some(setting) => (setting, "expression for "),
                None => (one.as_str(), ""),
            };
            Some(Change {
                what: format!("{how}{} of {owner}", label(setting)),
                at: place(Some(setting.to_string())),
            })
        }
        _ => Some(Change {
            what: format!("edit {owner}"),
            at: place(None),
        }),
    }
}

/// Whether `value`, put at `path` in `other`, is a default there, written by
/// no job file: `other` with it reads back without it.
fn is_default<T: Serialize + DeserializeOwned>(other: &T, path: &str, value: &Value) -> bool {
    let Ok(mut table) = Table::try_from(other) else {
        return false;
    };
    put(&mut table, path, Some(value.clone()));
    Value::Table(table)
        .try_into::<T>()
        .is_ok_and(|back| !leaves_of(&back).contains_key(path))
}

/// A setting's place as words: its last name, or its last two where the
/// last is one part of a pair or a place in a list (`stock_to_leave.radial`,
/// `size.0`).
fn label(path: &str) -> String {
    let mut parts = path.rsplit('.');
    let last = parts.next().unwrap_or(path);
    let part_of = last.parse::<usize>().is_ok() || matches!(last, "radial" | "axial");
    let words = match parts.next().filter(|_| part_of) {
        Some(parent) => format!("{parent} {last}"),
        None => last.to_string(),
    };
    words.replace('_', " ")
}

/// Every setting of `value` as a job file has it, by its place: what is
/// compared. Keys of `expressions` are settings' places themselves, so they
/// stay whole after it (`expressions.passes.stepover`).
fn leaves_of<T: Serialize + ?Sized>(value: &T) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    match Value::try_from(value) {
        Ok(Value::Table(t)) => leaves(&t, "", &mut out),
        Ok(Value::Array(list)) => {
            for (i, v) in list.iter().enumerate() {
                leaf(&i.to_string(), v, &mut out);
            }
        }
        Ok(v) => {
            out.insert(String::new(), v);
        }
        Err(_) => {}
    }
    out
}

fn leaves(table: &Table, prefix: &str, out: &mut BTreeMap<String, Value>) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if key == "expressions"
            && let Value::Table(t) = value
        {
            for (setting, text) in t {
                out.insert(format!("{path}.{setting}"), text.clone());
            }
            continue;
        }
        leaf(&path, value, out);
    }
}

fn leaf(path: &str, value: &Value, out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Table(t) => leaves(t, path, out),
        Value::Array(list) if list.iter().all(Value::is_table) => {
            for (i, v) in list.iter().enumerate() {
                leaf(&format!("{path}.{i}"), v, out);
            }
        }
        v => {
            out.insert(path.to_string(), v.clone());
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use camcraft_core::job::{StrategySpec, ToolSpec};

    fn job() -> Job {
        let mut job = Job::default();
        let mut setup = Setup::new("top");
        let mut rough = Operation::new("rough", StrategySpec::named("adaptive").unwrap());
        rough.tool = Some("t".into());
        setup.operations.push(rough);
        job.setups.push(setup);
        job.tools.insert(
            "t".into(),
            ToolSpec {
                diameter: 6.0,
                ..ToolSpec::default()
            },
        );
        job
    }

    /// One setting is named for itself and its operation, and shown there;
    /// several are an edit of the operation.
    #[test]
    fn a_setting_is_named_and_placed() {
        let before = job();
        let mut after = before.clone();
        after.setups[0].operations[0].linking.ramp_angle = 4.0;
        let c = change(&before, &after);
        assert_eq!(c.what, "ramp angle of rough");
        assert_eq!(
            c.at,
            Some(Place {
                selection: Selection::Operation("top".into(), 0),
                setting: Some("linking.ramp_angle".into()),
            })
        );
        after.setups[0].operations[0].passes.stock_to_leave.radial = 0.2;
        assert_eq!(change(&before, &after).what, "edit rough");
        let mut leave = before.clone();
        leave.setups[0].operations[0].passes.stock_to_leave.radial = 0.2;
        assert_eq!(
            change(&before, &leave).what,
            "stock to leave radial of rough"
        );
    }

    /// A tool's edit is named for the tool, not for the settings its
    /// operations' expressions worked out from it.
    #[test]
    fn what_an_expression_worked_out_is_not_the_edit() {
        let before = job();
        let mut after = before.clone();
        after.tools.get_mut("t").unwrap().diameter = 10.0;
        after.setups[0].operations[0].passes.stepover = 4.0;
        assert_eq!(change(&before, &after).what, "diameter of tool t");
        let mut expr = before.clone();
        expr.setups[0].operations[0]
            .expressions
            .insert("passes.stepover".into(), "tool_diameter * 0.2".into());
        expr.setups[0].operations[0].passes.stepover = 1.2;
        assert_eq!(
            change(&before, &expr).what,
            "expression for stepover of rough"
        );
    }

    /// Things added, deleted, renamed and reordered are named so.
    #[test]
    fn lists_are_added_to_deleted_from_renamed_and_reordered() {
        let before = job();
        let mut added = before.clone();
        added.setups[0].operations.push(Operation::new(
            "finish",
            StrategySpec::named("scallop").unwrap(),
        ));
        let c = change(&before, &added);
        assert_eq!(c.what, "add operation finish");
        assert_eq!(
            c.at.map(|p| p.selection),
            Some(Selection::Operation("top".into(), 1))
        );
        assert_eq!(change(&added, &before).what, "delete operation finish");
        let mut swapped = added.clone();
        swapped.setups[0].operations.swap(0, 1);
        assert_eq!(change(&added, &swapped).what, "reorder operations");
        let mut renamed = before.clone();
        renamed.setups[0].name = "front".into();
        assert_eq!(change(&before, &renamed).what, "rename setup top to front");
        let mut tighter = before.clone();
        tighter.tolerance = 0.01;
        assert_eq!(change(&before, &tighter).what, "tolerance");
    }
}
