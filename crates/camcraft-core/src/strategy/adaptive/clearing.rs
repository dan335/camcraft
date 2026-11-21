//! One level cleared: passes steered through what is left, each started
//! where the tool can stand, and linked.

use super::steer::{choose, clear_of, deepest};
use super::tiles::{Tiles, Window};
use super::*;

/// A level being cleared.
pub(super) struct Clearing<'a> {
    pub(super) p: &'a LevelParams,
    /// The level's height.
    z: f64,
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
    /// Whether the last pass ended turning round on the spot.
    orbited: bool,
    /// Where the passes that ended so started.
    orbits: Vec<DVec2>,
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
        z: f64,
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
            z,
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
            orbited: false,
            orbits: Vec::new(),
        })
    }

    /// Clear the level, its first pass starting nearest `from`, where the
    /// tool is when it comes to it, if that is known. A pass the tool cannot
    /// come to staying down from the last may rise to it from one of the
    /// `approach`'s arrivals, where passes of the levels below in the slab
    /// ended: each once, the nearest it can. Cut by area, the level's passes
    /// in an area other than the one the tool is in are cut after it has left
    /// that area's levels below, so the level comes to the area nearest where
    /// its own last pass below ended, rather than nearest where the tool is
    /// in this one.
    pub(super) fn run(
        &mut self,
        from: Option<DVec2>,
        approach: Approach,
    ) -> Result<(), StrategyError> {
        let Approach {
            mut arrivals,
            areas,
            level,
        } = approach;
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
                    let found = self.next_start(&s, at.or(from))?;
                    (s, found)
                }
            };
            let (s, found) =
                match self.start_in_area(areas.as_deref(), &arrivals, at.or(from), found)? {
                    Some(there) => (self.situation_everywhere()?, Some(there)),
                    None => (s, found),
                };
            let (start, dir, side, arrive, entered) = match found {
                Some((start, dir, side)) => {
                    let arrive =
                        self.arrival(&s, at, start, &mut arrivals, areas.as_deref(), level)?;
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
                None => {
                    // Narrower than the tool, as a crumb left where a pass
                    // went round on the spot: taken from outside in, not gone
                    // down into.
                    let along = match nearest_piece(&s.remaining, at.or(from)) {
                        Some(near) => self.take_small(near, self.p.side)?,
                        None => Vec::new(),
                    };
                    if !along.is_empty() {
                        futile = 0;
                        for (arrive, points) in along {
                            at = points.last().copied();
                            self.passes.push((arrive, points));
                            self.sides.push(self.p.side);
                        }
                        continue;
                    }
                    match self.enter(&s)? {
                        Some((start, dir, side, arrive)) => (start, dir, side, arrive, true),
                        None => {
                            self.no_room += 1;
                            break;
                        }
                    }
                }
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
            if self.orbited {
                // Started here again, a pass may go on from what this one
                // left; one that goes round on the spot from here a second
                // time is not started here again. What it turned round on,
                // if narrower than the tool, is taken from outside in.
                let near = |p: &DVec2| p.distance(start) < self.p.radius * 0.25;
                if self.orbits.iter().any(near) {
                    self.dead.push(start);
                }
                self.orbits.push(start);
                if let Some(end) = at {
                    for (arrive, points) in self.take_small(end, side)? {
                        at = points.last().copied();
                        self.passes.push((arrive, points));
                        self.sides.push(side);
                    }
                }
            }
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

    /// How the tool comes to `start`, the first way it can: staying down from
    /// where it is, `at`, through what `s` shows cleared; cut by area, back
    /// from where the level's last pass in `start`'s area ended
    /// ([`Clearing::back_in_area`]), this level being the slab's `level`th;
    /// rising from where a pass of a level below in the slab ended, the
    /// nearest of `arrivals` it can ([`Clearing::rise`]); otherwise however
    /// the linker sees fit.
    fn arrival(
        &mut self,
        s: &Situation,
        at: Option<DVec2>,
        start: DVec2,
        arrivals: &mut Vec<Arrival>,
        areas: Option<&[Indexed]>,
        level: usize,
    ) -> Result<Arrive, StrategyError> {
        if let Some(via) = at.and_then(|from| self.stay_route(s, from, start)) {
            return Ok(Arrive::Stay { via });
        }
        if let Some(back) = self.back_in_area(areas, level, at, start)? {
            return Ok(back);
        }
        if arrivals.is_empty() {
            return Ok(Arrive::Link);
        }
        // Where they ended may be far from where it is.
        let all = self.situation_everywhere()?;
        Ok(self.rise(&all, arrivals, start))
    }

    /// Where to start instead of `found`, the start nearest where the tool
    /// is, `tool`, where that is in another of `areas` than the tool: nearest
    /// where the last pass below in that area ended, among `arrivals`, if
    /// there is a start in it so. `None` to start at `found`.
    fn start_in_area(
        &mut self,
        areas: Option<&[Indexed]>,
        arrivals: &[Arrival],
        tool: Option<DVec2>,
        found: Option<(DVec2, DVec2, Side)>,
    ) -> Result<Option<(DVec2, DVec2, Side)>, StrategyError> {
        let (Some(areas), Some(tool), Some((start, ..))) = (areas, tool, found) else {
            return Ok(None);
        };
        let there = piece_at(areas, start);
        if piece_at(areas, tool) == there {
            return Ok(None);
        }
        let Some(last) = arrivals.iter().find(|a| piece_at(areas, a.at) == there) else {
            return Ok(None);
        };
        let all = self.situation_everywhere()?;
        Ok(self
            .next_start(&all, Some(last.at))?
            .filter(|(p, ..)| piece_at(areas, *p) == there))
    }

    /// Cut by area, the way to `start` from where the level's last pass in
    /// `start`'s area ended, where the tool is in another, `at`: by area, the
    /// passes between, in the other, are cut after, so it comes from that
    /// one, this level being the slab's `level`th. `None` where there is no
    /// such pass, or no way from it staying down.
    fn back_in_area(
        &mut self,
        areas: Option<&[Indexed]>,
        level: usize,
        at: Option<DVec2>,
        start: DVec2,
    ) -> Result<Option<Arrive>, StrategyError> {
        let (Some(areas), Some(at)) = (areas, at) else {
            return Ok(None);
        };
        let there = piece_at(areas, start);
        if piece_at(areas, at) == there {
            return Ok(None);
        }
        let last = self
            .passes
            .iter()
            .enumerate()
            .rev()
            .find(|(_, (_, pts))| area_of(areas, pts) == there)
            .and_then(|(k, (_, pts))| Some((k, *pts.last()?)));
        let Some((pass, end)) = last else {
            return Ok(None);
        };
        let all = self.situation_everywhere()?;
        Ok(self.stay_route(&all, end, start).map(|via| Arrive::Rise {
            after: (level, pass),
            at: end,
            via,
        }))
    }

    /// How the tool comes to `start` from the nearest of `arrivals` it can
    /// rise from and cross to staying down, that one taken out of them; or
    /// however the linker sees fit.
    fn rise(&self, s: &Situation, arrivals: &mut Vec<Arrival>, start: DVec2) -> Arrive {
        let mut near: Vec<(usize, f64)> = arrivals
            .iter()
            .enumerate()
            .map(|(k, a)| (k, a.at.distance(start)))
            .collect();
        near.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        for (k, _) in near {
            let Some(&a) = arrivals.get(k) else {
                continue;
            };
            if let Some(via) = self.stay_route(s, a.at, start) {
                arrivals.remove(k);
                return Arrive::Rise {
                    after: a.after,
                    at: a.at,
                    via,
                };
            }
        }
        Arrive::Link
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
            // A pass risen to may be lifted to instead, where it does not
            // follow the pass it rises from.
            if matches!(arrive, Arrive::Link | Arrive::Rise { .. })
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
    /// there is none so short, clear of the part.
    ///
    /// So short is the stay-down distance, or, where that is shorter, no
    /// longer than lifting would go: up to where a lifted link crosses,
    /// across and down again. Staying down through what is cleared is then
    /// no further than lifting, and nothing about it is odd; how much faster a
    /// rapid is than the feed is the machine's, not known here.
    pub(super) fn stay_route(&self, s: &Situation, from: DVec2, to: DVec2) -> Option<Vec<DVec2>> {
        // None asked for, it always lifts.
        if self.p.stay_down <= 0.0 {
            return None;
        }
        let straight = from.distance(to);
        let most = self
            .p
            .stay_down
            .max(straight + 2.0 * (self.p.lift_to - self.z).max(0.0));
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
    /// down. A helix is made smaller where there is no room for it, halved
    /// each time, but never below the least asked for: a smaller one goes
    /// round so many more times for its depth
    /// that it is mostly a plunge. Where none fits, a ramp along the area
    /// ([`Clearing::ramp_way`]). `None` where there is room for nothing.
    pub(super) fn enter(
        &mut self,
        s: &Situation,
    ) -> Result<Option<(DVec2, DVec2, Side, Arrive)>, StrategyError> {
        let mut sizes: Vec<f64> = Vec::new();
        if self.p.helix_radius > 0.0 {
            let least = self
                .p
                .least_helix_radius
                .clamp(self.p.sliver, self.p.helix_radius);
            let mut r = self.p.helix_radius;
            // Bounded: halving from a finite radius reaches any positive
            // least in a few dozen steps.
            while r > least + 1e-9 && sizes.len() < 64 {
                sizes.push(r);
                r *= 0.5;
            }
            sizes.push(least);
        } else {
            sizes.push(0.0);
        }
        let side = self.p.side;
        let dead: Vec<Contour> = self
            .dead
            .iter()
            .filter_map(|&d| disc(d, self.p.radius * 0.25, self.p.arc))
            .collect();
        let dead = Region::union_of(&dead)?;
        for &r in &sizes {
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
        if self.p.helix_radius <= 0.0 {
            return Ok(None);
        }
        let room = s
            .allowed
            .intersection(&s.remaining)
            .and_then(|room| room.difference(&dead))?;
        let Some(way) = self.ramp_way(&room) else {
            return Ok(None);
        };
        let (Some(&start), Some(&next)) = (way.first(), way.get(1)) else {
            return Ok(None);
        };
        let heading = (next - start).try_normalize().unwrap_or(DVec2::X);
        self.cut_material(&Region::around_polylines(
            std::slice::from_ref(&way),
            self.p.radius,
            self.p.swept_arc,
        ))?;
        self.window = None;
        Ok(Some((start, heading, side, Arrive::Ramp { way })))
    }

    /// Where to ramp down into `room`, the material where the tool may be, too
    /// narrow anywhere for the least helix: back and forth along a stretch of
    /// its middle, from a point as deep in it as any, as long as the largest
    /// helix goes round in one turn, or as long as the middle goes if it is
    /// shorter, but no shorter than the least helix is across. The middle is
    /// where the room is half as deep as it is at its deepest, or more: along
    /// a channel of about even width, a line down it. `None` where there is
    /// no such stretch.
    fn ramp_way(&self, room: &Region) -> Option<Vec<DVec2>> {
        let (deep, depth) = room.deepest_point(self.p.arc)?;
        if depth <= self.p.sliver {
            return None;
        }
        let middle = room.offset(-0.5 * depth, self.p.arc);
        let shortest = 2.0 * self.p.least_helix_radius.max(self.p.sliver);
        let longest = (TAU * self.p.helix_radius).max(shortest);
        // The ring of the middle nearest the deepest point, from its point
        // nearest it, round either way as far as the stretch goes.
        let ring = middle.rings().iter().min_by(|a, b| {
            let d = |r: &Contour| {
                r.points()
                    .iter()
                    .map(|p| p.distance(deep))
                    .fold(f64::INFINITY, f64::min)
            };
            d(a).total_cmp(&d(b))
        })?;
        let pts = ring.points();
        let n = pts.len();
        let from = nearest_to(pts, Some(deep));
        let mut way = vec![*pts.get(from)?];
        let mut length = 0.0;
        for i in 1..n {
            let p = *pts.get((from + i) % n)?;
            let last = *way.last()?;
            let leg = last.distance(p);
            if length + leg >= longest {
                way.push(last + (p - last) * ((longest - length) / leg.max(1e-12)));
                length = longest;
                break;
            }
            length += leg;
            way.push(p);
        }
        (length >= shortest - 1e-9).then_some(way)
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

    /// Material nowhere as wide as the tool, about `at`, taken from outside
    /// in, with it on `side`, in passes along it rather than steered for the
    /// load: a ridge or a sliver narrower than the load, which a pass turns
    /// hard toward at every step, never taking enough, and so goes round on
    /// the spot by; or a crumb such a pass left with no way in from the side,
    /// smaller than the helix that would go down into it. The tool's centre
    /// goes round what is left as far out as leaves the load to cut, within
    /// where it may be, until all of it is in reach of where it has been:
    /// along one side of a strip, round one edge of a ring; again, the load
    /// further in, until none is left but crumbs. Where the tool may not be
    /// so far in, it goes round further out, taking less. Each way along is a
    /// pass, started where the tool just touches the material, as a pass is;
    /// none that would cut into other material, which is left to the passes
    /// after, and none where what is there is as wide as the tool somewhere.
    fn take_small(
        &mut self,
        at: DVec2,
        side: Side,
    ) -> Result<Vec<(Arrive, Vec<DVec2>)>, StrategyError> {
        self.flush()?;
        let (r, load) = (self.p.radius, self.p.load);
        let remaining = self.uncut.whole()?.offset(-self.p.sliver, self.p.coarse);
        // The piece about `at`: the one it is in, or nearest, within the
        // tool's reach.
        let Some(mut left) = remaining
            .pieces()
            .into_iter()
            .map(|piece| (piece.depth_at(at), piece))
            .filter(|(d, _)| *d > -2.0 * r)
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, piece)| piece)
        else {
            return Ok(Vec::new());
        };
        if !left.offset(-r, self.p.coarse).is_empty() {
            return Ok(Vec::new());
        }
        let others = remaining.difference(&left)?;
        let mut out = Vec::new();
        let mut from = at;
        // Bounded: each round takes some of it, or ends.
        for _ in 0..SMALL_ROUNDS {
            self.p.watch.check()?;
            // Crumbs are left, as a level leaves them.
            if all_in(&left, &Region::default(), self.p)? || crumbs(&left, r * 0.25) {
                break;
            }
            // The load in from where the tool just touches it, or less where
            // only further out is where the tool may be.
            let mut into = load.min(r - self.p.sliver);
            let mut ways = self.ways_round(&left, r - into, side)?;
            while ways.is_empty() && into > 2.0 * self.p.sliver {
                into *= 0.5;
                ways = self.ways_round(&left, r - into, side)?;
            }
            let off = r - into;
            // Nearest where the tool is first, from its nearest point if it
            // goes all the way round.
            let mut ways: Vec<(f64, Vec<DVec2>)> = ways
                .into_iter()
                .map(|way| {
                    let closed = way.len() > 2
                        && way
                            .first()
                            .zip(way.last())
                            .is_some_and(|(a, b)| a.distance(*b) < 1e-9);
                    let way: Vec<DVec2> = if closed {
                        let start = nearest_to(&way, Some(from));
                        let ring = way.get(..way.len() - 1).unwrap_or(&[]);
                        ring.iter()
                            .skip(start)
                            .chain(ring.iter().take(start + 1))
                            .copied()
                            .collect()
                    } else {
                        way
                    };
                    let d = way.first().map_or(f64::INFINITY, |p| p.distance(from));
                    (d, way)
                })
                .collect();
            ways.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut took = false;
            for (_, way) in ways {
                // Along it until everything is in reach of where it has been.
                let mut taken: Vec<DVec2> = Vec::new();
                let mut since = 0.0;
                for p in way {
                    if let Some(&q) = taken.last() {
                        since += q.distance(p);
                    }
                    taken.push(p);
                    if since >= r {
                        since = 0.0;
                        let so_far = Region::around_polylines(
                            std::slice::from_ref(&taken),
                            r,
                            self.p.swept_arc,
                        );
                        if all_in(&left, &so_far, self.p)? {
                            break;
                        }
                    }
                }
                // Come at it from where the tool just touches it, square to
                // the way.
                if let (Some(&a), Some(&b)) = (taken.first(), taken.get(1))
                    && let Some(d) = (b - a).try_normalize()
                {
                    let touch = a + side.away(d) * (r - off);
                    if self.allowed.contains_segment(touch, a) {
                        taken.insert(0, touch);
                    }
                }
                if taken.len() < 2 {
                    continue;
                }
                let swept =
                    Region::around_polylines(std::slice::from_ref(&taken), r, self.p.swept_arc);
                let length = camcraft_geom::contour::polyline_length(&taken);
                if others.intersection(&swept)?.area() > self.p.sliver * length
                    || left.intersection(&swept)?.area() <= self.p.sliver * length
                {
                    continue;
                }
                let s = self.situation_near(from, 8.0 * r)?;
                let arrive = taken
                    .first()
                    .and_then(|&b| self.stay_route(&s, from, b))
                    .map_or(Arrive::Link, |via| Arrive::Stay { via });
                self.cut_material(&swept)?;
                self.window = None;
                left = left.difference(&swept)?;
                from = taken.last().copied().unwrap_or(from);
                out.push((arrive, taken));
                took = true;
                break;
            }
            if !took {
                break;
            }
        }
        Ok(out)
    }

    /// The ways round `material` `off` out from it, within where the tool
    /// may be, each with the material on `side`.
    fn ways_round(
        &self,
        material: &Region,
        off: f64,
        side: Side,
    ) -> Result<Vec<Vec<DVec2>>, StrategyError> {
        // The material on the left of each ring round it.
        let loops: Vec<Vec<DVec2>> = material
            .offset(off, self.p.arc)
            .rings()
            .iter()
            .map(|ring| {
                let mut l = ring.points().to_vec();
                if let Some(&first) = l.first() {
                    l.push(first);
                }
                l
            })
            .collect();
        Ok(self
            .allowed
            .clip_polylines(&loops)?
            .into_iter()
            .filter(|way| way.len() >= 2)
            .map(|mut way| {
                if side == Side::Right {
                    way.reverse();
                }
                way
            })
            .collect())
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
        // How far it has gone, how far its heading has turned and where it
        // was, at each step within the tool's radius back. A pass that turns
        // a full circle in less than that without once taking the load, and
        // comes back to within a step of where the circle began, is going
        // round on the spot: in material too narrow to load the tool, turning
        // as hard as it may toward it at every step at the shortest step, it
        // goes round a polygon a fraction of a millimetre across, again and
        // again. A loop in a narrow slot turns as tightly, and may take less
        // than the load all the way round, but moves on.
        let mut turns: std::collections::VecDeque<(f64, f64, DVec2)> =
            std::collections::VecDeque::new();
        let (mut gone, mut turned) = (0.0, 0.0);
        // How far it had gone when it last took the load.
        let mut fed = 0.0;
        self.orbited = false;
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
            turned += heading
                .perp_dot(chosen.heading)
                .atan2(heading.dot(chosen.heading));
            gone += step;
            at = chosen.at;
            heading = chosen.heading;
            points.push(at);
            if chosen.engagement >= self.p.target - self.p.within {
                fed = gone;
            }
            while turns
                .front()
                .is_some_and(|&(g, ..)| gone - g > self.p.radius)
            {
                turns.pop_front();
            }
            // The step that closes the circle is neither cut nor kept.
            if turns.iter().any(|&(g, t, p)| {
                g >= fed && (turned - t).abs() >= TAU && p.distance(at) < self.p.shortest
            }) {
                self.orbited = true;
                points.pop();
                break;
            }
            self.trail.push(at);
            turns.push_back((gone, turned, at));
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

/// The most times round material narrower than the tool is gone, each the
/// load further in: far more than such material needs.
const SMALL_ROUNDS: usize = 64;

/// Whether all of `material` worth a pass is within `swept`.
fn all_in(material: &Region, swept: &Region, p: &LevelParams) -> Result<bool, StrategyError> {
    Ok(material
        .difference(swept)?
        .offset(-p.sliver, p.coarse)
        .is_empty())
}

/// A point well inside the piece of `region` nearest `from`, or the first
/// piece without a `from`. `None` for a region with no area.
fn nearest_piece(region: &Region, from: Option<DVec2>) -> Option<DVec2> {
    let pieces = region.pieces();
    let piece = match from {
        Some(f) => pieces
            .iter()
            .max_by(|a, b| a.depth_at(f).total_cmp(&b.depth_at(f)))?,
        None => pieces.first()?,
    };
    piece
        .deepest_point(
            piece
                .bounds()
                .map_or(1e-3, |(lo, hi)| lo.distance(hi) * 1e-3),
        )
        .map(|(p, _)| p)
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
