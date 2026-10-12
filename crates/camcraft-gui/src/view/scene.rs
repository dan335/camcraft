//! What the viewport draws, built on the CPU from a finished run.
//!
//! Pure data, so the rules can be tested without a GPU:
//!
//! - **Segments are the program in machine order.** Each joins two consecutive
//!   moves of one operation, and its index in the list *is* its order, which is
//!   what the playhead walks.
//! - **Segments never join operations.** Between two operations is a tool change,
//!   not a move, and drawing a line there would show a traverse that never happens.
//! - **The cutter is the job's own tool,** revolved from its real profile, with the
//!   tip at the origin because the tip is the point the engine controls.

use camcraft_core::job::{Job, Setup, ToolSpec};
use camcraft_core::path::toolpath::{MoveKind, MoveRole};
use camcraft_core::replay::Position;
use camcraft_core::run::{PLUNGE_RATIO, SetupResult};
use camcraft_geom::heightmap::narrow;
use camcraft_geom::mesh::Aabb;
use glam::{DVec3, Vec3};

use crate::tools::drawing::tip_profile;
use crate::ui::theme;

/// One move, drawn from where the tool was to where it went: straight, or
/// round an arc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Where the tool was.
    pub a: DVec3,
    /// Where the move took it.
    pub b: DVec3,
    /// Straight, or the arc it follows.
    pub kind: MoveKind,
    /// Why the move exists.
    pub role: MoveRole,
    /// Which operation, from zero.
    pub op: u32,
    /// Feed, for the readout; `None` for a rapid.
    pub feed: Option<f64>,
    /// Machining time at the end of this move, in seconds from the start.
    pub ends_at: f64,
}

/// How closely an arc is drawn, in millimetres: finer than a pixel at any zoom
/// a part is looked at.
pub const ARC_DRAWN: f64 = 0.005;

impl Segment {
    /// Where the tool is a fraction `t` of the way along.
    pub fn point_at(&self, t: f64) -> DVec3 {
        match self.kind {
            MoveKind::Linear => self.a.lerp(self.b, t),
            MoveKind::Arc(arc) => arc.point_at(self.a, self.b, t),
        }
    }

    /// How far the tool goes.
    pub fn length(&self) -> f64 {
        match self.kind {
            MoveKind::Linear => self.a.distance(self.b),
            MoveKind::Arc(arc) => arc.length(self.a, self.b),
        }
    }

    /// The straight pieces it is drawn as, end to end from `a` to `b`.
    pub fn pieces(&self) -> Vec<(DVec3, DVec3)> {
        match self.kind {
            MoveKind::Linear => vec![(self.a, self.b)],
            MoveKind::Arc(arc) => {
                let mut at = self.a;
                arc.chords(self.a, self.b, ARC_DRAWN)
                    .into_iter()
                    .map(|p| (std::mem::replace(&mut at, p), p))
                    .collect()
            }
        }
    }
}

/// Rapid rate assumed for machining time until a machine profile supplies one, in
/// mm/min. Times are labelled as estimates for this reason.
pub const ASSUMED_RAPID: f64 = 5000.0;

/// A cutting move that drops further than its operation allows while barely
/// advancing: a plunge at cutting feed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Steep {
    /// Its index in [`Scene::segments`].
    pub index: usize,
    /// Which operation, from zero.
    pub op: usize,
    /// How far it drops in all, in millimetres.
    pub drop: f64,
    /// Where it ends.
    pub at: DVec3,
}

/// One operation's share of the program.
#[derive(Debug, Clone)]
pub struct OpSpan {
    /// Its name.
    pub name: String,
    /// Index of its first segment.
    pub first: usize,
    /// How many segments it has.
    pub count: usize,
    /// Its cutter, as triangles, tip at the origin.
    pub cutter: Vec<[Vec3; 3]>,
}

/// Everything drawn after a run, beyond the part itself.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    /// Every move of every operation, in machine order.
    pub segments: Vec<Segment>,
    /// Each operation's range of `segments`.
    pub ops: Vec<OpSpan>,
    /// Segments per role, indexed as [`theme::ROLES`].
    pub role_counts: [usize; 8],
    /// Continuous cutting descents steeper than [`PLUNGE_RATIO`] and deeper than
    /// their operation's `max_cut_step`, each at its last move: deepest first, the
    /// order they are listed in, and in machine order among equals.
    pub steep: Vec<Steep>,
}

/// Where the program stands: `shown` whole segments made, and `fraction` of the
/// next one. The one position everything is drawn from: the paths up to it, the
/// cutter on it, the material as it stands there, and the clock.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Playhead {
    /// Segments made, in machine order.
    pub shown: usize,
    /// How far along segment `shown` the tool is, from 0 up to but not 1.
    pub fraction: f64,
}

impl Playhead {
    /// After `shown` whole segments.
    pub fn at(shown: usize) -> Self {
        Self {
            shown,
            fraction: 0.0,
        }
    }
}

/// Where the playhead stands: the last move shown.
#[derive(Debug, Clone, PartialEq)]
pub struct AtMove<'a> {
    /// Which operation, from zero.
    pub op: usize,
    /// Its name.
    pub op_name: &'a str,
    /// The move.
    pub segment: &'a Segment,
}

impl Scene {
    /// The scene for a finished run of `setup`, in `job`.
    pub fn build(job: &Job, setup: &Setup, result: &SetupResult) -> Self {
        let mut scene = Self::default();
        let mut clock = 0.0;
        for (i, run) in result.operations.iter().enumerate() {
            let op = u32::try_from(i).unwrap_or(u32::MAX);
            let first = scene.segments.len();
            let allowance = setup
                .operations
                .get(i)
                .map_or(f64::INFINITY, |o| o.passes.max_cut_step);
            for pair in run.path.moves().windows(2) {
                let [from, to] = pair else { continue };
                let segment = Segment {
                    a: from.to,
                    b: to.to,
                    kind: to.kind,
                    role: to.role,
                    op,
                    feed: to.feed,
                    ends_at: 0.0,
                };
                clock += duration(segment.length(), to.feed);
                scene.segments.push(Segment {
                    ends_at: clock,
                    ..segment
                });
                let r = theme::role_index(to.role) as usize;
                if let Some(c) = scene.role_counts.get_mut(r) {
                    *c += 1;
                }
            }
            // The core's own finding, by the core's rule: continuous descents
            // steeper than the plunge ratio, too deep for the allowance. Segment `k`
            // of this operation ends at move `k + 1`.
            let moves = run.path.moves();
            let start = moves.first().map_or(DVec3::ZERO, |m| m.to);
            for d in run.path.steep_descents(start, PLUNGE_RATIO) {
                if d.depth > allowance
                    && let Some(end) = moves.get(d.last)
                {
                    scene.steep.push(Steep {
                        index: first + d.last.saturating_sub(1),
                        op: i,
                        drop: d.depth,
                        at: end.to,
                    });
                }
            }
            let cutter = setup
                .operations
                .get(i)
                .and_then(|spec| job.tool_for(spec))
                .map(cutter_mesh)
                .unwrap_or_default();
            scene.ops.push(OpSpan {
                name: run.name.clone(),
                first,
                count: scene.segments.len() - first,
                cutter,
            });
        }
        // Once, here, rather than each frame they are listed.
        scene.steep.sort_by(|a, b| b.drop.total_cmp(&a.drop));
        scene
    }

    /// Estimated machining time for the whole program, in seconds.
    pub fn total_time(&self) -> f64 {
        self.segments.last().map_or(0.0, |s| s.ends_at)
    }

    /// Where the program stands when `shown` segments are showing, as a replay
    /// position: an operation, and how many of its moves have been made.
    ///
    /// At the end of an operation this is that operation complete, not the next one
    /// unstarted: the material is the same either way, and the operation the
    /// playhead is on is the one to name.
    pub fn position(&self, shown: usize) -> Position {
        match self.at(shown) {
            // A segment is a move after the operation's first, so showing k of an
            // operation's segments means k + 1 of its moves are made.
            Some(at) => {
                let first = self.ops.get(at.op).map_or(0, |o| o.first);
                Position {
                    op: at.op,
                    moves: shown - first + 1,
                }
            }
            None => Position { op: 0, moves: 0 },
        }
    }

    /// Machining time at `p`, in seconds from the start.
    pub fn time_of(&self, p: Playhead) -> f64 {
        let start = p
            .shown
            .checked_sub(1)
            .and_then(|i| self.segments.get(i))
            .map_or(0.0, |s| s.ends_at);
        match self.segments.get(p.shown) {
            Some(s) => start + (s.ends_at - start) * p.fraction,
            None => start,
        }
    }

    /// Where the program stands after `seconds` of machining.
    pub fn locate(&self, seconds: f64) -> Playhead {
        let shown = self.segments.partition_point(|s| s.ends_at <= seconds);
        let Some(s) = self.segments.get(shown) else {
            return Playhead::at(self.segments.len());
        };
        let start = shown
            .checked_sub(1)
            .and_then(|i| self.segments.get(i))
            .map_or(0.0, |s| s.ends_at);
        let span = s.ends_at - start;
        let fraction = if span > 0.0 {
            ((seconds - start) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Numerically at the end is the end: the next segment, not all of this one.
        if fraction >= 1.0 {
            return Playhead::at(shown + 1);
        }
        Playhead { shown, fraction }
    }

    /// `seconds` of machining on from `p`, or back for a negative number, held to
    /// the program.
    pub fn advance(&self, p: Playhead, seconds: f64) -> Playhead {
        let t = (self.time_of(p) + seconds).clamp(0.0, self.total_time());
        if seconds > 0.0 && t >= self.total_time() {
            return Playhead::at(self.segments.len());
        }
        self.locate(t)
    }

    /// The cutter at `p`: its operation and where its tip is.
    pub fn cutter_at(&self, p: Playhead) -> Option<(usize, DVec3)> {
        match self.segments.get(p.shown) {
            Some(s) => Some((s.op as usize, s.point_at(p.fraction))),
            None => self.segments.last().map(|s| (s.op as usize, s.b)),
        }
    }

    /// `p` as a replay position, and how far along the move after it.
    pub fn target(&self, p: Playhead) -> (Position, f64) {
        if p.fraction > 0.0
            && let Some(s) = self.segments.get(p.shown)
        {
            let op = s.op as usize;
            let first = self.ops.get(op).map_or(0, |o| o.first);
            // Segment `k` of an operation joins its moves `k` and `k + 1`: part-way
            // along it, `k + 1` moves are made.
            return (
                Position {
                    op,
                    moves: p.shown - first + 1,
                },
                p.fraction,
            );
        }
        (self.position(p.shown), 0.0)
    }

    /// The segment the playhead ends on, when `shown` segments are showing.
    pub fn at(&self, shown: usize) -> Option<AtMove<'_>> {
        let index = shown.checked_sub(1)?;
        let segment = self.segments.get(index)?;
        let op = segment.op as usize;
        let op_name = self.ops.get(op).map_or("", |o| o.name.as_str());
        Some(AtMove {
            op,
            op_name,
            segment,
        })
    }
}

/// How long a move `length` long takes, in seconds: at its feed, or the
/// assumed rapid rate.
fn duration(length: f64, feed: Option<f64>) -> f64 {
    let rate = feed.filter(|f| *f > 0.0).unwrap_or(ASSUMED_RAPID);
    length / rate * 60.0
}

/// Seconds as `m:ss`, or `h:mm:ss` past an hour.
pub fn clock(seconds: f64) -> String {
    // Whole seconds of a machining program: far inside u64.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let s = seconds.max(0.0).round() as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The twelve edges of a box.
pub fn box_edges(b: &Aabb) -> Vec<(DVec3, DVec3)> {
    let (lo, hi) = (b.min, b.max);
    let corner = |i: u8| {
        DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        )
    };
    let mut edges = Vec::with_capacity(12);
    for i in 0..8u8 {
        for bit in [1u8, 2, 4] {
            if i & bit == 0 {
                edges.push((corner(i), corner(i | bit)));
            }
        }
    }
    edges
}

/// Segments around the cutter's axis.
const AROUND: u32 = 36;
/// The cutter as triangles: its profile revolved about +Z, tip at the origin, with
/// a shank three diameters long, or longer for a V-bit's long flank.
pub fn cutter_mesh(tool: &ToolSpec) -> Vec<[Vec3; 3]> {
    let mut profile: Vec<(f32, f32)> = tip_profile(tool)
        .into_iter()
        .map(|(r, z)| (narrow(r), narrow(z)))
        .collect();
    let (radius, end) = profile.last().copied().unwrap_or((0.5, 0.0));
    let height = (radius * 6.0).max(end + radius * 2.0);
    profile.push((radius, height));
    profile.push((0.0, height));

    let ring = |(r, z): (f32, f32), k: u32| {
        let a = std::f32::consts::TAU * f32_from(k) / f32_from(AROUND);
        Vec3::new(r * a.cos(), r * a.sin(), z)
    };
    let mut out = Vec::new();
    for pair in profile.windows(2) {
        let [p0, p1] = pair else { continue };
        for k in 0..AROUND {
            let (a0, a1) = (ring(*p0, k), ring(*p0, k + 1));
            let (b0, b1) = (ring(*p1, k), ring(*p1, k + 1));
            // On the axis a quad collapses to one triangle; skip the empty half.
            if p0.0 > 0.0 {
                out.push([a0, a1, b1]);
            }
            if p1.0 > 0.0 {
                out.push([a0, b1, b0]);
            }
        }
    }
    out
}

/// A small count as a float, for angles.
fn f32_from(n: u32) -> f32 {
    // Counts here are tens, far inside f32's exact integer range.
    #[allow(clippy::cast_precision_loss)]
    let f = n as f32;
    f
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::indexing_slicing
)]
mod tests;
