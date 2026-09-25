//! The models a job machines and the blocks of material it starts from.

use serde::{Deserialize, Serialize};

use super::is_default;

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

/// A block of a fixed size, as a board bought to size is: on each axis it is
/// placed against the box of the models, the way Fusion's fixed size box is, so
/// it follows them when they move.
///
/// A job from before placing wrote an absolute `corner`. That reads as [`Place::At`]
/// on each axis, and a box placed only that way is written back as it was, so
/// an older build still reads it ([`FixedBox::placed`]).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "FixedBoxFile", into = "FixedBoxFile")]
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
            place: [Place::Center, Place::Center, Place::Max { offset: 0.0 }],
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
#[serde(tag = "from", rename_all = "kebab-case")]
pub enum Place {
    /// Centred on the models.
    Center,
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
            Place::Center => (lo + hi - size) * 0.5,
            Place::Min { offset } => lo - offset,
            Place::Max { offset } => hi + offset - size,
            Place::At { coordinate } => coordinate,
        }
    }

    /// Its offset or coordinate; none when centred.
    pub fn value(self) -> Option<f64> {
        match self {
            Place::Center => None,
            Place::Min { offset } | Place::Max { offset } => Some(offset),
            Place::At { coordinate } => Some(coordinate),
        }
    }
}

/// A fixed box as its file holds it: either the old absolute `corner`, or a
/// place on each axis.
#[derive(Deserialize, Serialize)]
struct FixedBoxFile {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    corner: Option<[f64; 3]>,
    size: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    x: Option<Place>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    y: Option<Place>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    z: Option<Place>,
}

impl TryFrom<FixedBoxFile> for FixedBox {
    type Error = String;

    fn try_from(f: FixedBoxFile) -> Result<Self, String> {
        let axes = [f.x, f.y, f.z];
        let place = match f.corner {
            Some(_) if axes.iter().any(Option::is_some) => {
                return Err("a fixed box has a corner or a place on each axis, not both".into());
            }
            Some(c) => c.map(|coordinate| Place::At { coordinate }),
            None => match axes {
                [Some(x), Some(y), Some(z)] => [x, y, z],
                _ => return Err("a fixed box needs a corner, or x, y and z".into()),
            },
        };
        Ok(Self {
            size: f.size,
            models: f.models,
            place,
        })
    }
}

impl From<FixedBox> for FixedBoxFile {
    fn from(b: FixedBox) -> Self {
        let placed = b.placed();
        let [x, y, z] = b.place;
        let at = |p: Place| p.value().unwrap_or(0.0);
        Self {
            models: b.models,
            corner: (!placed).then(|| [at(x), at(y), at(z)]),
            size: b.size,
            x: placed.then_some(x),
            y: placed.then_some(y),
            z: placed.then_some(z),
        }
    }
}
