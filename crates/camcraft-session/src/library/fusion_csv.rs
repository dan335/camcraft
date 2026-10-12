//! Tools from a Fusion tool library exported as CSV, which is how makers such as
//! Bits & Bits publish theirs.
//!
//! Fusion writes one row per preset, tab-separated with every field quoted, under
//! a header whose cells read `"Label (key)"`: `"Diameter (tool_diameter)"`. Columns
//! are found by that key, never by position, so a file from another Fusion version
//! with columns added or moved still reads. The header ends with a
//! `CSV_TOOLS_VERSION_1` cell the rows have no value for.
//!
//! Rows describing the same tool become one tool with a preset for each, named as
//! Fusion named it. Lengths and feeds are converted to millimetres. Tool types this
//! build cannot cut with are skipped, and each skip says why.

use std::collections::HashMap;

use camcraft_core::job::{
    Coolant, Preset, SpindleDirection, ToolKind, ToolMaterial, ToolSpec, free_preset_name,
};

use super::{Folder, MM_PER_INCH, free_name};

/// A file that is not a Fusion tool library.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// Not readable as CSV.
    #[error("not a CSV file: {0}")]
    Csv(#[from] csv::Error),
    /// No header row.
    #[error("the file is empty")]
    Empty,
    /// A column every tool needs is not there.
    #[error("no '{0}' column: not a Fusion tool library export")]
    MissingColumn(&'static str),
}

/// What a file held: tools, each with a suggested name, and rows left out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Imported {
    /// The tools, in the file's order, with a name to give each.
    pub tools: Vec<(String, ToolSpec)>,
    /// Why each row left out was left out.
    pub skipped: Vec<String>,
}

/// Read a Fusion tool library CSV.
pub fn import(text: &str) -> Result<Imported, ImportError> {
    let first = text.lines().next().ok_or(ImportError::Empty)?;
    let delimiter = if first.contains('\t') { b'\t' } else { b',' };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_reader(text.as_bytes());
    let columns: HashMap<String, usize> = reader
        .headers()?
        .iter()
        .enumerate()
        .map(|(i, cell)| (key_of(cell), i))
        .collect();
    for needed in ["tool_type", "tool_diameter"] {
        if !columns.contains_key(needed) {
            return Err(ImportError::MissingColumn(needed));
        }
    }

    let mut out = Imported::default();
    // Which tool each (description, product, number, diameter) became.
    let mut seen: HashMap<(String, String, String, String), usize> = HashMap::new();
    for (i, record) in reader.records().enumerate() {
        let record = record?;
        let row = Row {
            record: &record,
            columns: &columns,
        };
        let line = i + 2;
        let tool = match row.tool() {
            Ok(tool) => tool,
            Err(why) => {
                out.skipped.push(format!("row {line}: {why}"));
                continue;
            }
        };
        let identity = (
            row.text("tool_description").unwrap_or_default(),
            row.text("tool_productId").unwrap_or_default(),
            row.text("tool_number").unwrap_or_default(),
            row.text("tool_diameter").unwrap_or_default(),
        );
        let preset = row.preset(out.tools.len());
        match seen.get(&identity) {
            Some(&at) => {
                if let Some((_, existing)) = out.tools.get_mut(at) {
                    let name = free_preset_name(&existing.presets, &preset.name);
                    existing.presets.push(Preset { name, ..preset });
                }
            }
            None => {
                let mut tool = tool;
                tool.presets.push(preset);
                seen.insert(identity, out.tools.len());
                let name = name_for(&tool);
                out.tools.push((name, tool));
            }
        }
    }
    // A tool the rest of the app would refuse is not handed to it.
    out.tools.retain(|(name, tool)| match tool.check().first() {
        Some(e) => {
            out.skipped.push(format!("'{name}': {e}"));
            false
        }
        None => true,
    });
    Ok(out)
}

/// What bringing tools into a folder did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Merged {
    /// Tools added, by the name each got.
    pub added: Vec<String>,
    /// Tools already in the folder that got presets, and how many.
    pub extended: Vec<(String, usize)>,
}

/// Put `tools` into `folder`. A tool the folder already has, the same product at
/// the same diameter, gets the new presets beside its own rather than a twin; any
/// other gets a free name.
pub fn merge(folder: &mut Folder, tools: Vec<(String, ToolSpec)>) -> Merged {
    let mut out = Merged::default();
    for (name, tool) in tools {
        let twin = folder.tools.iter_mut().find(|(_, t)| {
            t.product.is_some()
                && t.product == tool.product
                && t.kind == tool.kind
                && (t.diameter - tool.diameter).abs() < 1e-6
        });
        match twin {
            Some((existing, t)) => {
                let mut count = 0;
                for p in tool.presets {
                    if t.presets.iter().any(|q| q.name == p.name && q == &p) {
                        continue;
                    }
                    let name = free_preset_name(&t.presets, &p.name);
                    t.presets.push(Preset { name, ..p });
                    count += 1;
                }
                out.extended.push((existing.clone(), count));
            }
            None => {
                let name = free_name(&name, &folder.tools);
                folder.tools.insert(name.clone(), tool);
                out.added.push(name);
            }
        }
    }
    out
}

/// The key in a header cell: `tool_diameter` in `"Diameter (tool_diameter)"`, or
/// the whole cell when it has none.
fn key_of(cell: &str) -> String {
    let cell = cell.trim();
    match (cell.rfind('('), cell.ends_with(')')) {
        (Some(open), true) => cell
            .get(open + 1..cell.len() - 1)
            .unwrap_or(cell)
            .trim()
            .to_string(),
        _ => cell.to_string(),
    }
}

/// A name for an imported tool: its product code, else its description made
/// fit for a name.
fn name_for(tool: &ToolSpec) -> String {
    let base = tool.product.clone().unwrap_or_else(|| tool.comment.clone());
    let name: String = base
        .chars()
        .map(|c| if c == '/' { '-' } else { c })
        .collect::<String>()
        .trim()
        .to_string();
    if name.is_empty() { "tool".into() } else { name }
}

/// One row, read by column key.
struct Row<'a> {
    record: &'a csv::StringRecord,
    columns: &'a HashMap<String, usize>,
}

impl Row<'_> {
    /// The cell under `key`, trimmed; nothing when blank or absent.
    fn text(&self, key: &str) -> Option<String> {
        let cell = self.record.get(*self.columns.get(key)?)?.trim();
        (!cell.is_empty()).then(|| cell.to_string())
    }

    fn number(&self, key: &str) -> Option<f64> {
        self.text(key)?.parse().ok().filter(|v: &f64| v.is_finite())
    }

    fn flag(&self, key: &str) -> bool {
        self.text(key)
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    }

    /// Millimetres per unit the row is written in.
    fn scale(&self) -> f64 {
        match self.text("tool_unit").as_deref() {
            Some(u) if u.to_ascii_lowercase().starts_with("inch") => MM_PER_INCH,
            _ => 1.0,
        }
    }

    /// A length or feed, in millimetres; nothing unless it is greater than zero.
    fn scaled(&self, key: &str) -> Option<f64> {
        self.number(key)
            .filter(|v| *v > 0.0)
            .map(|v| v * self.scale())
    }

    fn whole(&self, key: &str) -> Option<u32> {
        let v = self.number(key)?.round();
        // Rounded, and in range, first.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (v >= 1.0 && v <= f64::from(u32::MAX)).then_some(v as u32)
    }

    /// The tool, without presets.
    fn tool(&self) -> Result<ToolSpec, String> {
        let kind_text = self.text("tool_type").unwrap_or_default();
        let kind = match kind_text.to_ascii_lowercase().as_str() {
            "flat end mill" => ToolKind::Flat,
            "ball end mill" => ToolKind::Ball,
            "bull nose end mill" => ToolKind::Bullnose,
            "chamfer mill" | "spot drill" => ToolKind::Cone,
            "drill" => ToolKind::Drill,
            "" => return Err("no tool type".into()),
            other => return Err(format!("'{other}' tools are not supported yet")),
        };
        let diameter = self.scaled("tool_diameter").ok_or("no diameter")?;
        let rpm = self.whole("tool_spindleSpeed").unwrap_or(18000);
        Ok(ToolSpec {
            kind,
            diameter,
            corner: match kind {
                ToolKind::Bullnose => self.scaled("tool_cornerRadius").unwrap_or(0.0),
                _ => 0.0,
            },
            // A chamfer mill's taper is measured from the axis; a spot drill's tip
            // angle is the whole point.
            angle: match kind_text.to_ascii_lowercase().as_str() {
                "chamfer mill" => self.number("tool_taperAngle").map(|a| 2.0 * a),
                "spot drill" | "drill" => self.number("tool_tipAngle"),
                _ => None,
            },
            tip_diameter: match kind {
                ToolKind::Cone => self.scaled("tool_tipDiameter").unwrap_or(0.0),
                _ => 0.0,
            },
            number: self.whole("tool_number").unwrap_or(1),
            rpm,
            comment: self.text("tool_description").unwrap_or_default(),
            flutes: self.whole("tool_numberOfFlutes"),
            flute_length: self.scaled("tool_fluteLength"),
            shoulder_length: self.scaled("tool_shoulderLength"),
            shank_diameter: self.scaled("tool_shaftDiameter"),
            overall_length: self.scaled("tool_overallLength"),
            stickout: self.scaled("tool_stickout"),
            material: match self.text("tool_material").map(|m| m.to_ascii_lowercase()) {
                Some(m) if m.contains("carbide") => Some(ToolMaterial::Carbide),
                Some(m) if m.contains("hss") || m.contains("high speed") => Some(ToolMaterial::Hss),
                _ => None,
            },
            vendor: self.text("tool_vendor"),
            product: self.text("tool_productId"),
            holder: None,
            spindle: if self.text("tool_clockwise").is_some() && !self.flag("tool_clockwise") {
                SpindleDirection::CounterClockwise
            } else {
                SpindleDirection::Clockwise
            },
            coolant: match self.text("tool_coolant").map(|c| c.to_ascii_lowercase()) {
                Some(c) if c.contains("flood") => Coolant::Flood,
                Some(c) if c.contains("mist") => Coolant::Mist,
                _ => Coolant::Off,
            },
            presets: Vec::new(),
        })
    }

    /// The row's preset; `index` names one Fusion left unnamed.
    fn preset(&self, index: usize) -> Preset {
        Preset {
            name: self
                .text("preset_name")
                .unwrap_or_else(|| format!("Preset {}", index + 1)),
            comment: String::new(),
            rpm: self.whole("tool_spindleSpeed"),
            feed: self.scaled("tool_feedCutting"),
            plunge_feed: self.scaled("tool_feedPlunge"),
            ramp_feed: self.scaled("tool_feedRamp"),
            stepdown: self
                .flag("use_tool_stepdown")
                .then(|| self.scaled("tool_stepdown"))
                .flatten(),
            stepover: self
                .flag("use_tool_stepover")
                .then(|| self.scaled("tool_stepover"))
                .flatten(),
            stock_to_leave: None,
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests;
