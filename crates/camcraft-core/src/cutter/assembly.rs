//! The whole tool, not only its cutting end: the shoulder and shank above the
//! flutes, and the holder above those, or where the tool has none, the
//! spindle's nose ([decision 0021]). Each piece is a frustum about the tool's
//! axis, from the tip up, as Fusion and OpenCAMLib describe a tool assembly.
//!
//! What cuts is [`crate::cutter::tool`]'s; this is what must not touch: the
//! [`Body`] above the flutes, with a clearance round each piece, swept through
//! the material without cutting, so that whatever stands where it would go
//! is a collision.
//!
//! [decision 0021]: ../../../../docs/decisions/0021-shank-and-holder.md

use camcraft_sim::stock::SweptTool;

use crate::job::{HolderSegment, ToolKind, ToolSpec};

/// What a piece of the assembly is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceKind {
    /// Between the end of the flutes and the shoulder, at the cutting
    /// diameter; a thread mill's at its tooth's root.
    Neck,
    /// The shank, up to the holder.
    Shank,
    /// The holder the tool is in.
    Holder,
    /// The spindle's nose, where the tool has no holder of its own: its
    /// collet nut and what is above it.
    Nose,
}

impl PieceKind {
    /// Whether it is held off the material by the shank's clearance, rather
    /// than the holder's.
    pub fn is_shank(self) -> bool {
        matches!(self, PieceKind::Neck | PieceKind::Shank)
    }

    /// What it is called, as a collision names it.
    pub fn name(self) -> &'static str {
        match self {
            PieceKind::Neck => "neck",
            PieceKind::Shank => "shank",
            PieceKind::Holder => "holder",
            PieceKind::Nose => "spindle nose",
        }
    }
}

/// One piece: a frustum about the axis, `bottom` to `top` above the tip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    /// What it is.
    pub kind: PieceKind,
    /// Its bottom, above the tip.
    pub bottom: f64,
    /// Its top, above the tip.
    pub top: f64,
    /// Radius at its bottom.
    pub lower_radius: f64,
    /// Radius at its top.
    pub upper_radius: f64,
}

/// The tool above its flutes, from the tip up, and which of its lengths were
/// assumed for want of being given.
#[derive(Debug, Clone, PartialEq)]
pub struct Assembly {
    /// Its pieces above the flutes, from the tip up.
    pub pieces: Vec<Piece>,
    /// How long the flutes are, above the tip: where the pieces start.
    pub flutes: f64,
    /// How far the tool sticks out of the holder or nose, above the tip.
    pub stickout: f64,
    /// What it took to be so for want of being told, as the tool's picture
    /// says: "flute length", "stickout", "shank diameter".
    pub assumed: Vec<&'static str>,
}

impl Assembly {
    /// `tool` as it is held: in its own holder, or where it has none, in a
    /// spindle whose nose is `nose`, segments from the face the tool comes
    /// out of upward. With neither, it is the tool alone, up to its stickout.
    pub fn of(tool: &ToolSpec, nose: Option<&[HolderSegment]>) -> Self {
        let mut assumed = Vec::new();
        let given = |v: Option<f64>| v.filter(|x| x.is_finite() && *x > 0.0);
        if given(tool.flute_length).is_none() {
            assumed.push("flute length");
        }
        if given(tool.stickout).is_none() {
            assumed.push("stickout");
        }
        if given(tool.shank_diameter).is_none() {
            assumed.push("shank diameter");
        }
        let flutes = tool.flute_length();
        let stickout = tool.stickout().max(flutes);
        // A thread mill's neck is no wider than its tooth's root, or the
        // tooth would not be a whole V: it is that wide, its widest, so it
        // passes inside the thread its tooth cuts, as a thread mill's must.
        let cutting = if tool.kind == ToolKind::ThreadMill {
            tool.thread_root_radius()
        } else {
            tool.diameter / 2.0
        };
        let shank = tool.shank_diameter() / 2.0;
        let mut pieces = Vec::new();
        // Up to the shoulder the tool keeps its cutting diameter, a thread
        // mill its root's; above it, its shank's.
        let shoulder = given(tool.shoulder_length)
            .unwrap_or(flutes)
            .clamp(flutes, stickout);
        if shoulder > flutes {
            pieces.push(Piece {
                kind: PieceKind::Neck,
                bottom: flutes,
                top: shoulder,
                lower_radius: cutting,
                upper_radius: cutting,
            });
        }
        if stickout > shoulder {
            pieces.push(Piece {
                kind: PieceKind::Shank,
                bottom: shoulder,
                top: stickout,
                lower_radius: shank,
                upper_radius: shank,
            });
        }
        let (kind, segments) = match (&tool.holder, nose) {
            (Some(h), _) if !h.segments.is_empty() => (PieceKind::Holder, h.segments.as_slice()),
            (_, Some(nose)) => (PieceKind::Nose, nose),
            _ => (PieceKind::Holder, &[][..]),
        };
        let mut z = stickout;
        for s in segments {
            pieces.push(Piece {
                kind,
                bottom: z,
                top: z + s.length,
                lower_radius: s.lower_diameter / 2.0,
                upper_radius: s.upper_diameter / 2.0,
            });
            z += s.length;
        }
        Self {
            pieces,
            flutes,
            stickout,
            assumed,
        }
    }

    /// Whether anything holds it: a holder of its own or a spindle's nose.
    pub fn is_held(&self) -> bool {
        self.pieces.iter().any(|p| !p.kind.is_shank())
    }

    /// The assembly above the flutes as what must not touch, each piece grown
    /// by its clearance, `shank` for the neck and shank and `holder` for the
    /// holder or nose: wider by it, and reaching it lower.
    pub fn body(&self, shank: f64, holder: f64) -> Body {
        let rings = self
            .pieces
            .iter()
            .map(|p| {
                let c = if p.kind.is_shank() { shank } else { holder };
                Ring {
                    kind: p.kind,
                    clearance: c,
                    // Never into the flutes' own reach: what they cut is theirs.
                    bottom: (p.bottom - c).max(self.flutes.min(p.bottom)),
                    top: p.top,
                    lower_radius: p.lower_radius + c,
                    upper_radius: p.upper_radius + c,
                }
            })
            .collect();
        Body { rings }
    }
}

/// One piece of a [`Body`], grown by its clearance.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ring {
    kind: PieceKind,
    /// What it was grown by.
    clearance: f64,
    bottom: f64,
    top: f64,
    lower_radius: f64,
    upper_radius: f64,
}

impl Ring {
    /// The lowest height, above the tip, at which it reaches `d` from the
    /// axis, if it does.
    fn lowest_at(&self, d: f64) -> Option<f64> {
        if self.lower_radius >= d {
            return Some(self.bottom);
        }
        // Widening upward, it reaches `d` part way up.
        (self.upper_radius >= d && self.upper_radius > self.lower_radius).then(|| {
            let t = (d - self.lower_radius) / (self.upper_radius - self.lower_radius);
            self.bottom + t * (self.top - self.bottom)
        })
    }
}

/// The tool above its flutes, as what must not touch: an underside, the
/// lowest any piece of it is at each distance from the axis, as a cutter's
/// is, so the simulator sweeps it the way it sweeps a cutter, cutting nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    rings: Vec<Ring>,
}

impl Body {
    /// The piece lowest at `d` from the axis, and how high above the tip it
    /// is there: what a collision there met.
    pub fn piece_at(&self, d: f64) -> Option<(PieceKind, f64)> {
        self.rings
            .iter()
            .filter_map(|r| Some((r.kind, r.lowest_at(d)?)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// The piece that is at `height` above the tip, the lowest whose span
    /// holds it, or the highest below it.
    pub fn piece_at_height(&self, height: f64) -> Option<PieceKind> {
        self.rings
            .iter()
            .filter(|r| r.bottom <= height)
            .max_by(|a, b| a.bottom.total_cmp(&b.bottom))
            .map(|r| r.kind)
    }

    /// Whether there is anything to it.
    pub fn is_empty(&self) -> bool {
        self.rings.is_empty()
    }

    /// The lowest any of it is, above the tip.
    pub fn lowest(&self) -> f64 {
        self.rings
            .iter()
            .map(|r| r.bottom)
            .fold(f64::INFINITY, f64::min)
    }

    /// It as flat-ended cylinders, each a radius and the height above the tip
    /// its bottom is at, holding it: a taper as steps no more than `step`
    /// wider than it, each as wide as the taper at its top. What a drop of
    /// the whole tool onto the part drops, as OpenCAMLib's composite cutter
    /// does.
    pub fn cylinders(&self, step: f64) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        for r in &self.rings {
            let widening = r.upper_radius - r.lower_radius;
            if widening <= 0.0 || !crate::is_positive(step) {
                out.push((r.lower_radius.max(r.upper_radius), r.bottom));
                continue;
            }
            let steps = camcraft_geom::steps::bounded_count((widening / step).ceil()).max(1);
            let n = f64::from(steps);
            for k in 0..steps {
                let k = f64::from(k);
                let bottom = r.bottom + (r.top - r.bottom) * k / n;
                out.push((r.lower_radius + widening * (k + 1.0) / n, bottom));
            }
        }
        out
    }

    /// The clearance `piece` was grown by: how near the material it may come
    /// before it meets it.
    pub fn clearance_of(&self, piece: PieceKind) -> f64 {
        self.rings
            .iter()
            .find(|r| r.kind == piece)
            .map_or(0.0, |r| r.clearance)
    }
}

impl SweptTool for Body {
    fn max_radius(&self) -> f64 {
        self.rings
            .iter()
            .map(|r| r.lower_radius.max(r.upper_radius))
            .fold(0.0, f64::max)
    }

    fn height_at(&self, d: f64) -> Option<f64> {
        self.piece_at(d).map(|(_, h)| h)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::job::{Holder, ToolKind};

    fn quarter_inch() -> ToolSpec {
        ToolSpec {
            kind: ToolKind::Flat,
            diameter: 3.175,
            flute_length: Some(12.0),
            shank_diameter: Some(6.35),
            stickout: Some(25.0),
            holder: Some(Holder {
                name: "ER20".into(),
                segments: vec![
                    HolderSegment::cylinder(35.0, 19.5),
                    HolderSegment {
                        length: 10.0,
                        lower_diameter: 30.0,
                        upper_diameter: 40.0,
                    },
                ],
            }),
            ..ToolSpec::default()
        }
    }

    /// An eighth-inch cutter on a quarter-inch shank in an ER20 holder: its
    /// shank from the end of the flutes to the holder, then the nut, then a
    /// widening taper; nothing assumed.
    #[test]
    fn a_tool_is_its_pieces_from_the_tip_up() {
        let a = Assembly::of(&quarter_inch(), None);
        assert!(a.assumed.is_empty());
        let kinds: Vec<PieceKind> = a.pieces.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            [PieceKind::Shank, PieceKind::Holder, PieceKind::Holder]
        );
        assert_eq!((a.pieces[0].bottom, a.pieces[0].top), (12.0, 25.0));
        assert_eq!((a.pieces[1].bottom, a.pieces[1].top), (25.0, 44.5));
        assert_eq!(a.pieces[2].top, 54.5);
        assert!(a.is_held());
    }

    /// The body's underside: the shank's clearance below it at its own
    /// width, the holder's further out, and a taper's part way up it.
    #[test]
    fn the_body_is_lowest_where_each_piece_reaches() {
        let body = Assembly::of(&quarter_inch(), None).body(1.0, 5.0);
        // Within the shank, grown by a millimetre: down to the flutes' end.
        assert_eq!(body.height_at(2.0), Some(12.0));
        assert_eq!(body.piece_at(2.0).unwrap().0, PieceKind::Shank);
        // Out at the nut, grown by 5 mm and reaching 5 mm lower.
        assert_eq!(body.height_at(20.0), Some(20.0));
        assert_eq!(body.piece_at(20.0).unwrap().0, PieceKind::Holder);
        // Beyond the nut, only the taper's top reaches: part way up it.
        let h = body.height_at(24.0).unwrap();
        assert!(h > 44.5 && h < 54.5, "{h}");
        assert_eq!(body.height_at(26.0), None);
        assert_eq!(body.max_radius(), 25.0);
    }

    /// A tool with no holder is held by the spindle's nose; with neither, it
    /// is its shank alone, and says what it assumed.
    #[test]
    fn a_tool_without_a_holder_is_held_by_the_nose() {
        let mut tool = quarter_inch();
        tool.holder = None;
        let nose = [HolderSegment::cylinder(78.0, 15.0)];
        let a = Assembly::of(&tool, Some(&nose));
        assert_eq!(a.pieces.last().unwrap().kind, PieceKind::Nose);
        let bare = ToolSpec {
            kind: ToolKind::Flat,
            diameter: 6.0,
            ..ToolSpec::default()
        };
        let a = Assembly::of(&bare, None);
        assert!(!a.is_held());
        assert_eq!(a.assumed, ["flute length", "stickout", "shank diameter"]);
        assert_eq!(a.stickout, 18.0, "three diameters");
    }
}
