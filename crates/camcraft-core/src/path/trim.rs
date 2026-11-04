//! A finishing path kept between a Top and a Bottom, as Fusion's 3D finishing
//! keeps to its Heights: what of each pass goes above the Top or below the
//! Bottom is cut away, and what is left of it, in pieces, is passes of their
//! own, for the linker to join.
//!
//! A finishing pass follows the part, so where it leaves the band between the
//! two it is cut where it crosses the height, not where a sample happens to
//! fall: each move is cut at the point it reaches the height exactly. The tool
//! tip is what is kept to them.

use glam::DVec3;

use super::toolpath::{Move, MoveKind, MoveRole, Toolpath, ToolpathError, starting_at};

/// How far past a height a point may be and still count as on it, in
/// millimetres: a pass along a floor at the Bottom is kept.
const ON_HEIGHT: f64 = 1.0e-6;

/// A stretch shorter than this, a micron, is no cut: a pass that only
/// touches a height leaves nothing between.
const LEAST_CUT: f64 = 1.0e-3;

/// What trimming did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Trimmed {
    /// Passes in the path given.
    pub before: u64,
    /// Passes left: pieces of them, each a pass of its own.
    pub after: u64,
}

/// `path`, a strategy's passes each a [`MoveRole::Link`] to its start and then
/// its cuts, kept between `top` and `bottom` where they are given. Arcs are
/// followed by chords within `tolerance`, and the pieces left are straight.
pub fn within_heights(
    path: &Toolpath,
    top: Option<f64>,
    bottom: Option<f64>,
    tolerance: f64,
) -> Result<(Toolpath, Trimmed), ToolpathError> {
    let lo = bottom.map_or(f64::NEG_INFINITY, |b| b - ON_HEIGHT);
    let hi = top.map_or(f64::INFINITY, |t| t + ON_HEIGHT);
    let mut pieces = Pieces::default();
    let mut before = 0;
    for pass in path.passes() {
        if pass.iter().any(|m| m.role == MoveRole::Cut) {
            before += 1;
        }
        let Some((first, rest)) = pass.split_first() else {
            continue;
        };
        if (lo..=hi).contains(&first.to.z) {
            pieces.start(first.to, first.feed);
        }
        for (at, m) in starting_at(first.to, rest) {
            let mut from = at;
            for to in m.chords(from, tolerance) {
                pieces.segment((from, to), (lo, hi), m);
                from = to;
            }
        }
        pieces.close();
    }
    let mut out = Toolpath::new();
    out.append(&pieces.kept)?;
    Ok((
        out,
        Trimmed {
            before,
            after: pieces.count,
        },
    ))
}

/// The pieces of a path being kept.
#[derive(Default)]
struct Pieces {
    /// Every piece kept so far, each from its link on.
    kept: Vec<Move>,
    /// How many.
    count: u64,
    /// The piece being kept, from its link on.
    piece: Vec<Move>,
}

impl Pieces {
    /// A piece starts at `at`, linked to.
    fn start(&mut self, at: DVec3, feed: Option<f64>) {
        self.piece.push(Move {
            to: at,
            kind: MoveKind::Linear,
            role: MoveRole::Link,
            feed,
        });
    }

    /// What of the straight stretch `a` to `b` of move `m` lies between `lo`
    /// and `hi`: carried on from the piece being kept, or a piece of its own.
    fn segment(&mut self, (a, b): (DVec3, DVec3), (lo, hi): (f64, f64), m: &Move) {
        // Where along it, 0 at `a` and 1 at `b`, it is between them.
        let dz = b.z - a.z;
        let (t0, t1) = if dz.abs() < f64::EPSILON {
            if (lo..=hi).contains(&a.z) {
                (0.0, 1.0)
            } else {
                (1.0, 0.0)
            }
        } else {
            let (p, q) = ((lo - a.z) / dz, (hi - a.z) / dz);
            (p.min(q).max(0.0), p.max(q).min(1.0))
        };
        if t0 > t1 {
            self.close();
            return;
        }
        let (enter, leave) = (a.lerp(b, t0), a.lerp(b, t1));
        if t0 > 0.0 || self.piece.is_empty() {
            self.close();
            if enter.distance(leave) < LEAST_CUT {
                return;
            }
            self.start(enter, m.feed);
        }
        self.piece.push(Move {
            to: leave,
            kind: MoveKind::Linear,
            ..*m
        });
        if t1 < 1.0 {
            self.close();
        }
    }

    /// The piece being kept ends: kept if it cuts anything.
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
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use super::*;
    use glam::dvec3;

    /// Passes as a strategy makes them: a link to each start, then its cuts.
    fn path(passes: &[&[DVec3]]) -> Toolpath {
        let mut p = Toolpath::new();
        for pass in passes {
            p.feed_to(pass[0], MoveRole::Link, 1000.0).unwrap();
            for &q in &pass[1..] {
                p.feed_to(q, MoveRole::Cut, 1000.0).unwrap();
            }
        }
        p
    }

    /// Each pass as its points, from its link.
    fn points(p: &Toolpath) -> Vec<Vec<DVec3>> {
        p.passes()
            .map(|pass| pass.iter().map(|m| m.to).collect())
            .collect()
    }

    /// The passes are these, each point to within a hundred-thousandth: a
    /// height is crossed where it is, less what counts as on it.
    fn assert_near(got: &[Vec<DVec3>], want: &[Vec<DVec3>]) {
        assert_eq!(got.len(), want.len(), "{got:?}");
        for (g, w) in got.iter().zip(want) {
            assert_eq!(g.len(), w.len(), "{got:?}");
            assert!(
                g.iter().zip(w).all(|(a, b)| a.distance(*b) < 1e-5),
                "{got:?}"
            );
        }
    }

    /// A pass down into a hollow and up out of it, kept above a Bottom: what is
    /// below is cut away where the pass crosses it, and the two sides are passes
    /// of their own.
    #[test]
    fn a_pass_below_the_bottom_is_cut_at_it() {
        let valley = path(&[&[
            dvec3(0.0, 0.0, 10.0),
            dvec3(10.0, 0.0, 0.0),
            dvec3(20.0, 0.0, 10.0),
        ]]);
        let (kept, report) = within_heights(&valley, None, Some(5.0), 0.01).unwrap();
        assert_eq!(
            report,
            Trimmed {
                before: 1,
                after: 2
            }
        );
        assert_near(
            &points(&kept),
            &[
                vec![dvec3(0.0, 0.0, 10.0), dvec3(5.0, 0.0, 5.0)],
                vec![dvec3(15.0, 0.0, 5.0), dvec3(20.0, 0.0, 10.0)],
            ],
        );
        assert!(kept.passes().all(|p| p[0].role == MoveRole::Link));
        assert!(
            kept.passes()
                .all(|p| p[1..].iter().all(|m| m.role == MoveRole::Cut))
        );
    }

    /// Kept below a Top as well, a pass over a hump keeps its two feet; a pass
    /// along a floor at the Bottom is kept whole; one wholly outside goes.
    #[test]
    fn a_pass_is_kept_between_top_and_bottom() {
        let hump = [
            dvec3(0.0, 0.0, 0.0),
            dvec3(10.0, 0.0, 20.0),
            dvec3(20.0, 0.0, 0.0),
        ];
        let floor = [dvec3(0.0, 5.0, 2.0), dvec3(20.0, 5.0, 2.0)];
        let high = [dvec3(0.0, 9.0, 30.0), dvec3(20.0, 9.0, 30.0)];
        let p = path(&[&hump, &floor, &high]);
        let (kept, report) = within_heights(&p, Some(10.0), Some(2.0), 0.01).unwrap();
        assert_eq!(
            report,
            Trimmed {
                before: 3,
                after: 3
            }
        );
        assert_near(
            &points(&kept),
            &[
                vec![dvec3(1.0, 0.0, 2.0), dvec3(5.0, 0.0, 10.0)],
                vec![dvec3(15.0, 0.0, 10.0), dvec3(19.0, 0.0, 2.0)],
                floor.to_vec(),
            ],
        );
    }

    /// With neither given, the path is as it was; a pass that only touches the
    /// Bottom leaves nothing.
    #[test]
    fn nothing_given_keeps_everything() {
        let p = path(&[&[
            dvec3(0.0, 0.0, 3.0),
            dvec3(5.0, 0.0, 1.0),
            dvec3(9.0, 0.0, 4.0),
        ]]);
        let (kept, report) = within_heights(&p, None, None, 0.01).unwrap();
        assert_eq!(kept, p);
        assert_eq!(
            report,
            Trimmed {
                before: 1,
                after: 1
            }
        );

        let touching = path(&[&[
            dvec3(0.0, 0.0, 0.0),
            dvec3(5.0, 0.0, 5.0),
            dvec3(9.0, 0.0, 0.0),
        ]]);
        let (kept, report) = within_heights(&touching, None, Some(5.0), 0.01).unwrap();
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!(report.after, 0);
    }
}
