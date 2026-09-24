//! A setup made ready to run: its models merged and turned the way the part sits,
//! its stock laid out, and everything measured from its work zero.
//!
//! Past this point everything is in the setup frame. Strategies, the linker, the
//! simulator and the post never see a rotation or a model coordinate. The one way
//! back is [`Prepared::pieces`]: which triangles of the merged part came from which
//! model, so a triangle picked on the part is a triangle of a model, and so of one
//! of its faces.

use std::collections::BTreeMap;
use std::ops::Range;

use camcraft_geom::mesh::{Aabb, MeshError, TriMesh};
use camcraft_geom::topology::Body;
use glam::DVec3;

use crate::features::{self, FeatureError};
use crate::frame::{BadOrientation, Frame, Orientation};
use crate::job::{BoxOf, Job, JobError, Origin, Setup, StockSpec};

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

/// Where one model's triangles and vertices are in the merged part.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    /// The model, by name.
    pub model: String,
    /// Its triangles in the part: triangle `i` of the model is triangle
    /// `triangles.start + i` of the part.
    pub triangles: Range<u32>,
    /// Its vertices in the part, likewise.
    pub vertices: Range<u32>,
}

/// A setup in its own frame.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// Every model the setup machines, as one mesh.
    pub part: TriMesh,
    /// Which of its triangles came from which model, in the setup's order.
    pub pieces: Vec<Piece>,
    /// Its bounds.
    pub model_bounds: Aabb,
    /// Each stock block.
    pub stocks: Vec<Aabb>,
    /// How model coordinates became these.
    pub frame: Frame,
}

impl Prepared {
    /// The model triangle `part_triangle` of the part came from, and its number
    /// in that model.
    pub fn piece_of(&self, part_triangle: u32) -> Option<(&Piece, u32)> {
        let p = self
            .pieces
            .iter()
            .find(|p| p.triangles.contains(&part_triangle))?;
        Some((p, part_triangle - p.triangles.start))
    }

    /// Where model `model`'s triangles and vertices are in the part.
    pub fn piece(&self, model: &str) -> Option<&Piece> {
        self.pieces.iter().find(|p| p.model == model)
    }

    /// The box around every stock block.
    pub fn stock_bounds(&self) -> Aabb {
        let mut it = self.stocks.iter();
        let first = it.next().copied().unwrap_or(self.model_bounds);
        it.fold(first, |a, b| a.union(b))
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
        StockSpec::FixedBox { corner, size } => {
            let min = DVec3::from_array(*corner);
            Aabb {
                min,
                max: min + DVec3::from_array(*size),
            }
        }
    })
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
    let b = turn(
        frame.orientation,
        &block(spec, bounds_of(models, bodies)?, bodies)?,
    );
    Ok(Aabb {
        min: b.min - frame.origin,
        max: b.max - frame.origin,
    })
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

    // Each block, in model coordinates.
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

    let o = setup.orientation;
    let turned_blocks: Vec<Aabb> = blocks.iter().map(|b| turn(o, b)).collect();
    let turned_model = turn(o, &model_box);

    let origin = match &setup.origin {
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
    // Where each model landed in the merge, which appends them in this order.
    let mut pieces = Vec::with_capacity(parts.len());
    let (mut t, mut v) = (0u32, 0u32);
    for (name, mesh) in setup.models.iter().zip(&parts) {
        let too_large = || MeshError::TooLarge {
            vertices: mesh.vertices().len(),
        };
        let nt = u32::try_from(mesh.len()).map_err(|_| too_large())?;
        let nv = u32::try_from(mesh.vertices().len()).map_err(|_| too_large())?;
        pieces.push(Piece {
            model: name.clone(),
            triangles: t..t.saturating_add(nt),
            vertices: v..v.saturating_add(nv),
        });
        t = t.saturating_add(nt);
        v = v.saturating_add(nv);
    }
    let part = part_in_model.mapped(|p| frame.to_setup(p))?;
    let model_bounds = part
        .bounds()
        .ok_or_else(|| PrepareError::EmptyModel(setup.name.clone()))?;
    Ok(Prepared {
        part,
        pieces,
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
        let body = Body::derived(mesh.clone());
        let meshes: Bodies<'_> = [("m", &body)].into_iter().collect();
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
        let body = Body::derived(mesh.clone());
        let meshes: Bodies<'_> = [("m", &body)].into_iter().collect();
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
        let body = Body::derived(mesh.clone());
        let meshes: Bodies<'_> = [("m", &body)].into_iter().collect();
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

    /// A stock placed in a setup's frame from outside it lands where the setup
    /// would put it, were it the setup's own.
    #[test]
    fn a_stock_in_frame_is_where_prepare_puts_it() {
        let mesh = brick();
        let body = Body::derived(mesh.clone());
        let meshes: Bodies<'_> = [("m", &body)].into_iter().collect();
        for spec in [
            StockSpec::AroundModels {
                models: vec![],
                margin: [1.0, 2.0, 3.0],
                below: 4.0,
            },
            StockSpec::FixedBox {
                corner: [-5.0, -5.0, -1.0],
                size: [30.0, 40.0, 9.0],
            },
        ] {
            let mut j = job(spec.clone());
            j.setups[0].orientation = Orientation {
                z: Axis::NegZ,
                x: Axis::PosX,
            };
            j.setups[0].origin = Origin::BoxPoint {
                of: BoxOf::Stock,
                x: Along::Max,
                y: Along::Center,
                z: Along::Min,
                offset: [1.0, 0.0, 0.5],
            };
            let p = prepare(&j, 0, &meshes).unwrap();
            let placed = stock_in_frame(&spec, &j.setups[0].models, &p.frame, &meshes).unwrap();
            assert_eq!(placed, p.stocks[0]);
        }
    }

    #[test]
    fn a_missing_mesh_is_named() {
        let meshes: Bodies<'_> = BTreeMap::new();
        let j = job(StockSpec::default());
        assert!(matches!(
            prepare(&j, 0, &meshes),
            Err(PrepareError::NotLoaded(n)) if n == "m"
        ));
    }

    /// A work zero on a hole's centre puts the hole's centre at the setup's
    /// origin, however the part sits.
    #[test]
    fn a_work_zero_on_a_picked_point() {
        use crate::job::{PointRef, Snap};
        use camcraft_geom::synthetic;
        use camcraft_geom::topology::EdgeKind;
        let body = Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 32));
        let rim = body
            .topology()
            .edges()
            .iter()
            .position(|e| matches!(e.kind, EdgeKind::Circle(c) if c.center.z > 2.5))
            .unwrap();
        let bodies: Bodies<'_> = [("m", &body)].into_iter().collect();
        let mut j = job(StockSpec::default());
        j.setups[0].origin = Origin::Point {
            point: PointRef {
                model: "m".into(),
                snap: Snap::CircleCenter,
                index: u32::try_from(rim).unwrap(),
                at: [0.0, 0.0, 5.0],
            },
            offset: [0.0; 3],
        };
        for flipped in [false, true] {
            if flipped {
                j.setups[0].orientation = Orientation::default().flipped();
            }
            let p = prepare(&j, 0, &bodies).unwrap();
            let centre = p.frame.to_setup(dvec3(0.0, 0.0, 5.0));
            assert!(centre.length() < 1e-9, "flipped {flipped}: {centre}");
        }
        // Picked on something that is not there: the setup is not laid out.
        if let Origin::Point { point, .. } = &mut j.setups[0].origin {
            point.at = [7.0, 7.0, 7.0];
        }
        assert!(matches!(
            prepare(&j, 0, &bodies),
            Err(PrepareError::Feature(_))
        ));
    }

    /// Two models merged: each triangle of the part is traced to its model.
    #[test]
    fn pieces_trace_part_triangles_to_their_models() {
        let a = Body::derived(brick());
        let two = TriMesh::new(
            vec![
                dvec3(0.0, 0.0, 0.0),
                dvec3(1.0, 0.0, 0.0),
                dvec3(0.0, 1.0, 0.0),
                dvec3(1.0, 1.0, 0.0),
            ],
            vec![[0, 1, 2], [1, 3, 2]],
        )
        .unwrap();
        let b = Body::derived(two);
        let bodies: Bodies<'_> = [("m", &a), ("n", &b)].into_iter().collect();
        let mut j = job(StockSpec::default());
        j.models.push(ModelDef {
            name: "n".into(),
            path: "n.stl".into(),
            units: None,
        });
        j.setups[0].models = vec!["n".into(), "m".into()];
        let p = prepare(&j, 0, &bodies).unwrap();
        assert_eq!(p.part.len(), 3);
        assert_eq!(
            p.piece_of(0).map(|(p, t)| (p.model.as_str(), t)),
            Some(("n", 0))
        );
        assert_eq!(
            p.piece_of(1).map(|(p, t)| (p.model.as_str(), t)),
            Some(("n", 1))
        );
        assert_eq!(
            p.piece_of(2).map(|(p, t)| (p.model.as_str(), t)),
            Some(("m", 0))
        );
        assert_eq!(p.piece_of(3), None);
        assert_eq!(p.piece("m").unwrap().vertices, 4..7);
    }
}
