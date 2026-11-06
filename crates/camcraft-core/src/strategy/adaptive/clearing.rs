//! One level cleared: passes steered through what is left, each started
//! where the tool can stand, and linked.

use super::steer::{choose, clear_of, deepest};
use super::tiles::{Tiles, Window};
use super::*;

/// A level being cleared.
pub(super) struct Clearing<'a> {
    pub(super) p: &'a LevelParams,
    pub(super) allowed: &'a Region,
    /// Where the tool may be, in tiles.
    pub(super) room: Tiles,
    /// A little inside it, off its edges, in tiles: where the edge of the free
    /// region is material's, not a wall's.
    pub(super) inner: Tiles,
    /// All of `inner`, which never changes, put together once.
    pub(super) whole_inner: Region,
    /// Material still standing, less what `trail` has swept.
    pub(super) uncut: Tiles,
    /// The material worth a pass grown by the tool's radius, where the tool
    /// may not stand ([`Clearing::situation`]), tile by tile of `uncut`, each
    /// once worked out and until a cut comes within reach of it: growing the
    /// whole level's every time it is asked for is most of a large level's
    /// work.
    pub(super) grown: Vec<Option<Region>>,
    /// Swept since the material was last brought up to date, the last point
    /// where the tool is.
    pub(super) trail: Vec<DVec2>,
    pub(super) trail_length: f64,
    /// Room for the trail and the step being tried, kept from step to step.
    pub(super) trying: Vec<DVec2>,
    pub(super) window: Option<Window>,
    pub(super) passes: Vec<(Arrive, Vec<DVec2>)>,
    /// Which side of each pass its material is on.
    pub(super) sides: Vec<Side>,
    pub(super) helixes: Vec<(DVec2, f64)>,
    /// Where passes have started that cut nothing.
    pub(super) dead: Vec<DVec2>,
    pub(super) steps: usize,
    pub(super) left: f64,
    pub(super) no_room: u64,
    /// Whether closed areas were left, as it is not to machine cavities.
    pub(super) cavities: bool,
    pub(super) overloads: u64,
    /// Closed areas first cut a slot along their middle.
    pub(super) slots: u64,
    /// How much of the material the window holds every flush has taken out
    /// since it was last asked ([`Clearing::taken`]).
    taken: f64,
}

/// What a part of a level looks like, between passes.
pub(super) struct Situation {
    /// The box it covers, and within which it is exact: what is further in than
    /// the tool and a margin from its edges.
    pub(super) lo: DVec2,
    pub(super) hi: DVec2,
    pub(super) exact: f64,
    /// Where the tool may be.
    pub(super) allowed: Region,
    /// A little inside it.
    pub(super) inner: Region,
    /// Material still worth a pass.
    pub(super) remaining: Region,
    /// Where the tool stands clear of it.
    pub(super) free: Region,
}

impl Situation {
    /// Whether `p` is far enough inside for what is said of it to be so.
    pub(super) fn knows(&self, p: DVec2) -> bool {
        p.cmpge(self.lo + DVec2::splat(self.exact)).all()
            && p.cmple(self.hi - DVec2::splat(self.exact)).all()
    }
}

impl<'a> Clearing<'a> {
    pub(super) fn new(
        allowed: &'a Region,
        uncut: &Region,
        p: &'a LevelParams,
    ) -> Result<Self, StrategyError> {
        // What the tool cannot reach from where it may be, it never touches.
        let reach = allowed.offset(p.radius, p.arc);
        let uncut = uncut.intersection(&reach)?;
        let size = TILE_RADII * p.radius;
        // Each in tiles on its own, in parallel.
        let (room, (inner, uncut)) = rayon::join(
            || Tiles::new(allowed, size),
            || {
                rayon::join(
                    || {
                        let inner = Tiles::new(&allowed.offset(-2.0 * p.sliver, p.coarse), size)?;
                        let whole = inner.whole()?;
                        Ok::<_, StrategyError>((inner, whole))
                    },
                    || Tiles::new(&uncut, size),
                )
            },
        );
        let (inner, whole_inner) = inner?;
        let uncut = uncut?;
        Ok(Self {
            p,
            allowed,
            room: room?,
            inner,
            whole_inner,
            grown: vec![None; uncut.cells.len()],
            uncut,
            trail: Vec::new(),
            trail_length: 0.0,
            trying: Vec::new(),
            window: None,
            passes: Vec::new(),
            sides: Vec::new(),
            helixes: Vec::new(),
            dead: Vec::new(),
            steps: 0,
            left: 0.0,
            no_room: 0,
            cavities: false,
            overloads: 0,
            slots: 0,
            taken: 0.0,
        })
    }

    pub(super) fn run(&mut self) -> Result<(), StrategyError> {
        let mut at: Option<DVec2> = None;
        let mut futile = 0;
        for _ in 0..MAX_PASSES {
            self.p.watch.check()?;
            if self.steps >= MAX_STEPS || futile >= MAX_FUTILE {
                break;
            }
            // Near where the tool is, first, further, and only where nothing is
            // near, everywhere.
            let mut near = None;
            if let Some(from) = at {
                for half in [8.0 * self.p.radius, 32.0 * self.p.radius] {
                    let s = self.situation_near(from, half)?;
                    if let Some(found) = self.next_start(&s, Some(from))? {
                        near = Some((s, found));
                        break;
                    }
                }
            }
            let (s, found) = match near {
                Some((s, found)) => (s, Some(found)),
                None => {
                    let s = self.situation_everywhere()?;
                    if s.remaining.is_empty() {
                        break;
                    }
                    let found = self.next_start(&s, at)?;
                    (s, found)
                }
            };
            let (start, dir, side, arrive, entered) = match found {
                Some((start, dir, side)) => {
                    let arrive = match at.and_then(|from| self.stay_route(&s, from, start)) {
                        Some(via) => Arrive::Stay { via },
                        None => Arrive::Link,
                    };
                    (start, dir, side, arrive, false)
                }
                // Nothing left but crumbs, where an edge of where the tool
                // may be cuts across the material within its reach: what
                // is left, not an area to go down into.
                None if crumbs(&s.remaining, self.p.radius * 0.25) => break,
                // What is left has no way in from the side: a cavity, left
                // where it is not to go down into one.
                None if !self.p.machine_cavities => {
                    self.cavities = true;
                    break;
                }
                None => match self.enter(&s)? {
                    Some((start, dir, side, arrive)) => (start, dir, side, arrive, true),
                    None => {
                        self.no_room += 1;
                        break;
                    }
                },
            };
            // Gone down into a closed area: a slot along its middle first,
            // where it is to cut one and there is room.
            if let (Arrive::Enter { centre }, Some(width)) = (&arrive, self.p.slot_width)
                && let Some(points) = self.slot(&s, *centre, start, side, width)?
            {
                futile = 0;
                self.slots += 1;
                at = points.last().copied();
                self.passes.push((arrive, points));
                self.sides.push(side);
                continue;
            }
            let points = self.pass(start, dir, side, entered)?;
            if points.len() < 2 {
                futile += 1;
                self.dead.push(start);
                if entered {
                    // The helix took what was left here, and is cut alone.
                    at = Some(start);
                    self.passes.push((arrive, vec![start]));
                    self.sides.push(side);
                }
                continue;
            }
            futile = 0;
            at = points.last().copied();
            self.passes.push((arrive, points));
            self.sides.push(side);
        }
        // What is left worth a pass, as a situation would have it.
        self.flush()?;
        self.left = self
            .uncut
            .whole()?
            .offset(-self.p.sliver, self.p.coarse)
            .area();
        self.stand_off();
        Ok(())
    }

    /// Where the tool lifts to link to a pass, start it a little off the
    /// material, and where it lifts after one, end it so: the linker comes down
    /// at rapid, and goes up, only where the tool is clear of material, and a
    /// pass starts and ends against it. A third of the tool is clear of any
    /// cell the simulator has, whose pitch is at most a quarter of it.
    pub(super) fn stand_off(&mut self) {
        let off = self.p.radius / 3.0;
        let n = self.passes.len();
        for k in 0..n {
            let side = self.sides.get(k).copied().unwrap_or(Side::Right);
            let lifts_after = self
                .passes
                .get(k + 1)
                .is_none_or(|(a, _)| !matches!(a, Arrive::Stay { .. }));
            let Some((arrive, points)) = self.passes.get_mut(k) else {
                continue;
            };
            if *arrive == Arrive::Link
                && let (Some(&a), Some(&b)) = (points.first(), points.get(1))
                && let Some(d) = (b - a).try_normalize()
            {
                let before = a + side.away(d) * off;
                if self.allowed.contains_segment(before, a) {
                    points.insert(0, before);
                }
            }
            if lifts_after
                && let (Some(&b), Some(&a)) = (
                    points.last(),
                    points.len().checked_sub(2).and_then(|i| points.get(i)),
                )
                && let Some(d) = (b - a).try_normalize()
            {
                let after = b + side.away(d) * off;
                if self.allowed.contains_segment(b, after) {
                    points.push(after);
                }
            }
        }
    }

    /// The level as it stands everywhere.
    pub(super) fn situation_everywhere(&mut self) -> Result<Situation, StrategyError> {
        self.flush()?;
        let uncut = self.uncut.whole()?;
        let (lo, hi) = self
            .allowed
            .bounds()
            .map_or((DVec2::ZERO, DVec2::ZERO), |(lo, hi)| {
                let grow = DVec2::splat(self.p.radius + 1.0);
                (lo - grow, hi + grow)
            });
        let inner = self.whole_inner.clone();
        let grown = self.grown_whole()?;
        self.situation_with(
            (lo, hi),
            self.allowed.clone(),
            inner,
            &uncut,
            0.0,
            Some(grown),
        )
    }

    /// How far a tile's growth reads the material round it: the tool, the
    /// sliver it is shrunk by and grown back, and the slack it is grown loosely
    /// by, each to its arcs, and a margin.
    fn grown_reach(&self) -> f64 {
        self.p.radius + 4.0 * (self.p.sliver + self.p.coarse)
    }

    /// The material worth a pass grown by the tool's radius, over the whole
    /// level: each tile's as kept, those since cut within its reach worked
    /// out again, in parallel.
    fn grown_whole(&mut self) -> Result<Region, StrategyError> {
        let reach = self.grown_reach();
        let missing: Vec<usize> = (0..self.grown.len())
            .filter(|&k| self.grown.get(k).is_some_and(Option::is_none))
            .collect();
        let (uncut, p) = (&self.uncut, self.p);
        let made: Vec<(usize, Region)> = missing
            .par_iter()
            .map(|&k| {
                let (lo, hi) = uncut.square(k);
                let around = uncut.window(lo - DVec2::splat(reach), hi + DVec2::splat(reach))?;
                // Its own, and past the tiles where it is on their edge: the
                // material grows beyond them by the tool.
                let (own_lo, own_hi) = uncut.square_beyond(k, reach);
                let piece = around
                    .offset(-p.sliver, p.coarse)
                    .grown_loosely(p.radius + p.sliver, p.coarse, p.coarse)
                    .intersection(&Region::rectangle(own_lo, own_hi))?;
                Ok((k, piece))
            })
            .collect::<Result<_, StrategyError>>()?;
        for (k, piece) in made {
            if let Some(slot) = self.grown.get_mut(k) {
                *slot = Some(piece);
            }
        }
        let pieces: Vec<Region> = self
            .grown
            .iter()
            .flatten()
            .filter(|r| !r.is_empty())
            .cloned()
            .collect();
        Ok(Region::union_all(&pieces)?)
    }

    /// Take `cut` out of the material, and forget the growth of every tile
    /// within reach of it.
    fn cut_material(&mut self, cut: &Region) -> Result<(), StrategyError> {
        if let Some((lo, hi)) = cut.bounds() {
            let reach = DVec2::splat(self.grown_reach());
            for k in self.uncut.within(lo - reach, hi + reach) {
                if let Some(slot) = self.grown.get_mut(k) {
                    *slot = None;
                }
            }
        }
        self.uncut.subtract(cut)
    }

    /// The level as it stands in a box `half` each way of `at`.
    pub(super) fn situation_near(
        &mut self,
        at: DVec2,
        half: f64,
    ) -> Result<Situation, StrategyError> {
        self.flush()?;
        let (lo, hi) = (at - DVec2::splat(half), at + DVec2::splat(half));
        let uncut = self.uncut.window(lo, hi)?;
        let allowed = self.room.window(lo, hi)?;
        let inner = self.inner.window(lo, hi)?;
        // What lies within the tool and a margin of a point is all that is said
        // of it.
        let exact = self.p.radius + 4.0 * self.p.sliver + self.p.coarse;
        self.situation(lo, hi, allowed, inner, &uncut, exact)
    }

    pub(super) fn situation(
        &self,
        lo: DVec2,
        hi: DVec2,
        allowed: Region,
        inner: Region,
        uncut: &Region,
        exact: f64,
    ) -> Result<Situation, StrategyError> {
        self.situation_with((lo, hi), allowed, inner, uncut, exact, None)
    }

    /// As [`Clearing::situation`], with the material worth a pass already
    /// grown by the tool's radius, where it is known.
    fn situation_with(
        &self,
        (lo, hi): (DVec2, DVec2),
        allowed: Region,
        inner: Region,
        uncut: &Region,
        exact: f64,
        grown: Option<Region>,
    ) -> Result<Situation, StrategyError> {
        // Shrunk, so slivers a rounding error wide are not passes.
        let remaining = uncut.offset(-self.p.sliver, self.p.coarse);
        // A radius clear of the material, grown back by what it was shrunk:
        // to where passes start, which need only be near, so loosely, its
        // outline simplified first.
        let grown = match grown {
            Some(grown) => grown,
            None => {
                remaining.grown_loosely(self.p.radius + self.p.sliver, self.p.coarse, self.p.coarse)
            }
        };
        let free = allowed.difference(&grown)?;
        Ok(Situation {
            lo,
            hi,
            exact,
            allowed,
            inner,
            remaining,
            free,
        })
    }

    /// Where the next pass starts, which way, and with the material on which
    /// side: the nearest place the tool stands clear of the material and
    /// touching it, where a stretch of the material's edge begins, heading along
    /// it with the material on the cutting side. `None` where there is nowhere
    /// so in what `s` knows of, and nearer `from` than it knows all round.
    ///
    /// Beginnings, not just anywhere along an edge: a pass started part way along
    /// runs into whatever ends the edge behind it, a wall or the part, and has
    /// nowhere to go.
    pub(super) fn next_start(
        &self,
        s: &Situation,
        from: Option<DVec2>,
    ) -> Result<Option<(DVec2, DVec2, Side)>, StrategyError> {
        // The free region's edge is where the tool may go no further, or where
        // it touches material: off the edge of where it may be, it is the
        // material's.
        let touching = &s.inner;
        // Each ring as a closed polyline, from a corner off the material where
        // it has one, so no stretch along it is split where it starts.
        let loops: Vec<Vec<DVec2>> = s
            .free
            .rings()
            .iter()
            .map(|ring| {
                let pts = ring.points();
                let k = pts
                    .iter()
                    .take(64)
                    .position(|p| !touching.contains(*p))
                    .unwrap_or_else(|| nearest_to(pts, from));
                let mut l: Vec<DVec2> = pts
                    .iter()
                    .skip(k)
                    .chain(pts.iter().take(k))
                    .copied()
                    .collect();
                if let Some(&first) = l.first() {
                    l.push(first);
                }
                l
            })
            .collect();
        let runs = touching.clip_polylines(&loops)?;
        // Nearer than the edge of what it knows, the nearest it knows is the
        // nearest there is.
        let within = match from {
            Some(f) if s.exact > 0.0 => {
                let edge = (f - s.lo).min(s.hi - f).min_element();
                Some(edge - s.exact)
            }
            _ => None,
        };
        let from = from.or_else(|| self.allowed.bounds().map(|b| b.0));
        let main = self.p.side;
        let other = main.opposite();
        let mut best: Option<(f64, DVec2, DVec2, Side)> = None;
        let mut consider = |p: DVec2, towards: DVec2, side: Side| {
            if !s.knows(p)
                || self
                    .dead
                    .iter()
                    .any(|d| d.distance(p) < self.p.radius * 0.25)
            {
                return;
            }
            let Some(dir) = (towards - p).try_normalize() else {
                return;
            };
            let d = from.map_or(0.0, |f| f.distance(p));
            if within.is_some_and(|w| d > w) {
                return;
            }
            if best.as_ref().is_none_or(|b| d < b.0 - 1e-9) {
                best = Some((d, p, dir, side));
            }
        };
        // Free is on a ring's left, so the material is on the right going its
        // way: a run begins at its first point going its way, and, cutting the
        // other way, at its last going back.
        for run in &runs {
            let (Some(&a), Some(&b)) = (run.first(), run.get(1)) else {
                continue;
            };
            let (Some(&z), Some(&y)) = (
                run.last(),
                run.len().checked_sub(2).and_then(|k| run.get(k)),
            ) else {
                continue;
            };
            let (fore, back) = if main == Side::Right {
                ((a, b), (z, y))
            } else {
                ((z, y), (a, b))
            };
            consider(fore.0, fore.1, main);
            if self.p.both_ways {
                consider(back.0, back.1, other);
            }
        }
        Ok(best.map(|(_, p, dir, side)| (p, dir, side)))
    }

    /// The way from `from` to `to` at the level without lifting, through what
    /// is cleared, as the corners it turns at between: straight where it can
    /// be, and otherwise round what stands between, the shortest way by the
    /// corners of where the tool stands clear of the material. `None` where
    /// there is none within the stay-down distance, clear of the part.
    pub(super) fn stay_route(&self, s: &Situation, from: DVec2, to: DVec2) -> Option<Vec<DVec2>> {
        let most = self.p.stay_down;
        let straight = from.distance(to);
        if straight > most || !s.knows(from) || !s.knows(to) {
            return None;
        }
        // Where the passes end and start the tool only grazes the material, so
        // it may come a little into the tool's reach; between, it keeps the
        // clearance off it.
        let keep = self.p.radius + self.p.stay_down_clearance;
        let clear = |a: DVec2, b: DVec2| {
            s.allowed.contains_segment(a, b)
                && clear_of(&s.remaining, a, b, self.p.radius * 0.9)
                && away_from(a, b, &[from, to], keep)
                    .into_iter()
                    .all(|(p, q)| clear_of(&s.remaining, p, q, keep))
        };
        if clear(from, to) {
            return Some(Vec::new());
        }
        // Round what stands between, as much longer as the level allows.
        let longest = straight + self.p.stay_down_level * (most - straight);
        if longest <= straight + 1e-9 {
            return None;
        }
        route(&s.free, from, to, longest, self.p, |p| s.knows(p), clear)
    }

    /// Go down into material there is no way into from the side: a helix, or a
    /// plunge, at the widest place there is room for one, cleared as it goes
    /// down. `None` where there is room nowhere.
    pub(super) fn enter(
        &mut self,
        s: &Situation,
    ) -> Result<Option<(DVec2, DVec2, Side, Arrive)>, StrategyError> {
        let sizes: &[f64] = if self.p.helix_radius > 0.0 {
            // As the linker tries them.
            &[1.0, 0.5, 0.25]
        } else {
            &[0.0]
        };
        let side = self.p.side;
        let dead: Vec<Contour> = self
            .dead
            .iter()
            .filter_map(|&d| disc(d, self.p.radius * 0.25, self.p.arc))
            .collect();
        let dead = Region::union_of(&dead)?;
        for &k in sizes {
            let r = self.p.helix_radius * k;
            let room = s
                .allowed
                .offset(-r, self.p.arc)
                .intersection(&s.remaining)
                .and_then(|room| room.difference(&dead))?;
            let Some(centre) = deepest(&room, self.p.arc) else {
                continue;
            };
            // The helix ends where the pass begins, and turns the way it climbs,
            // with the tool's centre on its circle: counter-clockwise for material
            // on the right.
            let heading = DVec2::X;
            let start = centre - side.away(heading) * r;
            if let Some(d) = disc(centre, r + self.p.radius, self.p.arc) {
                self.cut_material(&Region::union_of(&[d])?)?;
                self.window = None;
            }
            self.helixes.push((centre, r));
            return Ok(Some((start, heading, side, Arrive::Enter { centre })));
        }
        Ok(None)
    }

    /// A slot `width` wide along the middle of the closed area gone down into
    /// about `centre`, from `start`, where the helix ends: straight along the
    /// area's longest way through `centre`, the whole tool in the material,
    /// to one end and back to the other; then, wider than the tool, rings
    /// round it out to the width, each no more than the optimal load further
    /// out, with the material on `side`. Cut out of the material. `None` where
    /// the slot would not fit, or be shorter than the tool's radius.
    fn slot(
        &mut self,
        s: &Situation,
        centre: DVec2,
        start: DVec2,
        side: Side,
        width: f64,
    ) -> Result<Option<Vec<DVec2>>, StrategyError> {
        let r = self.p.radius;
        let out = (width * 0.5 - r).max(0.0);
        let core = if out > 0.0 {
            s.allowed.offset(-out, self.p.arc)
        } else {
            s.allowed.clone()
        };
        let Some(piece) = core.pieces().into_iter().find(|p| p.contains(centre)) else {
            return Ok(None);
        };
        let Some(u) = longest_way(&piece) else {
            return Ok(None);
        };
        let Some((lo, hi)) = piece.bounds() else {
            return Ok(None);
        };
        // As far as the piece goes each way from the centre.
        let reach = |dir: DVec2| -> f64 {
            let (mut inside, mut outside) = (0.0, lo.distance(hi) + 1.0);
            for _ in 0..40 {
                let mid = (inside + outside) * 0.5;
                if piece.contains_segment(centre, centre + dir * mid) {
                    inside = mid;
                } else {
                    outside = mid;
                }
            }
            inside
        };
        let (a, b) = (centre - u * reach(-u), centre + u * reach(u));
        if a.distance(b) < r || !s.allowed.contains_segment(start, a) {
            return Ok(None);
        }
        let mut points = vec![start, a, b];
        // Rings round it out to the width, each no more than the load out.
        let load = (r * (1.0 - self.p.target.cos())).max(self.p.sliver * 4.0);
        let rings = if out > 0.0 {
            camcraft_geom::steps::bounded_count((out / load - 1e-9).ceil())
        } else {
            0
        };
        for k in 1..=rings {
            let rho = out * f64::from(k) / f64::from(rings);
            points.extend(stadium(a, b, rho, side == Side::Right, self.p.arc));
        }
        let swept = Region::around_polylines(&[points.clone()], r, self.p.swept_arc);
        self.cut_material(&swept)?;
        self.window = None;
        Ok(Some(points))
    }

    /// Take what the trail has swept out of the material.
    pub(super) fn flush(&mut self) -> Result<(), StrategyError> {
        if self.trail.len() >= 2 {
            let last = self.trail.last().copied();
            let lines = vec![std::mem::take(&mut self.trail)];
            let swept = Region::around_polylines(&lines, self.p.radius, self.p.swept_arc);
            if let Some(w) = &mut self.window {
                let before = w.uncut.area();
                w.uncut = w.uncut.difference(&swept)?;
                w.uncut_index = Indexed::of(&w.uncut);
                self.taken += (before - w.uncut.area()).max(0.0);
            }
            self.cut_material(&swept)?;
            self.trail = last.into_iter().collect();
        }
        self.trail_length = 0.0;
        Ok(())
    }

    /// How much of the material the window holds has been taken out since
    /// this was last asked, by every flush, whatever asked for it.
    fn taken(&mut self) -> f64 {
        std::mem::take(&mut self.taken)
    }

    /// Make sure the window holds everything a step from `at` reads.
    pub(super) fn window_at(&mut self, at: DVec2) -> Result<(), StrategyError> {
        let margin = self.p.radius + 3.0 * self.p.longest;
        let fits = self.window.as_ref().is_some_and(|w| {
            at.cmpge(w.lo + DVec2::splat(margin)).all()
                && at.cmple(w.hi - DVec2::splat(margin)).all()
        });
        if fits {
            return Ok(());
        }
        self.flush()?;
        let half = DVec2::splat(margin + 4.0 * self.p.radius);
        let (lo, hi) = (at - half, at + half);
        let uncut = self.uncut.window(lo, hi)?;
        self.window = Some(Window {
            lo,
            hi,
            uncut_index: Indexed::of(&uncut),
            allowed_index: Indexed::of(&self.room.window(lo, hi)?),
            uncut,
        });
        Ok(())
    }

    /// Cut one pass, from `start` heading along `dir`, with the material on
    /// `side`: step after step, each turned to hold the engagement, until the
    /// material runs out or the next step would overload the tool. `entered`
    /// for a pass that begins where the tool went down, which may start
    /// heavily loaded. Empty where it cut nothing.
    pub(super) fn pass(
        &mut self,
        start: DVec2,
        dir: DVec2,
        side: Side,
        entered: bool,
    ) -> Result<Vec<DVec2>, StrategyError> {
        self.flush()?;
        self.trail = vec![start];
        self.trail_length = 0.0;
        let mut points = vec![start];
        let mut at = start;
        let mut heading = dir;
        let mut step = self.p.shortest;
        let mut air = 0.0;
        let mut last_cut = 0;
        let mut lenient = if entered { 2.0 * self.p.radius } else { 0.0 };
        // How far it has gone engaged, as it measures, since the material
        // last went down by more than a sliver's width along the way, and
        // where that was: what it touches can be a sliver no sweep takes, and
        // a pass that takes nothing is over, however engaged it says it is.
        // Going through air is the air's to end, above.
        let (mut idle, mut last_taken) = (0.0, 0);
        // How far it has gone engaged since that was last looked at.
        let mut since = 0.0;
        self.taken = 0.0;
        while self.steps < MAX_STEPS {
            self.steps += 1;
            self.window_at(at)?;
            let Some(w) = self.window.as_ref() else {
                break;
            };
            let Some(chosen) = choose(
                w,
                (&self.trail, &mut self.trying),
                at,
                heading,
                side,
                step,
                lenient > 0.0,
                self.p,
            ) else {
                self.overloads += 1;
                break;
            };
            at = chosen.at;
            heading = chosen.heading;
            points.push(at);
            self.trail.push(at);
            self.trail_length += step;
            lenient -= step;
            if chosen.engagement < self.p.air {
                air += step;
                if air > self.p.radius {
                    break;
                }
            } else {
                air = 0.0;
                last_cut = points.len() - 1;
                since += step;
            }
            // Short steps where it turns, long where it runs straight.
            step = if chosen.turn.abs() > 0.3 {
                (step * 0.5).max(self.p.shortest)
            } else if chosen.turn.abs() < 0.1 {
                (step * 1.5).min(self.p.longest)
            } else {
                step
            };
            if self.trail_length > self.p.radius || self.trail.len() > SWEPT_AT_ONCE {
                self.flush()?;
                if self.taken() > self.p.sliver * since {
                    idle = 0.0;
                    last_taken = points.len() - 1;
                } else {
                    idle += since;
                    if idle > self.p.radius {
                        last_cut = last_cut.min(last_taken);
                        break;
                    }
                }
                since = 0.0;
            }
        }
        self.flush()?;
        points.truncate(last_cut + 1);
        Ok(if points.len() >= 2 {
            points
        } else {
            Vec::new()
        })
    }
}

/// Whether every piece of `remaining` is smaller across than `size`.
fn crumbs(remaining: &Region, size: f64) -> bool {
    remaining
        .pieces()
        .iter()
        .all(|p| p.bounds().is_none_or(|(lo, hi)| lo.distance(hi) < size))
}

/// The point of `points` nearest `from`, by its place: the first without a
/// `from`.
fn nearest_to(points: &[DVec2], from: Option<DVec2>) -> usize {
    let Some(from) = from else {
        return 0;
    };
    points
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.distance(from).total_cmp(&b.1.distance(from)))
        .map_or(0, |(k, _)| k)
}

/// The way `region` is longest, as a unit vector: the axis its area spreads
/// furthest along. `None` for a region with no area.
fn longest_way(region: &Region) -> Option<DVec2> {
    // Its area, and its moments about the origin, over every ring: holes run
    // the other way round, and take theirs away.
    let (mut area, mut cx, mut cy) = (0.0, 0.0, 0.0);
    let (mut xx, mut yy, mut xy) = (0.0, 0.0, 0.0);
    for ring in region.rings() {
        for (p, q) in ring.edges() {
            let c = p.perp_dot(q);
            area += c * 0.5;
            cx += (p.x + q.x) * c / 6.0;
            cy += (p.y + q.y) * c / 6.0;
            xx += (p.x * p.x + p.x * q.x + q.x * q.x) * c / 12.0;
            yy += (p.y * p.y + p.y * q.y + q.y * q.y) * c / 12.0;
            xy += (p.x * q.y + 2.0 * p.x * p.y + 2.0 * q.x * q.y + q.x * p.y) * c / 24.0;
        }
    }
    if area.abs() < 1e-12 {
        return None;
    }
    let (mx, my) = (cx / area, cy / area);
    // About its centroid.
    let sxx = xx - area * mx * mx;
    let syy = yy - area * my * my;
    let sxy = xy - area * mx * my;
    let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    Some(DVec2::from_angle(angle))
}

/// A ring `rho` round the segment `a`-`b`, two straights and two half
/// circles, as chords within `arc` of them, starting and ending beside `b`:
/// counter-clockwise when `ccw`, so what is outside is on its right.
fn stadium(a: DVec2, b: DVec2, rho: f64, ccw: bool, arc: f64) -> Vec<DVec2> {
    let Some(u) = (b - a).try_normalize() else {
        return Vec::new();
    };
    let turn = if ccw { 1.0 } else { -1.0 };
    // Beside `b`, on the side it leaves from going round.
    let n = u.perp() * turn;
    let half = (1.0 - arc.min(rho) / rho).clamp(-1.0, 1.0).acos().max(1e-3);
    let steps = camcraft_geom::steps::bounded_count((PI / (2.0 * half)).ceil().clamp(4.0, 512.0));
    let round = |c: DVec2, from: DVec2| -> Vec<DVec2> {
        (0..=steps)
            .map(|k| {
                let angle = turn * PI * f64::from(k) / f64::from(steps);
                c + DVec2::from_angle(angle).rotate(from) * rho
            })
            .collect()
    };
    let mut out = round(b, -n);
    out.extend(round(a, n));
    out.push(b - n * rho);
    out
}

/// The pieces of the segment `a`–`b` further than `distance` from every one of
/// `ends`.
fn away_from(a: DVec2, b: DVec2, ends: &[DVec2], distance: f64) -> Vec<(DVec2, DVec2)> {
    let d = b - a;
    let dd = d.length_squared();
    let mut keep: Vec<(f64, f64)> = vec![(0.0, 1.0)];
    for &c in ends {
        // Where along it it is within the distance of `c`: |a + t d - c| < r.
        let f = a - c;
        let (qa, qb, qc) = (dd, 2.0 * f.dot(d), f.length_squared() - distance * distance);
        let (t0, t1) = if qa <= 1e-18 {
            if qc < 0.0 { (0.0, 1.0) } else { continue }
        } else {
            let disc = qb * qb - 4.0 * qa * qc;
            if disc <= 0.0 {
                continue;
            }
            let root = disc.sqrt();
            ((-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa))
        };
        keep = keep
            .into_iter()
            .flat_map(|(s, e)| {
                [(s, e.min(t0)), (s.max(t1), e)]
                    .into_iter()
                    .filter(|(s, e)| e - s > 1e-9)
            })
            .collect();
    }
    keep.into_iter()
        .map(|(s, e)| (a + d * s, a + d * e))
        .collect()
}

/// The most corners a way round is looked for among: the nearest to going
/// straight, by how much further they take it.
const ROUTE_CORNERS: usize = 64;

/// How far round a curve the corners a way round may turn at are apart, in
/// radians: the chord between two falls short of the curve by less than the
/// room it is kept inside it by.
const ROUTE_TURN: f64 = 0.26;

/// The most legs of a way round tried, each asked of the material and the
/// part.
const ROUTE_TRIES: usize = 400;

/// The shortest way from `from` to `to` no longer than `most`, by corners of
/// `free` that `known` says are so, each leg `clear`; the corners it turns at.
/// The shortest way round what stands between bends only where the region it
/// keeps to turns away from itself, so those are its corners, a little inside
/// it.
pub(super) fn route(
    free: &Region,
    from: DVec2,
    to: DVec2,
    most: f64,
    p: &LevelParams,
    known: impl Fn(DVec2) -> bool,
    clear: impl Fn(DVec2, DVec2) -> bool,
) -> Option<Vec<DVec2>> {
    // Off the edge, and the clearance the link keeps off the material.
    let inside = free.offset(-(0.05 * p.radius + p.stay_down_clearance), p.coarse);
    let mut corners: Vec<(f64, DVec2)> = Vec::new();
    for ring in inside.rings() {
        let pts = ring.points();
        let n = pts.len();
        // A curve round what stands is many corners a little apart: one each
        // so far round it is enough to go round it by.
        let mut turned = 0.0;
        for i in 0..n {
            let (Some(&prev), Some(&v), Some(&next)) =
                (pts.get((i + n - 1) % n), pts.get(i), pts.get((i + 1) % n))
            else {
                continue;
            };
            // The region on a ring's left: turning right, it turns away from
            // itself.
            let (into, out) = (v - prev, next - v);
            let turn = camcraft_geom::arc::turn(into, out);
            if turn >= 0.0 {
                turned = 0.0;
                continue;
            }
            turned -= turn;
            let ends = pts
                .get((i + 1) % n)
                .zip(pts.get((i + 2) % n))
                .is_none_or(|(&a, &b)| (a - v).perp_dot(b - a) >= 0.0);
            if turned < ROUTE_TURN && !ends {
                continue;
            }
            turned = 0.0;
            let via = from.distance(v) + v.distance(to);
            if via <= most && known(v) {
                corners.push((via, v));
            }
        }
    }
    corners.sort_by(|a, b| a.0.total_cmp(&b.0));
    corners.truncate(ROUTE_CORNERS);
    // From, to, then the corners; the shortest way, best first, each leg asked
    // only when it could make a way short enough.
    let nodes: Vec<DVec2> = [from, to]
        .into_iter()
        .chain(corners.into_iter().map(|(_, v)| v))
        .collect();
    let n = nodes.len();
    let mut best = vec![f64::INFINITY; n];
    let mut came: Vec<Option<usize>> = vec![None; n];
    let mut done = vec![false; n];
    if let Some(b) = best.first_mut() {
        *b = 0.0;
    }
    let mut tries = 0;
    loop {
        // The nearest not done, by the way so far and straight on from it.
        let next = (0..n)
            .filter(|&k| !done.get(k).copied().unwrap_or(true))
            .filter_map(|k| {
                let g = *best.get(k)?;
                let v = *nodes.get(k)?;
                g.is_finite().then_some((k, g + v.distance(to)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let (u, _) = next?;
        if u == 1 {
            break;
        }
        if let Some(d) = done.get_mut(u) {
            *d = true;
        }
        let (Some(&at), Some(&g)) = (nodes.get(u), best.get(u)) else {
            return None;
        };
        for w in 1..n {
            let (Some(&v), Some(&was), Some(&finished)) = (nodes.get(w), best.get(w), done.get(w))
            else {
                continue;
            };
            let through = g + at.distance(v);
            if finished || through >= was || through + v.distance(to) > most {
                continue;
            }
            if tries >= ROUTE_TRIES {
                return None;
            }
            tries += 1;
            if clear(at, v) {
                if let Some(b) = best.get_mut(w) {
                    *b = through;
                }
                if let Some(c) = came.get_mut(w) {
                    *c = Some(u);
                }
            }
        }
    }
    // Back from `to`, the corners on the way.
    let mut via = Vec::new();
    let mut k = came.get(1).copied().flatten();
    while let Some(c) = k {
        if c == 0 {
            break;
        }
        via.push(*nodes.get(c)?);
        k = came.get(c).copied().flatten();
    }
    via.reverse();
    Some(via)
}
