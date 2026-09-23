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

use super::{Folder, free_name};

const MM_PER_INCH: f64 = 25.4;

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
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/tools")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// Bits & Bits' 1/4" up-cut spiral: four rows, one tool, a preset for each,
    /// in millimetres.
    #[test]
    fn a_makers_library_reads_as_one_tool_with_presets() {
        let got = import(&fixture("srf4-250up.csv")).unwrap();
        assert!(got.skipped.is_empty(), "{:?}", got.skipped);
        assert_eq!(got.tools.len(), 1);
        let (name, t) = &got.tools[0];
        assert_eq!(name, "SRF4-250UP");
        assert_eq!(t.kind, ToolKind::Flat);
        assert!(close(t.diameter, 6.35));
        assert_eq!(t.flutes, Some(2));
        assert!(close(t.flute_length.unwrap(), 25.4));
        assert!(close(t.shoulder_length.unwrap(), 25.4254));
        assert!(close(t.shank_diameter.unwrap(), 6.35));
        assert!(close(t.overall_length.unwrap(), 63.5));
        assert_eq!(t.material, Some(ToolMaterial::Carbide));
        assert_eq!(t.vendor.as_deref(), Some("Bitsbits.com"));
        assert_eq!(t.product.as_deref(), Some("SRF4-250UP"));
        assert_eq!(
            t.comment,
            "SRF4-250UP *Astra Coated* 1/4\" Up-Cut Spiral Bit"
        );
        assert_eq!(t.rpm, 16000);
        assert_eq!(t.spindle, SpindleDirection::Clockwise);
        assert_eq!(t.coolant, Coolant::Off);

        let names: Vec<&str> = t.presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Wood / Plywood",
                "MDF / Laminate",
                "Aluminum",
                "Plastic / Acrylic"
            ]
        );
        let wood = &t.presets[0];
        assert_eq!(wood.rpm, Some(16000));
        assert!(close(wood.feed.unwrap(), 3810.0));
        assert!(close(wood.plunge_feed.unwrap(), 53.3333 * 25.4));
        assert!(close(wood.ramp_feed.unwrap(), 2286.0));
        // The file sets a stepdown and stepover but does not use them.
        assert_eq!(wood.stepdown, None);
        assert_eq!(wood.stepover, None);
        assert!(close(t.presets[1].feed.unwrap(), 4318.0));
        assert!(t.check().is_empty(), "{:?}", t.check());
    }

    #[test]
    fn a_one_flute_bit_reads_too() {
        let got = import(&fixture("425-up250.csv")).unwrap();
        let (name, t) = &got.tools[0];
        assert_eq!(name, "425-UP250");
        assert_eq!(t.flutes, Some(1));
        assert_eq!(t.presets.len(), 2);
        assert!(close(t.presets[0].feed.unwrap(), 2540.0));
    }

    /// Columns are found by key, so a comma-separated file with only some of them,
    /// in another order, reads.
    #[test]
    fn columns_are_found_by_key_in_any_order() {
        let text = "\"Preset Name (preset_name)\",\"Unit (tool_unit)\",\"Diameter (tool_diameter)\",\
                    \"Type (tool_type)\",\"Stepover (tool_stepover)\",\"Use Stepover (use_tool_stepover)\"\n\
                    \"Finishing\",\"millimeters\",6,\"ball end mill\",0.4,true\n";
        let got = import(text).unwrap();
        let (name, t) = &got.tools[0];
        assert_eq!(name, "tool");
        assert_eq!(t.kind, ToolKind::Ball);
        assert!(close(t.diameter, 6.0));
        assert_eq!(t.presets[0].name, "Finishing");
        assert!(close(t.presets[0].stepover.unwrap(), 0.4));
    }

    #[test]
    fn what_cannot_be_cut_with_is_skipped_and_said() {
        let text = "\"Type (tool_type)\"\t\"Diameter (tool_diameter)\"\n\
                    \"drill\"\t3\n\"flat end mill\"\t0\n\"flat end mill\"\t3\n";
        let got = import(text).unwrap();
        assert_eq!(got.tools.len(), 1);
        assert_eq!(got.skipped.len(), 2, "{:?}", got.skipped);
        assert!(got.skipped[0].contains("'drill'"), "{:?}", got.skipped);
    }

    #[test]
    fn not_a_tool_library_is_refused() {
        assert!(matches!(
            import("a,b\n1,2\n"),
            Err(ImportError::MissingColumn("tool_type"))
        ));
        assert!(matches!(import(""), Err(ImportError::Empty)));
    }

    /// Importing a maker's file again, or a second file for a tool already in the
    /// folder, adds presets to that tool rather than a twin of it.
    #[test]
    fn a_tool_already_in_the_folder_gets_the_presets() {
        let mut folder = Folder::default();
        let first = merge(
            &mut folder,
            import(&fixture("srf4-250up.csv")).unwrap().tools,
        );
        assert_eq!(first.added, ["SRF4-250UP"]);
        // The same file again adds nothing.
        let again = merge(
            &mut folder,
            import(&fixture("srf4-250up.csv")).unwrap().tools,
        );
        assert_eq!(again.extended, [("SRF4-250UP".to_string(), 0)]);
        assert_eq!(folder.tools.len(), 1);
        // A person's own preset stays beside the maker's.
        let tool = folder.tools.get_mut("SRF4-250UP").unwrap();
        tool.presets[0].feed = Some(3000.0);
        let changed = merge(
            &mut folder,
            import(&fixture("srf4-250up.csv")).unwrap().tools,
        );
        assert_eq!(changed.extended, [("SRF4-250UP".to_string(), 1)]);
        let names: Vec<&str> = folder.tools["SRF4-250UP"]
            .presets
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names.len(), 5);
        assert_eq!(names[4], "Wood / Plywood 2");
        // Another bit is another tool.
        let other = merge(
            &mut folder,
            import(&fixture("425-up250.csv")).unwrap().tools,
        );
        assert_eq!(other.added, ["425-UP250"]);
    }
}
