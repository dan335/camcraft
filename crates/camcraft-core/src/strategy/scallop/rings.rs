//! The rings, in the tree they are cut by.
//!
//! The passes are the lines where the field is a whole number of stepovers
//! ([`camcraft_geom::isoline`]), and the edges it is measured from: the area's
//! boundary, and the ground's edges. The regions each level's lines bound
//! nest ([`nest`]), and the tree has a node for each region, holding its
//! lines; one for each region's top-up, a ring halfway up to a crest that
//! rises more than half a stepover past its last; and one for each piece of
//! the area with no region of its own. Each line bounds one region, so is
//! taken by one. The boundary's rings, and the ground's edges, go to the
//! level-0 region of the piece of the area they bound; measured out from the
//! inner edges, the outer boundary's rings and where the tool leaves the part
//! are the field's far end, each piece's a node of its own, cut last.

use camcraft_geom::contour::Region;
use camcraft_geom::cut::View;
use camcraft_geom::isoline::{self, IsoLine, nest};
use camcraft_geom::trigrid::TriSurface;
use glam::DVec2;

use crate::strategy::field::{count, level_lines};
use crate::strategy::slope::Sloped;

use super::ScallopReport;
use super::distance::Field;
use super::ground::Edge;

/// The most levels of rings: far past any real part.
const MAX_LEVELS: u32 = 100_000;

/// A node of the tree, before its lines are laid on the part.
#[derive(Default)]
pub(super) struct Plan {
    /// The field's lines, placed by it, height and all.
    pub(super) lines: Vec<Sloped>,
    /// The area's edge, placed seen from above.
    pub(super) edges: Vec<Sloped>,
    /// The ground's edges, where faces avoided begin and where the tool
    /// leaves the part, placed by the field as its lines are.
    pub(super) found: Vec<Sloped>,
    /// The nodes nested in it.
    pub(super) children: Vec<usize>,
    /// Whether it is a root, cut from where the tool is.
    pub(super) root: bool,
}

/// The tree's nodes, and those cut last, each a root of its own: the field's
/// far end.
pub(super) struct Planned {
    pub(super) plans: Vec<Plan>,
    pub(super) last: Vec<usize>,
}

/// The rings of `field` over `grid`, its cut grid, `step` apart, in their tree,
/// with the edges of `area` it is measured from.
pub(super) fn plan(
    grid: &View<'_>,
    field: &Field,
    area: &Region,
    step: f64,
    report: &mut ScallopReport,
) -> Planned {
    let (lines, mut regions) = levels(grid, &field.d, step, report);
    let mut plans = by_region(grid, lines, &mut regions);
    let last = with_edges(grid, field, area, &regions, &mut plans, report);
    Planned { plans, last }
}

/// The lines of `d` over `grid` at each whole `step`, level by level in
/// parallel, and the regions they bound: at level 0 none, the edges the field
/// is measured from taking their place.
fn levels(
    grid: &View<'_>,
    d: &[f64],
    step: f64,
    report: &mut ScallopReport,
) -> (Vec<Vec<IsoLine>>, Vec<isoline::Region>) {
    let top = d
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
    let levels = count((top / step).floor()).unwrap_or(0).min(MAX_LEVELS);
    let mut lines = level_lines(grid, d, step, levels);
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    let regions = nest(grid, d, step, &lines);
    report.levels = u64::from(levels);
    report.lines = lines.iter().map(|l| l.len() as u64).sum();
    report.top_ups = regions.iter().filter(|r| !r.top_up.is_empty()).count() as u64;
    (lines, regions)
}

/// A node for each of `regions`, with its lines of `lines`, and one for each
/// region's top-up, nested in it.
fn by_region(
    grid: &View<'_>,
    mut lines: Vec<Vec<IsoLine>>,
    regions: &mut [isoline::Region],
) -> Vec<Plan> {
    let mut plans: Vec<Plan> = regions
        .iter()
        .map(|r| Plan {
            lines: lines
                .get_mut(usize::try_from(r.level).unwrap_or(usize::MAX))
                .map(|all| {
                    r.lines
                        .iter()
                        .filter_map(|&i| all.get_mut(i).map(|l| sloped(grid, l)))
                        .collect()
                })
                .unwrap_or_default(),
            children: r.children.clone(),
            root: r.level == 0,
            ..Plan::default()
        })
        .collect();
    for (i, r) in regions.iter_mut().enumerate() {
        if r.top_up.is_empty() {
            continue;
        }
        let id = plans.len();
        plans.push(Plan {
            lines: r.top_up.iter_mut().map(|l| sloped(grid, l)).collect(),
            ..Plan::default()
        });
        if let Some(plan) = plans.get_mut(i) {
            plan.children.push(id);
        }
    }
    plans
}

/// The area's rings and the field's edges, each to the level-0 region of the
/// piece of `area` it is in, or a root of its own where the piece has none;
/// measured out from the inner edges, the outer rings and the far edges to a
/// node of their own a piece, cut last: those nodes.
fn with_edges(
    grid: &View<'_>,
    field: &Field,
    area: &Region,
    regions: &[isoline::Region],
    plans: &mut Vec<Plan>,
    report: &mut ScallopReport,
) -> Vec<usize> {
    let mut last = Vec::new();
    let found = |edges: &[Edge], piece: &Region| -> Vec<Sloped> {
        edges
            .iter()
            .filter(|e| {
                e.points
                    .first()
                    .is_some_and(|p| piece.contains(p.truncate()))
            })
            .map(|e| {
                let flat: Vec<DVec2> = e.points.iter().map(|p| p.truncate()).collect();
                boundary(grid, &flat, e.closed)
            })
            .collect()
    };
    for piece in area.pieces() {
        let ring = |outer: bool| -> Vec<Sloped> {
            piece
                .rings()
                .iter()
                .filter(|c| (!field.outward || c.signed_area() > 0.0) == outer)
                .map(|c| boundary(grid, c.points(), true))
                .collect()
        };
        let (home_rings, far_rings) = if field.outward {
            (ring(false), ring(true))
        } else {
            (ring(true), Vec::new())
        };
        let (home_found, far_found) =
            (found(&field.edges, &piece), found(&field.far_edges, &piece));
        report.lines +=
            (home_rings.len() + far_rings.len() + home_found.len() + far_found.len()) as u64;
        if !far_rings.is_empty() || !far_found.is_empty() {
            last.push(plans.len());
            plans.push(Plan {
                edges: far_rings,
                found: far_found,
                ..Plan::default()
            });
        }
        if home_rings.is_empty() && home_found.is_empty() {
            continue;
        }
        let home = regions
            .iter()
            .position(|r| r.level == 0 && grid.xy(r.rep).is_some_and(|p| piece.contains(p)));
        match home.and_then(|i| plans.get_mut(i)) {
            Some(plan) => {
                plan.edges.extend(home_rings);
                plan.found.extend(home_found);
            }
            None => plans.push(Plan {
                edges: home_rings,
                found: home_found,
                root: true,
                ..Plan::default()
            }),
        }
    }
    last
}

/// A line of the field, its points taken, with its triangles' slopes.
fn sloped(grid: &impl TriSurface, line: &mut IsoLine) -> Sloped {
    Sloped {
        own: vec![true; line.points.len()],
        points: std::mem::take(&mut line.points),
        closed: line.closed,
        slopes: line.triangles.iter().map(|&t| grid.slope(t)).collect(),
    }
}

/// A ring of the boundary, split so no segment is longer than the grid's pitch,
/// with the slope under the middle of each.
fn boundary(grid: &(impl TriSurface + Sync), ring: &[DVec2], closed: bool) -> Sloped {
    Sloped::split(ring, closed, grid.pitch(), |m, _| {
        grid.locate(m).and_then(|t| grid.slope(t))
    })
}
