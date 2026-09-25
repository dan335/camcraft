//! A job's cutters: the physical tool, its speeds and feeds, and the checks that
//! it is one.

use serde::{Deserialize, Serialize};

use super::{StockToLeave, is_default};
use crate::is_positive;
use crate::tool::{Ball, Bullnose, Cone, Cutter, Flat, ToolError};

/// A cutter.
///
/// The engine cuts with `kind`, `diameter` and `corner`. Everything after them
/// describes the physical tool the way Fusion's tool library does, for the person
/// setting it up and, later, for holder collision in playback and the machine's
/// tool table. All of it is optional, so a job that names only the cutting
/// geometry is complete.
///
/// Its speeds and feeds are [`Preset`]s: named sets an operation copies from.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(from = "ToolSpecFile")]
pub struct ToolSpec {
    /// Shape.
    pub kind: ToolKind,
    /// Diameter in millimetres.
    pub diameter: f64,
    /// Corner radius, for a bullnose.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub corner: f64,
    /// Included angle in degrees, for a cone: 90 for a 90° V-bit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    /// Diameter of a cone's flat tip; zero comes to a point.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tip_diameter: f64,
    /// Tool number in the program.
    #[serde(default = "one_u32")]
    pub number: u32,
    /// Spindle speed.
    #[serde(default = "default_rpm")]
    pub rpm: u32,
    /// Description for the operator.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Cutting edges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flutes: Option<u32>,
    /// Length of the cutting edges, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flute_length: Option<f64>,
    /// Length from the tip to where the shank reaches full diameter, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shoulder_length: Option<f64>,
    /// Shank diameter, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shank_diameter: Option<f64>,
    /// Overall length, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overall_length: Option<f64>,
    /// Length out of the holder, tip to holder face, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stickout: Option<f64>,
    /// What the cutter is made of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<ToolMaterial>,
    /// Maker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// Maker's product code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    /// What holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<Holder>,
    /// Which way it turns.
    #[serde(default, skip_serializing_if = "is_default")]
    pub spindle: SpindleDirection,
    /// Coolant while it cuts.
    #[serde(default, skip_serializing_if = "is_default")]
    pub coolant: Coolant,
    /// Named speeds and feeds, the first being the one a new operation gets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<Preset>,
}

/// A [`ToolSpec`] as it may be written, including the single `cutting` table that
/// came before presets, which reads as a preset called "Default".
#[derive(Deserialize)]
struct ToolSpecFile {
    kind: ToolKind,
    diameter: f64,
    #[serde(default)]
    corner: f64,
    #[serde(default)]
    angle: Option<f64>,
    #[serde(default)]
    tip_diameter: f64,
    #[serde(default = "one_u32")]
    number: u32,
    #[serde(default = "default_rpm")]
    rpm: u32,
    #[serde(default)]
    comment: String,
    #[serde(default)]
    flutes: Option<u32>,
    #[serde(default)]
    flute_length: Option<f64>,
    #[serde(default)]
    shoulder_length: Option<f64>,
    #[serde(default)]
    shank_diameter: Option<f64>,
    #[serde(default)]
    overall_length: Option<f64>,
    #[serde(default)]
    stickout: Option<f64>,
    #[serde(default)]
    material: Option<ToolMaterial>,
    #[serde(default)]
    vendor: Option<String>,
    #[serde(default)]
    product: Option<String>,
    #[serde(default)]
    holder: Option<Holder>,
    #[serde(default)]
    spindle: SpindleDirection,
    #[serde(default)]
    coolant: Coolant,
    #[serde(default)]
    presets: Vec<Preset>,
    #[serde(default)]
    cutting: Option<CuttingData>,
}

/// Suggested feeds, as tools kept them before presets.
#[derive(Deserialize)]
struct CuttingData {
    #[serde(default)]
    feed: Option<f64>,
    #[serde(default)]
    plunge_feed: Option<f64>,
    #[serde(default)]
    ramp_feed: Option<f64>,
}

impl From<ToolSpecFile> for ToolSpec {
    fn from(f: ToolSpecFile) -> Self {
        let mut presets = f.presets;
        if let Some(c) = f.cutting
            && presets.is_empty()
        {
            presets.push(Preset {
                name: "Default".into(),
                rpm: Some(f.rpm),
                feed: c.feed,
                plunge_feed: c.plunge_feed,
                ramp_feed: c.ramp_feed,
                ..Preset::default()
            });
        }
        Self {
            kind: f.kind,
            diameter: f.diameter,
            corner: f.corner,
            angle: f.angle,
            tip_diameter: f.tip_diameter,
            number: f.number,
            rpm: f.rpm,
            comment: f.comment,
            flutes: f.flutes,
            flute_length: f.flute_length,
            shoulder_length: f.shoulder_length,
            shank_diameter: f.shank_diameter,
            overall_length: f.overall_length,
            stickout: f.stickout,
            material: f.material,
            vendor: f.vendor,
            product: f.product,
            holder: f.holder,
            spindle: f.spindle,
            coolant: f.coolant,
            presets,
        }
    }
}

/// Named speeds and feeds for a tool: "Roughing", "Finishing", "MDF", whatever
/// the person calls it. Nothing reads the name; an operation copies the values.
///
/// Every value is optional, and choosing a preset sets only the values it has,
/// so a "Finishing" preset can hold a stepover and stock to leave and keep the
/// operation's feeds. The ramp feed is kept for when entries ramp; nothing uses
/// it yet.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct Preset {
    /// What the person calls it.
    pub name: String,
    /// A note, e.g. where the numbers came from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Spindle speed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpm: Option<u32>,
    /// Cutting feed, in mm/min.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed: Option<f64>,
    /// Plunge feed, in mm/min.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plunge_feed: Option<f64>,
    /// Ramp feed, in mm/min.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_feed: Option<f64>,
    /// Depth of each slice, in millimetres, for strategies that slice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stepdown: Option<f64>,
    /// Distance between passes, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stepover: Option<f64>,
    /// Material to leave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock_to_leave: Option<StockToLeave>,
}

/// `base`, or `base 2`, `base 3`… if one of `presets` already has that name.
pub fn free_preset_name(presets: &[Preset], base: &str) -> String {
    let taken = |n: &str| presets.iter().any(|p| p.name == n);
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|k| format!("{base} {k}"))
        .find(|n| !taken(n))
        .unwrap_or_default()
}

impl Default for ToolSpec {
    /// A 6 mm flat end mill: the commonest router cutter, and a sensible start.
    fn default() -> Self {
        Self {
            kind: ToolKind::Flat,
            diameter: 6.0,
            corner: 0.0,
            angle: None,
            tip_diameter: 0.0,
            number: one_u32(),
            rpm: default_rpm(),
            comment: String::new(),
            flutes: None,
            flute_length: None,
            shoulder_length: None,
            shank_diameter: None,
            overall_length: None,
            stickout: None,
            material: None,
            vendor: None,
            product: None,
            holder: None,
            spindle: SpindleDirection::default(),
            coolant: Coolant::default(),
            presets: Vec::new(),
        }
    }
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// Which way a spindle turns, seen from above.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpindleDirection {
    /// Clockwise: every ordinary right-hand cutter.
    #[default]
    Clockwise,
    /// Counter-clockwise, for a left-hand cutter.
    CounterClockwise,
}

/// Coolant while a tool cuts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Coolant {
    /// None: dry, as a router usually cuts.
    #[default]
    Off,
    /// Flood.
    Flood,
    /// Mist.
    Mist,
}

/// What a cutter is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolMaterial {
    /// Solid carbide.
    Carbide,
    /// High-speed steel.
    Hss,
}

/// A tool holder, as a stack of cylinders from the holder face upward.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Holder {
    /// Its name, e.g. "ER20 collet chuck".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Cylinders, from the face that meets the tool upward.
    #[serde(default)]
    pub segments: Vec<HolderSegment>,
}

/// One cylinder of a holder.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct HolderSegment {
    /// Diameter, in millimetres.
    pub diameter: f64,
    /// Length, in millimetres.
    pub length: f64,
}

/// A tool field that is not physically meaningful, and why.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{field}: {why}")]
pub struct ToolFieldError {
    /// Which field, as it is named in the file: `presets.feed` for a preset's.
    pub field: &'static str,
    /// Which preset, by position, for a preset's field.
    pub preset: Option<usize>,
    /// What is wrong with it.
    pub why: String,
}

fn one_u32() -> u32 {
    1
}

fn default_rpm() -> u32 {
    18000
}

/// Cutter shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolKind {
    /// Square end.
    Flat,
    /// Ball nose.
    Ball,
    /// Corner radius.
    Bullnose,
    /// A cone: a V-bit, or a chamfer mill with a flat tip.
    Cone,
    /// A twist drill: its point is a cone, 118° unless its angle says otherwise.
    Drill,
}

/// The point angle a drill has when none is given.
pub const DRILL_POINT: f64 = 118.0;

impl ToolSpec {
    /// The preset called `name`.
    pub fn preset(&self, name: &str) -> Option<&Preset> {
        self.presets.iter().find(|p| p.name == name)
    }

    /// Every problem with the tool's fields, each against the field it concerns,
    /// so an editor can show them where they are.
    pub fn check(&self) -> Vec<ToolFieldError> {
        let mut out = Vec::new();
        let mut bad = |field, why: String| {
            out.push(ToolFieldError {
                field,
                preset: None,
                why,
            });
        };
        if self.kind == ToolKind::Cone && self.angle.is_none() {
            bad("angle", "a V-bit or chamfer mill needs its angle".into());
        } else if let Err(e) = self.build() {
            let field = match e {
                ToolError::BadRadius(_) => "diameter",
                ToolError::BadAngle(_) => "angle",
                ToolError::BadTip { .. } => "tip_diameter",
                ToolError::BadCorner { .. } | ToolError::ConeStock => "corner",
            };
            bad(field, e.to_string());
        }
        if self.rpm == 0 {
            bad("rpm", "spindle speed must be greater than zero".into());
        }
        if self.flutes == Some(0) {
            bad("flutes", "a cutter has at least one flute".into());
        }
        let lengths = [
            ("flute_length", self.flute_length),
            ("shoulder_length", self.shoulder_length),
            ("shank_diameter", self.shank_diameter),
            ("overall_length", self.overall_length),
            ("stickout", self.stickout),
        ];
        for (field, value) in lengths {
            if let Some(v) = value
                && !is_positive(v)
            {
                bad(field, format!("must be greater than zero, got {v}"));
            }
        }
        // Tip upward: the flutes end before the shoulder, the shoulder before the
        // holder, and what sticks out of the holder is part of the whole tool.
        let order = [
            ("flute_length", self.flute_length),
            ("shoulder_length", self.shoulder_length),
            ("stickout", self.stickout),
            ("overall_length", self.overall_length),
        ];
        let known: Vec<(&'static str, f64)> = order
            .iter()
            .filter_map(|(f, v)| v.map(|v| (*f, v)))
            .collect();
        let words = |field: &str| match field {
            "flute_length" => "the flute length",
            "shoulder_length" => "the shoulder length",
            "stickout" => "the stickout",
            _ => "the overall length",
        };
        for pair in known.windows(2) {
            if let [(shorter, a), (longer, b)] = pair
                && a > b
            {
                bad(
                    shorter,
                    format!("{a} mm is longer than {} ({b} mm)", words(longer)),
                );
            }
        }
        if let Some(h) = &self.holder {
            for (i, seg) in h.segments.iter().enumerate() {
                if !is_positive(seg.diameter) || !is_positive(seg.length) {
                    bad(
                        "holder",
                        format!("segment {} needs a positive diameter and length", i + 1),
                    );
                }
            }
        }
        out.extend(self.check_presets());
        out
    }

    fn check_presets(&self) -> Vec<ToolFieldError> {
        let mut out = Vec::new();
        for (i, p) in self.presets.iter().enumerate() {
            let mut bad = |field, why: String| {
                out.push(ToolFieldError {
                    field,
                    preset: Some(i),
                    why,
                });
            };
            let name = p.name.trim();
            if name.is_empty() {
                bad("presets.name", "a preset needs a name".into());
            } else if self.presets.iter().take(i).any(|q| q.name.trim() == name) {
                bad(
                    "presets.name",
                    format!("there is already a preset '{name}'"),
                );
            }
            if p.rpm == Some(0) {
                bad(
                    "presets.rpm",
                    "spindle speed must be greater than zero".into(),
                );
            }
            for (field, v) in [
                ("presets.feed", p.feed),
                ("presets.plunge_feed", p.plunge_feed),
                ("presets.ramp_feed", p.ramp_feed),
                ("presets.stepdown", p.stepdown),
                ("presets.stepover", p.stepover),
            ] {
                if let Some(v) = v
                    && !is_positive(v)
                {
                    bad(field, format!("must be greater than zero, got {v}"));
                }
            }
            if let Some(s) = p.stock_to_leave
                && !(s.radial >= 0.0 && s.axial >= 0.0)
            {
                bad(
                    "presets.stock_to_leave",
                    "stock to leave cannot be negative".into(),
                );
            }
        }
        out
    }

    /// The cutter this describes.
    pub fn build(&self) -> Result<Cutter, ToolError> {
        let r = self.diameter / 2.0;
        match self.kind {
            ToolKind::Flat => Flat::new(r).map(Cutter::from),
            ToolKind::Ball => Ball::new(r).map(Cutter::from),
            ToolKind::Bullnose => Bullnose::new(r, self.corner).map(Cutter::from),
            ToolKind::Cone => {
                let angle = self.angle.unwrap_or(0.0);
                Cone::new(r, self.tip_diameter / 2.0, (angle / 2.0).to_radians()).map(Cutter::from)
            }
            ToolKind::Drill => {
                let angle = self.angle.unwrap_or(DRILL_POINT);
                Cone::new(r, 0.0, (angle / 2.0).to_radians()).map(Cutter::from)
            }
        }
    }
}
