//! Tabs: bridges of material left standing across a cut, so what the cut frees
//! stays held. Fusion's holding tabs.
//!
//! # Any operation's
//!
//! Tabs are a `Toolpath -> Toolpath` transform, applied to a strategy's passes
//! after they are ordered and before they are linked, and they know nothing of
//! the strategy. They go on the closed loops cut at the operation's deepest level,
//! which for a profile is each ring that cuts through. Every cutting move that
//! passes over a tab below its top, at any level, is lifted over it. Linking then
//! plans around the tabs as it plans around anything else left standing, because
//! it cuts the passes into the material as it links them.
//!
//! # Where they go
//!
//! Evenly round each loop, about `spacing` apart and at least two to a loop,
//! starting half a gap from where the loop starts, so a pass never starts on one.
//! A loop too short for two tabs clear of its start gets one; too short for that,
//! none.
//!
//! # What they are
//!
//! A tab is centred on a point of the loop the tool's centre follows, `width` of
//! material wide along the cut and `height` above its floor. The tool's centre is
//! kept its radius clear of the material on either side, so it rides over a disc
//! `width` plus the tool's diameter across. Rectangular, it steps straight up
//! onto the tab and straight down off it at the plunge feed; triangular, it
//! climbs and descends at 45°, so the tab is `width` wide at its top and wider
//! below.

use camcraft_geom::arc::Arc;
use glam::{DVec2, DVec3, dvec3};

use crate::cutter::dropcutter::segment_in_disc;
use crate::job::{TabShape, Tabs};
use crate::path::toolpath::{
    Move, MoveKind, MoveRole, PLUNGE_RATIO, Toolpath, ToolpathError, steep, turns_about_z,
};

/// How closely a loop's arcs are followed when measuring along it, and an arc
/// that does not turn about Z is followed when lifted over a tab, in millimetres.
const FOLLOW: f64 = 1.0e-3;

/// Tabs to put on a path, and the tool that cuts round them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabParams {
    /// The tabs asked for.
    pub tabs: Tabs,
    /// The radius of the tool, in millimetres.
    pub tool_radius: f64,
    /// Feed for stepping down off a rectangular tab, in mm/min.
    pub plunge_feed: f64,
}

/// What putting tabs on a path did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TabReport {
    /// Loops given tabs.
    pub loops: u64,
    /// Tabs in all.
    pub tabs: u64,
    /// Loops too short for any.
    pub too_short: u64,
}

/// The tabs on a path: where each is, and the shape and height they share.
#[derive(Debug, Clone, PartialEq)]
struct TabField {
    /// Where each is centred, on the loop the tool's centre follows, in the
    /// order they were put on.
    centres: Vec<DVec2>,
    /// How high their tops are.
    top: f64,
    /// Their shape.
    shape: TabShape,
    /// How far either side of a tab's middle its top holds the tool's centre
    /// up: half its width, and the tool's radius.
    half: f64,
}

/// `path` with tabs on the closed loops of its deepest cut, and every cutting
/// move over one lifted onto it.
pub fn add_tabs(path: &Toolpath, p: &TabParams) -> Result<(Toolpath, TabReport), ToolpathError> {
    let mut report = TabReport::default();
    let Some(deepest) = path
        .moves()
        .iter()
        .filter(|m| m.role == MoveRole::Cut)
        .map(|m| m.to.z)
        .reduce(f64::min)
    else {
        return Ok((path.clone(), report));
    };
    let half = p.tabs.width / 2.0 + p.tool_radius;
    // How far from its centre a tab lifts the tool: its flank runs out a height
    // further when triangular.
    let reach = match p.tabs.shape {
        TabShape::Rectangular => half,
        TabShape::Triangular => half + p.tabs.height,
    };
    let top = deepest + p.tabs.height;
    let mut centres = Vec::new();
    for ring in loops(path, deepest) {
        let length: f64 = ring
            .iter()
            .zip(ring.iter().skip(1))
            .map(|(a, b)| a.distance(*b))
            .sum();
        // Each tab clear of the loop's start, and of the next tab.
        let fit = (length / (2.0 * reach)).floor();
        let wanted = (length / p.tabs.spacing).ceil().max(2.0);
        let n = wanted.min(fit);
        if n < 1.0 {
            report.too_short += 1;
            continue;
        }
        report.loops += 1;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = n as u32;
        for k in 0..count {
            let s = (f64::from(k) + 0.5) * length / n;
            if let Some(centre) = along(&ring, s) {
                centres.push(centre);
                report.tabs += 1;
            }
        }
    }
    if centres.is_empty() {
        return Ok((path.clone(), report));
    }

    let field = TabField {
        centres,
        top,
        shape: p.tabs.shape,
        half,
    };
    let mut out = Toolpath::new();
    let mut at: Option<DVec3> = None;
    for m in path.moves() {
        match (m.role, at) {
            (MoveRole::Link, _) => {
                let to = raised(m.to, field.height_at(m.to.truncate()));
                out.push(Move { to, ..*m })?;
            }
            // A lead runs on from the ring, towards the first tab or from the
            // last, at the ring's depth: it rides over them as the cut does.
            // A spiral's ramp onto the deepest level rides over them too.
            (
                MoveRole::Cut | MoveRole::Transition | MoveRole::Lead | MoveRole::Ramp,
                Some(from),
            ) => match m.kind {
                MoveKind::Arc(a) if !turns_about_z(&a) => {
                    // Not turning about Z, it is followed by its chords.
                    let mut at = from;
                    for q in m.chords(from, FOLLOW) {
                        let piece = Move {
                            to: q,
                            kind: MoveKind::Linear,
                            ..*m
                        };
                        field.over(&mut out, at, &piece, p.plunge_feed)?;
                        at = q;
                    }
                }
                _ => field.over(&mut out, from, m, p.plunge_feed)?,
            },
            _ => out.push(*m)?,
        }
        at = Some(m.to);
    }
    Ok((out, report))
}

/// `p` lifted to `height`, if that is higher.
fn raised(p: DVec3, height: Option<f64>) -> DVec3 {
    match height {
        Some(h) if h > p.z => dvec3(p.x, p.y, h),
        _ => p,
    }
}

impl TabField {
    /// How high the tabs stand at `q`, where any does.
    fn height_at(&self, q: DVec2) -> Option<f64> {
        self.height_among(&self.centres, q)
    }

    /// How high the tabs centred at `centres` stand at `q`, where any does.
    fn height_among(&self, centres: &[DVec2], q: DVec2) -> Option<f64> {
        centres
            .iter()
            .filter_map(|c| {
                let d = c.distance(q);
                match self.shape {
                    TabShape::Rectangular => (d <= self.half + 1e-9).then_some(self.top),
                    TabShape::Triangular => Some(self.top - (d - self.half).max(0.0)),
                }
            })
            .reduce(f64::max)
    }

    /// The tabs' centres near enough the box from `lo` to `hi`, seen from above,
    /// to reach into it from `reach` away, in the order they were put on.
    fn near(&self, lo: DVec2, hi: DVec2, reach: f64) -> Vec<DVec2> {
        let (lo, hi) = (lo - DVec2::splat(reach), hi + DVec2::splat(reach));
        self.centres
            .iter()
            .copied()
            .filter(|c| c.cmpge(lo).all() && c.cmple(hi).all())
            .collect()
    }

    /// Move `m`, from `from`, lifted over any tab it crosses below its top.
    /// Where it steps down off a rectangular tab, that step is a plunge. An arc,
    /// which turns about Z, is cut into arcs between the tabs' edges.
    fn over(
        &self,
        out: &mut Toolpath,
        from: DVec3,
        m: &Move,
        plunge_feed: f64,
    ) -> Result<(), ToolpathError> {
        let lowest = from.z.min(m.to.z);
        // Nowhere below the tabs' tops, it passes over them as it is.
        if lowest >= self.top {
            return out.push(*m);
        }
        // How far from its middle a tab lifts the tool this low: its flank runs
        // out further when triangular.
        let foot = match self.shape {
            TabShape::Rectangular => self.half,
            TabShape::Triangular => self.half + (self.top - lowest),
        };
        // Only a tab that far from the move reaches it: the rest are passed
        // by, lower than it everywhere along it.
        let reach = foot + 1e-6;
        let near = match m.kind {
            MoveKind::Linear => {
                let (a, b) = (from.truncate(), m.to.truncate());
                self.near(a.min(b), a.max(b), reach)
            }
            MoveKind::Arc(a) if turns_about_z(&a) => {
                let (c, r) = (a.centre.truncate(), DVec2::splat(a.radius(from)));
                self.near(c - r, c + r, reach)
            }
            MoveKind::Arc(_) => self.centres.clone(),
        };
        let lift = |q: DVec2| self.height_among(&near, q);
        // Where along the move a tab's foot, the edge of its top, or its middle
        // is: the only places the height over it bends. A triangular tab's
        // flank crossed off its middle bends smoothly, so it is sampled too.
        let mut ts: Vec<f64> = Vec::new();
        for &centre in &near {
            let feet = crossings(from, m, centre, foot);
            // Where the move is within the tab's foot, from the first crossing (or
            // its start, if it starts within) to the last (or its end).
            let within = |f: f64| m.point_at(from, f).truncate().distance(centre) < foot;
            let t0 = if within(0.0) {
                Some(0.0)
            } else {
                feet.first().copied()
            };
            let t1 = if within(1.0) {
                Some(1.0)
            } else {
                feet.last().copied()
            };
            let (Some(t0), Some(t1)) = (t0, t1) else {
                continue;
            };
            ts.extend(feet);
            ts.extend(nearest(from, m, centre));
            ts.extend(crossings(from, m, centre, self.half));
            if self.shape == TabShape::Triangular {
                for k in 1..8 {
                    ts.push(t0 + (t1 - t0) * f64::from(k) / 8.0);
                }
            }
        }
        ts.retain(|t| *t > 1e-9 && *t < 1.0 - 1e-9);
        if ts.is_empty() && lift(m.to.truncate()).is_none_or(|h| h <= m.to.z) {
            return out.push(*m);
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
        ts.push(1.0);

        let span = m.length(from);
        let point = |t: f64| m.point_at(from, t);
        let mut at = from;
        let mut done = 0.0;
        for t in ts {
            let base = point(t);
            // Either side of the breakpoint: a rectangular tab's edge is a step.
            let nudge = if span > 1e-9 { 1e-7 / span } else { 0.0 };
            let before = raised(base, lift(point((t - nudge).max(0.0)).truncate()));
            let after = raised(base, lift(point((t + nudge).min(1.0)).truncate()));
            // A slope either side is no step: the point is where it is.
            let step = t < 1.0 && (after.z - before.z).abs() > 1e-4;
            let before = if step {
                before
            } else {
                raised(base, lift(base.truncate()))
            };
            let before = dvec3(base.x, base.y, before.z);
            let after = if step {
                dvec3(base.x, base.y, after.z)
            } else {
                before
            };
            for q in [before, after] {
                if (q - at).length() < 1e-9 {
                    continue;
                }
                let drop = at.z - q.z;
                let flat = (q.truncate() - at.truncate()).length();
                // Along the move's own shape between breakpoints; straight up or
                // down at a step.
                let kind = match m.kind {
                    MoveKind::Arc(a) if flat > 1e-9 => MoveKind::Arc(Arc {
                        sweep: a.sweep * (t - done),
                        ..a
                    }),
                    _ => MoveKind::Linear,
                };
                if steep(drop, flat, PLUNGE_RATIO) && kind == MoveKind::Linear {
                    out.feed_to(q, MoveRole::Plunge, plunge_feed)?;
                } else {
                    out.push(Move { to: q, kind, ..*m })?;
                }
                at = q;
            }
            done = t;
        }
        Ok(())
    }
}

/// Where along move `m`, from `from`, as fractions of it, it is exactly `r`
/// from `c` seen from above: its crossings of that circle, in order.
fn crossings(from: DVec3, m: &Move, c: DVec2, r: f64) -> Vec<f64> {
    match m.kind {
        MoveKind::Linear if r <= 0.0 => Vec::new(),
        MoveKind::Linear => segment_in_disc(from.truncate(), m.to.truncate(), c, r, 1e-18)
            .map(|(t0, t1)| vec![t0, t1])
            .unwrap_or_default(),
        MoveKind::Arc(a) => arc_crossings(from, &a, c, r),
    }
}

/// Where along a level arc from `from`, as fractions of it, it is `r` from `c`.
fn arc_crossings(from: DVec3, a: &Arc, c: DVec2, r: f64) -> Vec<f64> {
    let centre = a.centre.truncate();
    let radius = a.radius(from);
    let d = c.distance(centre);
    if r <= 0.0 || d < 1e-12 || d > radius + r || d < (radius - r).abs() {
        return Vec::new();
    }
    let along = (radius * radius - r * r + d * d) / (2.0 * d);
    let h = (radius * radius - along * along).max(0.0).sqrt();
    let u = (c - centre) / d;
    let mid = centre + u * along;
    let mut ts: Vec<f64> = [mid + u.perp() * h, mid - u.perp() * h]
        .into_iter()
        .filter_map(|p| fraction_at(from, a, p))
        .collect();
    ts.sort_by(f64::total_cmp);
    ts
}

/// How far round a level arc from `from` it passes over `p`, seen from above,
/// as a fraction of it, if it does.
fn fraction_at(from: DVec3, a: &Arc, p: DVec2) -> Option<f64> {
    let centre = a.centre.truncate();
    let (r0, v) = (from.truncate() - centre, p - centre);
    // Counter-clockwise seen from above, whichever way up the axis is.
    let mut angle = r0.perp_dot(v).atan2(r0.dot(v)) * a.axis.z.signum();
    let tau = std::f64::consts::TAU;
    if a.sweep > 0.0 {
        angle = angle.rem_euclid(tau);
    } else {
        angle = -(-angle).rem_euclid(tau);
    }
    let t = angle / a.sweep;
    (0.0..=1.0).contains(&t).then_some(t)
}

/// The fraction along move `m`, from `from`, nearest `c` seen from above.
fn nearest(from: DVec3, m: &Move, c: DVec2) -> Option<f64> {
    match m.kind {
        MoveKind::Linear => Some(closest(from.truncate(), m.to.truncate(), c)),
        MoveKind::Arc(a) => {
            let centre = a.centre.truncate();
            (c.distance(centre) > 1e-12)
                .then(|| fraction_at(from, &a, centre + (c - centre).normalize() * a.radius(from)))
                .flatten()
        }
    }
}

/// The fraction along the segment from `a` to `b` nearest `c`.
fn closest(a: DVec2, b: DVec2, c: DVec2) -> f64 {
    let d = b - a;
    let l = d.dot(d);
    if l < 1e-18 {
        0.0
    } else {
        ((c - a).dot(d) / l).clamp(0.0, 1.0)
    }
}

/// The closed loops cut at `deepest`: in each pass, each unbroken run of cuts
/// all at that level that ends where it starts. Leads on and off it
/// are not part of it. Each as its points seen from above, the first repeated at
/// the end.
fn loops(path: &Toolpath, deepest: f64) -> Vec<Vec<DVec2>> {
    let mut out = Vec::new();
    // Each pass from its link: whatever comes before the first link is no pass.
    for pass in path
        .passes()
        .filter(|p| p.first().is_some_and(|m| m.role == MoveRole::Link))
    {
        let at_level = |z: f64| (z - deepest).abs() < 1e-6;
        // Each unbroken run of cuts, and where the tool is when it starts: one
        // lap of a ring, or each of a spiral's laps, one level to a lap.
        let mut k = 0;
        while k < pass.len() {
            if pass.get(k).is_none_or(|n| n.role != MoveRole::Cut) {
                k += 1;
                continue;
            }
            let run_end = pass
                .get(k..)
                .and_then(|rest| rest.iter().position(|n| n.role != MoveRole::Cut))
                .map_or(pass.len(), |r| k + r);
            if let (Some(from), Some(cuts)) = (
                k.checked_sub(1).and_then(|p| pass.get(p)),
                pass.get(k..run_end),
            ) {
                let start = from.to;
                let closes = cuts
                    .last()
                    .is_some_and(|c| c.to.truncate().distance(start.truncate()) < 1e-6);
                let flat = at_level(start.z) && cuts.iter().all(|n| at_level(n.to.z));
                if closes && flat {
                    // Arcs by points close along them, so lengths along the
                    // loop are its own.
                    let mut ring = vec![start.truncate()];
                    let mut at = start;
                    for n in cuts {
                        ring.extend(n.chords(at, FOLLOW).iter().map(|p| p.truncate()));
                        at = n.to;
                    }
                    out.push(ring);
                }
            }
            k = run_end;
        }
    }
    out
}

/// The point `s` millimetres along `ring`.
fn along(ring: &[DVec2], s: f64) -> Option<DVec2> {
    let mut left = s;
    for (&a, &b) in ring.iter().zip(ring.iter().skip(1)) {
        let l = a.distance(b);
        if l >= left && l > 0.0 {
            return Some(a + (b - a) * (left / l));
        }
        left -= l;
    }
    ring.last().copied()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests;
