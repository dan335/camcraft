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
//! Finding is in model coordinates. The same functions serve the engine and the app,
//! so what the app highlights is what gets cut.
//!
//! # Boundaries
//!
//! An operation kept to picked faces and edges is kept to a [`machining_boundary`],
//! in the setup's frame: the outline, seen from above, of the picked faces, and the
//! areas the picked edges close. It is where the tool's *centre* may go, offset for
//! the operation's [`Containment`] and margin.

use camcraft_geom::contour::{Contour, Region, RegionError};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::shadow::shadow_above;
use camcraft_geom::topology::{Body, EdgeKind, FaceSource};
use glam::{DVec2, DVec3};

use crate::job::{Boundary, Containment, FeatureRef, Geometry, PointRef, Snap};
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

/// How far a pick's point may lie from its feature and still be on it: the job's
/// tolerance, and the model's own fitting tolerance on top.
pub fn tolerance(job_tolerance: f64, body: &Body) -> f64 {
    job_tolerance.abs() + body.topology().tolerance()
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
    let at = DVec3::from_array(pick.at);
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
    let at = DVec3::from_array(pick.at);
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

/// Find a picked point on `body`: where it is now, in model coordinates.
pub fn find_point(body: &Body, pick: &PointRef, tol: f64) -> Result<(DVec3, Found), FeatureError> {
    let at = DVec3::from_array(pick.at);
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
pub fn picked_faces(
    g: &Geometry,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    job_tolerance: f64,
) -> Result<Vec<PickedFace>, FeatureError> {
    let mut out: Vec<PickedFace> = Vec::new();
    for pick in &g.faces {
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
    g: &Geometry,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    job_tolerance: f64,
) -> Result<Vec<Chain>, FeatureError> {
    let mut out: Vec<Chain> = Vec::new();
    for pick in &g.edges {
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
pub fn loops(chains: &[Chain]) -> Result<Vec<Vec<DVec3>>, FeatureError> {
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

/// An operation's machining boundary as chosen, before the tool is kept to it, in
/// the setup's frame; `None` for the stock's footprint, which the strategies keep
/// to themselves.
///
/// The bounding box and silhouette are the setup's models' seen from straight
/// above. A selection is the picked faces' triangles projected straight down and
/// joined, dropping any seen edge-on, with the loops the picked edges close read
/// even-odd, so a loop inside another is a hole.
pub fn boundary_shape(
    g: &Geometry,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    job_tolerance: f64,
) -> Result<Option<Region>, FeatureError> {
    let region_err = |e: RegionError| FeatureError::Region(e.to_string());
    let area = match g.boundary() {
        Boundary::Stock => return Ok(None),
        Boundary::BoundingBox => {
            let b = prepared.model_bounds;
            Region::rectangle(DVec2::new(b.min.x, b.min.y), DVec2::new(b.max.x, b.max.y))
        }
        Boundary::Silhouette => silhouette(&prepared.part).map_err(region_err)?,
        Boundary::Selection => {
            if !g.picked() {
                return Err(FeatureError::NothingPicked);
            }
            selection(g, prepared, bodies, job_tolerance)?
        }
    };
    if area.area() <= 1e-6 {
        return Err(FeatureError::NoArea);
    }
    Ok(Some(area))
}

/// A mesh's outline from straight above: everything it covers, seen from the top.
pub fn silhouette(mesh: &TriMesh) -> Result<Region, RegionError> {
    let Some(b) = mesh.bounds() else {
        return Ok(Region::default());
    };
    Ok(shadow_above(mesh, b.min.z - 1.0)?.area().clone())
}

/// The area picked faces and edges enclose, seen from above.
fn selection(
    g: &Geometry,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    job_tolerance: f64,
) -> Result<Region, FeatureError> {
    let region_err = |e: RegionError| FeatureError::Region(e.to_string());
    let flat = |p: &DVec3| DVec2::new(p.x, p.y);

    let faces = picked_faces(g, prepared, bodies, job_tolerance)?;
    // A triangle seen edge-on has no area from above, and a sliver of one only
    // adds points: anything narrower than a micron across is dropped.
    let polygons: Vec<Contour> = faces
        .iter()
        .flat_map(|f| &f.triangles)
        .filter_map(|t| {
            let [a, b, c] = t.map(|p| flat(&p));
            let longest = a.distance(b).max(b.distance(c)).max(c.distance(a));
            let ring = Contour::new(vec![a, b, c])?;
            (ring.signed_area().abs() * 2.0 > 1e-3 * longest).then_some(ring)
        })
        .collect();
    let from_faces = Region::union_of(&polygons).map_err(region_err)?;

    let chains = picked_chains(g, prepared, bodies, job_tolerance)?;
    let rings: Vec<Contour> = loops(&chains)?
        .into_iter()
        .filter_map(|l| Contour::new(l.iter().map(flat).collect()))
        .collect();
    let from_edges = Region::new(rings);

    from_faces.union(&from_edges).map_err(region_err)
}

/// How far the tool's centre may go past a boundary: Fusion's containment and
/// additional offset. The offset moves the boundary out, or in when the whole tool
/// is kept inside it.
pub fn containment_offset(g: &Geometry, tool_radius: f64) -> f64 {
    match g.containment {
        Containment::Center => g.margin,
        Containment::Inside => -tool_radius - g.margin,
        Containment::Outside => tool_radius + g.margin,
    }
}

/// Where the tool's centre may go for an operation, in the setup's frame: its
/// [`boundary_shape`], offset by [`containment_offset`]. `None` for the stock's
/// footprint. The arcs err by at most half the tolerance, inward when shrinking.
pub fn machining_boundary(
    g: &Geometry,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    tool_radius: f64,
    job_tolerance: f64,
) -> Result<Option<Region>, FeatureError> {
    let Some(area) = boundary_shape(g, prepared, bodies, job_tolerance)? else {
        return Ok(None);
    };
    let region = area.offset(containment_offset(g, tool_radius), job_tolerance / 2.0);
    if region.area() <= 1e-9 {
        return Err(FeatureError::NoRoom {
            diameter: tool_radius * 2.0,
        });
    }
    Ok(Some(region))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use camcraft_geom::synthetic;
    use camcraft_geom::topology::FaceKind;
    use glam::{dvec2, dvec3};

    fn plate() -> Body {
        Body::derived(synthetic::plate_with_hole(20.0, 5.0, 4.0, 32))
    }

    fn top_face(body: &Body) -> u32 {
        let i = body
            .topology()
            .faces()
            .iter()
            .position(|f| matches!(f.kind, FaceKind::Plane { normal, .. } if normal.z > 0.5))
            .unwrap();
        u32::try_from(i).unwrap()
    }

    fn rim(body: &Body, z: f64) -> u32 {
        let i = body
            .topology()
            .edges()
            .iter()
            .position(|e| matches!(e.kind, EdgeKind::Circle(c) if (c.center.z - z).abs() < 1e-9))
            .unwrap();
        u32::try_from(i).unwrap()
    }

    #[test]
    fn an_unchanged_model_finds_picks_by_number() {
        let body = plate();
        let face = FeatureRef {
            model: "p".into(),
            index: top_face(&body),
            at: [10.0, 10.0, 5.0],
        };
        let found = find_face(&body, &face, 0.01).unwrap();
        assert_eq!(found.by, FoundBy::Index);
        assert_eq!(found.index, face.index);

        let e = rim(&body, 5.0);
        let edge = FeatureRef {
            model: "p".into(),
            index: e,
            at: [4.0, 0.0, 5.0],
        };
        assert_eq!(find_edge(&body, &edge, 0.01).unwrap().index, e);

        let centre = PointRef {
            model: "p".into(),
            snap: Snap::CircleCenter,
            index: e,
            at: [0.0, 0.0, 5.0],
        };
        let (p, found) = find_point(&body, &centre, 0.01).unwrap();
        assert!(p.distance(dvec3(0.0, 0.0, 5.0)) < 1e-9);
        assert_eq!(found.by, FoundBy::Index);
    }

    /// Numbers that now belong to something else: each pick is found again by
    /// where it is.
    #[test]
    fn a_renumbered_model_finds_picks_by_position() {
        let body = plate();
        let top = top_face(&body);
        let wrong = (top + 1) % u32::try_from(body.topology().faces().len()).unwrap();
        let face = FeatureRef {
            model: "p".into(),
            index: wrong,
            at: [10.0, 10.0, 5.0],
        };
        let found = find_face(&body, &face, 0.01).unwrap();
        assert_eq!(
            found,
            Found {
                index: top,
                by: FoundBy::Position
            }
        );

        let bottom_rim = rim(&body, 0.0);
        let top_rim = rim(&body, 5.0);
        let edge = FeatureRef {
            model: "p".into(),
            index: bottom_rim,
            at: [0.0, 4.0, 5.0],
        };
        assert_eq!(find_edge(&body, &edge, 0.01).unwrap().index, top_rim);

        let centre = PointRef {
            model: "p".into(),
            snap: Snap::CircleCenter,
            index: bottom_rim,
            at: [0.0, 0.0, 5.0],
        };
        let (p, found) = find_point(&body, &centre, 0.01).unwrap();
        assert_eq!(found.index, top_rim);
        assert!(p.distance(dvec3(0.0, 0.0, 5.0)) < 1e-9);
    }

    #[test]
    fn a_pick_on_nothing_is_gone() {
        let body = plate();
        let face = FeatureRef {
            model: "p".into(),
            index: top_face(&body),
            at: [10.0, 10.0, 10.0],
        };
        let err = find_face(&body, &face, 0.01).unwrap_err();
        assert!(matches!(err, FeatureError::Gone { what: "face", .. }));
        assert!(err.to_string().contains("pick it again"), "{err}");
        let point = PointRef {
            model: "p".into(),
            snap: Snap::Vertex,
            index: 0,
            at: [3.0, 3.0, 3.0],
        };
        assert!(find_point(&body, &point, 0.01).is_err());
    }

    /// A face the file named that no longer passes through its pick is refused by
    /// name: it may have moved, or another face may have its number now, and the
    /// two cannot be told apart.
    #[test]
    fn a_moved_face_from_a_file_is_refused_by_name() {
        let mesh = synthetic::cuboid(DVec3::ZERO, dvec3(10.0, 10.0, 10.0));
        let labels: Vec<u32> = (0..12).map(|t| t / 2).collect();
        let body = Body::with_faces(mesh.clone(), labels.clone(), 6).unwrap();
        // Face 1 is the top; the part has since grown 2 mm taller.
        let taller = TriMesh::new(
            mesh.vertices()
                .iter()
                .map(|v| {
                    if v.z > 5.0 {
                        *v + dvec3(0.0, 0.0, 2.0)
                    } else {
                        *v
                    }
                })
                .collect(),
            mesh.triangles().to_vec(),
        )
        .unwrap();
        let moved = Body::with_faces(taller.clone(), labels, 6).unwrap();
        let pick = FeatureRef {
            model: "p".into(),
            index: 1,
            at: [5.0, 5.0, 10.0],
        };
        assert_eq!(find_face(&body, &pick, 0.01).unwrap().by, FoundBy::Index);
        let err = find_face(&moved, &pick, 0.01).unwrap_err();
        assert!(
            matches!(err, FeatureError::Moved { index: 1, off_by, .. } if (off_by - 2.0).abs() < 1e-9),
            "{err:?}"
        );
        assert!(err.to_string().contains("2.00 mm away"), "{err}");
        // Faces found from triangles have no numbers of their own to go by.
        let derived = Body::derived(taller);
        assert!(matches!(
            find_face(&derived, &pick, 0.01),
            Err(FeatureError::Gone { .. })
        ));
    }

    /// A point on a box's corner is on three edges: an edge picked there is
    /// ambiguous once its number is lost.
    #[test]
    fn an_edge_that_could_be_several_is_refused() {
        let body = Body::derived(synthetic::cuboid(DVec3::ZERO, DVec3::ONE));
        let pick = FeatureRef {
            model: "p".into(),
            index: 999,
            at: [0.0, 0.0, 0.0],
        };
        assert!(matches!(
            find_edge(&body, &pick, 0.01),
            Err(FeatureError::Ambiguous { .. })
        ));
    }

    fn laid_out(body: &Body) -> Prepared {
        use crate::job::{Job, ModelDef, Setup, StockDef, StockSpec};
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
        let mut setup = Setup::new("top");
        setup.models = vec!["p".into()];
        setup.stocks = vec!["s".into()];
        j.setups.push(setup);
        let bodies: Bodies<'_> = [("p", body)].into_iter().collect();
        crate::setup::prepare(&j, 0, &bodies).unwrap()
    }

    fn boundary(body: &Body, g: &Geometry, radius: f64) -> Result<Option<Region>, FeatureError> {
        let prepared = laid_out(body);
        let bodies: Bodies<'_> = [("p", body)].into_iter().collect();
        machining_boundary(g, &prepared, &bodies, radius, 0.01)
    }

    fn face_at(body: &Body, at: [f64; 3]) -> FeatureRef {
        let pick = FeatureRef {
            model: "p".into(),
            index: u32::MAX,
            at,
        };
        let found = find_face(body, &pick, 0.01).unwrap();
        FeatureRef {
            index: found.index,
            ..pick
        }
    }

    fn edge_at(body: &Body, at: [f64; 3]) -> FeatureRef {
        let pick = FeatureRef {
            model: "p".into(),
            index: u32::MAX,
            at,
        };
        let found = find_edge(body, &pick, 0.01).unwrap();
        FeatureRef {
            index: found.index,
            ..pick
        }
    }

    #[test]
    fn nothing_picked_is_no_boundary() {
        let body = plate();
        assert_eq!(boundary(&body, &Geometry::default(), 1.0), Ok(None));
    }

    /// The models' box and outline from above: the plate is 40 square with a
    /// hole through it, which the box covers and the silhouette leaves open.
    #[test]
    fn the_bounding_box_and_silhouette_are_the_part_from_above() {
        let body = plate();
        let mut g = Geometry {
            boundary_kind: Some(Boundary::BoundingBox),
            ..Geometry::default()
        };
        let b = boundary(&body, &g, 1.0).unwrap().unwrap();
        assert!((b.area() - 1600.0).abs() < 1e-6, "{}", b.area());
        assert!(b.contains(dvec2(0.0, 0.0)));

        g.boundary_kind = Some(Boundary::Silhouette);
        let s = boundary(&body, &g, 1.0).unwrap().unwrap();
        let hole = 0.5 * 32.0 * 16.0 * (std::f64::consts::TAU / 32.0).sin();
        assert!((s.area() - (1600.0 - hole)).abs() < 1e-3, "{}", s.area());
        assert!(!s.contains(dvec2(0.0, 0.0)));
        assert!(s.contains(dvec2(10.0, 10.0)));

        // Kept wholly inside, a positive offset moves it in, as Fusion's does.
        g.containment = Containment::Inside;
        g.margin = 1.0;
        let inside = boundary(&body, &g, 1.0).unwrap().unwrap();
        assert!(!inside.contains(dvec2(18.5, 0.0)));
        assert!(inside.contains(dvec2(17.5, 0.0)));
        // Reaching in from outside, it moves out.
        g.containment = Containment::Outside;
        let outside = boundary(&body, &g, 1.0).unwrap().unwrap();
        assert!(outside.contains(dvec2(21.9, 0.0)));
        assert!(!outside.contains(dvec2(22.1, 0.0)));
    }

    /// The stock's boundary is none; a selection with nothing picked is refused,
    /// not taken for the stock's.
    #[test]
    fn a_selection_needs_picks() {
        let body = plate();
        let mut g = Geometry {
            boundary_kind: Some(Boundary::Stock),
            ..Geometry::default()
        };
        assert_eq!(boundary(&body, &g, 1.0), Ok(None));
        g.boundary_kind = Some(Boundary::Selection);
        assert_eq!(boundary(&body, &g, 1.0), Err(FeatureError::NothingPicked));
        // Picks with the stock's boundary chosen are kept, and not used.
        g.faces.push(face_at(&body, [15.0, 15.0, 5.0]));
        g.boundary_kind = Some(Boundary::Stock);
        assert_eq!(boundary(&body, &g, 1.0), Ok(None));
    }

    /// The step's plateau, seen from above, is a 10 by 20 rectangle; the tool's
    /// centre may cover it, keep a radius inside it, or reach a radius past it.
    #[test]
    fn a_picked_face_bounds_by_its_outline_and_the_containment() {
        let body = Body::derived(synthetic::step(5.0, 10.0));
        let mut g = Geometry::default();
        g.faces.push(face_at(&body, [-5.0, 0.0, 5.0]));
        let centre = boundary(&body, &g, 1.0).unwrap().unwrap();
        assert!((centre.area() - 200.0).abs() < 0.01, "{}", centre.area());
        assert!(centre.contains(dvec2(-0.1, 9.9)));
        assert!(!centre.contains(dvec2(0.1, 0.0)));

        g.containment = Containment::Inside;
        let inside = boundary(&body, &g, 1.0).unwrap().unwrap();
        // Shrinking errs inward by the arc tolerance, half the job's 0.01, a side.
        let err = 0.005 * 2.0 * (8.0 + 18.0) * 2.0;
        assert!(
            (inside.area() - 8.0 * 18.0).abs() < err,
            "{}",
            inside.area()
        );
        assert!(!inside.contains(dvec2(-0.5, 0.0)));

        g.containment = Containment::Outside;
        let outside = boundary(&body, &g, 1.0).unwrap().unwrap();
        // Growing errs outward by the arc tolerance, so the radius is 1.005.
        let r: f64 = 1.005;
        let rounded = (10.0 + 2.0 * r) * (20.0 + 2.0 * r) - (4.0 - std::f64::consts::PI) * r * r;
        assert!(
            (outside.area() - rounded).abs() < 0.05,
            "{}",
            outside.area()
        );
        assert!(outside.contains(dvec2(0.9, 0.0)));

        // The margin moves the boundary out on top of the containment.
        g.containment = Containment::Center;
        g.margin = 1.0;
        let margin = boundary(&body, &g, 1.0).unwrap().unwrap();
        assert!((margin.area() - outside.area()).abs() < 0.1);

        // A tool too big to fit inside.
        g.margin = 0.0;
        g.containment = Containment::Inside;
        assert_eq!(
            boundary(&body, &g, 6.0),
            Err(FeatureError::NoRoom { diameter: 12.0 })
        );
    }

    /// The step's wall is seen edge-on from above: there is nothing to machine.
    #[test]
    fn a_wall_alone_has_no_area() {
        let body = Body::derived(synthetic::step(5.0, 10.0));
        let mut g = Geometry::default();
        g.faces.push(face_at(&body, [0.0, 0.0, 2.5]));
        assert_eq!(boundary(&body, &g, 1.0), Err(FeatureError::NoArea));
    }

    /// The four edges round the plate's top and the rim of its hole: a square with
    /// a round hole. Three of the four sides alone do not close.
    #[test]
    fn picked_edges_close_into_loops_with_holes() {
        let body = plate();
        let mut g = Geometry::default();
        for at in [
            [0.0, -20.0, 5.0],
            [20.0, 0.0, 5.0],
            [0.0, 20.0, 5.0],
            [-20.0, 0.0, 5.0],
            [4.0, 0.0, 5.0],
        ] {
            g.edges.push(edge_at(&body, at));
        }
        let region = boundary(&body, &g, 1.0).unwrap().unwrap();
        let hole = 32.0 / 2.0 * 16.0 * (std::f64::consts::TAU / 32.0).sin();
        assert!(
            (region.area() - (1600.0 - hole)).abs() < 0.01,
            "{}",
            region.area()
        );
        assert!(!region.contains(dvec2(0.0, 0.0)));
        assert!(region.contains(dvec2(10.0, 10.0)));

        g.edges.remove(3);
        let err = boundary(&body, &g, 1.0).unwrap_err();
        assert!(matches!(err, FeatureError::OpenLoop { .. }), "{err:?}");
        assert!(err.to_string().contains("do not close"), "{err}");
    }

    /// A pick that is gone stops the boundary, rather than dropping out of it.
    #[test]
    fn a_gone_pick_is_an_error_not_a_smaller_boundary() {
        let body = plate();
        let mut g = Geometry::default();
        g.faces.push(face_at(&body, [10.0, 10.0, 5.0]));
        g.faces.push(FeatureRef {
            model: "p".into(),
            index: 0,
            at: [50.0, 50.0, 50.0],
        });
        assert!(matches!(
            boundary(&body, &g, 1.0),
            Err(FeatureError::Gone { .. })
        ));
        g.faces[1].model = "elsewhere".into();
        assert!(matches!(
            boundary(&body, &g, 1.0),
            Err(FeatureError::NotLoaded(_))
        ));
    }
}
