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
//! Down a cliff, where a grid edge falls steeper than 45° and bends inside
//! its cell, the edge is measured along the surface the tool rides, from
//! drops along it ([`guided::bent`]), not by the straight line that cuts its
//! corner (decision 0035).

use std::collections::HashMap;

use camcraft_geom::contour::{Contour, Region};
use camcraft_geom::cut::{CutGrid, NO_BAND, View};
use camcraft_geom::geodesic::{Seed, march_along};
use camcraft_geom::trigrid::TriSurface;
use rayon::prelude::*;

use crate::cutter::surfaces::Seat;
use crate::strategy::StrategyError;
use crate::strategy::field::{AreaGrid, OFF_PART};
use crate::strategy::nesting::Order;

use super::ground::{AVOIDED, Edge, Ground, MACHINED};
use super::{ScallopParams, ScallopReport, guided};

/// The field of distances over the ground.
pub(super) struct Field {
    /// The ground it is measured over.
    pub(super) ground: Ground,
    /// Each node's distance, of the grid cut's: negative outside the area,
    /// infinite where unknown.
    pub(super) d: Vec<f64>,
    /// Measured out from the inner edges, each node's distance along the
    /// surface from the outer boundary.
    pub(super) beyond: Option<Vec<f64>>,
    /// Whether it is measured out from the ground's inner edges alone: inside
    /// out, where there are any.
    pub(super) outward: bool,
    /// The ground's edges the rings start from: where faces avoided begin,
    /// and, unless measured out from those, where the tool leaves the part.
    pub(super) edges: Vec<Edge>,
    /// Measured out from the inner edges, where the tool leaves the part: the
    /// field's far end, cut last.
    pub(super) far_edges: Vec<Edge>,
    /// The highest point of where the tool leaves the part from above.
    pub(super) leaves_at: Option<f64>,
    /// The length along the surface of each grid edge bent down a cliff
    /// ([`cliff_edges`]), by its key, lower node first.
    cliffs: HashMap<(u32, u32), f64>,
}

impl Field {
    /// The field over `area`, whose bounds are `bounds`, as `params` ask.
    pub(super) fn build(
        seat: &Seat<'_>,
        area: &Region,
        bounds: (glam::DVec2, glam::DVec2),
        params: &ScallopParams,
        report: &mut ScallopReport,
    ) -> Result<Self, StrategyError> {
        let ground = Ground::sampled(seat, area, bounds, params, report)?;
        let view = ground.cut.view(None);
        let cliffs = cliff_edges(seat, &view, params.tolerance);
        params.watch.check()?;
        let measure = |seeds: &[Seed]| march(&view, &cliffs, seeds);
        let (outer, holes): (Vec<Contour>, Vec<Contour>) = area
            .rings()
            .iter()
            .cloned()
            .partition(|r| r.signed_area() > 0.0);
        let inner = seeds_along(&ground.cut, AVOIDED);
        let outward = params.order == Order::InsideOut && (!holes.is_empty() || !inner.is_empty());
        let (mut d, beyond) = if outward {
            // From the inner edges; and how far the outer boundary is, along
            // the surface, which its ring finishes up to. Where the tool
            // leaves the part the field reaches exactly, its ring along it: a
            // line near it is no sliver in bits along the area's edge, and
            // left out, it would leave the way down to it, as steep as a
            // square root, wider than a stepover.
            let mut seeds = feet(seat, &ground.on, &Region::new(holes));
            seeds.extend(&inner);
            let d = measure(&seeds);
            (
                d,
                Some(measure(&feet(seat, &ground.on, &Region::new(outer)))),
            )
        } else {
            let mut seeds = feet(seat, &ground.on, area);
            seeds.extend(&inner);
            seeds.extend(&seeds_along(&ground.cut, OFF_PART));
            (measure(&seeds), None)
        };
        report.unreached = unreached(&ground, area, &d);
        // Outside the area, negative, so only the lines inside it are passes.
        // Measured out from the inner edges, the field runs on past the
        // area's outer edge from inside it, and its lines are cut there: it
        // does not fall to 0 there.
        if !outward {
            for (n, v) in (0u32..).zip(d.iter_mut()) {
                if !inside(&ground, area, n) {
                    *v = -*v;
                }
            }
        }
        let (far_edges, edges) = ground
            .edges()
            .into_iter()
            .partition(|e| outward && e.past == OFF_PART);
        let leaves_at = ground.leaves_at(area);
        Ok(Self {
            ground,
            d,
            beyond,
            outward,
            edges,
            far_edges,
            leaves_at,
            cliffs,
        })
    }

    /// Each node's distance along the surface from `seeds`, measured as the
    /// field is.
    pub(super) fn measure(&self, seeds: &[Seed]) -> Vec<f64> {
        march(&self.ground.cut.view(None), &self.cliffs, seeds)
    }

    /// Whether node `n` of its grid is inside `area`.
    pub(super) fn inside(&self, area: &Region, n: u32) -> bool {
        inside(&self.ground, area, n)
    }
}

/// Each node of `view`'s distance along the surface from `seeds`, down each
/// of `cliffs` along the surface the tool rides.
fn march(view: &View<'_>, cliffs: &HashMap<(u32, u32), f64>, seeds: &[Seed]) -> Vec<f64> {
    march_along(
        view,
        seeds,
        |n| view.point(n),
        |a, b| cliffs.get(&(a.min(b), a.max(b))).copied(),
    )
}

/// Seeds at each node within a cell's diagonal of `rings` where they cross the
/// part, at its exact distance to the nearest point of them, over the part,
/// one drop a node. Off the part the area's edge is no edge of the ground: it
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

/// Seeds at no distance at each cut of the ground's edge with `class` past
/// it: where faces avoided begin, or where the tool leaves the part.
fn seeds_along(cut: &CutGrid, class: u32) -> Vec<Seed> {
    let lattice = cut.grid().nodes();
    cut.cuts()
        .iter()
        .enumerate()
        .filter(|(_, c)| c.bands.1 == NO_BAND && cut.class(c.edge.1) == class)
        .map(|(i, _)| Seed {
            node: lattice + u32::try_from(i).unwrap_or(0),
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

/// How many nodes of the ground inside `area`, in a triangle of it, the field
/// `d` never reached.
fn unreached(ground: &Ground, area: &Region, d: &[f64]) -> u64 {
    let (cut, view) = (&ground.cut, ground.cut.view(None));
    (0..cut.grid().nodes())
        .filter(|&n| {
            inside(ground, area, n)
                && cut.class(n) == MACHINED
                && view.triangles_at(n).next().is_some()
                && !usize::try_from(n)
                    .ok()
                    .and_then(|i| d.get(i))
                    .is_some_and(|v| v.is_finite())
        })
        .count() as u64
}

/// The length along the surface the tool rides of each bent edge of `grid`
/// ([`guided::bent`]), by its key, lower node first: the straight line
/// between the nodes cuts the corner where a wall turns onto the ground above
/// or below it inside the cell, by up to the cell's width, and the rings
/// beside it would be that much too far apart.
fn cliff_edges(
    seat: &Seat<'_>,
    grid: &(impl TriSurface + Sync),
    tolerance: f64,
) -> HashMap<(u32, u32), f64> {
    let found: Vec<((u32, u32), f64)> = (0..grid.nodes())
        .into_par_iter()
        .map_init(Vec::new, |scratch, a| {
            let mut out = Vec::new();
            let Some(pa) = grid.point(a) else {
                return out;
            };
            let mut seen = Vec::new();
            for (_, nodes) in grid.triangles_at(a) {
                for b in nodes {
                    let Some(pb) = grid.point(b).filter(|_| b > a && !seen.contains(&b)) else {
                        continue;
                    };
                    seen.push(b);
                    if let Some(line) = guided::bent(seat, (pa, pb), tolerance, scratch) {
                        out.push(((a, b), guided::length(&line)));
                    }
                }
            }
            out
        })
        .flatten_iter()
        .collect();
    found.into_iter().collect()
}
