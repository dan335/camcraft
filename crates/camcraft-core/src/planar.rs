//! 2D strategies: a profile round a boundary, and a pocket cleared inside one,
//! each cut in flat levels down to a floor. Fusion's 2D Contour and 2D Pocket.
//!
//! # From the model, seen from above
//!
//! The boundary is the operation's: picked faces and edges, or the models'
//! silhouette, as Fusion's select what they cut. The tool's centre follows it
//! offset by the tool's radius and the stock to leave, and every ring here is an
//! offset of it, so every point of a path comes from the model's geometry.
//! Nothing here reads the stock. Nor the part below the boundary, as Fusion's 2D
//! operations do not: a part that reaches past its boundary lower down is cut
//! into, and the verifier says so.
//!
//! # Areas and their floors
//!
//! Where the tool's centre goes falls into areas, each an outer ring and the holes
//! inside it. Each is cut down to its own floor, the lowest picked point in it, so
//! pockets picked at different depths are each cut to theirs, as Fusion's
//! "selected contour" bottom. With nothing picked, or a bottom asked for, every
//! area goes to the same one; unless the bottom asked for is measured from the
//! selection, when each is measured from the picks in it. A top measured from the
//! selection is each area's own the same way.
//!
//! # Order
//!
//! A profile cuts each ring all the way down before the next, and never a ring
//! before everything inside it: a part's holes are cut while it is still held,
//! and the outline that frees it last. A pocket is cleared area by area, level by
//! level, from the middle out: the tool goes in where there is room, and the last
//! ring of each level is a light cut along the wall. Between them, the nearest
//! next, and each ring is started where it comes nearest to where the tool is.
//!
//! # Direction
//!
//! Climb by default. With a clockwise spindle a climb cut has the material kept on
//! the tool's right; a spindle turning the other way, or conventional cutting,
//! puts it on the left.

use camcraft_geom::contour::{Contour, Region, RegionError};
use glam::{DVec2, DVec3, dvec3};

use crate::job::{Operation, Picks, ProfileSide, StrategySpec};
use crate::strategy::{StrategyError, check};
use crate::toolpath::{MoveRole, Toolpath};

/// One connected piece of where the tool's centre goes, and the levels it is cut
/// between.
#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    /// An outer ring and the holes inside it.
    pub region: Region,
    /// Where it is cut down from.
    pub top: f64,
    /// The lowest level it is cut at.
    pub bottom: f64,
}

/// How to cut the areas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanarParams {
    /// Deepest a level may be below the one above, in millimetres: Fusion's
    /// multiple depths. The levels are spaced evenly, so none is thinner than
    /// the rest. `None` cuts the bottom alone.
    pub stepdown: Option<f64>,
    /// Distance between a pocket's rings, in millimetres.
    pub stepover: f64,
    /// Cutting feed, in mm/min.
    pub feed: f64,
    /// Chordal tolerance of a pocket's inner rings, which err inward.
    pub arc: f64,
    /// The tool's radius, which a pocket's middle must be within to be cut.
    pub radius: f64,
    /// Whether the material kept is on the tool's right: climbing with a
    /// clockwise spindle.
    pub kept_on_right: bool,
    /// The arcs a profile enters and leaves each ring along, if any.
    pub leads: Option<LeadArc>,
}

/// A level arc a profile enters a ring along and leaves it along, in the waste
/// beside it: Fusion's horizontal lead-in and lead-out, the same both ways.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeadArc {
    /// Its radius, in millimetres.
    pub radius: f64,
    /// How far round it turns, in radians.
    pub sweep: f64,
    /// A straight run before it in and after it out, in millimetres.
    pub straight: f64,
}

/// What a profile or a pocket did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlanarReport {
    /// Areas cut.
    pub areas: u64,
    /// Areas left alone because their floor is not below their top.
    pub too_shallow: u64,
    /// Rings round each level, counted once however many levels they are cut at.
    pub rings: u64,
    /// The most levels any area was cut in.
    pub levels: u64,
    /// Passes emitted: every ring at every level.
    pub passes: u64,
    /// Rings a lead was asked for that had no room for one in the waste, even
    /// a quarter the size, and so are entered and left without.
    pub no_room_to_lead: u64,
}

/// How far `op`'s tool's centre stays outside its boundary, inward when
/// negative: its radius, `cutter_radius`, and the stock to leave beside, on the
/// side a profile cuts, and inside for a pocket. Zero for anything else. The
/// geometry's additional offset is a 3D machining boundary's, and moves nothing
/// here: as in Fusion, the stock to leave moves a 2D cut.
///
/// The stock to leave moves the tool and not the boundary, so a floor is still
/// found among the picks on the boundary itself.
pub fn centre_offset(op: &Operation, cutter_radius: f64) -> f64 {
    let radius = cutter_radius + op.stock_to_leave().radial;
    match op.strategy {
        StrategySpec::Profile { side, .. } => match side {
            ProfileSide::Outside => radius,
            ProfileSide::Inside => -radius,
            ProfileSide::On => 0.0,
        },
        StrategySpec::Pocket { .. } => -radius,
        StrategySpec::Parallel { .. }
        | StrategySpec::ZlevelRough { .. }
        | StrategySpec::Face { .. }
        | StrategySpec::Drill { .. } => 0.0,
    }
}

/// The areas where the tool's centre goes, with their levels.
///
/// `shape` is the boundary; `offset` how far the tool's centre stays from it,
/// outward when positive. Each of the boundary's pieces has its top and bottom
/// from `levels`, given the `picked` points in it ([`picked_pieces`]); an area is
/// cut from the highest top of the pieces it came from down to their lowest
/// bottom. Rings err by at most `arc`, away from the boundary when growing and
/// inside it when shrinking.
pub fn areas(
    shape: &Region,
    offset: f64,
    arc: f64,
    picked: &[DVec3],
    levels: &dyn Fn(Option<Picks>) -> (f64, f64),
) -> Result<Vec<Area>, StrategyError> {
    let shapes: Vec<(Region, (f64, f64))> = picked_pieces(shape, arc, picked)
        .into_iter()
        .map(|(s, picks)| (s, levels(picks)))
        .collect();
    let centres = shape.offset(offset, arc);
    let mut out = Vec::new();
    for region in pieces(&centres) {
        // A piece grown from the boundary covers the boundary's pieces it took in;
        // one shrunk lies in the one it came from. Either way, the ones it
        // overlaps.
        let mut span: Option<(f64, f64)> = None;
        for (s, (top, bottom)) in &shapes {
            if !overlapping(&region, s) {
                continue;
            }
            if region.intersection(s).map_err(region_err)?.area() > 1e-9 {
                span = Some(span.map_or((*top, *bottom), |(t, b)| (t.max(*top), b.min(*bottom))));
            }
        }
        let (top, bottom) = span.unwrap_or_else(|| levels(None));
        out.push(Area {
            region,
            top,
            bottom,
        });
    }
    Ok(out)
}

/// The boundary's own pieces, each an outer ring with the holes inside it, and
/// how high the `picked` points in each go, if any are in it. A picked edge lies
/// on its piece's boundary, so each is grown by `arc`, or a micron at least, to
/// take in what lies on it.
pub fn picked_pieces(shape: &Region, arc: f64, picked: &[DVec3]) -> Vec<(Region, Option<Picks>)> {
    pieces(shape)
        .into_iter()
        .map(|s| {
            let near = s.offset(arc.max(1e-3), arc);
            let picks = Picks::of(
                picked
                    .iter()
                    .filter(|p| near.contains(p.truncate()))
                    .map(|p| p.z),
            );
            (s, picks)
        })
        .collect()
}

/// A profile round each area's rings, `side` of the boundary they came from,
/// each ring all the way down before the next.
pub fn profile(
    areas: &[Area],
    side: ProfileSide,
    params: &PlanarParams,
) -> Result<(Toolpath, PlanarReport), StrategyError> {
    check_params(params, areas)?;
    let mut report = PlanarReport::default();
    // Outside, what is kept is the side the region is on: the left of a ring as
    // the region winds it. Inside or on it, the right.
    let kept_left = side == ProfileSide::Outside;
    let reverse = kept_left == params.kept_on_right;
    let mut rings: Vec<(Vec<DVec2>, Vec<f64>, usize)> = Vec::new();
    for (i, area) in areas.iter().enumerate() {
        let levels = levels(area.top, area.bottom, params.stepdown);
        if levels.is_empty() {
            report.too_shallow += 1;
            continue;
        }
        report.areas += 1;
        report.levels = report.levels.max(levels.len() as u64);
        for ring in area.region.rings() {
            rings.push((wound(ring, reverse), levels.clone(), i));
        }
    }

    // Where a lead may go: the waste beside the rings, outside every area for a
    // profile outside its boundary, inside its own area for one inside. A
    // groove on the boundary has waste on neither side it can be sure of, so
    // has no leads.
    let waste = |area: usize, p: DVec2| match side {
        ProfileSide::Outside => areas.iter().all(|a| !a.region.contains(p)),
        ProfileSide::Inside => areas.get(area).is_some_and(|a| a.region.contains(p)),
        ProfileSide::On => false,
    };

    let mut path = Toolpath::new();
    let mut at: Option<DVec2> = None;
    let loops: Vec<&[DVec2]> = rings.iter().map(|(r, _, _)| r.as_slice()).collect();
    for i in inside_first(&loops) {
        let Some((ring, levels, area)) = rings.get(i) else {
            continue;
        };
        let mut ring = starting_near(ring, at);
        report.rings += 1;
        // The waste is on the side away from what is kept: the left, when what
        // is kept is on the right. A lead needs a straight run to join: where the
        // ring starts at a corner, it starts instead halfway along its longest
        // side.
        let asked = params.leads.filter(|_| side != ProfileSide::On);
        let fit = |r: &[DVec2]| {
            asked.and_then(|l| leads_for(r, l, params.kept_on_right, |p| waste(*area, p)))
        };
        let mut leads = fit(&ring);
        if asked.is_some() && leads.is_none() {
            let middle = longest_middle(&ring);
            leads = fit(&middle);
            if leads.is_some() {
                ring = middle;
            }
        }
        if asked.is_some() && leads.is_none() {
            report.no_room_to_lead += 1;
        }
        for &z in levels {
            emit(&ring, z, params.feed, leads.as_ref(), &mut path)?;
            report.passes += 1;
        }
        at = ring.first().copied();
    }
    Ok((path, report))
}

/// The lead-in onto the start of closed `ring` and the lead-out off its end, as
/// points seen from above, each in the order the tool goes, the ring's start
/// left out: arcs of `lead` tangent to the ring, on its left when `left`, else
/// its right. The largest of the lead's radius, its half and its quarter whose
/// points are all in the waste, as `waste` says; `None` where none is.
fn leads_for(
    ring: &[DVec2],
    lead: LeadArc,
    left: bool,
    waste: impl Fn(DVec2) -> bool,
) -> Option<(Vec<DVec2>, Vec<DVec2>)> {
    let start = *ring.first()?;
    // The way the ring leaves its start, and the way it comes back to it.
    let into = ring
        .iter()
        .skip(1)
        .map(|p| *p - start)
        .find(|d| d.length() > 1e-9)?
        .normalize();
    // The ring does not repeat its start at its end.
    let back = ring
        .iter()
        .rev()
        .map(|p| start - *p)
        .find(|d| d.length() > 1e-9)?
        .normalize();
    let aside = |t: DVec2| if left { t.perp() } else { -t.perp() };
    let steps = arc_steps(lead.sweep);
    let angles = move || (1..=steps).map(move |k| lead.sweep * f64::from(k) / f64::from(steps));
    [1.0, 0.5, 0.25].into_iter().find_map(|k| {
        let r = lead.radius * k;
        // In: round an arc from the waste onto the start, heading along `into`.
        // Angle `a` back from the start is at centre − n·r·cos a − t·r·sin a.
        let n = aside(into);
        let point_in = |a: f64| start + n * r * (1.0 - a.cos()) - into * r * a.sin();
        let heading_in = |a: f64| (into * a.cos() - n * a.sin()).normalize_or_zero();
        let mut lead_in: Vec<DVec2> = angles().map(point_in).collect();
        lead_in.reverse();
        if lead.straight > 0.0 {
            let far = point_in(lead.sweep);
            lead_in.insert(0, far - heading_in(lead.sweep) * lead.straight);
        }
        // Out: on along `back`, round an arc away into the waste.
        let n = aside(back);
        let point_out = |a: f64| start + n * r * (1.0 - a.cos()) + back * r * a.sin();
        let heading_out = |a: f64| (back * a.cos() + n * a.sin()).normalize_or_zero();
        let mut lead_out: Vec<DVec2> = angles().map(point_out).collect();
        if lead.straight > 0.0 {
            let far = point_out(lead.sweep);
            lead_out.push(far + heading_out(lead.sweep) * lead.straight);
        }
        let clear = lead_in.iter().chain(&lead_out).all(|p| waste(*p));
        clear.then_some((lead_in, lead_out))
    })
}

/// Closed `ring` started halfway along its longest side instead.
fn longest_middle(ring: &[DVec2]) -> Vec<DVec2> {
    let n = ring.len();
    let Some((i, _)) = (0..n)
        .filter_map(|i| Some((i, ring.get(i)?.distance(*ring.get((i + 1) % n)?))))
        .max_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return ring.to_vec();
    };
    let (Some(&a), Some(&b)) = (ring.get(i), ring.get((i + 1) % n)) else {
        return ring.to_vec();
    };
    let mut out = vec![(a + b) / 2.0];
    out.extend((1..=n).filter_map(|k| ring.get((i + k) % n)));
    out
}

/// How many chords an arc turning `sweep` radians is drawn in: one every five
/// degrees or so, at least two.
fn arc_steps(sweep: f64) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = (sweep.abs() / 5f64.to_radians()).ceil().clamp(2.0, 72.0) as u32;
    n
}

/// Each area cleared from its middle out to its walls, level by level down to its
/// floor, area after area.
pub fn pocket(
    areas: &[Area],
    params: &PlanarParams,
) -> Result<(Toolpath, PlanarReport), StrategyError> {
    check_params(params, areas)?;
    let mut report = PlanarReport::default();
    // The material kept, the wall, is outward of every ring: on the right of a
    // ring as the region winds it.
    let reverse = !params.kept_on_right;
    // Each area's rings, from its middle out, with its levels.
    let mut cleared: Vec<(Vec<RingSet>, Vec<f64>)> = Vec::new();
    for area in areas {
        let levels = levels(area.top, area.bottom, params.stepdown);
        if levels.is_empty() {
            report.too_shallow += 1;
            continue;
        }
        let rings = middle_out(&area.region, params, reverse);
        if rings.is_empty() {
            continue;
        }
        report.areas += 1;
        report.levels = report.levels.max(levels.len() as u64);
        report.rings += rings.iter().map(|r| r.len() as u64).sum::<u64>();
        cleared.push((rings, levels));
    }

    let mut path = Toolpath::new();
    let mut at: Option<DVec2> = None;
    let mut left: Vec<usize> = (0..cleared.len()).collect();
    while !left.is_empty() {
        // The nearest area next, by where its middle is first cut.
        let pick = nearest(&left, at, |&i| {
            cleared
                .get(i)
                .and_then(|(rings, _)| rings.first())
                .map(|set| set.iter().flatten().copied().collect())
                .unwrap_or_default()
        });
        let i = left.remove(pick);
        let Some((rings, levels)) = cleared.get(i) else {
            continue;
        };
        for &z in levels {
            for set in rings {
                let mut todo: Vec<usize> = (0..set.len()).collect();
                while !todo.is_empty() {
                    let k = todo.remove(nearest(&todo, at, |&j| {
                        set.get(j).cloned().unwrap_or_default()
                    }));
                    let Some(ring) = set.get(k) else { continue };
                    let ring = starting_near(ring, at);
                    emit(&ring, z, params.feed, None, &mut path)?;
                    report.passes += 1;
                    at = ring.first().copied();
                }
            }
        }
    }
    Ok((path, report))
}

fn check_params(params: &PlanarParams, areas: &[Area]) -> Result<(), StrategyError> {
    if let Some(d) = params.stepdown {
        check("stepdown", d)?;
    }
    check("stepover", params.stepover)?;
    check("feed", params.feed)?;
    check("arc tolerance", params.arc)?;
    check("tool radius", params.radius)?;
    if let Some(a) = areas.iter().find(|a| !a.top.is_finite()) {
        return Err(StrategyError::BadParameter {
            name: "top",
            value: a.top,
        });
    }
    Ok(())
}

/// The levels from `top` down to `bottom`, evenly spaced no more than `stepdown`
/// apart, the last at `bottom`: `bottom` alone with no stepdown. None when
/// `bottom` is not below `top`.
pub(crate) fn levels(top: f64, bottom: f64, stepdown: Option<f64>) -> Vec<f64> {
    let span = top - bottom;
    if !(span > 1e-9 && bottom.is_finite()) {
        return Vec::new();
    }
    // Bounded, so a tiny stepdown cannot run away.
    let count = stepdown
        .map_or(1.0, |d| (span / d - 1e-9).ceil())
        .clamp(1.0, 10_000.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = count as u32;
    (1..=count)
        .map(|i| {
            if i == count {
                bottom
            } else {
                top - span * f64::from(i) / f64::from(count)
            }
        })
        .collect()
}

/// The rings the same distance in from a pocket's walls.
type RingSet = Vec<Vec<DVec2>>;

/// A pocket's rings, grouped by how far in they are and from the middle out: each
/// an offset of the area inward by a whole number of stepovers, and where the
/// tool on the innermost would leave some of the middle, one ring nearer it.
fn middle_out(area: &Region, params: &PlanarParams, reverse: bool) -> Vec<RingSet> {
    let mut sets: Vec<Region> = Vec::new();
    // Bounded: an area cannot take more rings than its extent over the step, and
    // the loop stops as soon as an offset leaves nothing.
    for k in 0..10_000u32 {
        let inset = area.offset(-f64::from(k) * params.stepover, params.arc);
        if inset.is_empty() {
            break;
        }
        sets.push(inset);
    }
    if let Some(inner) = sets.last()
        && let Some(middle) = middle(inner, params)
    {
        sets.push(middle);
    }
    sets.iter()
        .rev()
        .map(|r| r.rings().iter().map(|c| wound(c, reverse)).collect())
        .collect()
}

/// A ring nearer the middle of `inner`, if the tool round its edge would leave
/// some of it uncut: an offset inward as far as leaves anything, found by
/// halving. `None` if the tool round its edge reaches everything in it.
fn middle(inner: &Region, params: &PlanarParams) -> Option<Region> {
    if inner.offset(-params.radius, params.arc).is_empty() {
        return None;
    }
    let (mut lo, mut hi) = (0.0, params.stepover);
    let mut best = None;
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        let r = inner.offset(-mid, params.arc);
        if r.is_empty() {
            hi = mid;
        } else {
            lo = mid;
            best = Some(r);
        }
    }
    best
}

/// `region`'s connected pieces: each outer ring with the holes directly inside it.
fn pieces(region: &Region) -> Vec<Region> {
    let rings = region.rings();
    let outers: Vec<(usize, Region, f64)> = rings
        .iter()
        .enumerate()
        .filter(|(_, r)| r.signed_area() > 0.0)
        .map(|(i, r)| (i, Region::new(vec![r.clone()]), r.signed_area()))
        .collect();
    let mut groups: Vec<Vec<Contour>> = outers
        .iter()
        .map(|&(i, ..)| rings.get(i).into_iter().cloned().collect())
        .collect();
    for hole in rings.iter().filter(|r| r.signed_area() <= 0.0) {
        let Some(&p) = hole.points().first() else {
            continue;
        };
        // The smallest outer round it is the one it is a hole of.
        let owner = outers
            .iter()
            .enumerate()
            .filter(|(_, (_, o, _))| o.contains(p))
            .min_by(|a, b| a.1.2.total_cmp(&b.1.2))
            .map(|(k, _)| k);
        if let Some(group) = owner.and_then(|k| groups.get_mut(k)) {
            group.push(hole.clone());
        }
    }
    groups.into_iter().map(Region::new).collect()
}

/// Whether two regions' bounds overlap: a quick test before an exact one.
fn overlapping(a: &Region, b: &Region) -> bool {
    match (a.bounds(), b.bounds()) {
        (Some((alo, ahi)), Some((blo, bhi))) => {
            alo.x <= bhi.x && blo.x <= ahi.x && alo.y <= bhi.y && blo.y <= ahi.y
        }
        _ => false,
    }
}

/// A ring's points as its region winds it, or the other way round.
fn wound(ring: &Contour, reverse: bool) -> Vec<DVec2> {
    let mut points = ring.points().to_vec();
    if reverse {
        points.reverse();
    }
    points
}

/// The order to cut closed `rings` in: never one before every ring inside it, and
/// of those that may go next, the nearest to where the last one started and
/// ended. Ties go to the earlier ring.
fn inside_first(rings: &[&[DVec2]]) -> Vec<usize> {
    let filled: Vec<Option<Region>> = rings
        .iter()
        .map(|r| Contour::new(r.to_vec()).map(|c| Region::new(vec![c])))
        .collect();
    // For each ring, the rings inside it.
    let inside: Vec<Vec<usize>> = filled
        .iter()
        .enumerate()
        .map(|(i, outer)| {
            let Some(outer) = outer else {
                return Vec::new();
            };
            rings
                .iter()
                .enumerate()
                .filter(|&(j, r)| {
                    j != i
                        && filled.get(j).and_then(Option::as_ref).is_some_and(|inner| {
                            overlapping(outer, inner)
                                && r.first().is_some_and(|&p| outer.contains(p))
                        })
                })
                .map(|(j, _)| j)
                .collect()
        })
        .collect();
    let mut done = vec![false; rings.len()];
    let mut order = Vec::with_capacity(rings.len());
    let mut at: Option<DVec2> = None;
    while order.len() < rings.len() {
        let ready: Vec<usize> = (0..rings.len())
            .filter(|&i| {
                !done.get(i).copied().unwrap_or(true)
                    && inside
                        .get(i)
                        .is_some_and(|js| js.iter().all(|&j| done.get(j).copied().unwrap_or(true)))
            })
            .collect();
        // Rings cannot each lie inside the other, but should rounding say they
        // do, the rest go in order rather than never.
        let ready = if ready.is_empty() {
            (0..rings.len())
                .filter(|&i| !done.get(i).copied().unwrap_or(true))
                .take(1)
                .collect()
        } else {
            ready
        };
        let i = ready
            .get(nearest(&ready, at, |&i| {
                rings.get(i).map(|r| r.to_vec()).unwrap_or_default()
            }))
            .copied()
            .unwrap_or(0);
        if let Some(d) = done.get_mut(i) {
            *d = true;
        }
        order.push(i);
        at = rings.get(i).and_then(|r| nearest_point(r, at?)).or(at);
    }
    order
}

/// Which of `items` (by place in it) comes nearest to `at`, the points of each
/// given by `points`; the first when there is nowhere yet. Ties go to the
/// earlier.
fn nearest<T>(items: &[T], at: Option<DVec2>, points: impl Fn(&T) -> Vec<DVec2>) -> usize {
    let Some(at) = at else {
        return 0;
    };
    let mut best = (0, f64::MAX);
    for (k, item) in items.iter().enumerate() {
        let d = nearest_point(&points(item), at).map_or(f64::MAX, |p| p.distance(at));
        if d < best.1 {
            best = (k, d);
        }
    }
    best.0
}

/// The point of closed ring `ring` nearest `at`.
fn nearest_point(ring: &[DVec2], at: DVec2) -> Option<DVec2> {
    closest_on(ring, at).map(|(_, p)| p)
}

/// Where closed ring `ring` comes nearest `at`: the segment it is on, from that
/// point, and the point.
fn closest_on(ring: &[DVec2], at: DVec2) -> Option<(usize, DVec2)> {
    let n = ring.len();
    let mut best: Option<(usize, DVec2, f64)> = None;
    for i in 0..n {
        let (Some(&a), Some(&b)) = (ring.get(i), ring.get((i + 1) % n)) else {
            continue;
        };
        let ab = b - a;
        let t = if ab.length_squared() > 0.0 {
            ((at - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let p = a + ab * t;
        let d = p.distance_squared(at);
        if best.is_none_or(|(.., bd)| d < bd) {
            best = Some((i, p, d));
        }
    }
    best.map(|(i, p, _)| (i, p))
}

/// Closed ring `ring`, starting where it comes nearest `at`: a point put in there
/// if it is not already one of its own.
fn starting_near(ring: &[DVec2], at: Option<DVec2>) -> Vec<DVec2> {
    let Some((i, p)) = at.and_then(|at| closest_on(ring, at)) else {
        return ring.to_vec();
    };
    let n = ring.len();
    let next = (i + 1) % n;
    let mut out = Vec::with_capacity(n + 1);
    let near = |q: Option<&DVec2>| q.is_some_and(|q| q.distance(p) < 1e-6);
    // Start at p, then round from the segment's far end back to its near one.
    let start = if near(ring.get(i)) {
        i
    } else if near(ring.get(next)) {
        next
    } else {
        out.push(p);
        next
    };
    for k in 0..n {
        if let Some(&q) = ring.get((start + k) % n) {
            out.push(q);
        }
    }
    out
}

/// A closed ring cut at height `z`: a link to its start, then round and back to it.
fn emit(
    ring: &[DVec2],
    z: f64,
    feed: f64,
    leads: Option<&(Vec<DVec2>, Vec<DVec2>)>,
    path: &mut Toolpath,
) -> Result<(), StrategyError> {
    let Some(first) = ring.first() else {
        return Ok(());
    };
    let (lead_in, lead_out) =
        leads.map_or((&[][..], &[][..]), |(i, o)| (i.as_slice(), o.as_slice()));
    // The strategy does not plan how to arrive; the linker does. With a lead, it
    // arrives where the lead starts, and goes round it onto the ring.
    let arrive = lead_in.first().unwrap_or(first);
    path.feed_to(dvec3(arrive.x, arrive.y, z), MoveRole::Link, feed)?;
    if !lead_in.is_empty() {
        for p in lead_in.iter().skip(1).chain(std::iter::once(first)) {
            path.feed_to(dvec3(p.x, p.y, z), MoveRole::Lead, feed)?;
        }
    }
    for p in ring.iter().skip(1).chain(std::iter::once(first)) {
        path.feed_to(dvec3(p.x, p.y, z), MoveRole::Cut, feed)?;
    }
    for p in lead_out {
        path.feed_to(dvec3(p.x, p.y, z), MoveRole::Lead, feed)?;
    }
    Ok(())
}

fn region_err(e: RegionError) -> StrategyError {
    StrategyError::Region(e.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use glam::dvec2;

    fn square(lo: f64, hi: f64) -> Contour {
        Contour::new(vec![
            dvec2(lo, lo),
            dvec2(hi, lo),
            dvec2(hi, hi),
            dvec2(lo, hi),
        ])
        .unwrap()
    }

    /// Levels as a 2D operation's are without a top or bottom asked for: from
    /// zero down to the lowest pick, or to `fallback` with none.
    fn floor(fallback: f64) -> impl Fn(Option<Picks>) -> (f64, f64) {
        move |p| (0.0, p.map_or(fallback, |p| p.lowest))
    }

    fn params() -> PlanarParams {
        PlanarParams {
            stepdown: Some(2.0),
            stepover: 2.0,
            feed: 1000.0,
            arc: 0.01,
            radius: 1.5,
            kept_on_right: true,
            leads: None,
        }
    }

    /// The cutting moves of a path, grouped into passes.
    fn passes(path: &Toolpath) -> Vec<Vec<DVec3>> {
        let mut out: Vec<Vec<DVec3>> = Vec::new();
        for m in path.moves() {
            if m.role == MoveRole::Link {
                out.push(vec![m.to]);
            } else if let Some(p) = out.last_mut() {
                p.push(m.to);
            }
        }
        out
    }

    fn signed_area(pass: &[DVec3]) -> f64 {
        pass.iter()
            .zip(pass.iter().skip(1))
            .map(|(a, b)| a.x * b.y - b.x * a.y)
            .sum::<f64>()
            * 0.5
    }

    /// A profile with leads arrives on each ring along an arc in the waste,
    /// tangent to the ring where it joins it, and leaves along another: outside
    /// a part cut out, inside a hole. Where the waste has no room even for a
    /// quarter of the arc, the ring goes without, and says so; a groove has none.
    #[test]
    fn leads_arc_on_and_off_in_the_waste() {
        let part = Region::new(vec![square(0.0, 40.0)]);
        let lead = LeadArc {
            radius: 4.0,
            sweep: 90f64.to_radians(),
            straight: 1.0,
        };
        let p = PlanarParams {
            leads: Some(lead),
            ..params()
        };
        for (side, offset) in [(ProfileSide::Outside, 1.5), (ProfileSide::Inside, -1.5)] {
            let areas = areas(&part, offset, 0.01, &[], &floor(-1.0)).unwrap();
            let region = &areas[0].region;
            let (path, report) = profile(&areas, side, &p).unwrap();
            assert_eq!(report.no_room_to_lead, 0, "{side:?}");
            let moves = path.moves();
            let first_cut = moves.iter().position(|m| m.role == MoveRole::Cut).unwrap();
            // Link, then the lead in, then the ring.
            assert_eq!(moves[0].role, MoveRole::Link);
            assert!(moves[1..first_cut].iter().all(|m| m.role == MoveRole::Lead));
            assert!(first_cut > 3, "{side:?}: a lead of several chords");
            for m in moves.iter().filter(|m| m.role == MoveRole::Lead) {
                let q = m.to.truncate();
                let ring = region.rings()[0].points();
                let on_ring = ring.iter().zip(ring.iter().cycle().skip(1)).any(|(a, b)| {
                    let t = ((q - *a).dot(*b - *a) / b.distance_squared(*a)).clamp(0.0, 1.0);
                    q.distance(*a + (*b - *a) * t) < 1e-9
                });
                let inside = region.contains(q);
                assert!(
                    on_ring || inside == (side == ProfileSide::Inside),
                    "{side:?}: a lead point at {q} is not in the waste"
                );
            }
            // Joining the ring along it: the lead's last chord heads the way
            // the ring's first cut does.
            let join = moves[first_cut - 1].to;
            let before = moves[first_cut - 2].to;
            let along = (moves[first_cut].to - join).truncate().normalize();
            let arriving = (join - before).truncate().normalize();
            assert!(
                along.dot(arriving) > 0.99,
                "{side:?}: {along} vs {arriving}"
            );
            // Left along a lead too, after the ring closes.
            assert_eq!(moves.last().unwrap().role, MoveRole::Lead);
        }

        // A 4 mm hole cut from inside: no room for even a 1 mm arc.
        let hole = Region::new(vec![square(0.0, 4.0)]);
        let small = areas(&hole, -1.5, 0.01, &[], &floor(-1.0)).unwrap();
        let (path, report) = profile(&small, ProfileSide::Inside, &p).unwrap();
        assert_eq!(report.no_room_to_lead, 1);
        assert!(path.moves().iter().all(|m| m.role != MoveRole::Lead));

        let on = areas(&part, 0.0, 0.01, &[], &floor(-1.0)).unwrap();
        let (path, report) = profile(&on, ProfileSide::On, &p).unwrap();
        assert_eq!(report.no_room_to_lead, 0);
        assert!(path.moves().iter().all(|m| m.role != MoveRole::Lead));
    }

    #[test]
    fn levels_are_even_and_end_at_the_bottom() {
        assert_eq!(
            levels(0.0, -5.0, Some(2.0)),
            vec![-5.0 / 3.0, -10.0 / 3.0, -5.0]
        );
        assert_eq!(levels(0.0, -4.0, Some(2.0)), vec![-2.0, -4.0]);
        assert_eq!(levels(0.0, -1.0, Some(2.0)), vec![-1.0]);
        assert_eq!(levels(0.0, -5.0, None), vec![-5.0], "the bottom alone");
        assert!(levels(0.0, 0.0, Some(2.0)).is_empty());
        assert!(levels(0.0, 1.0, None).is_empty());
    }

    /// Cutting a part out: its hole first, then its outline, each all the way
    /// down, climbing: clockwise round the outside, anticlockwise round the hole.
    #[test]
    fn a_profile_cuts_the_holes_before_the_outline() {
        let part = Region::new(vec![square(0.0, 40.0), square(15.0, 25.0)]);
        let areas = areas(&part, 1.5, 0.01, &[], &floor(-3.0)).unwrap();
        assert_eq!(areas.len(), 1);
        let (path, report) = profile(&areas, ProfileSide::Outside, &params()).unwrap();
        assert_eq!(report.rings, 2);
        assert_eq!(report.levels, 2);
        assert_eq!(report.passes, 4);
        let passes = passes(&path);
        assert_eq!(passes.len(), 4);
        // The hole, at both levels, then the outline.
        for p in &passes[..2] {
            assert!(p.iter().all(|q| q.x > 10.0 && q.x < 30.0));
            assert!(signed_area(p) > 0.0, "the hole is climbed anticlockwise");
        }
        for p in &passes[2..] {
            assert!(p.iter().any(|q| q.x < 0.0));
            assert!(signed_area(p) < 0.0, "the outline is climbed clockwise");
        }
        assert_eq!(passes[0][0].z, -1.5);
        assert_eq!(passes[1][0].z, -3.0);
        // The tool's side on the part's edge, to the arc tolerance.
        let left = passes[2].iter().map(|q| q.x).fold(f64::MAX, f64::min);
        assert!((left + 1.5).abs() < 0.02, "{left}");
    }

    /// Conventional cutting goes the other way round.
    #[test]
    fn conventional_reverses_a_profile() {
        let part = Region::new(vec![square(0.0, 40.0)]);
        let areas = areas(&part, 1.5, 0.01, &[], &floor(-1.0)).unwrap();
        let p = PlanarParams {
            kept_on_right: false,
            ..params()
        };
        let (path, _) = profile(&areas, ProfileSide::Outside, &p).unwrap();
        assert!(signed_area(&passes(&path)[0]) > 0.0);
        let (path, _) = profile(&areas, ProfileSide::Inside, &params()).unwrap();
        assert!(signed_area(&passes(&path)[0]) > 0.0);
    }

    /// A part inside another part's hole is cut out before that hole frees it.
    #[test]
    fn a_part_inside_a_hole_is_cut_first() {
        let rings = [square(0.0, 100.0), square(20.0, 80.0), square(40.0, 60.0)];
        let areas: Vec<Area> = rings
            .iter()
            .map(|r| Area {
                region: Region::new(vec![r.clone()]),
                top: 0.0,
                bottom: -1.0,
            })
            .collect();
        let (path, _) = profile(&areas, ProfileSide::On, &params()).unwrap();
        let widths: Vec<f64> = passes(&path)
            .iter()
            .map(|p| {
                let xs = p.iter().map(|q| q.x);
                xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
            })
            .collect();
        assert_eq!(widths, vec![20.0, 60.0, 100.0]);
    }

    /// A pocket goes from its middle out, its last ring along the wall, and
    /// leaves nothing in the middle the tool does not reach.
    #[test]
    fn a_pocket_clears_from_the_middle_out() {
        let shape = Region::new(vec![square(0.0, 21.0)]);
        let areas = areas(&shape, -1.5, 0.01, &[], &floor(-2.0)).unwrap();
        let (path, report) = pocket(&areas, &params()).unwrap();
        assert_eq!(report.areas, 1);
        let passes = passes(&path);
        let widths: Vec<f64> = passes
            .iter()
            .map(|p| {
                let xs = p.iter().map(|q| q.x);
                xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
            })
            .collect();
        assert!(widths.windows(2).all(|w| w[0] < w[1]), "{widths:?}");
        // The wall ring, erring inward by the arc tolerance on each side.
        let last = widths.last().copied().unwrap();
        assert!(last <= 18.0 && last > 18.0 - 0.03, "{last}");
        // The innermost ring is within the tool's radius of the middle.
        let first = passes[0].iter().map(|q| q.x).fold(f64::MAX, f64::min);
        assert!(10.5 - first <= 1.5, "{first}");
        // Climbing: the wall on the right, anticlockwise.
        assert!(passes.iter().all(|p| signed_area(p) > 0.0));
        // Each ring starts where the last one came nearest: a stepover away, and
        // the arc tolerance the inner rings are shrunk by.
        for w in passes.windows(2) {
            let end = w[0].last().unwrap();
            let start = w[1][0];
            assert!(end.truncate().distance(start.truncate()) <= 2.0 + 0.02);
        }
    }

    /// Pockets picked at different depths are each cut to their own floor.
    #[test]
    fn each_area_goes_to_its_own_floor() {
        let shape = Region::new(vec![square(0.0, 10.0), square(20.0, 30.0)]);
        let floors = [dvec3(0.0, 0.0, -3.0), dvec3(25.0, 25.0, -6.0)];
        let areas = areas(&shape, -1.5, 0.01, &floors, &floor(-1.0)).unwrap();
        let mut bottoms: Vec<f64> = areas.iter().map(|a| a.bottom).collect();
        bottoms.sort_by(f64::total_cmp);
        assert_eq!(bottoms, vec![-6.0, -3.0]);
    }

    /// Two outlines closer together than the tool is wide make one ring round
    /// both, cut to the lower of their floors.
    #[test]
    fn outlines_the_tool_cannot_pass_between_merge() {
        let shape = Region::new(vec![square(0.0, 10.0), square(11.0, 21.0)]);
        let shape = Region::union_of(shape.rings()).unwrap();
        let floors = [dvec3(0.0, 0.0, -2.0), dvec3(21.0, 21.0, -4.0)];
        let areas = areas(&shape, 1.5, 0.01, &floors, &floor(0.0)).unwrap();
        assert_eq!(areas.len(), 1);
        assert_eq!(areas[0].bottom, -4.0);
    }

    #[test]
    fn a_floor_above_the_top_cuts_nothing() {
        let shape = Region::new(vec![square(0.0, 10.0)]);
        let areas = areas(&shape, 1.5, 0.01, &[], &floor(1.0)).unwrap();
        let (path, report) = profile(&areas, ProfileSide::Outside, &params()).unwrap();
        assert!(path.is_empty());
        assert_eq!(report.too_shallow, 1);
    }
}
