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
    /// A box at a fixed place and size.
    FixedBox {
        /// Its lowest corner.
        corner: [f64; 3],
        /// Its size along X, Y and Z.
        size: [f64; 3],
    },
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
