//! A machine, as far as a job needs to know it: what its spindle holds a
//! tool in ([decision 0021]). On a router that holds the bit in its own
//! collet there is no holder: the collet nut and the spindle's nose are what
//! reach down past the tool, the same for every tool, so the machine gives
//! their shape, and a tool with no holder of its own is checked with it. On a
//! machine whose tools sit in holders (ISO20, ISO30, BT30), each tool's holder
//! is its own.
//!
//! [decision 0021]: ../../../../docs/decisions/0021-shank-and-holder.md

use serde::{Deserialize, Serialize};

use super::{HolderSegment, is_default};

/// A machine a setup is cut on.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    /// What it is, for the person: "Avid PRO4848, 80 mm ER20 spindle".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// What its spindle takes a tool in: "ER20", "ISO30". For the person, and
    /// for which holders suit it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub spindle: String,
    /// Whether its tools sit in holders of their own, changed with them, as
    /// an ATC spindle's do: then a tool's holder is checked, and the nose
    /// only above a tool that has none.
    #[serde(default, skip_serializing_if = "is_default")]
    pub holders: bool,
    /// The spindle's nose, from the face the tool comes out of upward, as a
    /// holder's segments are: its collet nut, then the spindle's own body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nose: Vec<HolderSegment>,
}

impl Machine {
    /// What holds a tool without a holder of its own, where the machine says.
    pub fn nose(&self) -> Option<&[HolderSegment]> {
        (!self.nose.is_empty()).then_some(self.nose.as_slice())
    }
}
