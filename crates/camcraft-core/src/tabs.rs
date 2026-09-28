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

use crate::job::{TabShape, Tabs};
use crate::toolpath::{Move, MoveKind, MoveRole, PLUNGE_RATIO, Toolpath, ToolpathError};

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

/// One tab: where its centre is, and how high its top.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tab {
    centre: DVec2,
    top: f64,
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
    let mut tabs = Vec::new();
    for ring in loops(path.moves(), deepest) {
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
                tabs.push(Tab { centre, top });
                report.tabs += 1;
            }
        }
    }
    if tabs.is_empty() {
        return Ok((path.clone(), report));
    }

    let lift = |q: DVec2| surface(&tabs, p.tabs.shape, half, q);
    let mut out = Toolpath::new();
    let mut at: Option<DVec3> = None;
    for m in path.moves() {
        match (m.role, at) {
            (MoveRole::Link, _) => {
                let to = raised(m.to, lift(m.to.truncate()));
                out.push(Move { to, ..*m })?;
            }
            // A lead runs on from the ring, towards the first tab or from the
            // last, at the ring's depth: it rides over them as the cut does.
            // A spiral's ramp onto the deepest level rides over them too.
            (
                MoveRole::Cut | MoveRole::Transition | MoveRole::Lead | MoveRole::Ramp,
                Some(from),
            ) => {
                let shape = (p.tabs.shape, half);
                match m.kind {
                    MoveKind::Arc(a) if !level(&a) => {
                        // Not turning about Z, it is followed by its chords.
                        let mut at = from;
                        for q in m.chords(from, FOLLOW) {
                            let piece = Move {
                                to: q,
                                kind: MoveKind::Linear,
                                ..*m
                            };
                            over(&mut out, at, &piece, &tabs, shape, &lift, p.plunge_feed)?;
                            at = q;
                        }
                    }
                    _ => over(&mut out, from, m, &tabs, shape, &lift, p.plunge_feed)?,
                }
            }
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

/// How high the tabs stand at `q`, where any does.
fn surface(tabs: &[Tab], shape: TabShape, half: f64, q: DVec2) -> Option<f64> {
    tabs.iter()
        .filter_map(|t| {
            let d = t.centre.distance(q);
            match shape {
                TabShape::Rectangular => (d <= half + 1e-9).then_some(t.top),
                TabShape::Triangular => Some(t.top - (d - half).max(0.0)),
            }
        })
        .reduce(f64::max)
}

/// Whether an arc turns about Z, so runs level or as a helix about a vertical
/// axis.
fn level(a: &Arc) -> bool {
    a.axis.z.abs() > 1.0 - 1e-9
}

/// Where along move `m`, from `from`, as fractions of it, it is exactly `r`
/// from `c` seen from above: its crossings of that circle, in order.
fn crossings(from: DVec3, m: &Move, c: DVec2, r: f64) -> Vec<f64> {
    match m.kind {
        MoveKind::Linear => crossing(from.truncate(), m.to.truncate(), c, r)
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

/// Move `m`, from `from`, lifted over any tab it crosses below its top, the tabs
/// being `shape` and their flat tops `half` across the tool's centre either side.
/// Where it steps down off a rectangular tab, that step is a plunge. An arc,
/// which turns about Z, is cut into arcs between the tabs' edges.
fn over(
    out: &mut Toolpath,
    from: DVec3,
    m: &Move,
    tabs: &[Tab],
    (shape, half): (TabShape, f64),
    lift: &impl Fn(DVec2) -> Option<f64>,
    plunge_feed: f64,
) -> Result<(), ToolpathError> {
    let lowest = from.z.min(m.to.z);
    // Where along the move a tab's foot, the edge of its top, or its middle is:
    // the only places the height over it bends. A triangular tab's flank crossed
    // off its middle bends smoothly, so it is sampled too.
    let mut ts: Vec<f64> = Vec::new();
    for t in tabs.iter().filter(|t| t.top > lowest) {
        let foot = match shape {
            TabShape::Rectangular => half,
            TabShape::Triangular => half + (t.top - lowest),
        };
        let feet = crossings(from, m, t.centre, foot);
        // Where the move is within the tab's foot, from the first crossing (or its
        // start, if it starts within) to the last (or its end).
        let within = |f: f64| m.point_at(from, f).truncate().distance(t.centre) < foot;
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
        ts.extend(nearest(from, m, t.centre));
        ts.extend(crossings(from, m, t.centre, half));
        if shape == TabShape::Triangular {
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
            // Along the move's own shape between breakpoints; straight up or down
            // at a step.
            let kind = match m.kind {
                MoveKind::Arc(a) if flat > 1e-9 => MoveKind::Arc(Arc {
                    sweep: a.sweep * (t - done),
                    ..a
                }),
                _ => MoveKind::Linear,
            };
            if drop > 0.0 && drop > flat * PLUNGE_RATIO && kind == MoveKind::Linear {
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

/// Where along the segment from `a` to `b`, as fractions of it, it is within
/// `r` of `c`: the first and last, if anywhere.
fn crossing(a: DVec2, b: DVec2, c: DVec2, r: f64) -> Option<(f64, f64)> {
    if r <= 0.0 {
        return None;
    }
    let d = b - a;
    let f = a - c;
    let qa = d.dot(d);
    if qa < 1e-18 {
        return (f.length() <= r).then_some((0.0, 1.0));
    }
    let qb = 2.0 * f.dot(d);
    let qc = f.dot(f) - r * r;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let (t0, t1) = ((-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa));
    (t1 >= 0.0 && t0 <= 1.0).then(|| (t0.max(0.0), t1.min(1.0)))
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
fn loops(moves: &[Move], deepest: f64) -> Vec<Vec<DVec2>> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(m) = moves.get(i) {
        if m.role != MoveRole::Link {
            i += 1;
            continue;
        }
        let end = moves
            .get(i + 1..)
            .and_then(|rest| rest.iter().position(|n| n.role == MoveRole::Link))
            .map_or(moves.len(), |k| i + 1 + k);
        let pass = moves.get(i..end).unwrap_or(&[]);
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
        i = end;
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
mod tests {
    use super::*;

    /// A square ring `side` across, cut at each of `levels`, as a profile emits
    /// it: a link to its corner, then round.
    fn square(side: f64, levels: &[f64]) -> Toolpath {
        let mut p = Toolpath::new();
        let corners = [
            dvec3(0.0, 0.0, 0.0),
            dvec3(side, 0.0, 0.0),
            dvec3(side, side, 0.0),
            dvec3(0.0, side, 0.0),
            dvec3(0.0, 0.0, 0.0),
        ];
        for &z in levels {
            let at = |c: DVec3| dvec3(c.x, c.y, z);
            p.feed_to(at(corners[0]), MoveRole::Link, 1000.0).unwrap();
            for c in &corners[1..] {
                p.feed_to(at(*c), MoveRole::Cut, 1000.0).unwrap();
            }
        }
        p
    }

    fn params(shape: TabShape, spacing: f64) -> TabParams {
        TabParams {
            tabs: Tabs {
                shape,
                width: 4.0,
                height: 2.0,
                spacing,
            },
            tool_radius: 1.0,
            plunge_feed: 300.0,
        }
    }

    /// The highest the tool is anywhere within `r` of `c`, seen from above,
    /// at or below `z`.
    fn highest_near(path: &Toolpath, c: DVec2, r: f64) -> f64 {
        path.moves()
            .iter()
            .filter(|m| m.to.truncate().distance(c) <= r)
            .map(|m| m.to.z)
            .fold(f64::MIN, f64::max)
    }

    /// A round ring, one full turn about the origin at `z`, as a single arc.
    fn circle(radius: f64, z: f64) -> Toolpath {
        let mut p = Toolpath::new();
        let start = dvec3(radius, 0.0, z);
        p.feed_to(start, MoveRole::Link, 1000.0).unwrap();
        let turn = Arc {
            centre: dvec3(0.0, 0.0, z),
            axis: DVec3::Z,
            sweep: std::f64::consts::TAU,
        };
        p.arc_to(start, turn, MoveRole::Cut, 1000.0).unwrap();
        p
    }

    /// An arc is lifted over a tab and stays an arc: the ring comes back as arcs
    /// on the same circle, on the floor and over each tab, joined by steps
    /// straight up and down, and goes all the way round.
    #[test]
    fn an_arc_is_lifted_over_a_tab_as_arcs() {
        let (radius, z) = (20.0, -2.0);
        let (out, report) =
            add_tabs(&circle(radius, z), &params(TabShape::Rectangular, 60.0)).unwrap();
        assert_eq!(report.tabs, 3, "{report:?}");
        let mut at = out.moves()[0].to;
        let mut turned = 0.0;
        let (mut floor, mut top) = (0, 0);
        for m in &out.moves()[1..] {
            match m.kind {
                MoveKind::Arc(a) => {
                    assert!((a.radius(at) - radius).abs() < 1e-9, "{m:?}");
                    assert!(a.centre.truncate().length() < 1e-9);
                    assert!((m.to.z - at.z).abs() < 1e-9, "level over a rectangular tab");
                    turned += a.sweep;
                    if (m.to.z - z).abs() < 1e-9 {
                        floor += 1;
                    } else {
                        assert!((m.to.z - (z + 2.0)).abs() < 1e-9, "{m:?}");
                        top += 1;
                    }
                }
                MoveKind::Linear => {
                    assert!(
                        m.to.truncate().distance(at.truncate()) < 1e-9,
                        "a step: {m:?}"
                    );
                }
            }
            at = m.to;
        }
        assert!((turned - std::f64::consts::TAU).abs() < 1e-9, "{turned}");
        // Over each tab, in two halves either side of its middle.
        assert_eq!(top, 2 * report.tabs, "{report:?}");
        assert_eq!(floor, report.tabs + 1, "between them, and on to the end");
        assert!(at.distance(dvec3(radius, 0.0, z)) < 1e-9);
    }

    /// A lead runs on from the ring towards the first tab, at the ring's depth:
    /// it rides over the tab as the cut does, rather than cutting into it.
    #[test]
    fn a_lead_rides_over_a_tab_as_the_cut_does() {
        // A ring started halfway along a side, as a profile starts one, and a
        // lead-out carrying on along that side into the waste.
        let z = -2.0;
        let mut p = Toolpath::new();
        p.feed_to(dvec3(10.0, 0.0, z), MoveRole::Link, 1000.0)
            .unwrap();
        for c in [
            (20.0, 0.0),
            (20.0, 20.0),
            (0.0, 20.0),
            (0.0, 0.0),
            (10.0, 0.0),
        ] {
            p.feed_to(dvec3(c.0, c.1, z), MoveRole::Cut, 1000.0)
                .unwrap();
        }
        for c in [(11.5, -0.1), (12.5, -0.4), (13.5, -1.0), (14.2, -1.8)] {
            p.feed_to(dvec3(c.0, c.1, z), MoveRole::Lead, 1000.0)
                .unwrap();
        }
        let (out, report) = add_tabs(&p, &params(TabShape::Rectangular, 10.0)).unwrap();
        assert!(report.tabs > 0);
        // The first tab, 80 mm round in eight: its centre 5 mm on from the start.
        let first = dvec2(15.0, 0.0);
        for m in out.moves().iter().filter(|m| m.role == MoveRole::Lead) {
            if m.to.truncate().distance(first) < 3.0 - 1e-6 {
                assert!(
                    m.to.z >= z + 2.0 - 1e-9,
                    "a lead cuts into the tab at {}",
                    m.to
                );
            }
        }
    }

    /// Tabs go evenly round the deepest loop, clear of its start, and lift only
    /// the cuts below their top: the level above them is left alone.
    #[test]
    fn a_ring_gets_tabs_on_its_deepest_level() {
        let path = square(100.0, &[-2.0, -4.0, -6.0]);
        let (out, report) = add_tabs(&path, &params(TabShape::Rectangular, 150.0)).unwrap();
        // 400 mm round, 150 apart: three, at 200/3 intervals from 200/3 / 2.
        assert_eq!(
            report,
            TabReport {
                loops: 1,
                tabs: 3,
                too_short: 0
            }
        );
        let first = dvec2(400.0 / 6.0, 0.0);
        // The tool rides over the tab at its top, 2 mm above the floor.
        assert_eq!(highest_near(&out, first, 3.0), -4.0);
        let deepest: Vec<&Move> = out.moves().iter().filter(|m| m.to.z == -4.0).collect();
        assert!(!deepest.is_empty());
        // Up onto it and down off it: straight down, at the plunge feed.
        let plunges: Vec<&Move> = out
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Plunge)
            .collect();
        assert_eq!(plunges.len(), 3);
        assert!(
            plunges
                .iter()
                .all(|m| m.to.z == -6.0 && m.feed == Some(300.0))
        );
        // Over a tab, 3 mm each side of its centre: the tab's width and the tool.
        let on: Vec<f64> = out
            .moves()
            .iter()
            .filter(|m| m.to.z == -4.0 && m.to.y == 0.0)
            .map(|m| m.to.x)
            .collect();
        assert!(
            on.iter().any(|x| (x - (first.x - 3.0)).abs() < 1e-6),
            "{on:?}"
        );
        assert!(
            on.iter().any(|x| (x - (first.x + 3.0)).abs() < 1e-6),
            "{on:?}"
        );
        // The levels above the tab's top are cut as they were.
        let above = |p: &Toolpath| {
            p.moves()
                .iter()
                .filter(|m| m.to.z > -4.0 + 1e-9)
                .map(|m| m.to)
                .collect::<Vec<_>>()
        };
        assert_eq!(above(&out), above(&path));
        // No pass starts on a tab.
        for m in out.moves().iter().filter(|m| m.role == MoveRole::Link) {
            assert_eq!(m.to.truncate(), DVec2::ZERO);
        }
    }

    /// A triangular tab is climbed and left at 45°, nothing steep.
    #[test]
    fn a_triangular_tab_is_ramped_over() {
        let path = square(100.0, &[-6.0]);
        let (out, report) = add_tabs(&path, &params(TabShape::Triangular, 1000.0)).unwrap();
        assert_eq!(report.tabs, 2);
        assert!(out.moves().iter().all(|m| m.role != MoveRole::Plunge));
        assert_eq!(out.steep_cut_descents(DVec3::ZERO, PLUNGE_RATIO).0, 0);
        let first = dvec2(100.0, 0.0);
        // Two tabs round 400 mm: at 100 and 300 along, the second corner and the
        // opposite one. Its top is 2 mm up.
        assert!((highest_near(&out, first, 0.5) - -4.0).abs() < 1e-6);
        // Its flank reaches 2 mm past the flat top, 3 + 2 from the middle.
        assert!((highest_near(&out, dvec2(95.5, 0.0), 0.01) - -6.0).abs() > 1e-6);
        assert!((highest_near(&out, dvec2(94.0, 0.0), 1.0) - -6.0).abs() < 1e-6);
    }

    /// A loop too small for a tab clear of its start gets none, and a path with
    /// no closed loop at its deepest is left as it was.
    #[test]
    fn too_small_or_open_is_left_alone() {
        let small = square(1.0, &[-1.0]);
        let (out, report) = add_tabs(&small, &params(TabShape::Rectangular, 10.0)).unwrap();
        assert_eq!(report.too_short, 1);
        assert_eq!(out, small);

        let mut open = Toolpath::new();
        open.feed_to(dvec3(0.0, 0.0, -1.0), MoveRole::Link, 1000.0)
            .unwrap();
        open.feed_to(dvec3(50.0, 0.0, -1.0), MoveRole::Cut, 1000.0)
            .unwrap();
        let (out, report) = add_tabs(&open, &params(TabShape::Rectangular, 10.0)).unwrap();
        assert_eq!(report, TabReport::default());
        assert_eq!(out, open);
    }

    use glam::dvec2;
}
