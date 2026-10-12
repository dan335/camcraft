//! Picked features, found again on the models as they are now.
//!
//! A pick is saved as a model, a feature's number, and a point on the feature in
//! model coordinates ([`FeatureRef`], [`PointRef`]). The point is what the pick is;
//! the number is a quick way to it. Finding one goes, in order:
//!
//! 1. **By number.** The feature with that number still passes through the point,
//!    within [`tolerance`]. This is every pick on an unchanged model.
//! 2. **By position.** Some feature of the same kind passes through the point: the
//!    model was tessellated again, re-exported with its faces in another order, or
//!    its faces were found by a newer rule. An edge found this way must be the only
//!    one through the point, or the pick is [`FeatureError::Ambiguous`].
//! 3. Otherwise the pick is refused, and nothing is cut until it is made again:
//!    [`FeatureError::Moved`] when a face the file named still has the number but
//!    no longer passes through the point, [`FeatureError::Gone`] when nothing does.
//!    A face that moved and another face that took its number after an edit look
//!    the same from here, and cutting the wrong face, or the whole part where one
//!    face was meant, is the failure this exists to prevent.
//!
//! The tolerance is the job's: STEP is tessellated to a quarter of it, so two
//! tessellations of one surface lie within half of it of each other.
//!
//! A pick's point is in its model's own coordinates, where its file put it, so a
//! pick moves with its model; it is found on the body where the model is placed
//! ([`placed`]). The same functions serve the engine and the app, so what the app
//! highlights is what gets cut.
//!
//! # Boundaries
//!
//! An operation kept to picked faces and edges is kept to a [`machining_boundary`],
//! in the setup's frame: the outline, seen from above, of the picked faces, and the
//! areas the picked edges close. It is where the tool's *centre* may go, offset for
//! the operation's [`Containment`] and margin.
//!
//! An operation's picks are found once, however many of its settings read them,
//! and only if one does ([`Picking`]).

use std::cell::OnceCell;

use camcraft_geom::contour::{Contour, Region, RegionError};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::shadow::shadow_above;
use camcraft_geom::topology::{Body, EdgeKind, FaceSource};
use glam::{DVec2, DVec3};

use crate::job::{
    Boundary, Containment, FeatureRef, MachiningBoundary, Operation, Outline, Picks, PointRef,
    Selection, Snap,
};
use crate::setup::{Bodies, Prepared};

/// How a pick was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundBy {
    /// By its number, as saved.
    Index,
    /// By its point: the number now belongs to something else.
    Position,
}

/// A pick, found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    /// The feature's number now.
    pub index: u32,
    /// How it was found.
    pub by: FoundBy,
}

/// A pick that cannot be found, or cannot be used.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum FeatureError {
    /// The pick names a model that is not loaded.
    #[error("model '{0}' is not loaded")]
    NotLoaded(String),
    /// Nothing passes through the pick's point any more.
    #[error(
        "the {what} picked on '{model}' near ({:.3}, {:.3}, {:.3}) is no longer there; \
         pick it again",
        at[0], at[1], at[2]
    )]
    Gone {
        /// The model.
        model: String,
        /// What was picked: face, edge, point.
        what: &'static str,
        /// Where.
        at: [f64; 3],
    },
    /// A face the file named still has the pick's number, but no longer passes
    /// through its point: it has moved, or another face has the number now.
    #[error(
        "face {index} on '{model}' no longer passes through where it was picked \
         (it is {off_by:.2} mm away): the model has changed; pick it again"
    )]
    Moved {
        /// The model.
        model: String,
        /// The face's number.
        index: u32,
        /// How far the pick's point is from it now.
        off_by: f64,
    },
    /// Picked edges that do not close into loops, so they bound nothing.
    #[error(
        "the picked edges on '{model}' do not close into a loop: one ends at \
         ({:.3}, {:.3}, {:.3}) with nothing picked to carry on from it",
        at[0], at[1], at[2]
    )]
    OpenLoop {
        /// The model.
        model: String,
        /// The loose end, in the setup's frame.
        at: [f64; 3],
    },
    /// Seen from above, the picks cover nothing: walls, say.
    #[error("seen from above, the picked faces and edges enclose no area to machine")]
    NoArea,
    /// The boundary is the selection, and nothing is selected.
    #[error("the machining boundary is a selection, and no face or edge is picked")]
    NothingPicked,
    /// The tool does not fit inside the boundary.
    #[error(
        "a {diameter:.3} mm tool does not fit inside the machining boundary; make it larger, \
         or let the tool's centre reach it"
    )]
    NoRoom {
        /// The tool's diameter.
        diameter: f64,
    },
    /// The tool does not fit inside the boundary a pocket or an inside profile
    /// cuts within.
    #[error("a {diameter:.3} mm tool does not fit inside the boundary; use a smaller one")]
    TooWide {
        /// The tool's diameter.
        diameter: f64,
    },
    /// A polygon operation failed.
    #[error("the boundary could not be built: {0}")]
    Region(String),
    /// More than one feature passes through the pick's point.
    #[error(
        "the {what} picked on '{model}' near ({:.3}, {:.3}, {:.3}) could now be more than \
         one; pick it again",
        at[0], at[1], at[2]
    )]
    Ambiguous {
        /// The model.
        model: String,
        /// What was picked.
        what: &'static str,
        /// Where.
        at: [f64; 3],
    },
}

impl From<RegionError> for FeatureError {
    fn from(e: RegionError) -> Self {
        FeatureError::Region(e.to_string())
    }
}

/// How far a pick's point may lie from its feature and still be on it: the job's
/// tolerance, and the model's own fitting tolerance on top.
pub fn tolerance(job_tolerance: f64, body: &Body) -> f64 {
    job_tolerance.abs() + body.topology().tolerance()
}

/// Where a pick's point, in its model's own coordinates, is on `body`, placed.
pub fn placed(body: &Body, at: [f64; 3]) -> DVec3 {
    body.placement().transform_point3(DVec3::from_array(at))
}

/// A point on `body`, placed, in its model's own coordinates: what a pick saves.
pub fn unplaced(body: &Body, p: DVec3) -> [f64; 3] {
    body.placement().inverse().transform_point3(p).to_array()
}

fn gone(model: &str, what: &'static str, at: [f64; 3]) -> FeatureError {
    FeatureError::Gone {
        model: model.to_string(),
        what,
        at,
    }
}

/// Find a picked face on `body`.
pub fn find_face(body: &Body, pick: &FeatureRef, tol: f64) -> Result<Found, FeatureError> {
    let (mesh, t) = (body.mesh(), body.topology());
    let at = placed(body, pick.at);
    let by_index = t.distance_to_face(mesh, pick.index, at);
    if let Some(d) = by_index
        && d <= tol
    {
        return Ok(Found {
            index: pick.index,
            by: FoundBy::Index,
        });
    }
    // The face of the triangle nearest the point, if that is near enough. Only the
    // fallback searches every triangle, and only for a model that has changed.
    let nearest = (0..mesh.len())
        .filter_map(|i| {
            let tri = mesh.triangle(i)?;
            let d = camcraft_geom::mesh::closest_point_on_triangle(at, tri).distance(at);
            Some((u32::try_from(i).ok()?, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    if let Some((tri, d)) = nearest
        && d <= tol
        && let Some(face) = t.face_of(tri)
    {
        return Ok(Found {
            index: face,
            by: FoundBy::Position,
        });
    }
    if t.source() == FaceSource::File
        && let Some(off_by) = by_index
    {
        return Err(FeatureError::Moved {
            model: pick.model.clone(),
            index: pick.index,
            off_by,
        });
    }
    Err(gone(&pick.model, "face", pick.at))
}

/// Find a picked edge on `body`.
pub fn find_edge(body: &Body, pick: &FeatureRef, tol: f64) -> Result<Found, FeatureError> {
    let (mesh, t) = (body.mesh(), body.topology());
    let at = placed(body, pick.at);
    if let Some((_, d)) = t.closest_on_edge(mesh, pick.index, at)
        && d <= tol
    {
        return Ok(Found {
            index: pick.index,
            by: FoundBy::Index,
        });
    }
    let mut near: Vec<(u32, f64)> = (0..t.edges().len())
        .filter_map(|e| {
            let e = u32::try_from(e).ok()?;
            let (_, d) = t.closest_on_edge(mesh, e, at)?;
            (d <= tol).then_some((e, d))
        })
        .collect();
    near.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    match near.as_slice() {
        [] => Err(gone(&pick.model, "edge", pick.at)),
        [(e, _)] => Ok(Found {
            index: *e,
            by: FoundBy::Position,
        }),
        [(e, d1), (_, d2), ..] => {
            if d2 - d1 > tol / 2.0 {
                Ok(Found {
                    index: *e,
                    by: FoundBy::Position,
                })
            } else {
                Err(FeatureError::Ambiguous {
                    model: pick.model.clone(),
                    what: "edge",
                    at: pick.at,
                })
            }
        }
    }
}

/// Where snap `snap` of vertex or edge `index` is, if it has one: only a round
/// edge has a centre, and only an open one a middle.
pub fn snap_point(body: &Body, snap: Snap, index: u32) -> Option<DVec3> {
    let (mesh, t) = (body.mesh(), body.topology());
    match snap {
        Snap::Vertex => t.vertex(index).map(|v| v.position),
        Snap::CircleCenter => match t.edge(index)?.kind {
            EdgeKind::Circle(c) => Some(c.center),
            EdgeKind::Line | EdgeKind::Curve => None,
        },
        // A closed edge's middle is only opposite wherever it happens to start.
        Snap::EdgeMiddle => t
            .edge(index)
            .filter(|e| !e.closed)
            .and_then(|_| t.edge_middle(mesh, index)),
    }
}

/// Find a picked point on `body`: where it is now, placed.
pub fn find_point(body: &Body, pick: &PointRef, tol: f64) -> Result<(DVec3, Found), FeatureError> {
    let at = placed(body, pick.at);
    if let Some(p) = snap_point(body, pick.snap, pick.index)
        && p.distance(at) <= tol
    {
        let found = Found {
            index: pick.index,
            by: FoundBy::Index,
        };
        return Ok((p, found));
    }
    let t = body.topology();
    let count = match pick.snap {
        Snap::Vertex => t.vertices().len(),
        Snap::CircleCenter | Snap::EdgeMiddle => t.edges().len(),
    };
    // Any point within tolerance is within tolerance of where the pick was, so
    // the nearest will do; none can be wrong by more than the tolerance.
    (0..count)
        .filter_map(|i| {
            let i = u32::try_from(i).ok()?;
            let p = snap_point(body, pick.snap, i)?;
            let d = p.distance(at);
            (d <= tol).then_some((i, p, d))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)))
        .map(|(index, p, _)| {
            let found = Found {
                index,
                by: FoundBy::Position,
            };
            (p, found)
        })
        .ok_or_else(|| gone(&pick.model, "point", pick.at))
}

/// A picked edge, found, in the setup's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    /// The model it is on.
    pub model: String,
    /// Its number there.
    pub edge: u32,
    /// Its points, in order, in the setup's frame.
    pub points: Vec<DVec3>,
    /// Whether it closes on itself.
    pub closed: bool,
    /// The vertices it ends at, in its model's topology; `None` when closed.
    pub ends: Option<[u32; 2]>,
}

/// A picked face, found, in the setup's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct PickedFace {
    /// The model it is on.
    pub model: String,
    /// Its number there.
    pub face: u32,
    /// Its triangles, in the setup's frame.
    pub triangles: Vec<[DVec3; 3]>,
}

fn body<'a>(bodies: &Bodies<'a>, model: &str) -> Result<&'a Body, FeatureError> {
    bodies
        .get(model)
        .copied()
        .ok_or_else(|| FeatureError::NotLoaded(model.to_string()))
}

/// The picked faces, found, each once.
fn picked_faces(
    selection: &Selection,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    job_tolerance: f64,
) -> Result<Vec<PickedFace>, FeatureError> {
    let mut out: Vec<PickedFace> = Vec::new();
    for pick in &selection.faces {
        let body = body(bodies, &pick.model)?;
        let found = find_face(body, pick, tolerance(job_tolerance, body))?;
        if out
            .iter()
            .any(|f| f.model == pick.model && f.face == found.index)
        {
            continue;
        }
        let triangles = body
            .topology()
            .face(found.index)
            .map(|f| {
                f.triangles
                    .iter()
                    .filter_map(|&t| body.mesh().triangle(t as usize))
                    .map(|t| t.map(|p| prepared.frame.to_setup(p)))
                    .collect()
            })
            .unwrap_or_default();
        out.push(PickedFace {
            model: pick.model.clone(),
            face: found.index,
            triangles,
        });
    }
    Ok(out)
}

/// The picked edges, found, each once, in the setup's frame.
pub fn picked_chains(
    selection: &Selection,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    job_tolerance: f64,
) -> Result<Vec<Chain>, FeatureError> {
    let mut out: Vec<Chain> = Vec::new();
    for pick in &selection.edges {
        let body = body(bodies, &pick.model)?;
        let found = find_edge(body, pick, tolerance(job_tolerance, body))?;
        if out
            .iter()
            .any(|c| c.model == pick.model && c.edge == found.index)
        {
            continue;
        }
        let Some(edge) = body.topology().edge(found.index) else {
            continue;
        };
        out.push(Chain {
            model: pick.model.clone(),
            edge: found.index,
            points: body
                .topology()
                .edge_points(body.mesh(), found.index)
                .map(|p| prepared.frame.to_setup(p))
                .collect(),
            closed: edge.closed,
            ends: edge.ends,
        });
    }
    Ok(out)
}

/// Picked edges joined into closed loops, end to end through the vertices they
/// share, each loop as its points in order.
///
/// Every chain must be part of a loop: a loose end is [`FeatureError::OpenLoop`],
/// never a guess at how to close it.
fn loops(chains: &[Chain]) -> Result<Vec<Vec<DVec3>>, FeatureError> {
    let mut out = Vec::new();
    let mut used: Vec<bool> = chains.iter().map(|c| c.closed).collect();
    out.extend(chains.iter().filter(|c| c.closed).map(|c| c.points.clone()));
    for (start, first) in chains.iter().enumerate() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let Some([origin, mut at]) = first.ends else {
            continue;
        };
        if let Some(u) = used.get_mut(start) {
            *u = true;
        }
        let mut ring = first.points.clone();
        let mut last = first;
        while at != origin {
            // The next picked chain on this model that ends here, lowest first.
            let next = chains.iter().enumerate().find(|(j, c)| {
                !used.get(*j).copied().unwrap_or(true)
                    && c.model == first.model
                    && c.ends.is_some_and(|e| e.contains(&at))
            });
            let Some((j, c)) = next else {
                let end = ring.last().copied().unwrap_or(DVec3::NAN);
                return Err(FeatureError::OpenLoop {
                    model: last.model.clone(),
                    at: end.to_array(),
                });
            };
            if let Some(u) = used.get_mut(j) {
                *u = true;
            }
            let Some([a, b]) = c.ends else { break };
            // Joined the right way round, without repeating the shared point.
            if a == at {
                ring.extend(c.points.iter().skip(1));
                at = b;
            } else {
                ring.extend(c.points.iter().rev().skip(1));
                at = a;
            }
            last = c;
        }
        // Back at the first point, which the ring already starts with.
        ring.pop();
        out.push(ring);
    }
    Ok(out)
}

/// A selection's picked faces and edges, found, each once, in the setup's frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Picked {
    /// The faces.
    pub faces: Vec<PickedFace>,
    /// The edges.
    pub chains: Vec<Chain>,
}

impl Picked {
    /// Find `selection`'s picks on `bodies`, laid out as `prepared`.
    pub fn find(
        selection: &Selection,
        prepared: &Prepared,
        bodies: &Bodies<'_>,
        job_tolerance: f64,
    ) -> Result<Self, FeatureError> {
        Ok(Self {
            faces: picked_faces(selection, prepared, bodies, job_tolerance)?,
            chains: picked_chains(selection, prepared, bodies, job_tolerance)?,
        })
    }

    /// Every point of the faces and edges: the faces' first.
    pub fn points(&self) -> impl Iterator<Item = DVec3> + '_ {
        self.faces
            .iter()
            .flat_map(|f| f.triangles.iter().flatten().copied())
            .chain(self.chains.iter().flat_map(|c| c.points.iter().copied()))
    }

    /// How high they go; `None` for no picks.
    pub fn picks(&self) -> Option<Picks> {
        Picks::of(self.points().map(|p| p.z))
    }

    /// The area they enclose, seen from above: the faces' triangles projected
    /// straight down and joined, dropping any seen edge-on, with the loops the
    /// edges close read even-odd, so a loop inside another is a hole.
    fn area(&self) -> Result<Region, FeatureError> {
        let flat = |p: &DVec3| DVec2::new(p.x, p.y);
        // A triangle seen edge-on has no area from above, and a sliver of one only
        // adds points: anything narrower than a micron across is dropped.
        let polygons: Vec<Contour> = self
            .faces
            .iter()
            .flat_map(|f| &f.triangles)
            .filter_map(|t| {
                let [a, b, c] = t.map(|p| flat(&p));
                let longest = a.distance(b).max(b.distance(c)).max(c.distance(a));
                let ring = Contour::new(vec![a, b, c])?;
                (ring.signed_area().abs() * 2.0 > 1e-3 * longest).then_some(ring)
            })
            .collect();
        let from_faces = Region::union_of(&polygons)?;

        let rings: Vec<Contour> = loops(&self.chains)?
            .into_iter()
            .filter_map(|l| Contour::new(l.iter().map(flat).collect()))
            .collect();
        let from_edges = Region::new(rings);

        Ok(from_faces.union(&from_edges)?)
    }
}

/// An operation's selection, found on the models the first time something reads
/// it, and kept: however many of the operation's settings read its picks (its
/// boundary or outline, a height measured from them), they are found once, and
/// picks nothing reads are never found, so cannot stop it.
pub struct Picking<'a> {
    selection: &'a Selection,
    prepared: &'a Prepared,
    bodies: &'a Bodies<'a>,
    job_tolerance: f64,
    found: OnceCell<Picked>,
}

impl<'a> Picking<'a> {
    /// `selection`, to be found on `bodies`, laid out as `prepared`, when first
    /// read.
    pub fn new(
        selection: &'a Selection,
        prepared: &'a Prepared,
        bodies: &'a Bodies<'a>,
        job_tolerance: f64,
    ) -> Self {
        Self {
            selection,
            prepared,
            bodies,
            job_tolerance,
            found: OnceCell::new(),
        }
    }

    /// The setup they are found in.
    pub fn prepared(&self) -> &'a Prepared {
        self.prepared
    }

    /// The picks, found: the first time asked, and then kept. Ones that cannot
    /// be found are not kept, and are looked for again.
    pub fn found(&self) -> Result<&Picked, FeatureError> {
        if let Some(found) = self.found.get() {
            return Ok(found);
        }
        let found = Picked::find(
            self.selection,
            self.prepared,
            self.bodies,
            self.job_tolerance,
        )?;
        Ok(self.found.get_or_init(|| found))
    }

    /// How high the picks go; `None` for none.
    pub fn picks(&self) -> Result<Option<Picks>, FeatureError> {
        Ok(self.found()?.picks())
    }

    /// A boundary of `kind` as chosen, before the tool is kept to it, in the
    /// setup's frame; `None` for the stock's footprint, which the strategies
    /// keep to themselves.
    ///
    /// The bounding box and silhouette are the setup's models' seen from
    /// straight above. A selection is the area its picks enclose
    /// ([`Picked`]'s), and finds them.
    pub fn boundary_shape(&self, kind: Boundary) -> Result<Option<Region>, FeatureError> {
        let prepared = self.prepared;
        let area = match kind {
            Boundary::Stock => return Ok(None),
            Boundary::BoundingBox => {
                let b = prepared.model_bounds;
                Region::rectangle(DVec2::new(b.min.x, b.min.y), DVec2::new(b.max.x, b.max.y))
            }
            Boundary::Silhouette => prepared.shadow()?.area().clone(),
            Boundary::Selection => {
                if self.selection.is_empty() {
                    return Err(FeatureError::NothingPicked);
                }
                self.found()?.area()?
            }
        };
        if area.area() <= 1e-6 {
            return Err(FeatureError::NoArea);
        }
        Ok(Some(area))
    }

    /// Where the tool's centre may go for a 3D operation kept to `b`, with a
    /// tool `tool_radius` in radius, in the setup's frame: its
    /// [`boundary_shape`](Self::boundary_shape), offset by
    /// [`containment_offset`]. `None` for the stock's footprint. The arcs err by
    /// at most half the tolerance, inward when shrinking.
    pub fn machining_boundary(
        &self,
        b: &MachiningBoundary,
        tool_radius: f64,
    ) -> Result<Option<Region>, FeatureError> {
        let Some(area) = self.boundary_shape(b.kind)? else {
            return Ok(None);
        };
        let tolerance = self.job_tolerance;
        let region = area.offset(containment_offset(b, tool_radius), tolerance / 2.0);
        if region.area() <= 1e-9 {
            return Err(FeatureError::NoRoom {
                diameter: tool_radius * 2.0,
            });
        }
        Ok(Some(region))
    }

    /// 2D operation `op`'s outline, in the setup's frame, and the picked points
    /// its heights are measured from; this is `op`'s selection.
    ///
    /// The points are every point of the picked faces and edges, when they are
    /// the outline or a height is measured from them ([`Operation::levels_2d`]);
    /// none otherwise.
    pub fn outline(&self, op: &Operation) -> Result<(Option<Region>, Vec<DVec3>), FeatureError> {
        let shape = self.boundary_shape(op.outline.into())?;
        if op.outline != Outline::Selection && !op.heights.measured_from_selection() {
            return Ok((shape, Vec::new()));
        }
        Ok((shape, self.found()?.points().collect()))
    }
}

/// A boundary of `kind` as chosen, before the tool is kept to it, in the setup's
/// frame; `None` for the stock's footprint. [`Picking::boundary_shape`], for
/// `selection` found once.
pub fn boundary_shape(
    kind: Boundary,
    selection: &Selection,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    job_tolerance: f64,
) -> Result<Option<Region>, FeatureError> {
    Picking::new(selection, prepared, bodies, job_tolerance).boundary_shape(kind)
}

/// A mesh's outline from straight above: everything it covers, seen from the top.
/// A setup's part has its own, cast once ([`Prepared::shadow`]).
pub fn silhouette(mesh: &TriMesh) -> Result<Region, RegionError> {
    let Some(b) = mesh.bounds() else {
        return Ok(Region::default());
    };
    Ok(shadow_above(mesh, b.min.z - 1.0)?.area().clone())
}

/// A 2D operation's outline, in the setup's frame, and the picked points its
/// heights are measured from: [`Picking::outline`], for its selection found once.
pub fn outline(
    op: &Operation,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    job_tolerance: f64,
) -> Result<(Option<Region>, Vec<DVec3>), FeatureError> {
    Picking::new(&op.selection, prepared, bodies, job_tolerance).outline(op)
}

/// Every point of the picked faces and edges, in the setup's frame.
pub fn picked_points(
    selection: &Selection,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    job_tolerance: f64,
) -> Result<Vec<DVec3>, FeatureError> {
    let found = Picked::find(selection, prepared, bodies, job_tolerance)?;
    Ok(found.points().collect())
}

/// How high the picked faces and edges go, in the setup's frame; `None` for no
/// picks. What a height measured from the selection reads.
pub fn picks(
    selection: &Selection,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    job_tolerance: f64,
) -> Result<Option<Picks>, FeatureError> {
    Ok(Picked::find(selection, prepared, bodies, job_tolerance)?.picks())
}

/// How far the tool's centre may go past a boundary: Fusion's containment and
/// additional offset. The offset moves the boundary out, or in when the whole tool
/// is kept inside it.
pub fn containment_offset(b: &MachiningBoundary, tool_radius: f64) -> f64 {
    match b.containment {
        Containment::Center => b.margin,
        Containment::Inside => -tool_radius - b.margin,
        Containment::Outside => tool_radius + b.margin,
    }
}

/// Where the tool's centre may go for a 3D operation, in the setup's frame:
/// [`Picking::machining_boundary`], for `selection` found once.
pub fn machining_boundary(
    b: &MachiningBoundary,
    selection: &Selection,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    tool_radius: f64,
    job_tolerance: f64,
) -> Result<Option<Region>, FeatureError> {
    Picking::new(selection, prepared, bodies, job_tolerance).machining_boundary(b, tool_radius)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests;
