//! What is under the pointer in the 3D view: a face, an edge, a point on a model,
//! or a point on the stock's box.
//!
//! Picking is done on the CPU, in `f64`, against the models themselves, not by
//! reading back what the GPU drew. The ray from the eye is taken into each model's
//! own coordinates with the setup's frame, which only turns and moves, and cast
//! against that model's triangles; a face is then the face its triangle belongs to,
//! exactly the face a saved pick will name. Nothing is picked from `f32`.
//!
//! Points and edges are thin, so they are found on the screen instead: the nearest
//! one within a few points of the pointer that is not hidden behind the part. The
//! order of preference is a point, then a point on the stock, then an edge, then the
//! face under the pointer, each only if the slot being filled takes it.
//!
//! Choosing what a setup machines picks whole models and whole stocks instead, and
//! among them the job's others, placed where the setup would put them, so one not
//! in the setup can be clicked into it.

use std::sync::Arc;

use camcraft_core::job::{Along, PointRef, Snap};
use camcraft_core::setup::Prepared;
use camcraft_core::setup::features::{FeatureError, snap_point, unplaced};
use camcraft_core::setup::frame::Frame;
use camcraft_geom::bvh::MeshBvh;
use camcraft_geom::contour::Region;
use camcraft_geom::mesh::Aabb;
use camcraft_geom::topology::Body;
use camcraft_session::Model;
use eframe::egui::Pos2;
use glam::DVec3;

use crate::view::camera::Projector;
use crate::view::camera::Sight;

/// How near the pointer a point must be to be picked, in points.
pub const POINT_REACH: f32 = 8.0;
/// How near the pointer an edge must be to be picked, in points.
pub const EDGE_REACH: f32 = 6.0;
/// How near the pointer a point on the stock's box must be, in points: they sit in
/// open air at the box's corners, so a little more.
pub const STOCK_REACH: f32 = 10.0;
/// Edges tried, nearest first, before giving up on finding one that is not hidden.
const EDGE_TRIES: usize = 16;
/// How close, in points, two things are drawn to count as drawn in the same
/// place, when the nearer is taken.
const SAME_PLACE: f32 = 0.5;

/// What a pick may land on: one kind of thing, as each slot takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accept {
    /// Faces of the models.
    Faces,
    /// Edges of the models.
    Edges,
    /// Points on the models (corners, round edges' centres, edges' middles), or
    /// on the stock's box.
    Points,
    /// Whole models, the setup's and the job's others.
    Models,
    /// Whole stocks, likewise.
    Stocks,
}

/// What was picked.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// A face of a model.
    Face {
        /// The model.
        model: String,
        /// The face's number.
        face: u32,
        /// Where on it, in the job's coordinates, where the model is placed.
        at: DVec3,
    },
    /// An edge of a model.
    Edge {
        /// The model.
        model: String,
        /// The edge's number.
        edge: u32,
        /// Where on it, in the job's coordinates, where the model is placed.
        at: DVec3,
    },
    /// A loop of a model's edges, end to end, as picking edges takes them: the
    /// loop the edge under the pointer goes round.
    Loop {
        /// The model.
        model: String,
        /// Each edge's number, and where on it, in the job's coordinates: where the
        /// pointer is on the one under it, the middle of the others.
        edges: Vec<(u32, DVec3)>,
    },
    /// A point on a model.
    Point {
        /// The point, as a work zero would name it.
        point: PointRef,
        /// Where it is in the setup's frame.
        setup: DVec3,
    },
    /// A point on the stock's box.
    StockPoint {
        /// Where along X.
        x: Along,
        /// Where along Y.
        y: Along,
        /// Where along Z.
        z: Along,
        /// Where it is in the setup's frame.
        setup: DVec3,
    },
    /// A whole model, in the setup or not.
    Model {
        /// The model.
        model: String,
    },
    /// A whole stock, in the setup or not.
    Stock {
        /// The stock.
        stock: String,
    },
}

/// One stock block, as picking needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct PickStock {
    /// The stock's name.
    pub name: String,
    /// Where it is, in the setup's frame.
    pub bounds: Aabb,
    /// Whether the setup starts from it.
    pub in_setup: bool,
}

/// One model, as picking needs it: its faces and edges, its index, and where its
/// edges and points sit in the setup's frame, worked out once per layout.
pub struct PickModel {
    name: String,
    /// Whether the setup machines it. One that does not is picked only whole, so
    /// its edges and points are not worked out.
    pub in_setup: bool,
    model: Arc<Model>,
    bvh: Arc<MeshBvh>,
    /// Each edge's points, in the setup's frame.
    pub edges: Vec<Vec<DVec3>>,
    /// Which of them close on themselves.
    closed: Vec<bool>,
    /// Each vertex, in the setup's frame.
    vertices: Vec<DVec3>,
    /// Each edge's circle centre, if it is round, in the setup's frame.
    centres: Vec<Option<DVec3>>,
    /// Each edge's middle, in the setup's frame.
    middles: Vec<Option<DVec3>>,
}

impl PickModel {
    /// The model's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The model's triangles and topology.
    pub fn body(&self) -> &Body {
        &self.model.body
    }
}

/// A laid-out setup, ready to pick from.
pub struct PickScene {
    /// The setup's models, in its order, then the job's others.
    pub models: Vec<PickModel>,
    /// How model coordinates became the setup's.
    pub frame: Frame,
    /// The box around the setup's stock, in the setup's frame.
    pub stock: Option<Aabb>,
    /// Every stock block of the job, the setup's and the others.
    pub stocks: Vec<PickStock>,
    /// How far behind a surface a point may be and still count as on it.
    eps: f64,
}

impl PickScene {
    /// Ready `prepared` for picking, with the job's models as loaded and their
    /// indexes, and its stocks. Models it does not machine come after its own.
    pub fn new(
        prepared: &Prepared,
        loaded: impl IntoIterator<Item = (String, Arc<Model>, Arc<MeshBvh>)>,
        stocks: Vec<PickStock>,
    ) -> Self {
        let frame = prepared.frame;
        let mut loaded: Vec<_> = loaded.into_iter().collect();
        loaded.sort_by_key(|(n, ..)| {
            prepared
                .pieces
                .iter()
                .position(|p| &p.model == n)
                .unwrap_or(usize::MAX)
        });
        let models = loaded
            .into_iter()
            .map(|(name, model, bvh)| {
                if prepared.piece(&name).is_none() {
                    return PickModel {
                        name,
                        in_setup: false,
                        model,
                        bvh,
                        edges: Vec::new(),
                        closed: Vec::new(),
                        vertices: Vec::new(),
                        centres: Vec::new(),
                        middles: Vec::new(),
                    };
                }
                let body = &model.body;
                let (mesh, t) = (body.mesh(), body.topology());
                let to = |p: DVec3| frame.to_setup(p);
                let ids = 0..u32::try_from(t.edges().len()).unwrap_or(u32::MAX);
                let edges = ids
                    .clone()
                    .map(|e| t.edge_points(mesh, e).map(to).collect())
                    .collect();
                let closed = t.edges().iter().map(|e| e.closed).collect();
                let vertices = t.vertices().iter().map(|v| to(v.position)).collect();
                let centres = ids
                    .clone()
                    .map(|e| snap_point(body, Snap::CircleCenter, e).map(to))
                    .collect();
                let middles = ids
                    .map(|e| snap_point(body, Snap::EdgeMiddle, e).map(to))
                    .collect();
                PickModel {
                    name,
                    in_setup: true,
                    model,
                    bvh,
                    edges,
                    closed,
                    vertices,
                    centres,
                    middles,
                }
            })
            .collect();
        let size = prepared.model_bounds.extent().length();
        Self {
            models,
            frame,
            stock: Some(prepared.stock_bounds()),
            stocks,
            eps: (1e-3 * size).max(1e-3),
        }
    }

    /// The setup's part seen from above, as the engine sees it: the shadow the
    /// layout, `prepared`, casts once and keeps ([`Prepared::shadow`]).
    pub fn silhouette<'p>(&self, prepared: &'p Prepared) -> Result<&'p Region, FeatureError> {
        Ok(prepared.shadow()?.area())
    }

    /// The model called `name`.
    pub fn model(&self, name: &str) -> Option<&PickModel> {
        self.models.iter().find(|m| m.name == name)
    }

    /// The setup's models, in its order.
    pub fn setup_models(&self) -> impl Iterator<Item = &PickModel> {
        self.models.iter().filter(|m| m.in_setup)
    }

    /// The first surface of the setup's models the ray from `o` along unit `d`
    /// meets within `max`: which model, which of its triangles, and how far.
    pub fn cast(&self, o: DVec3, d: DVec3, max: f64) -> Option<(usize, u32, f64)> {
        self.cast_among(o, d, max, false)
    }

    /// The same, among the job's other models too when `all`.
    fn cast_among(&self, o: DVec3, d: DVec3, max: f64, all: bool) -> Option<(usize, u32, f64)> {
        let om = self.frame.to_model(o);
        let dm = self.frame.to_model(o + d) - om;
        self.models
            .iter()
            .enumerate()
            .filter(|(_, m)| all || m.in_setup)
            .filter_map(|(i, m)| {
                let hit = m.bvh.cast_ray(m.body().mesh(), om, dm, max)?;
                Some((i, hit.triangle, hit.t))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
    }

    /// Whether `p` can be seen along its line of `sight`, and is not behind the
    /// part.
    fn visible(&self, sight: Sight, p: DVec3) -> bool {
        let eye = sight.to(p);
        let to = p - eye;
        let dist = to.length();
        if dist <= self.eps {
            return true;
        }
        self.cast(eye, to / dist, dist)
            .is_none_or(|(_, _, t)| t >= dist - self.eps)
    }

    /// What is under `cursor`, for a ray `(start, unit direction)` through it,
    /// among what `accept` takes, seen along the view's lines of `sight`.
    pub fn pick(
        &self,
        (eye, dir): (DVec3, DVec3),
        sight: Sight,
        proj: &Projector,
        cursor: Pos2,
        accept: Accept,
    ) -> Option<Target> {
        match accept {
            Accept::Models => {
                let (mi, _, _) = self.cast_among(eye, dir, f64::INFINITY, true)?;
                Some(Target::Model {
                    model: self.models.get(mi)?.name.clone(),
                })
            }
            Accept::Stocks => {
                let (_, s) = self
                    .stocks
                    .iter()
                    .filter_map(|s| Some((ray_box(eye, dir, &s.bounds)?, s)))
                    .min_by(|a, b| a.0.total_cmp(&b.0))?;
                Some(Target::Stock {
                    stock: s.name.clone(),
                })
            }
            Accept::Points => self
                .model_point(sight, proj, cursor)
                .or_else(|| self.stock_point(sight, proj, cursor)),
            Accept::Edges => {
                let (mi, edge, at) = self.nearest_edge(sight, proj, cursor)?;
                Some(Target::Edge {
                    model: self.models.get(mi)?.name.clone(),
                    edge,
                    at: self.frame.to_model(at),
                })
            }
            Accept::Faces => {
                let (mi, tri, t) = self.cast(eye, dir, f64::INFINITY)?;
                let m = self.models.get(mi)?;
                Some(Target::Face {
                    model: m.name.clone(),
                    face: m.body().topology().face_of(tri)?,
                    at: self.frame.to_model(eye + dir * t),
                })
            }
        }
    }

    /// Whether a point drawn `d` from the pointer, at `p`, is a better pick than
    /// the best so far, drawn `best.0` from it at depth `best.1`: of two drawn in
    /// the same place, the nearer. Looking straight at a wall, its top and bottom
    /// corners are one.
    fn better(&self, sight: Sight, d: f32, p: DVec3, best: Option<(f32, f64)>) -> bool {
        best.is_none_or(|(b, depth)| {
            d < b - SAME_PLACE || (d < b + SAME_PLACE && sight.depth(p) < depth - self.eps)
        })
    }

    /// The nearest point on a model within reach of `cursor` that can be seen,
    /// vertices first on a tie, as the likeliest meant; or over a round edge, its
    /// centre: the usual way to put a work zero on a hole.
    fn model_point(&self, sight: Sight, proj: &Projector, cursor: Pos2) -> Option<Target> {
        let mut best: Option<(f32, f64, Target)> = None;
        for m in &self.models {
            let points = (m.vertices.iter().map(|p| (Snap::Vertex, Some(*p))))
                .enumerate()
                .chain(
                    m.centres
                        .iter()
                        .map(|p| (Snap::CircleCenter, *p))
                        .enumerate(),
                )
                .chain(m.middles.iter().map(|p| (Snap::EdgeMiddle, *p)).enumerate());
            for (i, (snap, p)) in points {
                let Some(p) = p else { continue };
                let Some(d) = near(proj, cursor, p, POINT_REACH) else {
                    continue;
                };
                let was = best.as_ref().map(|(b, depth, _)| (*b, *depth));
                if !self.better(sight, d, p, was) || !self.visible(sight, p) {
                    continue;
                }
                if let Ok(index) = u32::try_from(i) {
                    best = Some((d, sight.depth(p), self.point(m, snap, index, p)));
                }
            }
        }
        if let Some((.., t)) = best {
            return Some(t);
        }
        let (mi, e, _) = self.nearest_edge(sight, proj, cursor)?;
        let m = self.models.get(mi)?;
        let c = (*m.centres.get(e as usize)?)?;
        Some(self.point(m, Snap::CircleCenter, e, c))
    }

    /// The nearest point on the stock's box within reach of `cursor` that can be
    /// seen.
    fn stock_point(&self, sight: Sight, proj: &Projector, cursor: Pos2) -> Option<Target> {
        let b = self.stock?;
        let mut best: Option<(f32, f64, Target)> = None;
        for (x, y, z) in box_points() {
            let p = DVec3::new(
                x.pick(b.min.x, b.max.x),
                y.pick(b.min.y, b.max.y),
                z.pick(b.min.z, b.max.z),
            );
            let Some(d) = near(proj, cursor, p, STOCK_REACH) else {
                continue;
            };
            let was = best.as_ref().map(|(b, depth, _)| (*b, *depth));
            if self.better(sight, d, p, was) && self.visible(sight, p) {
                best = Some((d, sight.depth(p), Target::StockPoint { x, y, z, setup: p }));
            }
        }
        best.map(|(.., t)| t)
    }

    fn point(&self, m: &PickModel, snap: Snap, index: u32, setup: DVec3) -> Target {
        Target::Point {
            point: PointRef {
                model: m.name.clone(),
                snap,
                index,
                // Saved where the model's file puts it, so it moves with the model.
                at: unplaced(m.body(), self.frame.to_model(setup)),
            },
            setup,
        }
    }

    /// The nearest edge on the screen within reach of `cursor` that is not hidden:
    /// which model, which edge, and the point of it nearest the cursor, in the
    /// setup's frame.
    fn nearest_edge(
        &self,
        sight: Sight,
        proj: &Projector,
        cursor: Pos2,
    ) -> Option<(usize, u32, DVec3)> {
        let mut near: Vec<(f32, usize, u32, DVec3)> = Vec::new();
        for (mi, m) in self.models.iter().enumerate() {
            for (e, points) in m.edges.iter().enumerate() {
                let closed = m.closed.get(e).copied().unwrap_or(false);
                let closing = points.first().filter(|_| closed && points.len() > 2);
                let pairs = points.windows(2).filter_map(|w| match w {
                    [a, b] => Some((*a, *b)),
                    _ => None,
                });
                let pairs = pairs.chain(points.last().zip(closing).map(|(a, b)| (*a, *b)));
                let mut best: Option<(f32, DVec3)> = None;
                for (a, b) in pairs {
                    let (Some(sa), Some(sb)) = (proj.screen(a), proj.screen(b)) else {
                        continue;
                    };
                    let ab = sb - sa;
                    let len2 = ab.length_sq();
                    let s = if len2 > 0.0 {
                        ((cursor - sa).dot(ab) / len2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let d = (sa + ab * s).distance(cursor);
                    if d <= EDGE_REACH && best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, a + (b - a) * f64::from(s)));
                    }
                }
                if let (Some((d, p)), Ok(e)) = (best, u32::try_from(e)) {
                    near.push((d, mi, e, p));
                }
            }
        }
        // Nearest the pointer, and of edges drawn in the same place, the nearer:
        // a hole's rims, looking straight down it.
        #[allow(clippy::cast_possible_truncation)]
        let place = |d: f32| (d / SAME_PLACE).round() as i64;
        near.sort_by(|a, b| {
            place(a.0)
                .cmp(&place(b.0))
                .then(sight.depth(a.3).total_cmp(&sight.depth(b.3)))
                .then(a.0.total_cmp(&b.0))
        });
        near.into_iter()
            .take(EDGE_TRIES)
            .find(|(_, _, _, p)| self.visible(sight, *p))
            .map(|(_, mi, e, p)| (mi, e, p))
    }
}

/// How far from `cursor` `p` is drawn, if it is within `reach` of it.
fn near(proj: &Projector, cursor: Pos2, p: DVec3, reach: f32) -> Option<f32> {
    proj.screen(p)
        .map(|s| s.distance(cursor))
        .filter(|d| *d <= reach)
}

/// How far along the ray from `o` along `d` it first meets `b`, if it does:
/// nothing when it starts inside, since then the box is all around.
fn ray_box(o: DVec3, d: DVec3, b: &Aabb) -> Option<f64> {
    let (mut near, mut far) = (0.0_f64, f64::INFINITY);
    for axis in 0..3 {
        let (o, d, lo, hi) = (o[axis], d[axis], b.min[axis], b.max[axis]);
        if d.abs() < 1e-12 {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (a, c) = ((lo - o) / d, (hi - o) / d);
        near = near.max(a.min(c));
        far = far.min(a.max(c));
        if near > far {
            return None;
        }
    }
    Some(near)
}

/// The 26 points of a box a work zero goes on: corners, the middles of edges and
/// the centres of faces. Its centre is inside it, where nothing can be clicked.
fn box_points() -> impl Iterator<Item = (Along, Along, Along)> {
    const ALL: [Along; 3] = [Along::Min, Along::Center, Along::Max];
    ALL.into_iter()
        .flat_map(|x| ALL.into_iter().flat_map(move |y| ALL.map(|z| (x, y, z))))
        .filter(|p| *p != (Along::Center, Along::Center, Along::Center))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests;
