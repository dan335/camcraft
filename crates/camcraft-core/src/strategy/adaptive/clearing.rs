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
    pub(super) overloads: u64,
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
        let size = 16.0 * p.radius;
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
        Ok(Self {
            p,
            allowed,
            room: room?,
            inner,
            whole_inner,
            uncut: uncut?,
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
            overloads: 0,
        })
    }

    pub(super) fn run(&mut self) -> Result<(), StrategyError> {
        let mut at: Option<DVec2> = None;
        let mut futile = 0;
        for _ in 0..MAX_PASSES {
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
                    let arrive = match at {
                        Some(from) if self.stays_down(&s, from, start) => Arrive::Stay,
                        _ => Arrive::Link,
                    };
                    (start, dir, side, arrive, false)
                }
                None => match self.enter(&s)? {
                    Some((start, dir, side, arrive)) => (start, dir, side, arrive, true),
                    None => {
                        self.no_room += 1;
                        break;
                    }
                },
            };
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
                .is_none_or(|(a, _)| *a != Arrive::Stay);
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
        self.situation(lo, hi, self.allowed.clone(), inner, &uncut, 0.0)
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
        // Shrunk, so slivers a rounding error wide are not passes.
        let remaining = uncut.offset(-self.p.sliver, self.p.coarse);
        // A radius clear of the material, grown back by what it was shrunk.
        let free =
            allowed.difference(&remaining.offset(self.p.radius + self.p.sliver, self.p.coarse))?;
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
                    .unwrap_or(0);
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

    /// Whether the way from `from` to `to` may be made at the level: short
    /// enough, clear of the part, and through what is cleared.
    pub(super) fn stays_down(&self, s: &Situation, from: DVec2, to: DVec2) -> bool {
        if from.distance(to) > self.p.stay_down || !s.knows(from) || !s.knows(to) {
            return false;
        }
        // Where the passes end and start the tool only grazes the material, so
        // it may come a little into the tool's reach.
        s.allowed.contains_segment(from, to)
            && clear_of(&s.remaining, from, to, self.p.radius * 0.9)
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
                self.uncut.subtract(&Region::union_of(&[d])?)?;
                self.window = None;
            }
            self.helixes.push((centre, r));
            return Ok(Some((start, heading, side, Arrive::Enter { centre })));
        }
        Ok(None)
    }

    /// Take what the trail has swept out of the material.
    pub(super) fn flush(&mut self) -> Result<(), StrategyError> {
        if self.trail.len() >= 2 {
            let last = self.trail.last().copied();
            let lines = vec![std::mem::take(&mut self.trail)];
            let swept = Region::around_polylines(&lines, self.p.radius, self.p.swept_arc);
            if let Some(w) = &mut self.window {
                w.uncut = w.uncut.difference(&swept)?;
            }
            self.uncut.subtract(&swept)?;
            self.trail = last.into_iter().collect();
        }
        self.trail_length = 0.0;
        Ok(())
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
        self.window = Some(Window {
            lo,
            hi,
            uncut: self.uncut.window(lo, hi)?,
            allowed: self.room.window(lo, hi)?,
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
            }
            // Short steps where it turns, long where it runs straight.
            step = if chosen.turn.abs() > 0.3 {
                (step * 0.5).max(self.p.shortest)
            } else if chosen.turn.abs() < 0.1 {
                (step * 1.5).min(self.p.longest)
            } else {
                step
            };
            if self.trail_length > self.p.radius || self.trail.len() > 48 {
                self.flush()?;
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
