//! A setup made ready to run: its models merged and turned the way the part sits,
//! its stock laid out, and everything measured from its work zero.
//!
//! Past this point everything is in the setup frame. Strategies, the linker, the
//! simulator and the post never see a rotation or a model coordinate. The one way
//! back is [`Prepared::frame`]: a pick is found on its own model, where its point
//! is in model coordinates, and brought into the frame ([`crate::setup::features`]).
//! [`Prepared::pieces`] names the models the part was merged from.
//!
//! What every operation of the setup reads from the part alone is worked out
//! once, when first asked for, and kept with it: its shadow from above.

pub mod features;
pub mod frame;

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use camcraft_geom::bvh::MeshBvh;
use camcraft_geom::contour::RegionError;
use camcraft_geom::mesh::{Aabb, MeshError, TriMesh};
use camcraft_geom::shadow::{Shadow, shadow_above};
use camcraft_geom::topology::Body;
use camcraft_sim::stock::{PartSurface, StockError};
use glam::DVec3;

use crate::cutter::dropcutter::DropCutter;
use crate::job::{BoxOf, HeightContext, Job, JobError, Origin, Setup, StockSpec};
use crate::memo::Memo;
use crate::strategy::adaptive::flat_heights;
use features::FeatureError;
use frame::{BadOrientation, Frame, Orientation};

/// A setup's models, by name, in model coordinates, with their topology.
pub type Bodies<'a> = BTreeMap<&'a str, &'a Body>;

/// A setup that could not be prepared.
#[derive(Debug, thiserror::Error)]
pub enum PrepareError {
    /// The job did not describe it fully.
    #[error(transparent)]
    Job(#[from] JobError),
    /// A model it needs was not given.
    #[error("model '{0}' is not loaded")]
    NotLoaded(String),
    /// A model it needs has no geometry.
    #[error("model '{0}' has no geometry")]
    EmptyModel(String),
    /// Its orientation is not square.
    #[error(transparent)]
    Orientation(#[from] BadOrientation),
    /// The models could not be merged.
    #[error(transparent)]
    Mesh(#[from] MeshError),
    /// The work zero was picked on something that is no longer there.
    #[error("work zero: {0}")]
    Feature(#[from] FeatureError),
}

/// One of the models merged into the part.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    /// The model, by name.
    pub model: String,
}

/// A setup in its own frame.
///
/// Its part is not to be changed once laid out: what is worked out from it
/// alone is kept with it, for every run that uses it to share: its shadow,
/// the index drops onto it search, the surface the simulator judges against,
/// and the heights of its flats.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// Every model the setup machines, as one mesh.
    pub part: TriMesh,
    /// The models it was merged from, in the setup's order.
    pub pieces: Vec<Piece>,
    /// Its bounds.
    pub model_bounds: Aabb,
    /// Each stock block.
    pub stocks: Vec<Aabb>,
    /// How model coordinates became these.
    pub frame: Frame,
    /// The part's shadow, once cast ([`Prepared::shadow`]).
    shadow: OnceLock<Shadow>,
    /// The part's spatial index, once built ([`Prepared::drop_cutter`]).
    bvh: OnceLock<Arc<MeshBvh>>,
    /// The part as the simulator sees it, for the grid pitch it was laid out
    /// at ([`Prepared::part_surface`]).
    surface: Memo<u64, PartSurface>,
    /// The heights of the part's flats, for the tolerance they were found to
    /// ([`Prepared::flat_heights`]).
    flats: Memo<u64, Vec<f64>>,
}

impl Prepared {
    /// Model `model`, if the part was merged from it.
    pub fn piece(&self, model: &str) -> Option<&Piece> {
        self.pieces.iter().find(|p| p.model == model)
    }

    /// The box around every stock block.
    pub fn stock_bounds(&self) -> Aabb {
        Aabb::union_all(&self.stocks).unwrap_or(self.model_bounds)
    }

    /// What a height resolves against in this setup: its stock's top and
    /// bottom, and its models'. No picks: an operation's are its own.
    pub fn heights(&self) -> HeightContext {
        let all = self.stock_bounds();
        HeightContext {
            stock_top: all.max.z,
            stock_bottom: all.min.z,
            model_top: self.model_bounds.max.z,
            model_bottom: self.model_bounds.min.z,
            picked: None,
        }
    }

    /// The part's shadow from straight above, cast onto a level below it:
    /// everything it covers, seen from the top. Its silhouette, and where a
    /// tool can touch it, are grown from this.
    ///
    /// Cast once, when first asked for, and kept for every operation after. A
    /// shadow that cannot be cast is not kept, since its error cannot be: it
    /// is cast again, and fails again, each time it is asked for.
    pub fn shadow(&self) -> Result<&Shadow, RegionError> {
        if let Some(cast) = self.shadow.get() {
            return Ok(cast);
        }
        let cast = shadow_above(&self.part, self.model_bounds.min.z - 1.0)?;
        Ok(self.shadow.get_or_init(|| cast))
    }

    /// Drops onto the part, its index built the first time one is asked for.
    pub fn drop_cutter(&self) -> DropCutter<'_> {
        let bvh = self.bvh.get_or_init(|| Arc::new(MeshBvh::new(&self.part)));
        DropCutter::sharing(&self.part, Arc::clone(bvh))
    }

    /// The part as the simulator sees it, over the stock's footprint on a grid
    /// of `pitch`: laid out the first time it is asked for at that pitch.
    pub fn part_surface(&self, pitch: f64) -> Result<Arc<PartSurface>, StockError> {
        self.surface.get_or_try(pitch.to_bits(), || {
            let all = self.stock_bounds();
            PartSurface::from_mesh(&self.part, all.min.truncate(), all.max.truncate(), pitch)
        })
    }

    /// The heights of the part's flat faces that face up, found to
    /// `tolerance` ([`flat_heights`]): found the first time they are asked
    /// for to it.
    pub fn flat_heights(&self, tolerance: f64) -> Arc<Vec<f64>> {
        self.flats
            .get_or(tolerance.to_bits(), || flat_heights(&self.part, tolerance))
    }

    /// The part's shadow, if [`Prepared::shadow`] has cast it already: never
    /// casts it, so a window can ask without waiting while another thread
    /// casts it.
    pub fn cast_shadow(&self) -> Option<&Shadow> {
        self.shadow.get()
    }

    /// Its shadow as cast, if it has been, in the setup's turned frame: before
    /// the work zero is taken off. It is the same for every work zero and every
    /// stock, so it is kept for whatever setup lays out the same models turned
    /// the same way ([`Prepared::adopt_shadow`]), rather than cast again.
    pub fn turned_shadow(&self) -> Option<Shadow> {
        let origin = self.frame.origin.truncate();
        self.shadow.get().map(|s| s.translated(origin))
    }

    /// Take `turned`, the shadow of this setup's part in its turned frame, as
    /// [`Prepared::turned_shadow`] gave it for the same models, placed and
    /// tessellated the same, turned the same way: moved to its work zero, as
    /// its own, instead of casting one. Whether it was taken: not if it has one
    /// already.
    ///
    /// The work zero only moves the part, and the shadow is cast from below
    /// all of it, so moved by as much it is the shadow the part casts.
    pub fn adopt_shadow(&self, turned: &Shadow) -> bool {
        let origin = self.frame.origin.truncate();
        self.shadow.set(turned.translated(-origin)).is_ok()
    }
}

/// The box around the named models, in model coordinates.
fn bounds_of(names: &[String], bodies: &Bodies<'_>) -> Result<Aabb, PrepareError> {
    let mut out: Option<Aabb> = None;
    for n in names {
        let body = bodies
            .get(n.as_str())
            .ok_or_else(|| PrepareError::NotLoaded(n.clone()))?;
        let b = body
            .mesh()
            .bounds()
            .ok_or_else(|| PrepareError::EmptyModel(n.clone()))?;
        out = Some(out.map_or(b, |o| o.union(&b)));
    }
    out.ok_or_else(|| PrepareError::Job(JobError::NoModels(String::new())))
}

/// A stock block in model coordinates. One around no named models is around
/// `model_box`, the box of the models of the setup using it.
fn block(spec: &StockSpec, model_box: Aabb, bodies: &Bodies<'_>) -> Result<Aabb, PrepareError> {
    Ok(match spec {
        StockSpec::AroundModels {
            models,
            margin,
            below,
        } => {
            let around = if models.is_empty() {
                model_box
            } else {
                bounds_of(models, bodies)?
            };
            Aabb {
                min: DVec3::new(
                    around.min.x - margin[0],
                    around.min.y - margin[1],
                    around.min.z - below,
                ),
                max: DVec3::new(
                    around.max.x + margin[0],
                    around.max.y + margin[1],
                    around.max.z + margin[2],
                ),
            }
        }
        StockSpec::FixedBox(b) => {
            let against = if b.models.is_empty() {
                model_box
            } else {
                bounds_of(&b.models, bodies)?
            };
            let ([px, py, pz], [sx, sy, sz]) = (b.place, b.size);
            let (lo, hi) = (against.min, against.max);
            let min = DVec3::new(
                px.low(lo.x, hi.x, sx),
                py.low(lo.y, hi.y, sy),
                pz.low(lo.z, hi.z, sz),
            );
            Aabb {
                min,
                max: min + DVec3::from_array(b.size),
            }
        }
    })
}

/// A stock block in model coordinates, for a setup machining `models`: where it
/// is, whichever setup it is shown for.
pub fn stock_block(
    spec: &StockSpec,
    models: &[String],
    bodies: &Bodies<'_>,
) -> Result<Aabb, PrepareError> {
    block(spec, bounds_of(models, bodies)?, bodies)
}

/// `b` turned the way the part sits. The turn only swaps and negates, so a box
/// stays a box: its corners' low and high ends are the new box.
fn turn(o: Orientation, b: &Aabb) -> Aabb {
    let (a, c) = (o.apply(b.min), o.apply(b.max));
    Aabb {
        min: a.min(c),
        max: a.max(c),
    }
}

/// Where a stock block would be in a setup's `frame`, were the setup to use it:
/// for showing a stock the setup does not use, to be picked into it. `models` are
/// the setup's, which a block around no named models is around.
pub fn stock_in_frame(
    spec: &StockSpec,
    models: &[String],
    frame: &Frame,
    bodies: &Bodies<'_>,
) -> Result<Aabb, PrepareError> {
    let b = turn(frame.orientation, &stock_block(spec, models, bodies)?);
    Ok(b.translated(-frame.origin))
}

/// Where setup `index`'s part and stock are, in its own frame.
pub fn prepare(job: &Job, index: usize, bodies: &Bodies<'_>) -> Result<Prepared, PrepareError> {
    let setup: &Setup = job.setups.get(index).ok_or(JobError::NoSetups)?;
    setup.orientation.check()?;
    if setup.models.is_empty() {
        return Err(JobError::NoModels(setup.name.clone()).into());
    }

    // The part, in model coordinates.
    let parts = setup
        .models
        .iter()
        .map(|n| {
            bodies
                .get(n.as_str())
                .map(|b| b.mesh())
                .ok_or_else(|| PrepareError::NotLoaded(n.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let model_box = bounds_of(&setup.models, bodies)?;
    let blocks = blocks_of(job, setup, model_box, bodies)?;

    let o = setup.orientation;
    let turned_blocks: Vec<Aabb> = blocks.iter().map(|b| turn(o, b)).collect();
    let turned_model = turn(o, &model_box);
    let origin = work_zero(job, setup, bodies, &turned_blocks, turned_model)?;
    let frame = Frame {
        orientation: o,
        origin,
    };

    let merged;
    let part_in_model = if let [one] = parts.as_slice() {
        *one
    } else {
        merged = TriMesh::merged(&parts)?;
        &merged
    };
    let pieces = setup
        .models
        .iter()
        .map(|m| Piece { model: m.clone() })
        .collect();
    let part = part_in_model.mapped(|p| frame.to_setup(p))?;
    let model_bounds = part
        .bounds()
        .ok_or_else(|| PrepareError::EmptyModel(setup.name.clone()))?;
    Ok(Prepared {
        part,
        pieces,
        model_bounds,
        stocks: turned_blocks
            .iter()
            .map(|b| b.translated(-origin))
            .collect(),
        frame,
        shadow: OnceLock::new(),
        bvh: OnceLock::new(),
        surface: Memo::new(),
        flats: Memo::new(),
    })
}

/// Each of `setup`'s stock blocks, in model coordinates. A block around no named
/// models is around `model_box`, the setup's.
fn blocks_of(
    job: &Job,
    setup: &Setup,
    model_box: Aabb,
    bodies: &Bodies<'_>,
) -> Result<Vec<Aabb>, PrepareError> {
    let mut blocks = Vec::with_capacity(setup.stocks.len());
    for name in &setup.stocks {
        let def = job.stock(name).ok_or_else(|| JobError::UnknownStock {
            setup: setup.name.clone(),
            stock: name.clone(),
        })?;
        blocks.push(block(&def.spec, model_box, bodies)?);
    }
    if blocks.is_empty() {
        return Err(JobError::NoStock(setup.name.clone()).into());
    }
    Ok(blocks)
}

/// Where `setup`'s work zero is, with the part turned the way it sits: its blocks
/// are `turned_blocks` and its models' box `turned_model`, both turned.
fn work_zero(
    job: &Job,
    setup: &Setup,
    bodies: &Bodies<'_>,
    turned_blocks: &[Aabb],
    turned_model: Aabb,
) -> Result<DVec3, PrepareError> {
    let o = setup.orientation;
    Ok(match &setup.origin {
        Origin::ModelOrigin { offset } => DVec3::from_array(*offset),
        Origin::Point { point, offset } => {
            let body = bodies
                .get(point.model.as_str())
                .ok_or_else(|| FeatureError::NotLoaded(point.model.clone()))?;
            let tol = features::tolerance(job.tolerance, body);
            let (p, _) = features::find_point(body, point, tol)?;
            o.apply(p) + DVec3::from_array(*offset)
        }
        &Origin::BoxPoint {
            of,
            x,
            y,
            z,
            offset,
        } => {
            let b = match of {
                BoxOf::Stock => Aabb::union_all(turned_blocks).unwrap_or(turned_model),
                BoxOf::Model => turned_model,
            };
            DVec3::new(
                x.pick(b.min.x, b.max.x),
                y.pick(b.min.y, b.max.y),
                z.pick(b.min.z, b.max.z),
            ) + DVec3::from_array(offset)
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests;
