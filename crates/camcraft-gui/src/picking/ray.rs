//! What is under the pointer in the 3D view: a face, an edge, a point on a model,
//! or a point on the stock's box.
//!
//! Picking is done on the CPU, in `f64`, against the models themselves, not by
//! reading back what the GPU drew. The ray from the eye is taken into each model's
//! own coordinates with the setup's frame, which only turns and moves, and cast
//! against that model's triangles; a face is then the face its triangle belongs to,
//! exactly the face a saved pick will name. Nothing is picked from `f32`.
//!
//! A model with several copies has one body and one index: the ray is taken back
//! through each copy's stand as well, into the body, and the nearest copy it meets
//! is the one hit. What is picked on any copy is saved on the body, where no copy
//! need stand, so it means the same feature on every copy.
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

use camcraft_core::arrange::Stand;
use camcraft_core::job::{Along, PointRef, Snap};
use camcraft_core::setup::Prepared;
use camcraft_core::setup::features::{snap_point, unplaced};
use camcraft_core::setup::frame::Frame;
use camcraft_geom::bvh::MeshBvh;
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
        /// Where on it, in the job's coordinates, on the model's body where it is
        /// placed: where no copy need stand.
        at: DVec3,
    },
    /// An edge of a model.
    Edge {
        /// The model.
        model: String,
        /// The edge's number.
        edge: u32,
        /// Where on it, in the job's coordinates, on the model's body where it is
        /// placed: where no copy need stand.
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

/// One model, as picking needs it: its faces and edges, its one index, and where
/// each copy's edges and points sit in the setup's frame, worked out once per
/// layout.
pub struct PickModel {
    name: String,
    /// Whether the setup machines it. One that does not is picked only whole, so
    /// its edges and points are not worked out.
    pub in_setup: bool,
    model: Arc<Model>,
    /// The index of its body, as no copy need stand: a ray is taken into each
    /// copy's place rather than an index built for each.
    bvh: Arc<MeshBvh>,
    /// Which of its edges close on themselves.
    closed: Vec<bool>,
    /// Where each copy stands, in order.
    stands: Vec<Stand>,
    /// Each copy, in the order its model has them.
    pub copies: Vec<PickCopy>,
}

/// One copy of a model, as picking needs it: where it stands, and where its
/// edges and points sit in the setup's frame. A copy of a model the setup does
/// not machine has only its stand.
pub struct PickCopy {
    /// Where it stands.
    pub stand: Stand,
    /// Each edge's points, in the setup's frame.
    pub edges: Vec<Vec<DVec3>>,
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

    /// The model's triangles and topology: the one body its copies are of.
    pub fn body(&self) -> &Body {
        &self.model.body
    }

    /// Where each copy of it stands.
    pub fn stands(&self) -> &[Stand] {
        &self.stands
    }
}

/// Where a ray first meets a model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// Which model, by its place in the scene.
    pub model: usize,
    /// Which copy of it.
    pub copy: usize,
    /// Which of its body's triangles.
    pub triangle: u32,
    /// How far along the ray.
    pub t: f64,
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
    /// Ready `prepared` for picking, with the job's models as loaded, their
    /// indexes and where their copies stand, and its stocks. Models it does
    /// not machine come after its own. A model it machines stands where
    /// `prepared` was laid out with it.
    pub fn new(
        prepared: &Prepared,
        loaded: impl IntoIterator<Item = (String, Arc<Model>, Arc<MeshBvh>, Vec<Stand>)>,
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
            .map(|(name, model, bvh, stands)| {
                if !prepared.machines(&name) {
                    let copies = stands
                        .iter()
                        .map(|&stand| PickCopy {
                            stand,
                            edges: Vec::new(),
                            vertices: Vec::new(),
                            centres: Vec::new(),
                            middles: Vec::new(),
                        })
                        .collect();
                    return PickModel {
                        name,
                        in_setup: false,
                        model,
                        bvh,
                        closed: Vec::new(),
                        stands,
                        copies,
                    };
                }
                // Where the part was laid out with it.
                let stands: Vec<Stand> = prepared
                    .pieces
                    .iter()
                    .filter(|p| p.model == name)
                    .map(|p| p.stand)
                    .collect();
                let body = &model.body;
                let (mesh, t) = (body.mesh(), body.topology());
                let ids = 0..u32::try_from(t.edges().len()).unwrap_or(u32::MAX);
                let copies = stands
                    .iter()
                    .map(|&stand| {
                        let to = |p: DVec3| frame.to_setup_as(&stand, p);
                        PickCopy {
                            stand,
                            edges: ids
                                .clone()
                                .map(|e| t.edge_points(mesh, e).map(to).collect())
                                .collect(),
                            vertices: t.vertices().iter().map(|v| to(v.position)).collect(),
                            centres: ids
                                .clone()
                                .map(|e| snap_point(body, Snap::CircleCenter, e).map(to))
                                .collect(),
                            middles: ids
                                .clone()
                                .map(|e| snap_point(body, Snap::EdgeMiddle, e).map(to))
                                .collect(),
                        }
                    })
                    .collect();
                PickModel {
                    name,
                    in_setup: true,
                    closed: t.edges().iter().map(|e| e.closed).collect(),
                    model,
                    bvh,
                    stands,
                    copies,
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

    /// The model called `name`.
    pub fn model(&self, name: &str) -> Option<&PickModel> {
        self.models.iter().find(|m| m.name == name)
    }

    /// Edge `edge` of `pm`'s body in the setup's frame, as its own transform
    /// places it, where no copy need stand. A pick names the same edges on
    /// every copy, so which edges a click takes with it (a loop, a chain) is
    /// worked out on this, once for them all, whichever copy was clicked.
    pub fn body_edge(&self, pm: &PickModel, edge: u32) -> Vec<DVec3> {
        let body = pm.body();
        body.topology()
            .edge_points(body.mesh(), edge)
            .map(|p| self.frame.to_setup(p))
            .collect()
    }

    /// The first surface of the setup's models the ray from `o` along unit `d`
    /// meets within `max`: which model, which copy of it, which of its
    /// triangles, and how far.
    pub fn cast(&self, o: DVec3, d: DVec3, max: f64) -> Option<Hit> {
        self.cast_among(o, d, max, false)
    }

    /// The same, among the job's other models too when `all`. The ray is taken
    /// into each copy's place on its model's body, so one index serves every
    /// copy.
    fn cast_among(&self, o: DVec3, d: DVec3, max: f64, all: bool) -> Option<Hit> {
        self.models
            .iter()
            .enumerate()
            .filter(|(_, m)| all || m.in_setup)
            .flat_map(|(model, m)| {
                m.copies.iter().enumerate().filter_map(move |(copy, c)| {
                    let om = self.frame.to_model_as(&c.stand, o);
                    let dm = self.frame.to_model_as(&c.stand, o + d) - om;
                    let hit = m.bvh.cast_ray(m.body().mesh(), om, dm, max)?;
                    Some(Hit {
                        model,
                        copy,
                        triangle: hit.triangle,
                        t: hit.t,
                    })
                })
            })
            .min_by(|a, b| a.t.total_cmp(&b.t))
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
            .is_none_or(|hit| hit.t >= dist - self.eps)
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
                let hit = self.cast_among(eye, dir, f64::INFINITY, true)?;
                Some(Target::Model {
                    model: self.models.get(hit.model)?.name.clone(),
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
                let near = self.nearest_edge(sight, proj, cursor)?;
                let m = self.models.get(near.model)?;
                Some(Target::Edge {
                    model: m.name.clone(),
                    edge: near.edge,
                    at: self.on_body(m, near.copy, near.at)?,
                })
            }
            Accept::Faces => {
                let hit = self.cast(eye, dir, f64::INFINITY)?;
                let m = self.models.get(hit.model)?;
                Some(Target::Face {
                    model: m.name.clone(),
                    face: m.body().topology().face_of(hit.triangle)?,
                    at: self.on_body(m, hit.copy, eye + dir * hit.t)?,
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
            for c in &m.copies {
                let points = (c.vertices.iter().map(|p| (Snap::Vertex, Some(*p))))
                    .enumerate()
                    .chain(
                        c.centres
                            .iter()
                            .map(|p| (Snap::CircleCenter, *p))
                            .enumerate(),
                    )
                    .chain(c.middles.iter().map(|p| (Snap::EdgeMiddle, *p)).enumerate());
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
                        best = Some((d, sight.depth(p), self.point(m, c, snap, index, p)));
                    }
                }
            }
        }
        if let Some((.., t)) = best {
            return Some(t);
        }
        let near = self.nearest_edge(sight, proj, cursor)?;
        let m = self.models.get(near.model)?;
        let c = m.copies.get(near.copy)?;
        let centre = (*c.centres.get(near.edge as usize)?)?;
        Some(self.point(m, c, Snap::CircleCenter, near.edge, centre))
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

    /// Point `setup`, in the setup's frame, on copy `c` of `m`: snap `snap` of
    /// its vertex or edge `index`.
    fn point(&self, m: &PickModel, c: &PickCopy, snap: Snap, index: u32, setup: DVec3) -> Target {
        Target::Point {
            point: PointRef {
                model: m.name.clone(),
                snap,
                index,
                // Saved where the model's file puts it, so it moves with the model,
                // and wherever its copies stand.
                at: unplaced(m.body(), self.frame.to_model_as(&c.stand, setup)),
            },
            setup,
        }
    }

    /// Setup point `p`, on copy `copy` of `m`, on the model's body where it is
    /// placed: where the copy has it, taken back to where no copy need stand.
    fn on_body(&self, m: &PickModel, copy: usize, p: DVec3) -> Option<DVec3> {
        let c = m.copies.get(copy)?;
        Some(self.frame.to_model_as(&c.stand, p))
    }

    /// The nearest edge on the screen within reach of `cursor` that is not hidden:
    /// which model and copy, which edge, and the point of it nearest the cursor,
    /// in the setup's frame.
    fn nearest_edge(&self, sight: Sight, proj: &Projector, cursor: Pos2) -> Option<NearEdge> {
        let mut near: Vec<(f32, NearEdge)> = Vec::new();
        let copies = self.models.iter().enumerate().flat_map(|(mi, m)| {
            m.copies
                .iter()
                .enumerate()
                .map(move |(ci, c)| (mi, m, ci, c))
        });
        for (mi, m, ci, c) in copies {
            for (e, points) in c.edges.iter().enumerate() {
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
                if let (Some((d, at)), Ok(edge)) = (best, u32::try_from(e)) {
                    let edge = NearEdge {
                        model: mi,
                        copy: ci,
                        edge,
                        at,
                    };
                    near.push((d, edge));
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
                .then(sight.depth(a.1.at).total_cmp(&sight.depth(b.1.at)))
                .then(a.0.total_cmp(&b.0))
        });
        near.into_iter()
            .take(EDGE_TRIES)
            .find(|(_, e)| self.visible(sight, e.at))
            .map(|(_, e)| e)
    }
}

/// An edge near the pointer, on one copy of one model.
struct NearEdge {
    /// The model, by its place in the scene.
    model: usize,
    /// Which copy of it.
    copy: usize,
    /// The edge's number.
    edge: u32,
    /// Its point nearest the pointer, in the setup's frame.
    at: DVec3,
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
