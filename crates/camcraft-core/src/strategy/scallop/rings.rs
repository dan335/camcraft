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
//!
//! Inside the last ring with no corner going in, the field's rings further
//! in give way to rings laid out from a spine ([`spine`]), one node's
//! sequence under that ring's region, listed from it in to the spine.
//!
//! Where the rings blend into the far edge ([`blend`]), the field's lines past
//! the last whole ring give way to the blended rings and the far edge, one
//! node's sequence under that ring's region. The rings below where the tool
//! leaves the part are a sequence too, a root of its own, listed from the
//! Bottom up to that line: cut first from the boundary in, and last from the
//! middle out. When each line and sequence is cut is the rule in
//! [`order`](super::order).

use camcraft_geom::contour::Region;
use camcraft_geom::cut::View;
use camcraft_geom::isoline::{self, IsoLine, nest};
use camcraft_geom::trigrid::TriSurface;
use glam::DVec2;

use crate::cutter::surfaces::Seat;
use crate::cutter::tool::ToolProfile;
use crate::strategy::field::{OFF_PART, count, level_lines};
use crate::strategy::nesting::Order;
use crate::strategy::parallel::Run;
use crate::strategy::slope::Sloped;

use super::blend::{self, Band};
use super::distance::Field;
use super::ground::Edge;
use super::order::Side;
use super::spine;
use super::{ScallopParams, ScallopReport};

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
    /// Rings already on the part: below where the tool leaves it.
    pub(super) placed: Vec<Run>,
    /// The nodes nested in it.
    pub(super) children: Vec<usize>,
    /// Whether it is a root, cut from where the tool is.
    pub(super) root: bool,
    /// Rings that are no region's, cut one after another, each its own
    /// plan's lines, listed from the boundary in.
    pub(super) sequence: Vec<Plan>,
}

/// The tree's nodes; those cut before every root, each a root of its own;
/// and those cut after them. The side of its regions that its field's lines
/// lie on, where the field says: every one a hole's, measured out from the
/// inner edges.
pub(super) struct Planned {
    pub(super) plans: Vec<Plan>,
    pub(super) first: Vec<usize>,
    pub(super) last: Vec<usize>,
    pub(super) side: Side,
}

/// A field's lines at each whole stepover, level by level, and the regions
/// they bound.
pub(super) struct Levels {
    lines: Vec<Vec<IsoLine>>,
    regions: Vec<isoline::Region>,
    /// How many levels.
    count: u32,
}

impl Levels {
    /// The lines of `field` over `grid` at each whole `step`, level by level
    /// in parallel, and the regions they bound: at level 0 none, the edges the
    /// field is measured from taking their place.
    pub(super) fn of(grid: &View<'_>, field: &Field<'_>, step: f64) -> Self {
        let top = field
            .d
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold(0.0, f64::max);
        let count = count((top / step).floor()).unwrap_or(0).min(MAX_LEVELS);
        let mut lines = level_lines(grid, &field.d, step, count);
        if lines.is_empty() {
            lines.push(Vec::new());
        }
        let regions = nest(grid, &field.d, step, &lines);
        Self {
            lines,
            regions,
            count,
        }
    }

    /// Where `band` goes: the region of its last whole ring that it holds,
    /// its home. `None` where there is none, and it is no band.
    pub(super) fn home(&self, band: &Band) -> Option<usize> {
        self.regions
            .iter()
            .position(|r| r.level == band.level && band.holds(r.rep))
    }
}

/// The rings of `field` over `grid`, its cut grid, in their tree, from its
/// `levels`, with the edges of `area` it is measured from, as `params` ask,
/// for the tool `seat` holds; `bands` its band into each of its far edges and
/// that band's home ([`Levels::home`]), where it has one.
pub(super) fn plan(
    seat: &Seat<'_>,
    grid: &View<'_>,
    (field, area): (&Field<'_>, &Region),
    (levels, bands): (Levels, Vec<Option<(Band, usize)>>),
    params: &ScallopParams,
    report: &mut ScallopReport,
) -> Planned {
    let step = params.stepover;
    let Levels {
        lines,
        mut regions,
        count,
    } = levels;
    report.levels = report.levels.max(u64::from(count));
    report.lines += lines.iter().map(|l| l.len() as u64).sum::<u64>();
    report.top_ups += regions.iter().filter(|r| !r.top_up.is_empty()).count() as u64;
    let mut plans = by_region(grid, lines, &mut regions);
    let diameter = 2.0 * seat.tool().max_radius();
    // Inside the last smooth ring of each chain, rings round its spine.
    for star in spine::last_smooth(&regions, &plans, step) {
        let ring = spine::ring_of(&plans, star).map(|l| l.points.clone());
        if let (Some(region), Some(ring)) = (regions.get(star), ring)
            && let Some(inside) = spine::inside(grid, field, (region, &ring), (step, diameter))
        {
            report.blended += inside.blended as u64;
            give_way(grid, &mut plans, star, inside);
        }
    }
    // The far edge the rings blend into, and the region of the last whole
    // ring before it: measured out from the inner edges, where the tool
    // leaves the part or the strip round them; in toward a zone's one inner
    // edge, that edge.
    let blended = bands
        .into_iter()
        .enumerate()
        .find_map(|(i, band)| band.map(|(band, home)| (i, band, home)));
    let far_edges: Vec<&Edge> = field
        .far_edges
        .iter()
        .enumerate()
        .filter(|(i, _)| blended.as_ref().is_none_or(|(b, ..)| b != i))
        .map(|(_, e)| e)
        .collect();
    let mut last = with_edges(
        grid,
        field,
        (area, &far_edges),
        &regions,
        &mut plans,
        report,
    );
    if let Some((i, band, home)) = blended
        && let Some(far) = field.far_edges.get(i)
    {
        report.blended += band.rings.len() as u64;
        into_band(
            grid,
            &regions,
            &mut plans,
            (band, home),
            (far, field.outward),
        );
    }
    let mut first = Vec::new();
    if let Some(root) = below(seat, field, params, &mut plans, report) {
        match params.order {
            Order::OutsideIn => first.push(root),
            Order::InsideOut => last.push(root),
        }
    }
    Planned {
        plans,
        first,
        last,
        side: if field.outward {
            Side::Hole
        } else {
            Side::Outer
        },
    }
}

/// Inside `R*`, region `star`, the field's rings further in give way to
/// those laid out from its spine (`inside`): a sequence under `star`, listed
/// from the boundary in, the rings in to the spine and the spine, an open
/// pass, last.
fn give_way(grid: &View<'_>, plans: &mut Vec<Plan>, star: usize, inside: spine::Inside) {
    let mut stack: Vec<usize> = plans
        .get(star)
        .map(|p| p.children.clone())
        .unwrap_or_default();
    while let Some(i) = stack.pop() {
        if let Some(plan) = plans.get_mut(i) {
            plan.lines.clear();
            stack.extend(std::mem::take(&mut plan.children));
        }
    }
    let flat: Vec<DVec2> = inside.spine.iter().map(|p| p.truncate()).collect();
    let spine = Plan {
        found: vec![boundary(grid, &flat, false)],
        ..Plan::default()
    };
    // The rings out from the spine, turned to go in to it.
    let mut sequence: Vec<Plan> = inside
        .rings
        .into_iter()
        .rev()
        .map(|mut ring| Plan {
            lines: vec![sloped(grid, &mut ring)],
            ..Plan::default()
        })
        .collect();
    sequence.push(spine);
    let id = push_sequence(plans, sequence);
    if let Some(plan) = plans.get_mut(star) {
        plan.children = id.into_iter().collect();
    }
}

/// `sequence` pushed onto `plans` as one node: its index, where it has any.
fn push_sequence(plans: &mut Vec<Plan>, sequence: Vec<Plan>) -> Option<usize> {
    if sequence.is_empty() {
        return None;
    }
    plans.push(Plan {
        sequence,
        ..Plan::default()
    });
    Some(plans.len() - 1)
}

/// The rings below where the tool leaves the part ([`rings_below`]), a
/// sequence listed from the Bottom up to that line: its node. And, where
/// there are any, the height the level rings start under, theirs.
fn below(
    seat: &Seat<'_>,
    field: &Field<'_>,
    params: &ScallopParams,
    plans: &mut Vec<Plan>,
    report: &mut ScallopReport,
) -> Option<usize> {
    let (rings, to) = rings_below(seat, field, params)?;
    report.blended += rings.len() as u64;
    report.walls_from = Some(to + params.stepover);
    let sequence = rings
        .into_iter()
        .rev()
        .map(|ring| Plan {
            placed: vec![ring],
            ..Plan::default()
        })
        .collect();
    push_sequence(plans, sequence)
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
/// measured out from the inner edges, the outer rings and `far_edges` to a
/// node of their own a piece, cut last: those nodes.
fn with_edges(
    grid: &View<'_>,
    field: &Field<'_>,
    (area, far_edges): (&Region, &[&Edge]),
    regions: &[isoline::Region],
    plans: &mut Vec<Plan>,
    report: &mut ScallopReport,
) -> Vec<usize> {
    let mut last = Vec::new();
    let home_edges: Vec<&Edge> = field.edges.iter().collect();
    let found = |edges: &[&Edge], piece: &Region, far: bool| -> Vec<Sloped> {
        edges
            .iter()
            .filter(|e| {
                e.points
                    .first()
                    .is_some_and(|p| piece.contains(p.truncate()))
            })
            .map(|e| line_along(grid, e, far))
            .collect()
    };
    for piece in area.pieces() {
        let ring = |outer: bool| -> Vec<Sloped> {
            piece
                .rings()
                .iter()
                .filter(|c| field.owns(c))
                .filter(|c| (!field.outward || c.signed_area() > 0.0) == outer)
                .map(|c| {
                    // The field rises toward an outer ring it is measured out to:
                    // turned round, it has the higher side on its left.
                    let mut points = c.points().to_vec();
                    if field.outward {
                        points.reverse();
                    }
                    boundary(grid, &points, true)
                })
                .collect()
        };
        let (home_rings, far_rings) = if field.outward {
            (ring(false), ring(true))
        } else {
            (ring(true), Vec::new())
        };
        let home_found = found(&home_edges, &piece, false);
        let far_found = found(far_edges, &piece, true);
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

/// `band`'s rings in place of the field's lines past its last whole ring:
/// each region past that level on the band, and its top-up, emptied, and a
/// sequence under the region of that ring, `home`, of the band's rings and
/// the far edge `far`, listed from the boundary in: from the far edge in
/// where it is the outside of the band, `far_outside`, as where the field
/// is measured out to it, and to it otherwise.
fn into_band(
    grid: &View<'_>,
    regions: &[isoline::Region],
    plans: &mut Vec<Plan>,
    (band, home): (Band, usize),
    (far, far_outside): (&Edge, bool),
) {
    for (i, r) in regions.iter().enumerate() {
        if r.level <= band.level || !band.holds(r.rep) {
            continue;
        }
        let Some(plan) = plans.get_mut(i) else {
            continue;
        };
        plan.lines.clear();
        // The region's children past the regions are its top-up's node.
        for c in std::mem::take(&mut plan.children) {
            if let Some(top_up) = plans.get_mut(c).filter(|_| c >= regions.len()) {
                top_up.lines.clear();
            }
        }
    }
    let mut sequence: Vec<Plan> = band
        .rings
        .into_iter()
        .map(|mut ring| Plan {
            lines: vec![sloped(grid, &mut ring)],
            ..Plan::default()
        })
        .collect();
    sequence.push(Plan {
        found: vec![line_along(grid, far, true)],
        ..Plan::default()
    });
    if far_outside {
        sequence.reverse();
    }
    let id = push_sequence(plans, sequence);
    if let Some(plan) = plans.get_mut(home) {
        plan.children = id.into_iter().collect();
    }
}

/// The rings below where the tool leaves the part, blended from that line
/// down to the first level ring ([`blend::below`]), each the way that line is
/// cut, and the height the level rings start at: where it is the one line
/// the tool leaves the part along and closed, the slope range reaches
/// upright, and the field reaches it.
fn rings_below(
    seat: &Seat<'_>,
    field: &Field<'_>,
    params: &ScallopParams,
) -> Option<(Vec<Run>, f64)> {
    if params.slope_to < std::f64::consts::FRAC_PI_2 {
        return None;
    }
    let mut leaving = field
        .edges
        .iter()
        .chain(&field.far_edges)
        .filter(|e| e.past == OFF_PART);
    let edge = leaving.next().filter(|e| e.closed)?;
    if leaving.next().is_some() {
        return None;
    }
    let (mut rings, to) =
        blend::below(seat, edge, params.stepover, params.bottom, params.tolerance)?;
    // A far edge is turned round to be cut as the field's lines are.
    if field.outward {
        for ring in &mut rings {
            ring.points.reverse();
        }
    }
    Some((rings, to))
}

/// The ground's edge `edge` as a line placed by the field: as it runs, the
/// ground on its left, or, a far edge, turned round, the field's higher side
/// on its left as its lines have, so it is cut the way they are.
fn line_along(grid: &View<'_>, edge: &Edge, far: bool) -> Sloped {
    let mut flat: Vec<DVec2> = edge.points.iter().map(|p| p.truncate()).collect();
    if far {
        flat.reverse();
    }
    boundary(grid, &flat, edge.closed)
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
