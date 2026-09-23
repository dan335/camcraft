//! Editing a job file's tools in place, keeping everything else as it was written.
//!
//! Job files are written by people and carry their reasoning in comments. The
//! example job explains why its heights are symbolic. Rewriting the whole file
//! from the parsed job would throw all of that away, so edits here go through
//! `toml_edit` and touch only what changed:
//!
//! - a changed field keeps its place, its comment and its neighbours;
//! - a removed field or tool goes, and nothing else does;
//! - a new tool is written after the last existing one;
//! - a renamed tool takes its operations' `tool = "..."` references with it.
//!
//! Each edit is `text -> text`, so it can be checked line by line, and nothing is
//! written unless the result still parses as a job.

use std::collections::BTreeMap;
use std::path::Path;

use camcraft_core::job::{Job, ToolSpec};
use toml_edit::{DocumentMut, Item, Table};

/// An edit that could not be made.
#[derive(Debug, thiserror::Error)]
pub enum JobFileError {
    /// The file is not valid TOML.
    #[error("the job file is not valid TOML: {0}")]
    Parse(#[from] toml_edit::TomlError),
    /// The edited file would not be a job.
    #[error("the edited file would not be a valid job: {0}")]
    NotAJob(#[from] toml::de::Error),
    /// A tool could not be written as TOML.
    #[error("cannot write tool '{name}': {why}")]
    Write {
        /// Which tool.
        name: String,
        /// Why.
        why: String,
    },
    /// No tool of that name.
    #[error("the job has no tool '{0}'")]
    NoSuchTool(String),
    /// A tool of that name already exists.
    #[error("the job already has a tool '{0}'")]
    NameTaken(String),
    /// Removing it would leave operations pointing at nothing.
    #[error("tool '{name}' is used by operation '{op}'")]
    InUse {
        /// Which tool.
        name: String,
        /// An operation that uses it.
        op: String,
    },
    /// The file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// Which file.
        path: String,
        /// Why.
        source: std::io::Error,
    },
}

/// The file's `[tools]` table, made if the file has none. A `tools` that is
/// something other than a table is a malformed job, and is refused rather than
/// overwritten.
fn tools_table(doc: &mut DocumentMut) -> Result<&mut Table, JobFileError> {
    doc.entry("tools")
        .or_insert_with(|| {
            let mut t = Table::new();
            // `[tools]` itself need not appear; only its `[tools.<name>]` children do.
            t.set_implicit(true);
            Item::Table(t)
        })
        .as_table_mut()
        .ok_or_else(|| JobFileError::Write {
            name: "tools".into(),
            why: "`tools` in this file is not a table".into(),
        })
}

/// A tool as a TOML table, the way the file format writes it: one block, with
/// anything nested (the holder, cutting data) inline on its own line.
///
/// Left as sub-tables they would become `[tools.<name>.holder]` headers placed
/// anywhere in the file, which is valid and unreadable.
fn tool_table(name: &str, tool: &ToolSpec) -> Result<Table, JobFileError> {
    let text = toml::to_string(tool).map_err(|e| JobFileError::Write {
        name: name.to_string(),
        why: e.to_string(),
    })?;
    let doc: DocumentMut = text.parse()?;
    // Rebuilt with fresh keys: a key that was a table header carries header
    // formatting, and would print as `holder= {` once inline.
    let mut table = Table::new();
    for (k, item) in doc.as_table().iter() {
        let mut item = item.clone();
        inline(&mut item);
        table.insert(k, item);
    }
    Ok(table)
}

/// Turn a sub-table or array of tables into its inline form.
fn inline(item: &mut Item) {
    let taken = std::mem::replace(item, Item::None);
    *item = match taken {
        Item::Table(t) => Item::Value(toml_edit::Value::InlineTable(t.into_inline_table())),
        Item::ArrayOfTables(a) => Item::Value(toml_edit::Value::Array(a.into_array())),
        other => other,
    };
}

/// Operations that use tool `name`.
fn users(doc: &DocumentMut, name: &str) -> Vec<String> {
    let Some(ops) = doc.get("operation").and_then(Item::as_array_of_tables) else {
        return Vec::new();
    };
    ops.iter()
        .filter(|op| op.get("tool").and_then(Item::as_str) == Some(name))
        .map(|op| {
            op.get("name")
                .and_then(Item::as_str)
                .unwrap_or("(unnamed)")
                .to_string()
        })
        .collect()
}

/// Write `tool` as `name`: change what differs in an existing tool, or add a new one
/// after the last.
pub fn upsert_tool(text: &str, name: &str, tool: &ToolSpec) -> Result<String, JobFileError> {
    let mut doc: DocumentMut = text.parse()?;
    let fresh = tool_table(name, tool)?;
    let tools = tools_table(&mut doc)?;

    match tools.get_mut(name).and_then(Item::as_table_mut) {
        Some(existing) => {
            // Fields the tool no longer has go.
            let gone: Vec<String> = existing
                .iter()
                .map(|(k, _)| k.to_string())
                .filter(|k| !fresh.contains_key(k))
                .collect();
            for k in gone {
                existing.remove(&k);
            }
            // Fields that changed keep their line: the key, its comment and its
            // spacing stay; only the value is new.
            for (k, item) in fresh.iter() {
                let Some(new) = item.as_value() else {
                    existing.insert(k, item.clone());
                    continue;
                };
                match existing.get_mut(k).and_then(Item::as_value_mut) {
                    Some(old) if old.to_string().trim() == new.to_string().trim() => {}
                    Some(old) => {
                        let decor = old.decor().clone();
                        *old = new.clone();
                        *old.decor_mut() = decor;
                    }
                    None => {
                        existing.insert(k, item.clone());
                    }
                }
            }
        }
        None => {
            // After the last tool, not at the end of the file.
            let after = tools
                .iter()
                .filter_map(|(_, item)| item.as_table().and_then(Table::position))
                .max();
            let mut table = fresh;
            if let Some(p) = after {
                table.set_position(Some(p));
            }
            tools.insert(name, Item::Table(table));
        }
    }
    finish(doc)
}

/// Remove tool `name`, which no operation may still use.
pub fn remove_tool(text: &str, name: &str) -> Result<String, JobFileError> {
    let mut doc: DocumentMut = text.parse()?;
    if let Some(op) = users(&doc, name).into_iter().next() {
        return Err(JobFileError::InUse {
            name: name.to_string(),
            op,
        });
    }
    if tools_table(&mut doc)?.remove(name).is_none() {
        return Err(JobFileError::NoSuchTool(name.to_string()));
    }
    finish(doc)
}

/// Rename tool `from` to `to`, and every operation's reference to it.
pub fn rename_tool(text: &str, from: &str, to: &str) -> Result<String, JobFileError> {
    let mut doc: DocumentMut = text.parse()?;
    let tools = tools_table(&mut doc)?;
    if tools.contains_key(to) {
        return Err(JobFileError::NameTaken(to.to_string()));
    }
    let item = tools
        .remove(from)
        .ok_or_else(|| JobFileError::NoSuchTool(from.to_string()))?;
    tools.insert(to, item);
    if let Some(ops) = doc
        .get_mut("operation")
        .and_then(Item::as_array_of_tables_mut)
    {
        for op in ops.iter_mut() {
            if let Some(value) = op.get_mut("tool").and_then(Item::as_value_mut)
                && value.as_str() == Some(from)
            {
                let decor = value.decor().clone();
                *value = to.into();
                *value.decor_mut() = decor;
            }
        }
    }
    finish(doc)
}

/// Bring the file's tools to `edited`, from `original`, applying `renames` first.
///
/// Renames are `(from, to)`. Tools in `original` that are not in `edited` (after
/// renaming) are removed; tools that are new or different are written.
pub fn apply_tools(
    text: &str,
    original: &BTreeMap<String, ToolSpec>,
    edited: &BTreeMap<String, ToolSpec>,
    renames: &[(String, String)],
) -> Result<String, JobFileError> {
    let mut text = text.to_string();
    let mut before = original.clone();
    for (from, to) in renames {
        text = rename_tool(&text, from, to)?;
        if let Some(t) = before.remove(from) {
            before.insert(to.clone(), t);
        }
    }
    for name in before.keys() {
        if !edited.contains_key(name) {
            text = remove_tool(&text, name)?;
        }
    }
    for (name, tool) in edited {
        if before.get(name) != Some(tool) {
            text = upsert_tool(&text, name, tool)?;
        }
    }
    Ok(text)
}

/// The edited text, provided it is still a job.
fn finish(doc: DocumentMut) -> Result<String, JobFileError> {
    let text = doc.to_string();
    toml::from_str::<Job>(&text)?;
    Ok(text)
}

/// Apply [`apply_tools`] to the file at `path`, writing only if the result is still a
/// job, and atomically, so an interrupted save cannot leave half a file.
pub fn save_tools(
    path: &Path,
    original: &BTreeMap<String, ToolSpec>,
    edited: &BTreeMap<String, ToolSpec>,
    renames: &[(String, String)],
) -> Result<(), JobFileError> {
    let io = |source| JobFileError::Io {
        path: path.display().to_string(),
        source,
    };
    let text = std::fs::read_to_string(path).map_err(io)?;
    let text = apply_tools(&text, original, edited, renames)?;
    write_atomically(path, &text).map_err(io)
}

/// Write `text` beside `path` and move it into place.
pub(crate) fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".camcraft-saving");
    let temp = std::path::PathBuf::from(temp);
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use camcraft_core::job::ToolKind;

    const EXAMPLE: &str = include_str!("../../../examples/rail-jig.toml");

    fn tools(text: &str) -> BTreeMap<String, ToolSpec> {
        toml::from_str::<Job>(text).unwrap().tools
    }

    /// Lines that differ between two texts, as (before, after) pairs of line sets.
    fn diff(a: &str, b: &str) -> (Vec<String>, Vec<String>) {
        let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
        let gone = la
            .iter()
            .filter(|l| !lb.contains(l))
            .map(|l| l.to_string())
            .collect();
        let new = lb
            .iter()
            .filter(|l| !la.contains(l))
            .map(|l| l.to_string())
            .collect();
        (gone, new)
    }

    /// A new tool adds its own lines and nothing else: every comment and every other
    /// line of the hand-written file survives.
    #[test]
    fn adding_a_tool_changes_only_that_tool() {
        let tool = ToolSpec {
            kind: ToolKind::Ball,
            diameter: 3.0,
            number: 3,
            comment: "3mm ball".into(),
            flutes: Some(2),
            ..ToolSpec::default()
        };
        let out = upsert_tool(EXAMPLE, "detail-3-ball", &tool).unwrap();
        let (gone, new) = diff(EXAMPLE, &out);
        assert!(gone.is_empty(), "lines lost: {gone:?}");
        assert!(new.iter().any(|l| l == "[tools.detail-3-ball]"), "{new:?}");
        assert!(
            new.iter().all(|l| !l.starts_with('#')),
            "no comments invented"
        );
        let got = tools(&out);
        assert_eq!(got["detail-3-ball"], tool);
        assert_eq!(got.len(), tools(EXAMPLE).len() + 1);
        // After the last tool, before the operations.
        let (t, o) = (
            out.find("[tools.detail-3-ball]").unwrap(),
            out.find("[[operation]]").unwrap(),
        );
        assert!(t < o, "the new tool landed after the operations:\n{out}");
    }

    /// A tool with a holder is still one block: the holder is inline on its own
    /// line, not a `[tools.<name>.holder]` header somewhere else in the file.
    #[test]
    fn a_tool_with_a_holder_stays_one_block() {
        let tool = ToolSpec {
            number: 3,
            holder: Some(camcraft_core::job::Holder {
                name: "ER20".into(),
                segments: vec![camcraft_core::job::HolderSegment {
                    diameter: 32.0,
                    length: 20.0,
                }],
            }),
            cutting: Some(camcraft_core::job::CuttingData {
                feed: Some(2000.0),
                ..Default::default()
            }),
            ..ToolSpec::default()
        };
        let out = upsert_tool(EXAMPLE, "held", &tool).unwrap();
        let (_, new) = diff(EXAMPLE, &out);
        let headers: Vec<&String> = new.iter().filter(|l| l.starts_with('[')).collect();
        assert_eq!(
            headers,
            ["[tools.held]"],
            "one header for one tool: {new:?}"
        );
        // Every new line is inside the tool's own block, and the block alone is the
        // whole tool.
        let start = out.find("[tools.held]").unwrap();
        let block: Vec<&str> = out[start..].lines().take_while(|l| !l.is_empty()).collect();
        for line in new.iter().filter(|l| !l.is_empty()) {
            assert!(
                block.contains(&line.as_str()),
                "{line:?} is outside the block:\n{out}"
            );
        }
        let alone: toml::Table = toml::from_str(&block.join("\n")).unwrap();
        let held: ToolSpec = alone["tools"]["held"].clone().try_into().unwrap();
        assert_eq!(held, tool);
        assert!(out.contains("holder = {"), "{out}");
    }

    /// Changing one field changes one line.
    #[test]
    fn changing_a_field_changes_only_its_line() {
        let mut tool = tools(EXAMPLE)["finish-6-ball"].clone();
        tool.rpm = 20000;
        let out = upsert_tool(EXAMPLE, "finish-6-ball", &tool).unwrap();
        let (gone, new) = diff(EXAMPLE, &out);
        assert_eq!(gone, ["rpm = 18000"]);
        assert_eq!(new, ["rpm = 20000"]);
    }

    /// Adding optional fields to an existing tool keeps the tool's existing lines.
    #[test]
    fn describing_a_tool_further_keeps_what_was_there() {
        let mut tool = tools(EXAMPLE)["rough-6-flat"].clone();
        tool.flutes = Some(2);
        tool.stickout = Some(22.0);
        let out = upsert_tool(EXAMPLE, "rough-6-flat", &tool).unwrap();
        let (gone, new) = diff(EXAMPLE, &out);
        assert!(gone.is_empty(), "{gone:?}");
        assert_eq!(new.len(), 2, "{new:?}");
        assert_eq!(tools(&out)["rough-6-flat"], tool);
    }

    /// Renaming moves the tool and both operations that referred to it, nothing else.
    #[test]
    fn renaming_carries_the_operations_with_it() {
        let out = rename_tool(EXAMPLE, "rough-6-flat", "rough-flat").unwrap();
        let job: Job = toml::from_str(&out).unwrap();
        assert!(job.tools.contains_key("rough-flat") && !job.tools.contains_key("rough-6-flat"));
        assert_eq!(job.operations[0].tool, "rough-flat");
        assert_eq!(job.operations[1].tool, "finish-6-ball");
        job.validate().unwrap();
        let (gone, new) = diff(EXAMPLE, &out);
        assert_eq!(gone.len(), 2, "the header and one reference: {gone:?}");
        assert_eq!(new.len(), 2, "{new:?}");
        assert!(
            EXAMPLE
                .lines()
                .filter(|l| l.starts_with('#'))
                .all(|c| out.contains(c))
        );
    }

    #[test]
    fn a_tool_in_use_cannot_be_removed() {
        assert!(matches!(
            remove_tool(EXAMPLE, "rough-6-flat"),
            Err(JobFileError::InUse { .. })
        ));
        let with_spare = upsert_tool(EXAMPLE, "spare", &ToolSpec::default()).unwrap();
        let out = remove_tool(&with_spare, "spare").unwrap();
        assert_eq!(tools(&out), tools(EXAMPLE));
    }

    #[test]
    fn a_rename_onto_an_existing_name_is_refused() {
        assert!(matches!(
            rename_tool(EXAMPLE, "rough-6-flat", "finish-6-ball"),
            Err(JobFileError::NameTaken(_))
        ));
    }

    /// The whole save: a rename, a change and an addition at once.
    #[test]
    fn applying_edits_reaches_exactly_the_edited_tools() {
        let original = tools(EXAMPLE);
        let mut edited = original.clone();
        let mut rough = edited.remove("rough-6-flat").unwrap();
        rough.rpm = 15000;
        edited.insert("rough".into(), rough);
        edited.insert(
            "detail".into(),
            ToolSpec {
                number: 3,
                ..ToolSpec::default()
            },
        );
        let renames = [("rough-6-flat".to_string(), "rough".to_string())];
        let out = apply_tools(EXAMPLE, &original, &edited, &renames).unwrap();
        assert_eq!(tools(&out), edited);
        toml::from_str::<Job>(&out).unwrap().validate().unwrap();
    }

    #[test]
    fn saving_writes_the_file_atomically() {
        let dir = std::env::temp_dir().join(format!("camcraft-job-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("job.toml");
        std::fs::write(&path, EXAMPLE).unwrap();
        let original = tools(EXAMPLE);
        let mut edited = original.clone();
        edited.get_mut("finish-6-ball").unwrap().rpm = 17000;
        save_tools(&path, &original, &edited, &[]).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(tools(&written), edited);
        assert!(
            !dir.join("job.toml.camcraft-saving").exists(),
            "no temporary left behind"
        );
    }
}
