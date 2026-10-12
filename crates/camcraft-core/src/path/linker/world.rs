//! What linking plans against: the material, as it is cut, and the part.

use camcraft_geom::heightmap::HeightMap;
use glam::{DVec2, DVec3};

use super::clearance::highest_under;
use crate::path::toolpath::Move;

/// The material an operation cuts, as it stands while its moves are linked.
pub trait Material {
    /// The top of the material anywhere under the tool's disc swept from `from` to
    /// `to`, or `None` where there is none: the cells the tool covers, as the
    /// simulator counts them, not the material standing beside it.
    fn highest(&self, from: DVec2, to: DVec2, radius: f64) -> Option<f64>;

    /// Cut `moves` into the material. The first is where the tool already is:
    /// the last move of the previous call, or the program's start.
    fn cut(&mut self, moves: &[Move]);
}

/// No material anywhere: every entry is into air.
#[derive(Debug, Clone, Copy, Default)]
pub struct Air;

impl Material for Air {
    fn highest(&self, _from: DVec2, _to: DVec2, _radius: f64) -> Option<f64> {
        None
    }

    fn cut(&mut self, _moves: &[Move]) {}
}

/// Material that stays as a height field says, whatever is cut: for tests.
#[derive(Debug, Clone)]
pub struct Unchanging(pub HeightMap);

impl Material for Unchanging {
    fn highest(&self, from: DVec2, to: DVec2, radius: f64) -> Option<f64> {
        highest_under(&self.0, from, to, radius).top
    }

    fn cut(&mut self, _moves: &[Move]) {}
}

/// The part, as the tool meets it: where its tip rests without gouging.
pub trait Surface {
    /// The tool's tip on the part at `p`, or `None` where nothing is under it.
    fn at(&self, p: DVec2) -> Option<f64>;

    /// The tool's tip on the part along the line from `from` to `to`, sampled
    /// finely enough to follow the part within the tolerance. Only points with
    /// part under them; ends included where there is.
    fn along(&self, from: DVec2, to: DVec2) -> Vec<DVec3>;

    /// [`at`](Self::at), for a tool that may graze the part: one running beside
    /// a finished wall, as a 2D cut's ring does, touches it without cutting it,
    /// so contact within the tolerance is not contact. For moves at feed; a rapid
    /// keeps clear by `at`.
    fn grazing_at(&self, p: DVec2) -> Option<f64> {
        self.at(p)
    }

    /// [`along`](Self::along), for a tool that may graze the part.
    fn grazing_along(&self, from: DVec2, to: DVec2) -> Vec<DVec3> {
        self.along(from, to)
    }
}

/// No part anywhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPart;

impl Surface for NoPart {
    fn at(&self, _p: DVec2) -> Option<f64> {
        None
    }

    fn along(&self, _from: DVec2, _to: DVec2) -> Vec<DVec3> {
        Vec::new()
    }
}
