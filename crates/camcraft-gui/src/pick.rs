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

use std::sync::Arc;

use camcraft_core::features::snap_point;
use camcraft_core::frame::Frame;
use camcraft_core::job::{Along, PointRef, Snap};
use camcraft_core::setup::Prepared;
use camcraft_geom::bvh::MeshBvh;
use camcraft_geom::mesh::Aabb;
use camcraft_geom::topology::{Body, EdgeKind};
use camcraft_session::Model;
use eframe::egui::{Pos2, Rect, pos2};
use glam::{DVec3, Mat4};

/// How near the pointer a point must be to be picked, in points.
pub const POINT_REACH: f32 = 8.0;
/// How near the pointer an edge must be to be picked, in points.
pub const EDGE_REACH: f32 = 6.0;
/// How near the pointer a point on the stock's box must be, in points: they sit in
/// open air at the box's corners, so a little more.
pub const STOCK_REACH: f32 = 10.0;
/// Edges tried, nearest first, before giving up on finding one that is not hidden.
const EDGE_TRIES: usize = 16;

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
}

impl Accept {
    /// Anything: for looking at what is there.
    pub const ALL: Self = Self {
        faces: true,
        edges: true,
        points: true,
        stock: true,
    };
    /// Faces only.
    pub const FACES: Self = Self {
        faces: true,
        edges: false,
        points: false,
        stock: false,
    };
    /// Edges only.
    pub const EDGES: Self = Self {
        faces: false,
        edges: true,
        points: false,
        stock: false,
    };
    /// Points, on the models or the stock.
    pub const POINTS: Self = Self {
        faces: false,
        edges: false,
        points: true,
        stock: true,
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
}

/// One model, as picking needs it: its faces and edges, its index, and where its
/// edges and points sit in the setup's frame, worked out once per layout.
pub struct PickModel {
    name: String,
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
    /// The setup's models, in its order.
    pub models: Vec<PickModel>,
    /// How model coordinates became the setup's.
    pub frame: Frame,
    /// The box around the stock, in the setup's frame.
    pub stock: Option<Aabb>,
    /// How far behind a surface a point may be and still count as on it.
    eps: f64,
}

impl PickScene {
    /// Ready `prepared` for picking, with its models as loaded and their indexes.
    /// Models it does not machine are left out.
    pub fn new(
        prepared: &Prepared,
        loaded: impl IntoIterator<Item = (String, Arc<Model>, Arc<MeshBvh>)>,
    ) -> Self {
        let frame = prepared.frame;
        let mut loaded: Vec<_> = loaded
            .into_iter()
            .filter(|(n, ..)| prepared.piece(n).is_some())
            .collect();
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
            eps: (1e-3 * size).max(1e-3),
        }
    }

    /// The model called `name`.
    pub fn model(&self, name: &str) -> Option<&PickModel> {
        self.models.iter().find(|m| m.name == name)
    }

    /// The first surface the ray from `o` along unit `d` meets within `max`: which
    /// model, which of its triangles, and how far.
    pub fn cast(&self, o: DVec3, d: DVec3, max: f64) -> Option<(usize, u32, f64)> {
        let om = self.frame.to_model(o);
        let dm = self.frame.to_model(o + d) - om;
        self.models
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let hit = m.bvh.cast_ray(m.body().mesh(), om, dm, max)?;
                Some((i, hit.triangle, hit.t))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
    }

    /// Whether `p` can be seen from `eye`, and is not behind the part.
    fn visible(&self, eye: DVec3, p: DVec3) -> bool {
        let to = p - eye;
        let dist = to.length();
        if dist <= self.eps {
            return true;
        }
        self.cast(eye, to / dist, dist)
            .is_none_or(|(_, _, t)| t >= dist - self.eps)
    }

    /// What is under `cursor`, for a ray `(eye, unit direction)` from the eye
    /// through it, among what `accept` takes.
    pub fn pick(
        &self,
        (eye, dir): (DVec3, DVec3),
        proj: &Projector,
        cursor: Pos2,
        accept: Accept,
    ) -> Option<Target> {
        let near = |p: DVec3, reach: f32| {
            proj.screen(p)
                .map(|s| s.distance(cursor))
                .filter(|d| *d <= reach)
        };

        if accept.points {
            // The nearest point within reach that can be seen: vertices first on
            // a tie, as the likeliest meant.
            let mut best: Option<(f32, Target)> = None;
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
                        if best.as_ref().is_some_and(|(b, _)| *b <= d) || !self.visible(eye, p) {
                            continue;
                        }
                        if let Ok(index) = u32::try_from(i) {
                            best = Some((d, self.point(m, snap, index, p)));
                        }
                    }
                }
            }
            if let Some((_, t)) = best {
                return Some(t);
            }
            // Over a round edge, its centre: the usual way to put a work zero on a
            // hole.
            if let Some((mi, e, _)) = self.nearest_edge(eye, proj, cursor)
                && let Some(m) = self.models.get(mi)
                && let Some(Some(c)) = m.centres.get(e as usize)
            {
                return Some(self.point(m, Snap::CircleCenter, e, *c));
            }
        }

        if accept.stock
            && let Some(b) = self.stock
        {
            let mut best: Option<(f32, Target)> = None;
            for (x, y, z) in box_points() {
                let p = DVec3::new(
                    x.pick(b.min.x, b.max.x),
                    y.pick(b.min.y, b.max.y),
                    z.pick(b.min.z, b.max.z),
                );
                let Some(d) = near(p, STOCK_REACH) else {
                    continue;
                };
                if best.as_ref().is_none_or(|(b, _)| d < *b) && self.visible(eye, p) {
                    best = Some((d, Target::StockPoint { x, y, z, setup: p }));
                }
            }
            if let Some((_, t)) = best {
                return Some(t);
            }
        }

        if accept.edges
            && let Some((mi, edge, at)) = self.nearest_edge(eye, proj, cursor)
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
        eye: DVec3,
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
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.into_iter()
            .take(EDGE_TRIES)
            .find(|(_, _, _, p)| self.visible(eye, *p))
            .map(|(_, mi, e, p)| (mi, e, p))
    }
}

/// The 26 points of a box a work zero goes on: corners, the middles of edges and
/// the centres of faces. Its centre is inside it, where nothing can be clicked.
pub fn box_points() -> impl Iterator<Item = (Along, Along, Along)> {
    const ALL: [Along; 3] = [Along::Min, Along::Center, Along::Max];
    ALL.into_iter()
        .flat_map(|x| ALL.into_iter().flat_map(move |y| ALL.map(|z| (x, y, z))))
        .filter(|p| *p != (Along::Center, Along::Center, Along::Center))
}

/// Where the stock's corner, edge middle or face centre is, in words: "top,
/// front-left corner", "the middle of the top's front edge", "the centre of the
/// top".
pub fn describe_box_point(x: Along, y: Along, z: Along) -> String {
    let side = |a: Along, lo: &'static str, hi: &'static str| match a {
        Along::Min => Some(lo),
        Along::Max => Some(hi),
        Along::Center => None,
    };
    let (sx, sy, sz) = (
        side(x, "left", "right"),
        side(y, "front", "back"),
        side(z, "bottom", "top"),
    );
    match (sz, sy, sx) {
        (Some(z), Some(y), Some(x)) => format!("{z}, {y}-{x} corner"),
        (Some(z), Some(y), None) => format!("middle of the {z}'s {y} edge"),
        (Some(z), None, Some(x)) => format!("middle of the {z}'s {x} edge"),
        (None, Some(y), Some(x)) => format!("middle of the {y}-{x} upright edge"),
        (Some(s), None, None) | (None, Some(s), None) | (None, None, Some(s)) => {
            format!("centre of the {s}")
        }
        (None, None, None) => "centre".to_string(),
    }
}

/// How a round edge is described: "Ø6.35 circle" or "R3.00 arc".
pub fn describe_circle(body: &Body, edge: u32) -> Option<String> {
    match body.topology().edge(edge)?.kind {
        EdgeKind::Circle(c) if c.full => Some(format!("Ø{:.2} circle", c.radius * 2.0)),
        EdgeKind::Circle(c) => Some(format!("R{:.2} arc", c.radius)),
        EdgeKind::Line | EdgeKind::Curve => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::camera::Camera;
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
        let scene = PickScene::new(&prepared, [("p".to_string(), model.clone(), bvh)]);
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

    fn pick_at(scene: &PickScene, screen: Pos2, accept: Accept) -> Option<Target> {
        let (c, proj, aspect) = looking_down();
        let ray = c.ray(aspect, proj.ndc(screen));
        scene.pick(ray, &proj, screen, accept)
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
        assert_eq!(point.at, [20.0, 20.0, 5.0]);
        // A bottom corner lies straight under the top one, and is hidden by it.
        let v = scene.models[0]
            .vertices
            .iter()
            .position(|p| (*p - dvec3(-20.0, -20.0, 0.0)).length() < 1e-9)
            .unwrap();
        let (c, _, _) = looking_down();
        assert!(!scene.visible(c.eye().as_dvec3(), scene.models[0].vertices[v]));
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

    /// What a slot does not take is not picked, even right under the pointer.
    #[test]
    fn only_what_is_accepted_is_picked() {
        let (scene, _) = scene();
        let (_, proj, _) = looking_down();
        let face = proj.screen(dvec3(12.0, 12.0, 5.0)).unwrap();
        assert!(pick_at(&scene, face, Accept::EDGES).is_none());
        assert!(pick_at(&scene, face, Accept::POINTS).is_none());
        assert!(matches!(
            pick_at(&scene, face, Accept::ALL),
            Some(Target::Face { .. })
        ));
    }
}
