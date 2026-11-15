//! The models a job machines and the blocks of material it starts from.

use camcraft_geom::turn::Turn;
use glam::{DAffine3, DQuat, DVec3};
use serde::{Deserialize, Serialize};

use super::is_default;
use crate::arrange::{Corner, Direction};

/// Units for a file that does not say, as an STL does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LengthUnit {
    /// Millimetres.
    Mm,
    /// Inches.
    Inch,
}

/// One design file.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDef {
    /// Name, unique in the job; setups and stocks refer to it by this.
    pub name: String,
    /// The file, relative to the job file's folder, with `/` between folders. An
    /// absolute path is allowed.
    pub path: String,
    /// What an STL's numbers mean. Required for STL, which does not say; never
    /// guessed. STEP carries its own units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<LengthUnit>,
    /// Where it sits in the job, from where its file put it.
    #[serde(default, skip_serializing_if = "is_default")]
    pub transform: Transform,
}

/// Where a model sits in the job: its file's points scaled, then turned, then
/// moved, as Fusion's position, rotation and scale. Everything past loading sees
/// the model where this puts it; only picks keep the file's coordinates, so they
/// move with it.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(default)]
pub struct Transform {
    /// Where the file's origin goes, millimetres.
    pub position: [f64; 3],
    /// Turns about the job's X, then Y, then Z, in degrees, about the file's
    /// origin.
    pub rotation: [f64; 3],
    /// How much larger along the file's X, Y and Z. Never mirrored: a scale
    /// must be more than zero.
    pub scale: [f64; 3],
}

impl Default for Transform {
    /// Where the file put it.
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}

impl Transform {
    /// From the file's coordinates to the job's.
    pub fn matrix(&self) -> DAffine3 {
        let [x, y, z] = self.rotation.map(f64::to_radians);
        let turn =
            DQuat::from_rotation_z(z) * DQuat::from_rotation_y(y) * DQuat::from_rotation_x(x);
        DAffine3::from_scale_rotation_translation(
            DVec3::from_array(self.scale),
            turn,
            DVec3::from_array(self.position),
        )
    }
}

/// One block of material.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct StockDef {
    /// Name, unique in the job; setups refer to it by this.
    pub name: String,
    /// Its shape.
    #[serde(flatten)]
    pub spec: StockSpec,
}

/// The shape of a block, in model coordinates, millimetres.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StockSpec {
    /// A box around models, grown by a margin.
    AroundModels {
        /// The models it is around, by name. Empty means the models of whichever
        /// setup uses it.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        models: Vec<String>,
        /// Extra material each side in X and Y, and on top in Z.
        #[serde(default)]
        margin: [f64; 3],
        /// Extra material under the models.
        ///
        /// Zero by default: for a part cut from the top only, material below the
        /// model is the part of the block the cutter never reaches.
        #[serde(default, skip_serializing_if = "is_default")]
        below: f64,
    },
    /// A box of a fixed size, placed against models.
    FixedBox(FixedBox),
    /// A sheet of a fixed size where it is, which arranges its parts on
    /// itself.
    Arranged(Arranged),
}

impl Default for StockSpec {
    /// Around the setup's models, as the block a new job starts with.
    fn default() -> Self {
        StockSpec::AroundModels {
            models: Vec::new(),
            margin: [2.0, 2.0, 2.0],
            below: 0.0,
        }
    }
}

impl StockSpec {
    /// The models it names, by name, in order: those a box is placed against,
    /// where empty means the models of whichever setup uses it, or those a
    /// sheet arranges.
    pub fn models(&self) -> Vec<&str> {
        match self {
            StockSpec::AroundModels { models, .. }
            | StockSpec::FixedBox(FixedBox { models, .. }) => {
                models.iter().map(String::as_str).collect()
            }
            StockSpec::Arranged(a) => a.parts.iter().map(|p| p.model.as_str()).collect(),
        }
    }

    /// Name model `to` wherever it names model `from`, for when a model is
    /// renamed.
    pub fn rename_model(&mut self, from: &str, to: &str) {
        for n in self.names_mut().filter(|n| *n == from) {
            *n = to.to_string();
        }
    }

    /// Name model `copy` too wherever it names model `of`, after the names it
    /// has: for a copy of a model, made with it.
    pub fn copy_model(&mut self, of: &str, copy: &str) {
        if !self.models().contains(&of) {
            return;
        }
        match self {
            StockSpec::AroundModels { models, .. }
            | StockSpec::FixedBox(FixedBox { models, .. }) => models.push(copy.to_string()),
            StockSpec::Arranged(a) => a.parts.push(SheetPart::of(copy)),
        }
    }

    /// Every name it holds, to change.
    fn names_mut(&mut self) -> Box<dyn Iterator<Item = &mut String> + '_> {
        match self {
            StockSpec::AroundModels { models, .. }
            | StockSpec::FixedBox(FixedBox { models, .. }) => Box::new(models.iter_mut()),
            StockSpec::Arranged(a) => Box::new(a.parts.iter_mut().map(|p| &mut p.model)),
        }
    }
}

/// A sheet of material of a fixed size, at a corner of its own, that arranges
/// its parts on itself: where each stands is worked out from their shapes and
/// these settings, and is never written down
/// ([`crate::arrange::arrange`]).
///
/// Left out of its file, the corner is the origin, parts are kept 10 mm apart
/// and 10 mm in from the edge, may be turned by quarter turns, are packed from
/// the front-left corner along X, and stand on the sheet's bottom face. Its
/// size has no default: a sheet is the size it is bought at.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Arranged {
    /// Its size along X, Y and Z: its length, its width and its thickness.
    pub size: [f64; 3],
    /// Its corner at the front, left and bottom, in model coordinates,
    /// wherever the models are.
    #[serde(default, skip_serializing_if = "is_default")]
    pub corner: [f64; 3],
    /// The least distance between two parts, seen from above.
    #[serde(default = "default_spacing")]
    pub spacing: f64,
    /// The least distance between a part and the sheet's edge, seen from
    /// above.
    #[serde(default = "default_border")]
    pub border: f64,
    /// The turns about Z each part may be laid at.
    #[serde(default)]
    pub rotation: Rotation,
    /// The corner packing starts from.
    #[serde(default)]
    pub from: Corner,
    /// The axis the sheet is used up along.
    #[serde(default)]
    pub along: Direction,
    /// Which face of the sheet the parts stand on.
    #[serde(default)]
    pub seat: Seat,
    /// The models it arranges, in order: each one copy, as it stands by its
    /// own rotation and scale, with no position of its own.
    #[serde(default)]
    pub parts: Vec<SheetPart>,
}

fn default_spacing() -> f64 {
    10.0
}

fn default_border() -> f64 {
    10.0
}

impl Arranged {
    /// A sheet of `size` at the origin, with no parts, every other setting as
    /// a file that leaves it out has it.
    pub fn new(size: [f64; 3]) -> Self {
        Self {
            size,
            corner: [0.0; 3],
            spacing: default_spacing(),
            border: default_border(),
            rotation: Rotation::default(),
            from: Corner::default(),
            along: Direction::default(),
            seat: Seat::default(),
            parts: Vec::new(),
        }
    }

    /// Whether it arranges model `name`.
    pub fn holds(&self, name: &str) -> bool {
        self.parts.iter().any(|p| p.model == name)
    }
}

/// One of the models a sheet arranges.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SheetPart {
    /// The model, by name.
    pub model: String,
}

impl SheetPart {
    /// Model `name`, as a part.
    pub fn of(name: &str) -> Self {
        Self {
            model: name.to_string(),
        }
    }
}

/// Which face of its sheet a part stands on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Seat {
    /// Its lowest point on the sheet's bottom face: the part is inside the
    /// sheet, to be cut out of it.
    #[default]
    Bottom,
    /// Its highest point on the sheet's top face: the part is cut down from
    /// the sheet's top.
    Top,
}

/// The turns about Z a sheet's parts may be laid at.
///
/// In a job file, a name, `"none"`, `"half-turn"` or `"quarter-turns"`, or a
/// table, `{ step = 15.0 }`, for every whole number of a step in degrees.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Rotation {
    /// As it stands only.
    None,
    /// As it stands, or turned half round.
    HalfTurn,
    /// By any number of quarter turns.
    #[default]
    QuarterTurns,
    /// By any whole number of this many degrees, from one to 180.
    Step(f64),
}

/// The least and most a [`Rotation::Step`] may be, in degrees.
pub const STEP_DEGREES: std::ops::RangeInclusive<f64> = 1.0..=180.0;

impl Rotation {
    /// Every turn it allows, the turn of none first and the rest in the order
    /// they go round; `None` for a step outside [`STEP_DEGREES`].
    pub fn turns(&self) -> Option<Vec<Turn>> {
        let degrees: Vec<f64> = match *self {
            Rotation::None => vec![0.0],
            Rotation::HalfTurn => vec![0.0, 180.0],
            Rotation::QuarterTurns => vec![0.0, 90.0, 180.0, 270.0],
            Rotation::Step(step) => {
                if !(step.is_finite() && STEP_DEGREES.contains(&step)) {
                    return None;
                }
                // A whole turn is the turn of none again; a step that goes
                // into it a whole number of times rounds just short of it, or
                // just past.
                (0..360_u32)
                    .map(|k| f64::from(k) * step)
                    .take_while(|d| *d < 360.0 - 1e-9)
                    .collect()
            }
        };
        degrees.into_iter().map(Turn::from_degrees).collect()
    }

    /// Its name as a job file writes it, and for a step, the step.
    fn named(&self) -> (&'static str, Option<f64>) {
        match *self {
            Rotation::None => ("none", None),
            Rotation::HalfTurn => ("half-turn", None),
            Rotation::QuarterTurns => ("quarter-turns", None),
            Rotation::Step(step) => ("step", Some(step)),
        }
    }
}

impl Serialize for Rotation {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap as _;
        match self.named() {
            (name, None) => s.serialize_str(name),
            (name, Some(step)) => {
                let mut map = s.serialize_map(Some(1))?;
                map.serialize_entry(name, &step)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Rotation {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(RotationVisitor)
    }
}

/// Reads a [`Rotation`]: a name, or a table holding a step.
struct RotationVisitor;

impl<'de> serde::de::Visitor<'de> for RotationVisitor {
    type Value = Rotation;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("\"none\", \"half-turn\", \"quarter-turns\" or { step = degrees }")
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Rotation, E> {
        match v {
            "none" => Ok(Rotation::None),
            "half-turn" => Ok(Rotation::HalfTurn),
            "quarter-turns" => Ok(Rotation::QuarterTurns),
            other => Err(E::unknown_variant(
                other,
                &["none", "half-turn", "quarter-turns"],
            )),
        }
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Rotation, A::Error> {
        use serde::de::Error as _;
        let mut step = None;
        while let Some(key) = map.next_key::<String>()? {
            if key != "step" {
                return Err(A::Error::unknown_field(&key, &["step"]));
            }
            if step.is_some() {
                return Err(A::Error::duplicate_field("step"));
            }
            step = Some(map.next_value::<f64>()?);
        }
        step.map(Rotation::Step)
            .ok_or_else(|| A::Error::missing_field("step"))
    }
}

/// A block of a fixed size, as a board bought to size is: on each axis it is
/// placed against the box of the models, the way Fusion's fixed size box is, so
/// it follows them when they move.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(from = "FixedBoxFile", into = "FixedBoxFile")]
pub struct FixedBox {
    /// Its size along X, Y and Z.
    pub size: [f64; 3],
    /// The models it is placed against, by name. Empty means the models of
    /// whichever setup uses it.
    pub models: Vec<String>,
    /// Where it sits on X, Y and Z.
    pub place: [Place; 3],
}

impl FixedBox {
    /// A block of `size`, centred on the models in X and Y with its top flush with
    /// theirs: where Fusion puts a new one.
    pub fn centred(size: [f64; 3]) -> Self {
        Self {
            size,
            models: Vec::new(),
            place: [
                Place::Center {},
                Place::Center {},
                Place::Max { offset: 0.0 },
            ],
        }
    }

    /// A block of `size` with its low corner at `corner`, wherever the models are.
    pub fn at(corner: [f64; 3], size: [f64; 3]) -> Self {
        Self {
            size,
            models: Vec::new(),
            place: corner.map(|coordinate| Place::At { coordinate }),
        }
    }

    /// Whether it is placed against models, rather than only at a corner.
    pub fn placed(&self) -> bool {
        !self.models.is_empty() || self.place.iter().any(|p| !matches!(p, Place::At { .. }))
    }
}

/// Where a fixed box sits along one axis, against the models' box.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "from", rename_all = "kebab-case")]
pub enum Place {
    /// Centred on the models.
    Center {},
    /// Its low side (left, front or bottom) `offset` beyond the models'.
    Min {
        /// How far beyond; less than zero is inside.
        offset: f64,
    },
    /// Its high side (right, back or top) `offset` beyond the models'.
    Max {
        /// How far beyond; less than zero is inside.
        offset: f64,
    },
    /// Its low side at a coordinate, wherever the models are.
    At {
        /// The model coordinate.
        coordinate: f64,
    },
}

impl Place {
    /// Where a block `size` long goes, against the models' `lo..hi`: its low end.
    pub fn low(self, lo: f64, hi: f64, size: f64) -> f64 {
        match self {
            Place::Center {} => (lo + hi - size) * 0.5,
            Place::Min { offset } => lo - offset,
            Place::Max { offset } => hi + offset - size,
            Place::At { coordinate } => coordinate,
        }
    }

    /// Its offset or coordinate; none when centred.
    pub fn value(self) -> Option<f64> {
        match self {
            Place::Center {} => None,
            Place::Min { offset } | Place::Max { offset } => Some(offset),
            Place::At { coordinate } => Some(coordinate),
        }
    }
}

/// A fixed box as its file holds it: a place on each axis.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FixedBoxFile {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    models: Vec<String>,
    size: [f64; 3],
    x: Place,
    y: Place,
    z: Place,
}

impl From<FixedBoxFile> for FixedBox {
    fn from(f: FixedBoxFile) -> Self {
        Self {
            size: f.size,
            models: f.models,
            place: [f.x, f.y, f.z],
        }
    }
}

impl From<FixedBox> for FixedBoxFile {
    fn from(b: FixedBox) -> Self {
        let [x, y, z] = b.place;
        Self {
            models: b.models,
            size: b.size,
            x,
            y,
            z,
        }
    }
}
