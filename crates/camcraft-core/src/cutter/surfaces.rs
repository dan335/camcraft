//! The part as an operation's tool meets it.
//!
//! Every 3D strategy drops its tool onto the part, and those that cut at
//! levels ask where at a level it may not be. Each asks it of [`PartSurfaces`],
//! not of the part's mesh, so what an operation meets is said in one place,
//! and no strategy needs to know what it is: the whole part, met by the
//! operation's tool, or the part's faces in groups.
//!
//! A tool is held over it once, as a [`Seat`], before anything is dropped:
//! whatever the part asks of the tool is worked out then, and a drop is the
//! drop-cutter's own, exact, with nothing worked out again for it.
//!
//! # Groups
//!
//! An operation's [`Surfaces`](crate::job::Surfaces) put the part's faces in
//! groups, each machined or avoided, and each met by the tool grown by the
//! group's own offsets as well as the operation's stock to leave. Each group is
//! a mesh of its faces with an index of its own. The tool rests on whatever is
//! highest, as Fusion's does: dropped, it comes to rest on the highest of each
//! group's drops, exactly, and at a level it may be only where none of them
//! forbids it. A pass laid on that is kept where it rests on a face machined,
//! and cut out where it rests on one avoided alone ([`Seat::avoids`]); resting
//! on both, to within the tolerance, it is machined, as Fusion's ring round
//! the foot of an avoided wall, touching the machined floor, stayed.
//!
//! An operation that puts nothing in groups meets the whole part, through the
//! drop-cutter the part's every operation shares, exactly as before there were
//! groups.

use std::sync::{Arc, OnceLock};

use camcraft_geom::bvh::MeshBvh;
use camcraft_geom::contour::{Region, RegionError};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::shadow::{Shadow, shadow_above};
use glam::{DVec2, DVec3, dvec3};

use crate::cutter::allowance::{Allowance, EffectiveTool};
use crate::cutter::dropcutter::DropCutter;
use crate::cutter::tool::{ToolError, ToolProfile};
use crate::strategy::StrategyError;
use crate::strategy::adaptive::flat_heights;
use crate::strategy::merged_highest;
use crate::strategy::reach::{LevelReach, ReachError};
use crate::strategy::scallop::{contact_region, past_contact};

/// The part as an operation's tool meets it: what the tool comes to rest on,
/// dropped anywhere, and what keeps it off at a level.
#[derive(Debug)]
pub struct PartSurfaces<'a> {
    form: Form<'a>,
}

/// What the part is, to the operation.
#[derive(Debug)]
enum Form<'a> {
    /// All of it, as it is, met by the tool itself.
    Whole(DropCutter<'a>),
    /// Its faces in groups, each met by the tool as the group grows it.
    Grouped(Vec<Group>),
}

/// Some of the part's faces, as an operation meets them: their triangles,
/// with an index of their own, how much more the tool is grown to meet them
/// than by the operation's stock to leave, and whether they are machined.
#[derive(Debug)]
pub struct Group {
    mesh: TriMesh,
    bvh: Arc<MeshBvh>,
    offset: Allowance,
    machined: bool,
    /// Its faces' shadow from above, cast the first time it is asked for.
    shadow: OnceLock<Shadow>,
}

impl Group {
    /// The faces `mesh`, met by the tool grown by `offset` as well as by the
    /// operation's stock to leave: machined, or avoided.
    pub fn new(mesh: TriMesh, offset: Allowance, machined: bool) -> Self {
        let bvh = Arc::new(MeshBvh::new(&mesh));
        Self {
            mesh,
            bvh,
            offset,
            machined,
            shadow: OnceLock::new(),
        }
    }

    /// The shadow its faces cast from above, from below all of them.
    fn shadow(&self) -> Result<&Shadow, RegionError> {
        if let Some(cast) = self.shadow.get() {
            return Ok(cast);
        }
        let below = self.mesh.bounds().map_or(0.0, |b| b.min.z) - 1.0;
        let cast = shadow_above(&self.mesh, below)?;
        Ok(self.shadow.get_or_init(|| cast))
    }
}

impl<'a> PartSurfaces<'a> {
    /// The whole part, as `cutter` drops onto it: the part's own drop-cutter,
    /// sharing its index with every other.
    pub fn whole(cutter: DropCutter<'a>) -> Self {
        Self {
            form: Form::Whole(cutter),
        }
    }

    /// The part as `groups` of its faces, between them every face it has.
    /// A group of no triangles is left out: there is nothing of it to meet.
    pub fn grouped(groups: Vec<Group>) -> Self {
        Self {
            form: Form::Grouped(groups.into_iter().filter(|g| !g.mesh.is_empty()).collect()),
        }
    }

    /// `tool` held over the part, to be dropped onto it anywhere: over each
    /// group grown by its offsets too, which a cone cannot be
    /// ([`ToolError::ConeStock`]).
    pub fn under(&self, tool: &EffectiveTool) -> Result<Seat<'_>, ToolError> {
        let on = match &self.form {
            Form::Whole(cutter) => On::Whole(cutter),
            Form::Grouped(groups) => On::Grouped(
                groups
                    .iter()
                    .map(|g| {
                        Ok(Held {
                            cutter: DropCutter::sharing(&g.mesh, Arc::clone(&g.bvh)),
                            tool: grown(tool, g.offset)?,
                            machined: g.machined,
                        })
                    })
                    .collect::<Result<_, ToolError>>()?,
            ),
        };
        Ok(Seat { tool: *tool, on })
    }

    /// Where at a level `tool` may not be, against the part, to `tolerance`
    /// ([`LevelReach::new`]): against each group, grown by its offsets too,
    /// wherever any of them forbids it ([`LevelReach::grouped`]).
    pub fn reach(
        &self,
        tool: &EffectiveTool,
        tolerance: f64,
    ) -> Result<LevelReach<'_>, ReachError> {
        match &self.form {
            Form::Whole(cutter) => LevelReach::new(cutter.mesh(), tool, tolerance),
            Form::Grouped(groups) => {
                let meshes = groups
                    .iter()
                    .map(|g| Ok((&g.mesh, grown(tool, g.offset)?)))
                    .collect::<Result<Vec<_>, ToolError>>()?;
                LevelReach::grouped(meshes, tolerance)
            }
        }
    }

    /// The widest `tool` is grown to meet any of the part: where it touches
    /// the part, seen from above, is within this of the part's shadow.
    pub fn widest(&self, tool: &EffectiveTool) -> f64 {
        match &self.form {
            Form::Whole(_) => tool.max_radius(),
            Form::Grouped(groups) => groups
                .iter()
                .map(|g| tool.max_radius() + g.offset.radial())
                .fold(tool.max_radius(), f64::max),
        }
    }

    /// Where `tool` touches the part, seen from above, to `tolerance`
    /// ([`contact_region`]): within its widest radius of `shadow`, the whole
    /// part's; and, where a group grows it wider, within that radius of the
    /// group's own faces. Grown as wide as the widest group all round, the
    /// part would be touched where the tool meets nothing, and a pattern laid
    /// from where it stops touching, as scallop's is, would move.
    pub fn contact(
        &self,
        shadow: &Shadow,
        tool: &EffectiveTool,
        tolerance: f64,
    ) -> Result<Region, StrategyError> {
        self.touched(shadow, tool, tolerance, contact_region)
    }

    /// As [`contact`](Self::contact), a tolerance past where `tool` stops
    /// touching the part rather than short of it ([`past_contact`]): for a
    /// pattern that finds that line itself, as scallop's does.
    pub fn past_contact(
        &self,
        shadow: &Shadow,
        tool: &EffectiveTool,
        tolerance: f64,
    ) -> Result<Region, StrategyError> {
        self.touched(shadow, tool, tolerance, past_contact)
    }

    /// `region` round the part's shadow and each group's grown wider.
    fn touched(
        &self,
        shadow: &Shadow,
        tool: &EffectiveTool,
        tolerance: f64,
        region: impl Fn(&Shadow, f64, f64) -> Result<Region, StrategyError>,
    ) -> Result<Region, StrategyError> {
        let radius = tool.max_radius();
        let mut touched = region(shadow, radius, tolerance)?;
        if let Form::Grouped(groups) = &self.form {
            for g in groups.iter().filter(|g| g.offset.radial() > 0.0) {
                let wider = region(g.shadow()?, radius + g.offset.radial(), tolerance)?;
                touched = touched.union(&wider)?;
            }
        }
        Ok(touched)
    }

    /// The heights of each group's flat faces that face up, found to
    /// `tolerance` as [`flat_heights`] finds them, each raised by the group's
    /// own axial offset: a level at a flat is raised by that as well as by the
    /// operation's stock to leave. The groups avoided too, where `avoided`: a
    /// wall machined standing on a flat avoided has its foot that flat's
    /// clearance above it. Heights within a tenth of the tolerance of each
    /// other are one, the highest. `None` for the whole part, whose flats are
    /// its own ([`Prepared::flat_heights`]).
    ///
    /// [`Prepared::flat_heights`]: crate::setup::Prepared::flat_heights
    pub fn flat_heights(&self, tolerance: f64, avoided: bool) -> Option<Vec<f64>> {
        let Form::Grouped(groups) = &self.form else {
            return None;
        };
        let raised = groups
            .iter()
            .filter(|g| g.machined || avoided)
            .flat_map(|g| {
                let axial = g.offset.axial();
                flat_heights(&g.mesh, tolerance)
                    .into_iter()
                    .map(move |z| z + axial)
            })
            .collect();
        Some(merged_highest(raised, tolerance * 0.1))
    }

    /// Whether any of the part is avoided.
    pub fn avoids_any(&self) -> bool {
        match &self.form {
            Form::Whole(_) => false,
            Form::Grouped(groups) => groups.iter().any(|g| !g.machined),
        }
    }
}

/// `tool` grown by `offset` as well.
fn grown(tool: &EffectiveTool, offset: Allowance) -> Result<EffectiveTool, ToolError> {
    EffectiveTool::of(tool.base(), *tool.allowance() + offset)
}

/// A tool held over the part as an operation meets it
/// ([`PartSurfaces::under`]): where it comes to rest, dropped at any point.
#[derive(Debug)]
pub struct Seat<'s> {
    tool: EffectiveTool,
    on: On<'s>,
}

/// What a held tool is dropped onto.
#[derive(Debug)]
enum On<'s> {
    /// The whole part, by its drop-cutter.
    Whole(&'s DropCutter<'s>),
    /// Each group, by a drop-cutter of its own, with the tool as it meets it.
    Grouped(Vec<Held<'s>>),
}

/// The tool as it meets one group of the part's faces.
#[derive(Debug)]
struct Held<'s> {
    cutter: DropCutter<'s>,
    tool: EffectiveTool,
    machined: bool,
}

/// Where a tool dropped onto the part comes to rest, and whether a face it
/// machines holds it there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rest {
    /// The tool's tip.
    pub z: f64,
    /// Whether a face machined holds it there, to within the tolerance asked:
    /// not where it rests on faces avoided alone.
    pub machined: bool,
}

impl Seat<'_> {
    /// The tool held, as it was given: over a part in groups, the narrowest
    /// any of them grows it to, since their offsets are never negative.
    pub fn tool(&self) -> &EffectiveTool {
        &self.tool
    }

    /// Whether any of the part is avoided: never, on the whole part.
    pub fn avoids_any(&self) -> bool {
        match &self.on {
            On::Whole(_) => false,
            On::Grouped(held) => held.iter().any(|h| !h.machined),
        }
    }

    /// Where the tool's tip comes to rest dropped at `(x, y)`, or `None` where
    /// nothing is under it ([`DropCutter::drop_at`]): over a part in groups,
    /// the highest any of them holds it.
    #[inline]
    pub fn drop_at(&self, x: f64, y: f64) -> Option<f64> {
        match &self.on {
            On::Whole(cutter) => cutter.drop_at(&self.tool, x, y),
            On::Grouped(held) => highest(held.iter().map(|h| h.cutter.drop_at(&h.tool, x, y))),
        }
    }

    /// As [`drop_at`](Self::drop_at), with `scratch` for the triangles it
    /// looks at: for a caller making many drops, which then allocates once.
    #[inline]
    pub fn drop_at_in(&self, x: f64, y: f64, scratch: &mut Vec<u32>) -> Option<f64> {
        match &self.on {
            On::Whole(cutter) => cutter.drop_at_in(&self.tool, x, y, scratch),
            On::Grouped(held) => highest(
                held.iter()
                    .map(|h| h.cutter.drop_at_in(&h.tool, x, y, scratch)),
            ),
        }
    }

    /// Where the tool comes to rest dropped at `(x, y)`, and whether a face
    /// it machines holds it there: over a part in groups, whether the group
    /// that holds it highest is one machined, a machined one winning a tie
    /// to rounding; always on the whole part. `None` where nothing is under
    /// it. Exact, where [`rest_at`](Self::rest_at) asks to a tolerance: the
    /// change from one to the other along a line is found where it is, and
    /// not a tolerance's height either side of it, which across a face
    /// running smoothly into one avoided is far along it.
    pub fn rest_in(&self, x: f64, y: f64, scratch: &mut Vec<u32>) -> Option<Rest> {
        match &self.on {
            On::Whole(cutter) => cutter
                .drop_at_in(&self.tool, x, y, scratch)
                .map(|z| Rest { z, machined: true }),
            On::Grouped(held) => {
                let mut best: Option<Rest> = None;
                for h in held {
                    let Some(z) = h.cutter.drop_at_in(&h.tool, x, y, scratch) else {
                        continue;
                    };
                    // As high to rounding is a tie, which the machined win.
                    let noise = 1e-9 * (1.0 + z.abs());
                    let higher = best.is_none_or(|b| {
                        z > b.z + noise || (z >= b.z - noise && h.machined && !b.machined)
                    });
                    if higher {
                        best = Some(Rest {
                            z,
                            machined: h.machined,
                        });
                    }
                }
                best
            }
        }
    }

    /// The way, seen from above, from `at` toward a wall that rises past `z`
    /// beside it: the middle of the arc of ways round `at`, `reach` out,
    /// where the tool dropped comes to rest above `z`, its ends found by
    /// halving. Beside a wall straight, or curved no tighter than `reach`,
    /// that arc faces it, its middle square to it. `None` where the tool
    /// rests above `z` every way round or none, or the ways above it are not
    /// one arc.
    pub fn upright_toward(
        &self,
        at: DVec2,
        z: f64,
        reach: f64,
        scratch: &mut Vec<u32>,
    ) -> Option<DVec2> {
        /// Ways looked along first.
        const WAYS: u32 = 16;
        /// Halvings of the angle between two of them where the arc ends:
        /// far past any tolerance at any reach.
        const HALVINGS: u32 = 30;
        let step = std::f64::consts::TAU / f64::from(WAYS);
        let above = |angle: f64, scratch: &mut Vec<u32>| {
            let q = at + DVec2::from_angle(angle) * reach;
            self.drop_at_in(q.x, q.y, scratch).is_some_and(|h| h > z)
        };
        let ways: Vec<bool> = (0..WAYS)
            .map(|k| above(step * f64::from(k), scratch))
            .collect();
        // Where the arc starts, going round, and where it ends.
        let (mut start, mut end) = (None, None);
        for (k, (&here, &there)) in (0..WAYS).zip(ways.iter().zip(ways.iter().cycle().skip(1))) {
            if here == there {
                continue;
            }
            let (mut lo, mut hi) = (step * f64::from(k), step * f64::from(k + 1));
            for _ in 0..HALVINGS {
                let mid = 0.5 * (lo + hi);
                if above(mid, scratch) == here {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let crossing = 0.5 * (lo + hi);
            let slot = if there { &mut start } else { &mut end };
            if slot.replace(crossing).is_some() {
                return None;
            }
        }
        let (start, mut end) = (start?, end?);
        if end < start {
            end += std::f64::consts::TAU;
        }
        Some(DVec2::from_angle(0.5 * (start + end)))
    }

    /// What the tool touches with its tip at `z` over `at`, beside a face that
    /// stands upright toward `toward` (a unit vector seen from above): the
    /// point of the part nearest the tool's widest circle on that side, and
    /// whether a face it machines holds it; `None` where nothing is within
    /// the tool's radius of it.
    ///
    /// Where the tool's path stands upright, beside a wall, a drop rests at
    /// its foot or at its top and finds nothing between: moved up beside the
    /// wall, the tool touches it where it is widest, its profile's largest
    /// radius at the height it reaches it ([`ToolProfile::height_at`]), a
    /// ball at its equator, a flat mill at its tip's edge. It is looked for
    /// as far as the tool's radius, not the tolerance: a wall a hair off
    /// upright, or hollowed a little under a rim above it, is still the face
    /// the tool rises beside, the tool within its radius of nothing else,
    /// where a hollow of half a tenth under a user's bowl's rim, deeper than
    /// the tolerance, flipped the answer along it. Over a part in groups, the
    /// nearest of them, the machined winning a tie, as a drop's.
    pub fn beside(&self, at: DVec2, toward: DVec2, z: f64) -> Option<(DVec3, bool)> {
        let widest = |tool: &EffectiveTool| {
            let r = tool.max_radius();
            let h = tool.height_at(r).unwrap_or(0.0);
            (at + toward * r).extend(z - tool.tip_offset() + h)
        };
        match &self.on {
            On::Whole(cutter) => {
                let (_, point) = cutter.nearest(widest(&self.tool), self.tool.max_radius())?;
                Some((point, true))
            }
            On::Grouped(held) => {
                let mut best: Option<(f64, DVec3, bool)> = None;
                for h in held {
                    let q = widest(&h.tool);
                    let Some((_, point)) = h.cutter.nearest(q, h.tool.max_radius()) else {
                        continue;
                    };
                    let d = point.distance(q);
                    let noise = 1e-9 * (1.0 + d);
                    let nearer = best.is_none_or(|(b, _, machined)| {
                        d < b - noise || (d <= b + noise && h.machined && !machined)
                    });
                    if nearer {
                        best = Some((d, point, h.machined));
                    }
                }
                best.map(|(_, point, machined)| (point, machined))
            }
        }
    }

    /// Where the tool comes to rest dropped at `(x, y)`, and whether a face it
    /// machines holds it there, to within `tolerance`; `None` where nothing
    /// is under it. On the whole part it is always machined, and resting on a
    /// face machined and one avoided at once, it is machined.
    pub fn rest_at(&self, x: f64, y: f64, tolerance: f64) -> Option<Rest> {
        let z = self.drop_at(x, y)?;
        let machined = match &self.on {
            On::Whole(_) => true,
            On::Grouped(held) => held
                .iter()
                .any(|h| h.machined && h.meets(dvec3(x, y, z), tolerance)),
        };
        Some(Rest { z, machined })
    }

    /// Whether the tool with its tip at `p` comes within `tolerance` of faces
    /// it avoids, grown by their clearance, and of none it machines: there it
    /// would only rub what it is kept clear of. Never, on the whole part.
    ///
    /// Asked of the tool where it is, not where it would rest dropped: a ring
    /// beside a wall avoided touches it only from the side.
    pub fn avoids(&self, p: DVec3, tolerance: f64) -> bool {
        let On::Grouped(held) = &self.on else {
            return false;
        };
        let mut avoided = false;
        for h in held.iter().filter(|h| h.meets(p, tolerance)) {
            if h.machined {
                return false;
            }
            avoided = true;
        }
        avoided
    }

    /// Whether the tool with its tip at `p` comes within `tolerance` of a face
    /// it machines: on the whole part, of any of it. In the air, touching
    /// nothing, it does not.
    pub fn machines(&self, p: DVec3, tolerance: f64) -> bool {
        match &self.on {
            On::Whole(cutter) => {
                drop_by(cutter, &self.tool, (p.x, p.y), tolerance).is_some_and(|z| z >= p.z)
            }
            On::Grouped(held) => held.iter().any(|h| h.machined && h.meets(p, tolerance)),
        }
    }

    /// How far the tool with its tip at `p` is inside the clearance of a face
    /// it avoids, past `slack`: how much higher than `p` it would come to rest
    /// dropped onto the faces avoided, their clearance less `slack`. Nothing
    /// where it is clear of them, and on the whole part.
    pub fn into_avoided(&self, p: DVec3, slack: f64) -> f64 {
        let On::Grouped(held) = &self.on else {
            return 0.0;
        };
        held.iter()
            .filter(|h| !h.machined)
            .filter_map(|h| h.drop_by(p.x, p.y, -slack))
            .map(|z| z - p.z)
            .fold(0.0, f64::max)
    }
}

impl Held<'_> {
    /// Whether the tool with its tip at `p` comes within `by` of these faces:
    /// grown by `by` all round, it would come to rest no lower than `p`.
    fn meets(&self, p: DVec3, by: f64) -> bool {
        self.drop_by(p.x, p.y, by).is_some_and(|z| z >= p.z)
    }

    /// Where the tool, grown by `by` all round, comes to rest dropped at
    /// `(x, y)` onto these faces ([`drop_by`]).
    fn drop_by(&self, x: f64, y: f64, by: f64) -> Option<f64> {
        drop_by(&self.cutter, &self.tool, (x, y), by)
    }
}

/// Where `tool`, grown by `by` all round, comes to rest dropped at `(x, y)`
/// by `cutter`; shrunk, where `by` is less than nothing. Grown, it is the tool
/// with a ball of `by` round it, so it rests at or above a point just where
/// the tool there is within `by` of the faces. A tool that cannot be so,
/// shrunk past nothing, or a cone, which grown is no cone, has its drop taken
/// `by` higher instead, as if lowered that far: as much nearer the faces from
/// above, if not from the side. No operation that avoids faces cuts with a
/// cone ([`Operation::cannot_use`]).
///
/// [`Operation::cannot_use`]: crate::job::Operation::cannot_use
fn drop_by(
    cutter: &DropCutter<'_>,
    tool: &EffectiveTool,
    (x, y): (f64, f64),
    by: f64,
) -> Option<f64> {
    let reshaped = Allowance::isotropic(by)
        .ok()
        .and_then(|by| EffectiveTool::of(tool.base(), *tool.allowance() + by).ok());
    match reshaped {
        Some(grown) => cutter.drop_at(&grown, x, y),
        None => cutter.drop_at(tool, x, y).map(|z| z + by),
    }
}

/// The highest of `drops`, `None` where none has anything under it.
fn highest(drops: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    drops.flatten().reduce(f64::max)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests;
