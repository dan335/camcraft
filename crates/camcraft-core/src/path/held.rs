//! A finishing path over the part kept clear of it by the rest of the tool:
//! its shank, and its holder or the spindle's nose ([decision 0021]), as
//! Fusion's Shaft and Holder does for its 3D finishing.
//!
//! The rest of the tool is dropped onto the part as one tool with the cutter,
//! each piece a flat-ended cylinder at its own height above the tip,
//! OpenCAMLib's composite cutter: the tip may go no lower than the highest any
//! of them comes to rest, less its height. Where that is higher than the pass,
//! by more than the tolerance, the pass would bring the rest of the tool into
//! the part, and it is either **pulled away**, lifted to it and leaving the
//! material there, or **trimmed**, cut out where it is and the pieces either
//! side passes of their own, for the linker to join round; or the operation
//! **fails**, at the first.
//!
//! The pass is looked at at each of its points, and between them as finely as
//! the narrowest piece is wide; where it changes from clear to not, it is
//! found to within the tolerance, so what is kept is clear wherever it was
//! looked at, and the simulator checks it all again.
//!
//! [decision 0021]: ../../../../docs/decisions/0021-shank-and-holder.md

use camcraft_sim::stock::{Guard, Stock};
use glam::{DVec2, DVec3};

use super::linker::Clearance;
use super::toolpath::{Move, MoveKind, MoveRole, Toolpath, ToolpathError, starting_at};
use crate::cutter::assembly::Body;
use crate::cutter::dropcutter::DropCutter;
use crate::cutter::tool::{Flat, ToolError};

/// How deep a stretch is halved looking for where it changes, or where a
/// pulled-away pass bends: a stretch of 1 m to under a micron.
const MOST_HALVINGS: u32 = 30;

/// A stretch shorter than this, a micron, is no cut.
const LEAST_CUT: f64 = 1.0e-3;

/// The rest of the tool, dropped onto the part, or onto the material as it
/// stands, which holds the part.
pub struct HeldDrop<'a> {
    /// The part, and each piece as a cylinder and how far above the tip its
    /// bottom is.
    part: Option<(&'a DropCutter<'a>, Vec<(Flat, f64)>)>,
    /// The material, and the body watched for in it.
    stock: Option<(&'a Guard<'a, Body>, &'a Stock)>,
    /// The narrowest piece, its radius: how finely a pass is looked along.
    narrowest: f64,
}

impl<'a> HeldDrop<'a> {
    /// `body`, the rest of the tool grown by its clearances, dropped onto the
    /// part by `cutter` exactly; its tapers in steps of `tolerance`.
    pub fn over_part(
        cutter: &'a DropCutter<'a>,
        body: &Body,
        tolerance: f64,
    ) -> Result<Self, ToolError> {
        let sizes = body.cylinders(tolerance);
        let cylinders = sizes
            .iter()
            .map(|&(r, bottom)| Ok((Flat::new(r)?, bottom)))
            .collect::<Result<Vec<_>, ToolError>>()?;
        Ok(Self {
            part: Some((cutter, cylinders)),
            stock: None,
            narrowest: narrowest(body, tolerance),
        })
    }

    /// `body` against `stock` as `guard`, made over it with the same body,
    /// finds it: the material as it stands, which holds the part, to the
    /// simulator's grid, and is quick to ask however wide the holder is.
    pub fn over_stock(
        guard: &'a Guard<'a, Body>,
        stock: &'a Stock,
        body: &Body,
        tolerance: f64,
    ) -> Self {
        Self {
            part: None,
            stock: Some((guard, stock)),
            narrowest: narrowest(body, tolerance),
        }
    }

    /// The lowest the tip may be at `(x, y)` for none of the rest of the tool
    /// to meet the part, where any of it is over the part.
    pub fn tip_at(&self, x: f64, y: f64) -> Option<f64> {
        let (cutter, cylinders) = self.part.as_ref()?;
        cylinders
            .iter()
            .filter_map(|(c, bottom)| Some(cutter.drop_at(c, x, y)? - bottom))
            .reduce(f64::max)
    }

    /// How far above `p` the tip must be: nothing where it is clear.
    fn lift_at(&self, p: DVec3) -> f64 {
        let part = self.tip_at(p.x, p.y).map_or(0.0, |z| (z - p.z).max(0.0));
        let stock = self
            .stock
            .map_or(0.0, |(guard, stock)| guard.rise_at(stock, p));
        part.max(stock)
    }
}

/// The radius of the narrowest piece of `body`.
fn narrowest(body: &Body, tolerance: f64) -> f64 {
    body.cylinders(tolerance)
        .iter()
        .map(|c| c.0)
        .fold(f64::INFINITY, f64::min)
}

/// How high a link crosses, `inner`'s height or higher where the rest of the
/// tool, as `guard` watches for it, would meet the material as it stood
/// before the operation, `stock`: so a link never drags a holder across the
/// work.
pub struct HeldClearance<'a, C> {
    /// How high the cutter alone must cross.
    pub inner: &'a C,
    /// The rest of the tool, and the material, where the operation keeps it
    /// clear.
    pub held: Option<(&'a Guard<'a, Body>, &'a Stock)>,
}

impl<C: Clearance> Clearance for HeldClearance<'_, C> {
    fn safe_z(&self, from: DVec2, to: DVec2, tool_radius: f64) -> f64 {
        let z = self.inner.safe_z(from, to, tool_radius);
        z + self
            .held
            .map_or(0.0, |(guard, stock)| guard.rise_along(stock, from, to, z))
    }
}

/// What to do where the rest of the tool would meet the part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clear {
    /// Lift the pass clear, leaving the material there.
    PullAway,
    /// Cut it out, the pieces either side passes of their own.
    Trim,
}

/// What keeping clear did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Kept {
    /// Passes in the path given.
    pub before: u64,
    /// Passes after.
    pub after: u64,
    /// Stretches lifted or cut out.
    pub stretches: u64,
    /// The first place the rest of the tool met the part, if it did.
    pub first: Option<DVec3>,
}

/// `path`, a strategy's passes each a [`MoveRole::Link`] to its start and then
/// its cuts, kept clear of the part by `held`, as `how` says, to within
/// `tolerance`. A move that is clear is kept as it is; one that is not is
/// followed by chords within the tolerance, and what is made of it is
/// straight.
pub fn keep_clear(
    path: &Toolpath,
    held: &HeldDrop<'_>,
    how: Clear,
    tolerance: f64,
) -> Result<(Toolpath, Kept), ToolpathError> {
    let mut out = Out::default();
    let mut kept = Kept::default();
    // Looked along no coarser than half the narrowest piece is wide; less
    // than the tolerance is clear.
    let step = (held.narrowest * 0.5).max(tolerance);
    let lift = |p: DVec3| {
        let l = held.lift_at(p);
        if l > tolerance { l } else { 0.0 }
    };
    for pass in path.passes() {
        if pass.iter().any(|m| m.role == MoveRole::Cut) {
            kept.before += 1;
        }
        let Some((first, rest)) = pass.split_first() else {
            continue;
        };
        let mut was = (first.to, lift(first.to));
        if was.1 > 0.0 {
            kept.first.get_or_insert(first.to);
            kept.stretches += 1;
        }
        match how {
            Clear::PullAway => out.start(first.to + DVec3::Z * was.1, first.feed),
            Clear::Trim if was.1 > 0.0 => {}
            Clear::Trim => out.start(first.to, first.feed),
        }
        for (at, m) in starting_at(first.to, rest) {
            // The move looked at along its chords, as finely as the
            // narrowest piece is wide.
            let mut looked = Vec::new();
            let mut from = at;
            for to in m.chords(from, tolerance) {
                let n = camcraft_geom::steps::bounded_count((from.distance(to) / step).ceil());
                for k in 1..=n {
                    let p = from.lerp(to, f64::from(k) / f64::from(n));
                    looked.push((p, lift(p)));
                }
                from = to;
            }
            if was.1 <= 0.0 && looked.iter().all(|l| l.1 <= 0.0) {
                // Clear all the way: as it is, where there is a pass to go on.
                out.push(m);
                was = (m.to, 0.0);
                continue;
            }
            for (p, l) in looked {
                if l > 0.0 {
                    kept.first.get_or_insert(p);
                }
                match how {
                    Clear::PullAway => {
                        if l > 0.0 && was.1 <= 0.0 {
                            kept.stretches += 1;
                        }
                        pull_between(was, (p, l), &lift, tolerance, m, &mut out, 0);
                    }
                    Clear::Trim => {
                        let (a_over, over) = (was.1 > 0.0, l > 0.0);
                        if a_over != over {
                            // Where it changes, to within the tolerance.
                            let edge = edge_between(was.0, p, &lift, tolerance, a_over);
                            if a_over {
                                out.start(edge, m.feed);
                            } else {
                                kept.stretches += 1;
                                out.cut(edge, m);
                                out.close();
                            }
                        }
                        if !over {
                            out.cut(p, m);
                        }
                    }
                }
                was = (p, l);
            }
        }
        out.close();
    }
    kept.after = out.count;
    let mut path = Toolpath::new();
    path.append(&out.kept)?;
    Ok((path, kept))
}

/// `path`, holes each a pass, less those `held` says the rest of the tool
/// would meet the part or the material going down into, to within
/// `tolerance`: a hole is drilled to its depth or not at all, as Fusion's
/// drilling skips what it cannot reach.
pub fn skip_unreached(
    path: &Toolpath,
    held: &HeldDrop<'_>,
    tolerance: f64,
) -> Result<(Toolpath, Kept), ToolpathError> {
    let mut kept = Kept::default();
    let mut moves = Vec::new();
    for pass in path.passes() {
        kept.before += 1;
        // A hole goes straight down: its lowest point is where the rest of
        // the tool is lowest.
        let deepest = pass.iter().map(|m| m.to).min_by(|a, b| a.z.total_cmp(&b.z));
        match deepest {
            Some(p) if held.lift_at(p) > tolerance => {
                kept.first.get_or_insert(p);
                kept.stretches += 1;
            }
            _ => {
                kept.after += 1;
                moves.extend_from_slice(pass);
            }
        }
    }
    let mut out = Toolpath::new();
    out.append(&moves)?;
    Ok((out, kept))
}

/// From `a`, lifted by `la`, to `b`, lifted by `lb`: the lifted pass between,
/// bent where the lift between them bends by more than the tolerance.
fn pull_between(
    (a, la): (DVec3, f64),
    (b, lb): (DVec3, f64),
    lift: &impl Fn(DVec3) -> f64,
    tolerance: f64,
    m: &Move,
    out: &mut Out,
    depth: u32,
) {
    let mid = a.lerp(b, 0.5);
    if depth < MOST_HALVINGS && a.distance(b) > tolerance {
        let lm = lift(mid);
        let straight = (a.z + la + b.z + lb) * 0.5;
        if (mid.z + lm - straight).abs() > tolerance {
            pull_between((a, la), (mid, lm), lift, tolerance, m, out, depth + 1);
            pull_between((mid, lm), (b, lb), lift, tolerance, m, out, depth + 1);
            return;
        }
    }
    out.cut(b + DVec3::Z * lb, m);
}

/// Where between `a` and `b` the pass changes from clear to not, or back,
/// to within the tolerance: a point that is clear, on the side it is.
fn edge_between(
    a: DVec3,
    b: DVec3,
    lift: &impl Fn(DVec3) -> f64,
    tolerance: f64,
    a_over: bool,
) -> DVec3 {
    // `clear` is the clear end, `over` the other, closing in.
    let (mut clear, mut over) = if a_over { (b, a) } else { (a, b) };
    for _ in 0..MOST_HALVINGS {
        if clear.distance(over) <= tolerance {
            break;
        }
        let mid = clear.lerp(over, 0.5);
        if lift(mid) > 0.0 {
            over = mid;
        } else {
            clear = mid;
        }
    }
    clear
}

/// The passes being made.
#[derive(Default)]
struct Out {
    /// Every pass made so far, each from its link on.
    kept: Vec<Move>,
    /// How many.
    count: u64,
    /// The pass being made, from its link on.
    piece: Vec<Move>,
}

impl Out {
    /// A pass starts at `at`, linked to.
    fn start(&mut self, at: DVec3, feed: Option<f64>) {
        self.close();
        self.piece.push(Move {
            to: at,
            kind: MoveKind::Linear,
            role: MoveRole::Link,
            feed,
        });
    }

    /// The pass goes on by `m`, as it is.
    fn push(&mut self, m: &Move) {
        if !self.piece.is_empty() {
            self.piece.push(*m);
        }
    }

    /// The pass goes on to `to` as `m` does, straight.
    fn cut(&mut self, to: DVec3, m: &Move) {
        if self.piece.is_empty() {
            return;
        }
        self.piece.push(Move {
            to,
            kind: MoveKind::Linear,
            ..*m
        });
    }

    /// The pass ends: kept if it cuts anything.
    fn close(&mut self) {
        let piece = std::mem::take(&mut self.piece);
        let cuts = piece.windows(2).any(|w| match w {
            [a, b] => a.to.distance(b.to) >= LEAST_CUT,
            _ => false,
        });
        if cuts {
            self.count += 1;
            self.kept.extend(piece);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::cutter::assembly::Assembly;
    use crate::job::{Holder, HolderSegment, ToolSpec};
    use camcraft_geom::synthetic;
    use glam::dvec3;

    /// A 6 mm flat sticking out 10 mm from a 30 mm holder.
    fn held_tool() -> ToolSpec {
        ToolSpec {
            diameter: 6.0,
            flute_length: Some(10.0),
            stickout: Some(10.0),
            holder: Some(Holder {
                name: String::new(),
                segments: vec![HolderSegment::cylinder(30.0, 40.0)],
            }),
            ..ToolSpec::default()
        }
    }

    /// A pass along the floor toward a wall 20 high, the part standing at
    /// x < 0: the holder, 10 up and 15 out, meets its top within 15 of it.
    fn across() -> Toolpath {
        let mut p = Toolpath::new();
        p.feed_to(dvec3(40.0, 0.0, 0.0), MoveRole::Link, 1000.0)
            .unwrap();
        p.feed_to(dvec3(3.0, 0.0, 0.0), MoveRole::Cut, 1000.0)
            .unwrap();
        p
    }

    fn wall() -> camcraft_geom::mesh::TriMesh {
        synthetic::step(20.0, 200.0)
    }

    #[test]
    fn a_pass_is_pulled_away_where_the_holder_would_meet_the_part() {
        let mesh = wall();
        let cutter = DropCutter::new(&mesh);
        let body = Assembly::of(&held_tool(), None).body(0.0, 0.0);
        let held = HeldDrop::over_part(&cutter, &body, 0.01).unwrap();
        let (path, kept) = keep_clear(&across(), &held, Clear::PullAway, 0.01).unwrap();
        assert!(kept.first.is_some());
        // Far from the wall it stays down; within the holder's reach of the
        // wall, the tip is lifted so the holder clears its top: 20 - 10.
        for m in path.moves() {
            if m.to.x > 15.5 {
                assert!(m.to.z.abs() < 1e-9, "{m:?}");
            }
            if m.to.x < 14.5 {
                assert!((m.to.z - 10.0).abs() < 0.02, "{m:?}");
            }
        }
    }

    /// Two holes down to the floor, one far from the wall and one within the
    /// holder's reach of it: that one is not drilled, the other is.
    #[test]
    fn a_hole_the_holder_cannot_reach_is_not_drilled() {
        let mesh = wall();
        let cutter = DropCutter::new(&mesh);
        let body = Assembly::of(&held_tool(), None).body(0.0, 0.0);
        let held = HeldDrop::over_part(&cutter, &body, 0.01).unwrap();
        let mut holes = Toolpath::new();
        for x in [40.0, 10.0] {
            holes
                .feed_to(dvec3(x, 0.0, 25.0), MoveRole::Link, 1000.0)
                .unwrap();
            holes
                .feed_to(dvec3(x, 0.0, 0.0), MoveRole::Plunge, 300.0)
                .unwrap();
            holes
                .feed_to(dvec3(x, 0.0, 25.0), MoveRole::Retract, 1000.0)
                .unwrap();
        }
        let (kept, report) = skip_unreached(&holes, &held, 0.01).unwrap();
        assert_eq!((report.before, report.after, report.stretches), (2, 1, 1));
        assert!(kept.moves().iter().all(|m| (m.to.x - 40.0).abs() < 1e-9));
    }

    #[test]
    fn a_pass_is_trimmed_where_the_holder_would_meet_the_part() {
        let mesh = wall();
        let cutter = DropCutter::new(&mesh);
        let body = Assembly::of(&held_tool(), None).body(0.0, 0.0);
        let held = HeldDrop::over_part(&cutter, &body, 0.01).unwrap();
        let (path, kept) = keep_clear(&across(), &held, Clear::Trim, 0.01).unwrap();
        assert_eq!(kept.after, 1);
        assert_eq!(kept.stretches, 1);
        let last = path.moves().last().unwrap().to;
        assert!((last.x - 15.0).abs() < 0.02, "trimmed at {last:?}");
        assert!(path.moves().iter().all(|m| m.to.z.abs() < 1e-9));
    }
}
