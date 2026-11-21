//! The field: each node's distance along the surface the tool rides from the
//! edges the rings start from, by fast marching ([`camcraft_geom::geodesic`]).
//!
//! Outside in, it is measured from the area's boundary where it crosses the
//! part and from every edge of the ground: where faces avoided begin, and
//! where the tool leaves the part. Inside out, where the ground has inner
//! edges (the area's holes, the edges of faces avoided), from those alone:
//! the rings grow outward from them, and where the tool leaves the part is
//! their far end, cut last.
//!
//! # Zones
//!
//! Where strips the tool rolls over a sharp edge along break the ground
//! ([`ROLLING`]), each piece of it either side is a zone with a field of its
//! own, cut one after the next (decision 0039). Inside out, from the innermost
//! out, each is measured from its inner edges: the area's holes on it, faces
//! avoided, and the far border of the strip inside it; its outer edge, the
//! near border of the strip round it or where the tool leaves the part, is
//! its far end. Outside in, from the outermost in, the mirror: a zone with one
//! inner edge, closed, and no hole of the area on it is measured from its
//! outer edges, and that inner edge is its far end. Any other zone is
//! measured from every edge of it, as the whole ground with no strip is;
//! inside out, inside the last smooth ring going in, its rings are laid out
//! from a spine instead ([`spine`]).
//!
//! The whole ground is one field, measured out from every inner edge of it or
//! from every edge, where a zone's is measured from its own: the same seeds
//! in another order, and the march breaks ties by seed, so the two are kept
//! apart.
//!
//! Down a cliff, where a grid edge falls steeper than 45° and bends inside
//! its cell, the edge is measured along the surface the tool rides, from
//! drops along it ([`guided::bent`]), not by the straight line that cuts its
//! corner (decision 0035).
//!
//! [`guided::bent`]: super::guided::bent
//! [`spine::seeds`]: super::spine::seeds
//! [`spine`]: super::spine

use std::collections::HashSet;

use camcraft_geom::contour::{Contour, Region};
use camcraft_geom::cut::{CutGrid, NO_BAND};
use camcraft_geom::geodesic::Seed;
use camcraft_geom::isoline::{self, IsoLine};
use camcraft_geom::trigrid::TriSurface;
use glam::DVec2;
use rayon::prelude::*;

use crate::cutter::surfaces::Seat;
use crate::strategy::field::{AreaGrid, OFF_PART};
use crate::strategy::nesting::Order;

use super::ground::{AVOIDED, Edge, Ground, MACHINED, ROLLING};
use super::{ScallopParams, ScallopReport};

/// The field of distances over a zone of the ground, or all of it.
pub(super) struct Field<'g> {
    /// The ground it is measured over.
    pub(super) ground: &'g Ground,
    /// Each node's distance, of the grid cut's: negative outside the area,
    /// infinite where unknown, as off the zone.
    pub(super) d: Vec<f64>,
    /// Whether it is measured out from the zone's inner edges alone: inside
    /// out, where there are any.
    pub(super) outward: bool,
    /// The zone's edges the rings start from: where faces avoided begin,
    /// and, unless measured out from those, where the tool leaves the part
    /// and the strips either side.
    pub(super) edges: Vec<Edge>,
    /// Measured out from the inner edges, its outer edges: where the tool
    /// leaves the part, or the near border of a strip round it; measured in
    /// toward a zone's one inner edge, that edge. The field's far end, cut
    /// last.
    pub(super) far_edges: Vec<Edge>,
    /// The band of the grid cut the zone is; `None`, all of the ground.
    band: Option<u32>,
    /// Whether the area's rings that lie on no band are cut with the zone's:
    /// the last zone's.
    last: bool,
}

/// The fields over the ground, a zone each, in the order they are cut.
pub(super) struct Fields<'g> {
    pub(super) zones: Vec<Field<'g>>,
    /// Measured out from the inner edges, each node's distance along the
    /// surface from the outer boundary.
    pub(super) beyond: Option<Vec<f64>>,
}

impl<'g> Field<'g> {
    /// The fields over `ground`, inside `area`, as `params` ask: one, or,
    /// where strips break it, one a zone.
    pub(super) fn zones(
        seat: &Seat<'_>,
        ground: &'g Ground,
        area: &Region,
        params: &ScallopParams,
        report: &mut ScallopReport,
    ) -> Fields<'g> {
        let (outer, holes): (Vec<Contour>, Vec<Contour>) = area
            .rings()
            .iter()
            .cloned()
            .partition(|r| r.signed_area() > 0.0);
        let edges = ground.edges();
        let zones = if edges.iter().any(|e| e.past == ROLLING) {
            zoned(seat, ground, area, &holes, edges, params.order)
        } else {
            vec![whole(seat, ground, area, &holes, edges, params.order)]
        };
        report.unreached = unreached(ground, area, &zones);
        // How far the outer boundary is, along the surface, which the outer
        // zone's ring finishes up to.
        let beyond = zones
            .iter()
            .any(|z| z.outward)
            .then(|| ground.measure(&feet(seat, &ground.on, &Region::new(outer))));
        Fields { zones, beyond }
    }

    /// Each node's distance along the surface from `seeds`, measured as the
    /// field is.
    pub(super) fn measure(&self, seeds: &[Seed]) -> Vec<f64> {
        self.ground.measure(seeds)
    }

    /// Whether the area's ring `ring` is cut with the zone's: where most of
    /// the nodes beside it are on its band, or, beside none of the ground,
    /// it is the last.
    pub(super) fn owns(&self, ring: &Contour) -> bool {
        let Some(band) = self.band else {
            return true;
        };
        let cut = &self.ground.cut;
        let mut count: Vec<(u32, usize)> = Vec::new();
        for (n, ..) in self
            .ground
            .on
            .boundary_feet(&Region::new(vec![ring.clone()]))
        {
            let b = cut.band_of(n);
            if b == NO_BAND {
                continue;
            }
            match count.iter_mut().find(|(c, _)| *c == b) {
                Some((_, k)) => *k += 1,
                None => count.push((b, 1)),
            }
        }
        match count
            .iter()
            .max_by_key(|(b, k)| (*k, std::cmp::Reverse(*b)))
        {
            Some(&(most, _)) => most == band,
            None => self.last,
        }
    }

    /// Whether node `n` of its grid is inside `area`.
    pub(super) fn inside(&self, area: &Region, n: u32) -> bool {
        inside(self.ground, area, n)
    }
}

/// The one field over all of `ground`, with `edges`, inside `area` with
/// `holes`, cut in `order`.
fn whole<'g>(
    seat: &Seat<'_>,
    ground: &'g Ground,
    area: &Region,
    holes: &[Contour],
    edges: Vec<Edge>,
    order: Order,
) -> Field<'g> {
    let inner = seeds_along(&ground.cut, |_, class| class == AVOIDED);
    let outward = order == Order::InsideOut && (!holes.is_empty() || !inner.is_empty());
    let seeds = if outward {
        // From the inner edges. Where the tool leaves the part the field
        // reaches exactly, its ring along it: a line near it is no sliver in
        // bits along the area's edge, and left out, it would leave the way
        // down to it, as steep as a square root, wider than a stepover.
        let mut seeds = feet(seat, &ground.on, &Region::new(holes.to_vec()));
        seeds.extend(&inner);
        seeds
    } else {
        let mut seeds = feet(seat, &ground.on, area);
        seeds.extend(&inner);
        seeds.extend(seeds_along(&ground.cut, |_, class| class == OFF_PART));
        seeds
    };
    let d = measured(ground, area, &seeds, outward);
    let (far_edges, edges) = edges
        .into_iter()
        .partition(|e| outward && e.past == OFF_PART);
    Field {
        ground,
        d,
        outward,
        edges,
        far_edges,
        band: None,
        last: true,
    }
}

/// A field for each band of `ground`, its `edges` broken by strips, inside
/// `area` with `holes`, in the order they are cut: inside out, from the
/// innermost, and outside in, from the outermost. Inside out, measured out
/// from the zone's inner edges, where it has any; outside in, in from its
/// outer edges toward its one inner edge, where that is closed and no hole of
/// the area is on the zone; from every edge of it otherwise. Each of the area's rings is cut
/// with the zone most of it lies beside, or, beside none, the last
/// ([`Field::owns`]).
fn zoned<'g>(
    seat: &Seat<'_>,
    ground: &'g Ground,
    area: &Region,
    holes: &[Contour],
    edges: Vec<Edge>,
    order: Order,
) -> Vec<Field<'g>> {
    let cut = &ground.cut;
    let band_at = |n: u32| cut.cut(n).map_or_else(|| cut.band_of(n), |c| c.bands.0);
    // A strip's far border is a hole of the ground round it.
    let inner = |e: &Edge| e.past == AVOIDED || (e.past == ROLLING && e.closed && !turns_left(e));
    let inner_nodes: HashSet<u32> = edges
        .iter()
        .filter(|e| e.past == ROLLING && inner(e))
        .flat_map(|e| e.nodes.iter().copied())
        .collect();
    let hole_feet = feet(seat, &ground.on, &Region::new(holes.to_vec()));
    let area_feet = feet(seat, &ground.on, area);
    let mut by_band: Vec<Vec<Edge>> = (0..cut.bands()).map(|_| Vec::new()).collect();
    for e in edges {
        if let Some(own) = usize::try_from(e.band)
            .ok()
            .and_then(|b| by_band.get_mut(b))
        {
            own.push(e);
        }
    }
    let extents = extents(ground);
    let mut zones: Vec<(f64, u32, Field<'g>)> = Vec::new();
    for (b, edges) in (0u32..).zip(by_band) {
        let inner_node =
            |n: u32, class: u32| class == AVOIDED || (class == ROLLING && inner_nodes.contains(&n));
        let mut seeds: Vec<Seed> = hole_feet
            .iter()
            .filter(|s| band_at(s.node) == b)
            .copied()
            .collect();
        let holed = !seeds.is_empty();
        seeds.extend(seeds_along(cut, |n, class| {
            band_at(n) == b && inner_node(n, class)
        }));
        let outward = order == Order::InsideOut && !seeds.is_empty();
        // Outside in, in toward the zone's one inner edge, where it is closed
        // and no hole of the area is on the zone: that edge its far end.
        let inward = (order == Order::OutsideIn && !holed)
            .then(|| one_closed(&edges, inner))
            .flatten();
        if !outward {
            seeds = area_feet
                .iter()
                .filter(|s| band_at(s.node) == b)
                .copied()
                .collect();
            seeds.extend(seeds_along(cut, |n, class| {
                band_at(n) == b && (inward.is_none() || !inner_node(n, class))
            }));
        }
        if seeds.is_empty() {
            continue;
        }
        let d = measured(ground, area, &seeds, outward);
        let (far_edges, edges): (Vec<_>, Vec<_>) = edges
            .into_iter()
            .enumerate()
            .partition(|&(i, ref e)| (outward && !inner(e)) || inward == Some(i));
        let (far_edges, edges) = (
            far_edges.into_iter().map(|(_, e)| e).collect(),
            edges.into_iter().map(|(_, e)| e).collect(),
        );
        let extent = extents
            .get(usize::try_from(b).unwrap_or(usize::MAX))
            .copied();
        zones.push((
            extent.unwrap_or(0.0),
            b,
            Field {
                ground,
                d,
                outward,
                edges,
                far_edges,
                band: Some(b),
                last: false,
            },
        ));
    }
    // Inside out, the innermost first: each zone lies in the hole of the
    // one round it, so its extent is less.
    zones.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    if order == Order::OutsideIn {
        zones.reverse();
    }
    if let Some((.., zone)) = zones.last_mut() {
        zone.last = true;
    }
    zones.into_iter().map(|(.., zone)| zone).collect()
}

/// Which of `edges` is the one `inner` says is, where there is one and it is
/// closed.
fn one_closed(edges: &[Edge], inner: impl Fn(&Edge) -> bool) -> Option<usize> {
    let mut found = edges.iter().enumerate().filter(|(_, e)| inner(e));
    match (found.next(), found.next()) {
        (Some((i, e)), None) if e.closed => Some(i),
        _ => None,
    }
}

/// Whether closed `edge` turns left round what it bounds, seen from above:
/// the ground inside it.
fn turns_left(edge: &Edge) -> bool {
    let flat: Vec<DVec2> = edge.points.iter().map(|p| p.truncate()).collect();
    super::turns_left(&flat)
}

/// The area seen from above of the box round each band of `ground`'s grid
/// cut.
fn extents(ground: &Ground) -> Vec<f64> {
    let cut = &ground.cut;
    let mut boxes =
        vec![(DVec2::INFINITY, DVec2::NEG_INFINITY); usize::try_from(cut.bands()).unwrap_or(0)];
    for n in 0..cut.grid().nodes() {
        let b = cut.band_of(n);
        if let (Some(p), Some(bx)) = (
            cut.grid().xy(n),
            usize::try_from(b).ok().and_then(|b| boxes.get_mut(b)),
        ) {
            *bx = (bx.0.min(p), bx.1.max(p));
        }
    }
    boxes
        .into_iter()
        .map(|(lo, hi)| (hi - lo).max(DVec2::ZERO).element_product())
        .collect()
}

/// Each node's distance along the surface of `ground` from `seeds`, measured
/// `outward` from inner edges or not. Measured in, it is negative outside
/// `area`, so only the lines inside it are passes; measured out, the field
/// runs on past the area's outer edge from inside it, and its lines are cut
/// there: it does not fall to 0 there.
fn measured(ground: &Ground, area: &Region, seeds: &[Seed], outward: bool) -> Vec<f64> {
    let mut d = ground.measure(seeds);
    if !outward {
        for (n, v) in (0u32..).zip(d.iter_mut()) {
            if !inside(ground, area, n) {
                *v = -*v;
            }
        }
    }
    d
}

/// Node `n`'s value in the field `d`, where it has one.
pub(super) fn at(d: &[f64], n: u32) -> Option<f64> {
    usize::try_from(n)
        .ok()
        .and_then(|i| d.get(i))
        .copied()
        .filter(|v| v.is_finite())
}

/// The line of `d` over `grid` at `level`, across `triangles`, where it is
/// the only one.
pub(super) fn only_line(
    grid: &impl TriSurface,
    d: &[f64],
    level: f64,
    triangles: impl IntoIterator<Item = u32>,
) -> Option<IsoLine> {
    let mut found = isoline::lines(grid, d, level, triangles);
    match (found.pop(), found.is_empty()) {
        (Some(line), true) => Some(line),
        _ => None,
    }
}

/// Seeds at each node within a cell's diagonal of `rings` where they cross the
/// part, at its exact distance to the nearest point of them, over the part,
/// one drop a node: the area's rings are seen from above, so each node is
/// measured to its foot on them, dropped onto the part, where a spine's seeds
/// ([`spine::seeds`]) are measured to a line already on it. Off the part the area's edge is no edge of the ground: it
/// ends where the tool leaves the part, found exactly, and a node up the steep
/// way down to there measured to the area's edge seen from above would be a
/// fraction of its way along the surface from it.
fn feet(seat: &Seat<'_>, on: &AreaGrid, rings: &Region) -> Vec<Seed> {
    on.boundary_feet(rings)
        .par_iter()
        .map_init(Vec::new, |scratch, &(n, _, foot)| {
            let rest = seat.drop_at_in(foot.x, foot.y, scratch)?;
            Some(Seed {
                node: n,
                distance: on.grid.point(n)?.distance(foot.extend(rest)),
            })
        })
        .flatten()
        .collect()
}

/// Seeds at no distance at each cut of the ground's edge, as a node of the
/// grid cut, that `keep` says to, by the class of what lies past it.
fn seeds_along(cut: &CutGrid, keep: impl Fn(u32, u32) -> bool) -> Vec<Seed> {
    let lattice = cut.grid().nodes();
    (lattice..)
        .zip(cut.cuts())
        .filter(|(n, c)| c.bands.1 == NO_BAND && keep(*n, cut.class(c.edge.1)))
        .map(|(node, _)| Seed {
            node,
            distance: 0.0,
        })
        .collect()
}

/// Whether node `n` of the ground's cut grid is inside `area`.
fn inside(ground: &Ground, area: &Region, n: u32) -> bool {
    match ground.cut.cut(n) {
        Some(c) => area.contains(c.at.truncate()),
        None => usize::try_from(n)
            .ok()
            .and_then(|i| ground.on.inside.get(i))
            .copied()
            .unwrap_or(false),
    }
}

/// How many nodes of the ground inside `area`, in a triangle of it, no zone's
/// field reached.
fn unreached(ground: &Ground, area: &Region, zones: &[Field<'_>]) -> u64 {
    let (cut, view) = (&ground.cut, ground.cut.view(None));
    (0..cut.grid().nodes())
        .filter(|&n| {
            inside(ground, area, n)
                && cut.class(n) == MACHINED
                && view.triangles_at(n).next().is_some()
                && !zones.iter().any(|z| at(&z.d, n).is_some())
        })
        .count() as u64
}
