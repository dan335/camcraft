//! Laying the parts on the grid, in several orders, and keeping the best.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;

use camcraft_geom::contour::Region;
use camcraft_geom::turn::Turn;
use rayon::prelude::*;

use super::grid::{Board, Grid, Mask};
use super::{ArrangeError, Direction, Outcome, Part, Sheet, check, verify};
use crate::control::{Stopped, Watch, share};

/// An order the parts are laid in, largest first by some measure of each as
/// it stands; parts that measure the same keep the order they were given in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Order {
    /// By area.
    Area,
    /// By the longest side of the box round it.
    Longest,
    /// By its width across the direction the sheet is used up in.
    Across,
}

/// The orders tried, in turn: where two lay as well, the earlier is kept.
pub(super) const ORDERS: [Order; 3] = [Order::Area, Order::Longest, Order::Across];

/// Where each of `parts` stands on `sheet`, in the order given, each an
/// outline among `shapes`, as many as fit; or why they could not be
/// arranged.
///
/// The parts are laid in each of several orders, and the layout kept that
/// places the most, then uses the least of the sheet's length, then was laid
/// first. It is checked exactly ([`verify`]) before it is given back.
/// `watch` is told how far it has got, and stops it.
///
/// Every stand moves its part across the sheet only, never up or down:
/// seating a part on the sheet is for the caller, who knows how tall it is.
pub fn pack(
    sheet: &Sheet,
    shapes: &[Region],
    parts: &[Part],
    watch: &Watch,
) -> Result<Vec<Outcome>, ArrangeError> {
    pack_in(sheet, shapes, parts, &ORDERS, watch)
}

/// [`pack`], trying only `orders`.
pub(super) fn pack_in(
    sheet: &Sheet,
    shapes: &[Region],
    parts: &[Part],
    orders: &[Order],
    watch: &Watch,
) -> Result<Vec<Outcome>, ArrangeError> {
    check(sheet, shapes, parts)?;
    watch.check()?;
    let grid = Grid::new(sheet);
    let masks = Masks::new(&grid, sheet.spacing, shapes, parts, watch)?;
    watch.report(0.1);

    // Orders that come out the same are laid once.
    let mut sequences: Vec<Vec<usize>> = Vec::new();
    for &order in orders {
        let s = sequence(order, shapes, parts, sheet.along);
        if !sequences.contains(&s) {
            sequences.push(s);
        }
    }
    let laying = watch.part(0.1, 0.9);
    let each = share(1, sequences.len() * parts.len());
    let layouts = sequences
        .par_iter()
        .map(|s| lay(&grid, &masks, s, &laying, each))
        .collect::<Result<Vec<Layout>, Stopped>>()?;
    let best = layouts
        .into_iter()
        .enumerate()
        .min_by_key(|(k, l)| (Reverse(l.placed), l.used, *k))
        .map(|(_, l)| l.outcomes)
        .unwrap_or_default();
    verify(sheet, shapes, parts, &best)?;
    watch.report(1.0);
    Ok(best)
}

/// The cells every part covers at each turn it may take: one mask for each
/// outline and turn, however many parts share them.
struct Masks {
    all: Vec<Mask>,
    /// Each part's masks, for its turns in order, by their place in `all`.
    of_part: Vec<Vec<usize>>,
}

impl Masks {
    fn new(
        grid: &Grid,
        spacing: f64,
        shapes: &[Region],
        parts: &[Part],
        watch: &Watch,
    ) -> Result<Self, ArrangeError> {
        let mut keys: Vec<(usize, Turn)> = Vec::new();
        let mut index: HashMap<(usize, Turn), usize> = HashMap::new();
        let of_part = parts
            .iter()
            .map(|p| {
                p.turns
                    .iter()
                    .map(|&t| {
                        *index.entry((p.shape, t)).or_insert_with(|| {
                            keys.push((p.shape, t));
                            keys.len() - 1
                        })
                    })
                    .collect()
            })
            .collect();

        // Each outline is grown once, and turned for each of its turns.
        let (growth, arc) = (grid.growth(spacing), grid.arc_tolerance());
        let grown: Vec<Option<Region>> = shapes
            .par_iter()
            .enumerate()
            .map(|(s, shape)| {
                let named = parts.iter().any(|p| p.shape == s);
                (named && !watch.stopped()).then(|| shape.offset(growth, arc))
            })
            .collect();
        watch.check()?;
        let all: Vec<Option<Mask>> = keys
            .par_iter()
            .map(|&(s, turn)| {
                let grown = grown.get(s)?.as_ref()?;
                if watch.stopped() {
                    return None;
                }
                Mask::new(grid, &grown.rotated(turn), turn)
            })
            .collect();
        watch.check()?;
        // An outline with area always covers a cell, so none is left out but
        // for a fault.
        let all = all
            .into_iter()
            .zip(&keys)
            .map(|(m, &(shape, _))| m.ok_or(ArrangeError::EmptyShape { shape }))
            .collect::<Result<_, _>>()?;
        Ok(Self { all, of_part })
    }
}

/// The parts in `order`, largest first, by their places.
fn sequence(order: Order, shapes: &[Region], parts: &[Part], along: Direction) -> Vec<usize> {
    let size = |shape: &Region| -> f64 {
        let (lo, hi) = shape.bounds().unwrap_or_default();
        let extent = hi - lo;
        match (order, along) {
            (Order::Area, _) => shape.area(),
            (Order::Longest, _) => extent.max_element(),
            (Order::Across, Direction::X) => extent.y,
            (Order::Across, Direction::Y) => extent.x,
        }
    };
    let sizes: Vec<f64> = parts
        .iter()
        .map(|p| shapes.get(p.shape).map_or(0.0, size))
        .collect();
    let of = |k: usize| sizes.get(k).copied().unwrap_or(0.0);
    let mut order: Vec<usize> = (0..parts.len()).collect();
    // Stable: those that measure the same stay in the order given.
    order.sort_by(|&a, &b| of(b).total_cmp(&of(a)));
    order
}

/// One order's layout.
struct Layout {
    outcomes: Vec<Outcome>,
    placed: usize,
    /// Columns of the sheet used.
    used: usize,
}

/// Where a mask goes, in the order places are preferred in: the sheet used
/// once it is there, its first column, its first cell across, and the place
/// of its turn among the part's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Place {
    used: usize,
    p: usize,
    q: usize,
    turn: usize,
}

/// The parts laid one at a time in `sequence`, each where it uses least of
/// the sheet, its turns searched at once.
fn lay(
    grid: &Grid,
    masks: &Masks,
    sequence: &[usize],
    watch: &Watch,
    each: f64,
) -> Result<Layout, Stopped> {
    let mut board = Board::new(grid.long(), grid.wide());
    let mut layout = Layout {
        outcomes: vec![Outcome::NoRoom; sequence.len()],
        placed: 0,
        used: 0,
    };
    for &k in sequence {
        watch.check()?;
        let mine: Vec<&Mask> = masks
            .of_part
            .get(k)
            .into_iter()
            .flatten()
            .filter_map(|&m| masks.all.get(m))
            .collect();
        let outcome = if mine.iter().any(|m| board.holds(m)) {
            let bound = AtomicU64::new(u64::MAX);
            let used = layout.used;
            let best = mine
                .par_iter()
                .enumerate()
                .filter_map(|(turn, mask)| {
                    let (p, q) = board.first_fit(mask, used, &bound, || watch.stopped())?;
                    let used = used.max(p + mask.long());
                    Some((Place { used, p, q, turn }, *mask))
                })
                .min_by_key(|&(place, _)| place);
            // A search stopped part way finds nothing, which is not no room.
            watch.check()?;
            match best {
                Some((place, mask)) => {
                    board.take(mask, place.p, place.q);
                    layout.used = place.used;
                    layout.placed += 1;
                    Outcome::Placed(grid.stand(mask, place.p, place.q))
                }
                None => Outcome::NoRoom,
            }
        } else {
            Outcome::TooLarge
        };
        if let Some(o) = layout.outcomes.get_mut(k) {
            *o = outcome;
        }
        watch.add(each);
    }
    Ok(layout)
}
