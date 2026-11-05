//! A job's cutters: the physical tool, its speeds and feeds, and the checks that
//! it is one.

use serde::{Deserialize, Serialize};

use super::{StockToLeave, is_default};
use crate::cutter::tool::{Ball, Bullnose, Cone, Cutter, Flat, ToolError};
use crate::is_positive;

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
#[serde(deny_unknown_fields)]
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

/// Named speeds and feeds for a tool: "Roughing", "Finishing", "MDF", whatever
/// the person calls it. Nothing reads the name; an operation copies the values.
///
/// Every value is optional, and choosing a preset sets only the values it has,
/// so a "Finishing" preset can hold a stepover and stock to leave and keep the
/// operation's feeds.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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

/// A tool holder, or a spindle's nose, as a stack of segments from the face
/// the tool comes out of upward, as Fusion's holders are.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Holder {
    /// Its name, e.g. "ER20 collet chuck".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Segments, from the face that meets the tool upward.
    #[serde(default)]
    pub segments: Vec<HolderSegment>,
}

/// One segment of a holder: a cylinder, or where its diameters differ, a
/// taper, as an ER nut's chamfer or an ISO flange's groove is. A job file
/// writes a cylinder's as `diameter`, a taper's as `lower_diameter` and
/// `upper_diameter`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "SegmentFile", into = "SegmentFile")]
pub struct HolderSegment {
    /// Length, in millimetres.
    pub length: f64,
    /// Diameter at its lower end, nearer the tool, in millimetres.
    pub lower_diameter: f64,
    /// Diameter at its upper end, in millimetres.
    pub upper_diameter: f64,
}

impl HolderSegment {
    /// A cylinder `diameter` across and `length` long.
    pub fn cylinder(diameter: f64, length: f64) -> Self {
        Self {
            length,
            lower_diameter: diameter,
            upper_diameter: diameter,
        }
    }

    /// Whether it is a cylinder.
    pub fn is_cylinder(&self) -> bool {
        self.lower_diameter.to_bits() == self.upper_diameter.to_bits()
    }
}

/// A holder segment as a job file writes it.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SegmentFile {
    length: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diameter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lower_diameter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    upper_diameter: Option<f64>,
}

impl TryFrom<SegmentFile> for HolderSegment {
    type Error = String;

    fn try_from(f: SegmentFile) -> Result<Self, String> {
        match (f.diameter, f.lower_diameter, f.upper_diameter) {
            (Some(d), None, None) => Ok(Self::cylinder(d, f.length)),
            (None, Some(lower), Some(upper)) => Ok(Self {
                length: f.length,
                lower_diameter: lower,
                upper_diameter: upper,
            }),
            _ => Err(
                "a holder segment has a `diameter`, or a `lower_diameter` and an `upper_diameter`"
                    .into(),
            ),
        }
    }
}

impl From<HolderSegment> for SegmentFile {
    fn from(s: HolderSegment) -> Self {
        let (diameter, lower, upper) = if s.is_cylinder() {
            (Some(s.lower_diameter), None, None)
        } else {
            (None, Some(s.lower_diameter), Some(s.upper_diameter))
        };
        Self {
            length: s.length,
            diameter,
            lower_diameter: lower,
            upper_diameter: upper,
        }
    }
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

impl ToolKind {
    /// Every kind, as Fusion's tool libraries and expressions name it.
    pub const FUSION_TYPES: [(Self, &'static str); 5] = [
        (Self::Flat, "flat end mill"),
        (Self::Ball, "ball end mill"),
        (Self::Bullnose, "bull nose end mill"),
        (Self::Cone, "chamfer mill"),
        (Self::Drill, "drill"),
    ];

    /// What Fusion calls this kind of tool (`tool_type`).
    pub fn fusion_type(self) -> &'static str {
        Self::FUSION_TYPES
            .iter()
            .find(|(k, _)| *k == self)
            .map_or("", |(_, name)| name)
    }

    /// The kind Fusion's `name` is, ignoring case: a spot drill is a cone.
    pub fn from_fusion_type(name: &str) -> Option<Self> {
        let name = name.to_ascii_lowercase();
        if name == "spot drill" {
            return Some(Self::Cone);
        }
        Self::FUSION_TYPES
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(k, _)| *k)
    }
}

/// The point angle a drill has when none is given.
pub const DRILL_POINT: f64 = 118.0;

impl ToolSpec {
    /// The radius of its corner, as Fusion's `tool_cornerRadius`: a ball's is
    /// half its diameter, a bullnose's its corner, any other's none.
    pub fn corner_radius(&self) -> f64 {
        match self.kind {
            ToolKind::Ball => self.diameter / 2.0,
            ToolKind::Bullnose => self.corner,
            ToolKind::Flat | ToolKind::Cone | ToolKind::Drill => 0.0,
        }
    }

    /// How long its flutes are taken to be where it does not say: two
    /// diameters, as a common end mill's.
    pub fn assumed_flute_length(&self) -> f64 {
        2.0 * self.diameter
    }

    /// Its flute length, as Fusion's `tool_fluteLength`: its own, or where it
    /// gives none, [`ToolSpec::assumed_flute_length`].
    pub fn flute_length(&self) -> f64 {
        self.flute_length
            .filter(|l| l.is_finite() && *l > 0.0)
            .unwrap_or_else(|| self.assumed_flute_length())
    }

    /// How far it is taken to stick out of its holder where it does not say:
    /// its flutes, and no less than three diameters, as a common end mill is
    /// held.
    pub fn assumed_stickout(&self) -> f64 {
        self.flute_length().max(3.0 * self.diameter)
    }

    /// How far it sticks out of its holder, tip to the holder's face, as
    /// Fusion's body length: its own, or where it gives none,
    /// [`ToolSpec::assumed_stickout`].
    pub fn stickout(&self) -> f64 {
        self.stickout
            .filter(|l| l.is_finite() && *l > 0.0)
            .unwrap_or_else(|| self.assumed_stickout())
    }

    /// Its shank's diameter: its own, or where it gives none, its cutting
    /// diameter, as a common end mill's.
    pub fn shank_diameter(&self) -> f64 {
        self.shank_diameter
            .filter(|d| d.is_finite() && *d > 0.0)
            .unwrap_or(self.diameter)
    }

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
        if self.number == 0 {
            // Most controllers read T0 as no tool, with no length offset: every
            // depth would be off by the tool's length.
            bad("number", "tool numbers start at 1: T0 is no tool".into());
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
                if !is_positive(seg.lower_diameter)
                    || !is_positive(seg.upper_diameter)
                    || !is_positive(seg.length)
                {
                    bad(
                        "holder",
                        format!("segment {} needs positive diameters and a length", i + 1),
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
            if let Some(v) = p.stock_to_leave.and_then(|s| s.unusable()) {
                bad(
                    "presets.stock_to_leave",
                    format!("stock to leave must be a number, got {v}"),
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
