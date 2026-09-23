//! A setup made ready to run: its models merged and turned the way the part sits,
//! its stock laid out, and everything measured from its work zero.
//!
//! Past this point everything is in the setup frame. Strategies, the linker, the
//! simulator and the post never see a rotation or a model coordinate.

use std::collections::BTreeMap;

use camcraft_geom::mesh::{Aabb, MeshError, TriMesh};
use glam::DVec3;

use crate::frame::{BadOrientation, Frame};
use crate::job::{BoxOf, Job, JobError, Origin, Setup, StockSpec};

/// A setup's models, by name, in model coordinates.
pub type Meshes<'a> = BTreeMap<&'a str, &'a TriMesh>;

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
}

/// A setup in its own frame.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// Every model the setup machines, as one mesh.
    pub part: TriMesh,
    /// Its bounds.
    pub model_bounds: Aabb,
    /// Each stock block.
    pub stocks: Vec<Aabb>,
    /// How model coordinates became these.
    pub frame: Frame,
}

impl Prepared {
    /// The box around every stock block.
    pub fn stock_bounds(&self) -> Aabb {
        let mut it = self.stocks.iter();
        let first = it.next().copied().unwrap_or(self.model_bounds);
        it.fold(first, |a, b| a.union(b))
    }
}

/// The box around the named models, in model coordinates.
fn bounds_of(names: &[String], meshes: &Meshes<'_>) -> Result<Aabb, PrepareError> {
    let mut out: Option<Aabb> = None;
    for n in names {
        let mesh = meshes
            .get(n.as_str())
            .ok_or_else(|| PrepareError::NotLoaded(n.clone()))?;
        let b = mesh
            .bounds()
            .ok_or_else(|| PrepareError::EmptyModel(n.clone()))?;
        out = Some(out.map_or(b, |o| o.union(&b)));
    }
    out.ok_or_else(|| PrepareError::Job(JobError::NoModels(String::new())))
}

/// Where setup `index`'s part and stock are, in its own frame.
pub fn prepare(job: &Job, index: usize, meshes: &Meshes<'_>) -> Result<Prepared, PrepareError> {
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
            meshes
                .get(n.as_str())
                .copied()
                .ok_or_else(|| PrepareError::NotLoaded(n.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let model_box = bounds_of(&setup.models, meshes)?;

    // Each block, in model coordinates.
    let mut blocks = Vec::with_capacity(setup.stocks.len());
    for name in &setup.stocks {
        let def = job.stock(name).ok_or_else(|| JobError::UnknownStock {
            setup: setup.name.clone(),
            stock: name.clone(),
        })?;
        blocks.push(match &def.spec {
            StockSpec::AroundModels {
                models,
                margin,
                below,
            } => {
                let around = if models.is_empty() {
                    model_box
                } else {
                    bounds_of(models, meshes)?
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
            StockSpec::FixedBox { corner, size } => {
                let min = DVec3::from_array(*corner);
                Aabb {
                    min,
                    max: min + DVec3::from_array(*size),
                }
            }
        });
    }
    if blocks.is_empty() {
        return Err(JobError::NoStock(setup.name.clone()).into());
    }

    // Turned the way the part sits. The turn only swaps and negates, so a box
    // stays a box: its corners' low and high ends are the new box.
    let o = setup.orientation;
    let turn = |b: &Aabb| {
        let (a, c) = (o.apply(b.min), o.apply(b.max));
        Aabb {
            min: a.min(c),
            max: a.max(c),
        }
    };
    let turned_blocks: Vec<Aabb> = blocks.iter().map(turn).collect();
    let turned_model = turn(&model_box);

    let origin = match setup.origin {
        Origin::ModelOrigin { offset } => DVec3::from_array(offset),
        Origin::BoxPoint {
            of,
            x,
            y,
            z,
            offset,
        } => {
            let b = match of {
                BoxOf::Stock => turned_blocks
                    .iter()
                    .fold(None, |a: Option<Aabb>, b| {
                        Some(a.map_or(*b, |a| a.union(b)))
                    })
                    .unwrap_or(turned_model),
                BoxOf::Model => turned_model,
            };
            DVec3::new(
                x.pick(b.min.x, b.max.x),
                y.pick(b.min.y, b.max.y),
                z.pick(b.min.z, b.max.z),
            ) + DVec3::from_array(offset)
        }
    };
    let frame = Frame {
        orientation: o,
        origin,
    };
    let shift = |b: &Aabb| Aabb {
        min: b.min - origin,
        max: b.max - origin,
    };

    let merged;
    let part_in_model = if let [one] = parts.as_slice() {
        *one
    } else {
        merged = TriMesh::merged(&parts)?;
        &merged
    };
    let part = part_in_model.mapped(|p| frame.to_setup(p))?;
    let model_bounds = part
        .bounds()
        .ok_or_else(|| PrepareError::EmptyModel(setup.name.clone()))?;
    Ok(Prepared {
        part,
        model_bounds,
        stocks: turned_blocks.iter().map(shift).collect(),
        frame,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::frame::{Axis, Orientation};
    use crate::job::{Along, ModelDef, StockDef};
    use glam::dvec3;

    fn job(stock: StockSpec) -> Job {
        let mut j = Job::default();
        j.models.push(ModelDef {
            name: "m".into(),
            path: "m.stl".into(),
            units: None,
        });
        j.stocks.push(StockDef {
            name: "s".into(),
            spec: stock,
        });
        let mut s = Setup::new("top");
        s.models = vec!["m".into()];
        s.stocks = vec!["s".into()];
        j.setups.push(s);
        j
    }

    /// A box from (0, 0, 0) to (10, 20, 5).
    fn brick() -> TriMesh {
        TriMesh::new(
            vec![
                dvec3(0.0, 0.0, 0.0),
                dvec3(10.0, 20.0, 5.0),
                dvec3(10.0, 0.0, 0.0),
            ],
            vec![[0, 1, 2]],
        )
        .unwrap()
    }

    /// As modelled, with the model's own zero, nothing moves: the path the old
    /// single-setup job took.
    #[test]
    fn the_default_setup_is_the_model_as_drawn() {
        let mesh = brick();
        let meshes: Meshes<'_> = [("m", &mesh)].into_iter().collect();
        let j = job(StockSpec::AroundModels {
            models: vec![],
            margin: [2.0, 2.0, 3.0],
            below: 0.0,
        });
        let p = prepare(&j, 0, &meshes).unwrap();
        assert_eq!(p.part, mesh);
        assert_eq!(p.stocks[0].min, dvec3(-2.0, -2.0, 0.0));
        assert_eq!(p.stocks[0].max, dvec3(12.0, 22.0, 8.0));
    }

    /// Turned over, the underside is on top, and a work zero at the stock's top
    /// centre puts the top of the block at Z = 0.
    #[test]
    fn a_flipped_setup_sees_the_underside() {
        let mesh = brick();
        let meshes: Meshes<'_> = [("m", &mesh)].into_iter().collect();
        let mut j = job(StockSpec::FixedBox {
            corner: [-1.0, -1.0, -1.0],
            size: [12.0, 22.0, 7.0],
        });
        j.setups[0].orientation = Orientation::default().flipped();
        j.setups[0].origin = Origin::BoxPoint {
            of: BoxOf::Stock,
            x: Along::Center,
            y: Along::Center,
            z: Along::Max,
            offset: [0.0; 3],
        };
        let p = prepare(&j, 0, &meshes).unwrap();
        let s = p.stocks[0];
        assert_eq!(s.max.z, 0.0);
        assert_eq!(s.min.z, -7.0);
        assert_eq!((s.min.x + s.max.x) / 2.0, 0.0);
        assert_eq!((s.min.y + s.max.y) / 2.0, 0.0);
        // The model's bottom (z = 0) is now 1 mm under the stock top.
        assert_eq!(p.model_bounds.max.z, -1.0);
        // And the frame takes a model point to where it now is.
        assert_eq!(p.frame.to_setup(dvec3(0.0, 0.0, 0.0)).z, -1.0);
    }

    /// One block serves a top and a bottom setup: the same material, seen from
    /// each side.
    #[test]
    fn two_setups_share_one_block() {
        let mesh = brick();
        let meshes: Meshes<'_> = [("m", &mesh)].into_iter().collect();
        let mut j = job(StockSpec::AroundModels {
            models: vec![],
            margin: [1.0, 1.0, 1.0],
            below: 2.0,
        });
        let mut bottom = j.setups[0].clone();
        bottom.name = "bottom".into();
        bottom.orientation = Orientation {
            z: Axis::NegZ,
            x: Axis::PosX,
        };
        j.setups.push(bottom);
        let top = prepare(&j, 0, &meshes).unwrap();
        let under = prepare(&j, 1, &meshes).unwrap();
        let (a, b) = (top.stocks[0], under.stocks[0]);
        assert_eq!(a.extent(), b.extent());
        // Model zero stays zero, so the block's top is the top's 6 and the
        // bottom's 2.
        assert_eq!(a.max.z, 6.0);
        assert_eq!(b.max.z, 2.0);
    }

    #[test]
    fn a_missing_mesh_is_named() {
        let meshes: Meshes<'_> = BTreeMap::new();
        let j = job(StockSpec::default());
        assert!(matches!(
            prepare(&j, 0, &meshes),
            Err(PrepareError::NotLoaded(n)) if n == "m"
        ));
    }
}
