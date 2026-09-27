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

use camcraft_core::features::snap_point;
use camcraft_core::frame::Frame;
use camcraft_core::job::{Along, PointRef, Snap};
use camcraft_core::setup::Prepared;
use camcraft_geom::bvh::MeshBvh;
use camcraft_geom::mesh::Aabb;
use camcraft_geom::topology::Body;
use camcraft_session::Model;
use eframe::egui::{Pos2, Rect, pos2};
use glam::{DVec3, Mat4};

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

/// Where points in the setup's frame are drawn on the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projector {
    view_proj: Mat4,
    rect: Rect,
}

impl Projector {
    /// For a view drawn with `view_proj` into `rect`.
    pub fn new(view_proj: Mat4, rect: Rect) -> Self {
        Self { view_proj, rect }
    }

    /// Where `p` is drawn, or `None` if it is behind the eye.
    pub fn screen(&self, p: DVec3) -> Option<Pos2> {
        let clip = self.view_proj * p.as_vec3().extend(1.0);
        if clip.w <= 1e-6 {
            return None;
        }
        let (x, y) = (clip.x / clip.w, clip.y / clip.w);
        let c = self.rect.center();
        Some(pos2(
            c.x + x * self.rect.width() * 0.5,
            c.y - y * self.rect.height() * 0.5,
        ))
    }

    /// The point on the screen as the camera measures it: x right and y up, -1 to
    /// 1 across the view.
    pub fn ndc(&self, p: Pos2) -> glam::Vec2 {
        let c = self.rect.center();
        glam::Vec2::new(
            (p.x - c.x) / (self.rect.width() * 0.5).max(1e-3),
            (c.y - p.y) / (self.rect.height() * 0.5).max(1e-3),
        )
    }
}

/// What a pick may land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accept {
    /// Faces of the models.
    pub faces: bool,
    /// Edges of the models.
    pub edges: bool,
    /// Points on the models: corners, round edges' centres, edges' middles.
    pub points: bool,
    /// Points on the stock's box.
    pub stock: bool,
    /// Whole models, the setup's and the job's others.
    pub models: bool,
    /// Whole stocks, likewise.
    pub stocks: bool,
}

impl Accept {
    /// Faces only.
    pub const FACES: Self = Self {
        faces: true,
        edges: false,
        points: false,
        stock: false,
        models: false,
        stocks: false,
    };
    /// Edges only.
    pub const EDGES: Self = Self {
        faces: false,
        edges: true,
        points: false,
        stock: false,
        models: false,
        stocks: false,
    };
    /// Points, on the models or the stock.
    pub const POINTS: Self = Self {
        faces: false,
        edges: false,
        points: true,
        stock: true,
        models: false,
        stocks: false,
    };
    /// Whole models.
    pub const MODELS: Self = Self {
        faces: false,
        edges: false,
        points: false,
        stock: false,
        models: true,
        stocks: false,
    };
    /// Whole stocks.
    pub const STOCKS: Self = Self {
        faces: false,
        edges: false,
        points: false,
        stock: false,
        models: false,
        stocks: true,
    };
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
        /// Where on it, in model coordinates.
        at: DVec3,
    },
    /// An edge of a model.
    Edge {
        /// The model.
        model: String,
        /// The edge's number.
        edge: u32,
        /// Where on it, in model coordinates.
        at: DVec3,
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
        let near = |p: DVec3, reach: f32| {
            proj.screen(p)
                .map(|s| s.distance(cursor))
                .filter(|d| *d <= reach)
        };

        if accept.models
            && let Some((mi, _, _)) = self.cast_among(eye, dir, f64::INFINITY, true)
            && let Some(m) = self.models.get(mi)
        {
            return Some(Target::Model {
                model: m.name.clone(),
            });
        }

        if accept.stocks
            && let Some(s) = self
                .stocks
                .iter()
                .filter_map(|s| Some((ray_box(eye, dir, &s.bounds)?, s)))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, s)| s)
        {
            return Some(Target::Stock {
                stock: s.name.clone(),
            });
        }

        // Of two points drawn in the same place, the nearer: looking straight at
        // a wall, its top and bottom corners are one.
        let better = |d: f32, p: DVec3, best: Option<(f32, f64)>| {
            best.is_none_or(|(b, depth)| {
                d < b - SAME_PLACE || (d < b + SAME_PLACE && sight.depth(p) < depth - self.eps)
            })
        };

        if accept.points {
            // The nearest point within reach that can be seen: vertices first on
            // a tie, as the likeliest meant.
            let mut best: Option<(f32, f64, Target)> = None;
            for m in &self.models {
                let kinds = [
                    (
                        Snap::Vertex,
                        m.vertices.iter().map(|p| Some(*p)).collect::<Vec<_>>(),
                    ),
                    (Snap::CircleCenter, m.centres.clone()),
                    (Snap::EdgeMiddle, m.middles.clone()),
                ];
                for (snap, points) in kinds {
                    for (i, p) in points.iter().enumerate() {
                        let Some(p) = *p else { continue };
                        let Some(d) = near(p, POINT_REACH) else {
                            continue;
                        };
                        let was = best.as_ref().map(|(b, depth, _)| (*b, *depth));
                        if !better(d, p, was) || !self.visible(sight, p) {
                            continue;
                        }
                        if let Ok(index) = u32::try_from(i) {
                            best = Some((d, sight.depth(p), self.point(m, snap, index, p)));
                        }
                    }
                }
            }
            if let Some((.., t)) = best {
                return Some(t);
            }
            // Over a round edge, its centre: the usual way to put a work zero on a
            // hole.
            if let Some((mi, e, _)) = self.nearest_edge(sight, proj, cursor)
                && let Some(m) = self.models.get(mi)
                && let Some(Some(c)) = m.centres.get(e as usize)
            {
                return Some(self.point(m, Snap::CircleCenter, e, *c));
            }
        }

        if accept.stock
            && let Some(b) = self.stock
        {
            let mut best: Option<(f32, f64, Target)> = None;
            for (x, y, z) in box_points() {
                let p = DVec3::new(
                    x.pick(b.min.x, b.max.x),
                    y.pick(b.min.y, b.max.y),
                    z.pick(b.min.z, b.max.z),
                );
                let Some(d) = near(p, STOCK_REACH) else {
                    continue;
                };
                let was = best.as_ref().map(|(b, depth, _)| (*b, *depth));
                if better(d, p, was) && self.visible(sight, p) {
                    best = Some((d, sight.depth(p), Target::StockPoint { x, y, z, setup: p }));
                }
            }
            if let Some((.., t)) = best {
                return Some(t);
            }
        }

        if accept.edges
            && let Some((mi, edge, at)) = self.nearest_edge(sight, proj, cursor)
            && let Some(m) = self.models.get(mi)
        {
            return Some(Target::Edge {
                model: m.name.clone(),
                edge,
                at: self.frame.to_model(at),
            });
        }

        if accept.faces
            && let Some((mi, tri, t)) = self.cast(eye, dir, f64::INFINITY)
            && let Some(m) = self.models.get(mi)
            && let Some(face) = m.body().topology().face_of(tri)
        {
            return Some(Target::Face {
                model: m.name.clone(),
                face,
                at: self.frame.to_model(eye + dir * t),
            });
        }
        None
    }

    fn point(&self, m: &PickModel, snap: Snap, index: u32, setup: DVec3) -> Target {
        Target::Point {
            point: PointRef {
                model: m.name.clone(),
                snap,
                index,
                at: self.frame.to_model(setup).to_array(),
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
pub fn box_points() -> impl Iterator<Item = (Along, Along, Along)> {
    const ALL: [Along; 3] = [Along::Min, Along::Center, Along::Max];
    ALL.into_iter()
        .flat_map(|x| ALL.into_iter().flat_map(move |y| ALL.map(|z| (x, y, z))))
        .filter(|p| *p != (Along::Center, Along::Center, Along::Center))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::picking::describe::{describe_box_point, describe_circle};
    use crate::view::camera::Camera;
    use camcraft_core::job::{Job, ModelDef, Setup, StockDef, StockSpec};
    use camcraft_core::setup::{Bodies, prepare};
    use camcraft_geom::synthetic;
    use camcraft_geom::topology::FaceKind;
    use eframe::egui::vec2;
    use glam::{Vec3, dvec3};

    /// The plate with a hole, 40 square and 5 thick, laid out with the stock 2 mm
    /// round it and the work zero at the model's own.
    fn scene() -> (PickScene, Arc<Model>) {
        let body =
            camcraft_geom::topology::Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 32));
        let mesh = body.mesh();
        let model = Arc::new(Model {
            bounds: mesh.bounds().unwrap(),
            body: body.clone(),
            path: "p.stl".into(),
            units: "mm".into(),
            notes: Vec::new(),
            warnings: Vec::new(),
        });
        let bvh = Arc::new(MeshBvh::new(model.mesh()));
        let mut j = Job::default();
        j.models.push(ModelDef {
            name: "p".into(),
            path: "p.stl".into(),
            units: None,
        });
        j.stocks.push(StockDef {
            name: "s".into(),
            spec: StockSpec::default(),
        });
        let mut s = Setup::new("top");
        s.models = vec!["p".into()];
        s.stocks = vec!["s".into()];
        j.setups.push(s);
        let bodies: Bodies<'_> = [("p", &model.body)].into_iter().collect();
        let prepared = prepare(&j, 0, &bodies).unwrap();
        let scene = PickScene::new(
            &prepared,
            [("p".to_string(), model.clone(), bvh)],
            Vec::new(),
        );
        (scene, model)
    }

    /// Looking straight down on the plate from above, 800 by 600 points.
    fn looking_down() -> (Camera, Projector, f32) {
        let mut c = Camera::framing(Vec3::new(0.0, 0.0, 2.5), 40.0);
        c.pitch = 1.5;
        c.yaw = -std::f32::consts::FRAC_PI_2;
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let aspect = 800.0 / 600.0;
        (c, Projector::new(c.view_projection(aspect), rect), aspect)
    }

    /// Looking straight down on the plate, orthographically, as the view cube's
    /// Top does.
    fn straight_down() -> (Camera, Projector, f32) {
        let c = Camera::framing(Vec3::new(0.0, 0.0, 2.5), 40.0).turned_to(Vec3::Z);
        assert!(c.orthographic());
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let aspect = 800.0 / 600.0;
        (c, Projector::new(c.view_projection(aspect), rect), aspect)
    }

    fn pick_at(scene: &PickScene, screen: Pos2, accept: Accept) -> Option<Target> {
        pick_from(looking_down(), scene, screen, accept)
    }

    fn pick_from(
        (c, proj, aspect): (Camera, Projector, f32),
        scene: &PickScene,
        screen: Pos2,
        accept: Accept,
    ) -> Option<Target> {
        let ray = c.ray(aspect, proj.ndc(screen));
        scene.pick(ray, c.sight(), &proj, screen, accept)
    }

    /// Looking straight down orthographically, a corner on top is picked, and
    /// the one straight under it is hidden by the part.
    #[test]
    fn picking_works_straight_down() {
        let (scene, _) = scene();
        let (c, proj, _) = straight_down();
        let top = proj.screen(dvec3(20.0, 20.0, 5.0)).unwrap();
        let Some(Target::Point { point, .. }) =
            pick_from(straight_down(), &scene, top, Accept::POINTS)
        else {
            unreachable!("no point");
        };
        let at = DVec3::from(point.at);
        assert!(at.distance(dvec3(20.0, 20.0, 5.0)) < 1e-9, "{at}");
        // Under the top face, hidden by it.
        let under = dvec3(10.0, 12.0, 0.0);
        assert!(!scene.visible(c.sight(), under));
        assert!(scene.visible(c.sight(), dvec3(20.0, 20.0, 5.0)));
    }

    #[test]
    fn the_projector_and_the_ray_agree() {
        let (c, proj, aspect) = looking_down();
        for p in [dvec3(10.0, 10.0, 5.0), dvec3(-15.0, 3.0, 0.0)] {
            let s = proj.screen(p).unwrap();
            let (o, d) = c.ray(aspect, proj.ndc(s));
            let to = p - o;
            assert!((to - d * to.dot(d)).length() < 1e-3 * to.length());
        }
    }

    #[test]
    fn a_face_under_the_pointer_is_picked_where_it_was_hit() {
        let (scene, model) = scene();
        let (_, proj, _) = looking_down();
        let at = proj.screen(dvec3(12.0, 12.0, 5.0)).unwrap();
        let Some(Target::Face { face, at, .. }) = pick_at(&scene, at, Accept::FACES) else {
            unreachable!("no face");
        };
        let kind = model.body.topology().face(face).unwrap().kind;
        assert!(matches!(kind, FaceKind::Plane { normal, .. } if normal.z > 0.5));
        assert!((at - dvec3(12.0, 12.0, 5.0)).length() < 0.05, "{at}");
        // Over the hole there is nothing: the ray goes straight through.
        let hole = proj.screen(dvec3(0.0, 0.0, 5.0)).unwrap();
        assert_eq!(pick_at(&scene, hole, Accept::FACES), None);
    }

    /// Pointing at a hole's rim offers its centre, for a work zero.
    #[test]
    fn a_round_rim_offers_its_centre() {
        let (scene, model) = scene();
        let (_, proj, _) = looking_down();
        // Between two of the rim's points, away from any of them.
        let a = std::f64::consts::TAU / 64.0;
        let rim = proj
            .screen(dvec3(4.0 * a.cos(), 4.0 * a.sin(), 5.0))
            .unwrap();
        let Some(Target::Point { point, setup }) = pick_at(&scene, rim, Accept::POINTS) else {
            unreachable!("no point");
        };
        assert_eq!(point.snap, Snap::CircleCenter);
        assert!(setup.distance(dvec3(0.0, 0.0, 5.0)) < 1e-9, "{setup}");
        assert!(
            describe_circle(&model.body, point.index)
                .unwrap()
                .contains("Ø8.00")
        );
        // The same place, picking edges, is the rim.
        let Some(Target::Edge { edge, .. }) = pick_at(&scene, rim, Accept::EDGES) else {
            unreachable!("no edge");
        };
        assert_eq!(edge, point.index);
    }

    /// A corner under the pointer is a vertex; one on the far side, hidden by the
    /// plate, is not picked.
    #[test]
    fn hidden_points_are_not_picked() {
        let (scene, _) = scene();
        let (_, proj, _) = looking_down();
        let top = proj.screen(dvec3(20.0, 20.0, 5.0)).unwrap();
        let Some(Target::Point { point, .. }) = pick_at(&scene, top, Accept::POINTS) else {
            unreachable!("no point");
        };
        assert_eq!(point.snap, Snap::Vertex);
        let at = DVec3::from(point.at);
        assert!(at.distance(dvec3(20.0, 20.0, 5.0)) < 1e-9, "{at}");
        // A bottom corner lies straight under the top one, and is hidden by it.
        let v = scene.models[0]
            .vertices
            .iter()
            .position(|p| (*p - dvec3(-20.0, -20.0, 0.0)).length() < 1e-9)
            .unwrap();
        let (c, _, _) = looking_down();
        assert!(!scene.visible(c.sight(), scene.models[0].vertices[v]));
    }

    #[test]
    fn a_stock_corner_is_named_by_where_it_is_on_the_box() {
        let (scene, _) = scene();
        let (_, proj, _) = looking_down();
        let b = scene.stock.unwrap();
        let corner = proj.screen(DVec3::new(b.min.x, b.min.y, b.max.z)).unwrap();
        let Some(Target::StockPoint { x, y, z, .. }) = pick_at(&scene, corner, Accept::POINTS)
        else {
            unreachable!("no stock point");
        };
        assert_eq!((x, y, z), (Along::Min, Along::Min, Along::Max));
        assert_eq!(describe_box_point(x, y, z), "top, front-left corner");
        assert_eq!(box_points().count(), 26);
    }

    /// A model the setup does not machine is picked only whole, and only when
    /// whole models are wanted; a stock box, likewise.
    #[test]
    fn others_are_picked_only_whole() {
        let body =
            camcraft_geom::topology::Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 32));
        let mesh = body.mesh();
        let model = |path: &str| {
            Arc::new(Model {
                bounds: mesh.bounds().unwrap(),
                body: body.clone(),
                path: path.into(),
                units: "mm".into(),
                notes: Vec::new(),
                warnings: Vec::new(),
            })
        };
        let (p, q) = (model("p.stl"), model("q.stl"));
        let bvh = Arc::new(MeshBvh::new(p.mesh()));
        let mut j = Job::default();
        for n in ["p", "q"] {
            j.models.push(ModelDef {
                name: n.into(),
                path: format!("{n}.stl"),
                units: None,
            });
        }
        j.stocks.push(StockDef {
            name: "s".into(),
            spec: StockSpec::default(),
        });
        let mut s = Setup::new("top");
        s.models = vec!["p".into()];
        s.stocks = vec!["s".into()];
        j.setups.push(s);
        let bodies: Bodies<'_> = [("p", &p.body)].into_iter().collect();
        let prepared = prepare(&j, 0, &bodies).unwrap();
        // "q" lies exactly on "p", but only "p" is the setup's, so only its faces
        // are picked. The job's other stock is off to one side.
        let far = Aabb {
            min: dvec3(100.0, 100.0, 0.0),
            max: dvec3(110.0, 110.0, 5.0),
        };
        let scene = PickScene::new(
            &prepared,
            [("q".to_string(), q, bvh.clone()), ("p".to_string(), p, bvh)],
            vec![PickStock {
                name: "other".into(),
                bounds: far,
                in_setup: false,
            }],
        );
        assert_eq!(scene.models[0].name(), "p", "the setup's own come first");
        assert!(!scene.models[1].in_setup);
        assert!(scene.models[1].edges.is_empty());
        assert_eq!(scene.setup_models().count(), 1);

        let (_, proj, _) = looking_down();
        let face = proj.screen(dvec3(12.0, 12.0, 5.0)).unwrap();
        assert!(matches!(
            pick_at(&scene, face, Accept::FACES),
            Some(Target::Face { model, .. }) if model == "p"
        ));
        assert!(matches!(
            pick_at(&scene, face, Accept::MODELS),
            Some(Target::Model { .. })
        ));
        assert!(pick_at(&scene, face, Accept::STOCKS).is_none());
        let off = proj.screen(dvec3(105.0, 105.0, 5.0)).unwrap();
        assert_eq!(
            pick_at(&scene, off, Accept::STOCKS),
            Some(Target::Stock {
                stock: "other".into()
            })
        );
        assert!(pick_at(&scene, off, Accept::MODELS).is_none());
    }

    #[test]
    fn a_ray_meets_a_box_where_it_enters() {
        let b = Aabb {
            min: dvec3(0.0, 0.0, 0.0),
            max: dvec3(1.0, 1.0, 1.0),
        };
        let down = dvec3(0.0, 0.0, -1.0);
        assert_eq!(ray_box(dvec3(0.5, 0.5, 5.0), down, &b), Some(4.0));
        assert_eq!(ray_box(dvec3(2.0, 0.5, 5.0), down, &b), None);
        assert_eq!(ray_box(dvec3(0.5, 0.5, 0.5), down, &b), Some(0.0));
        assert_eq!(ray_box(dvec3(0.5, 0.5, -1.0), down, &b), None);
    }

    /// What a slot does not take is not picked, even right under the pointer.
    #[test]
    fn only_what_is_accepted_is_picked() {
        let (scene, _) = scene();
        let (_, proj, _) = looking_down();
        let face = proj.screen(dvec3(12.0, 12.0, 5.0)).unwrap();
        assert!(pick_at(&scene, face, Accept::EDGES).is_none());
        assert!(pick_at(&scene, face, Accept::POINTS).is_none());
        assert!(matches!(
            pick_at(&scene, face, Accept::FACES),
            Some(Target::Face { .. })
        ));
    }
}
