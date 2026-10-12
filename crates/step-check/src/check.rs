//! Holding a kernel's mesh up against what the file says.

use glam::DVec3;

use crate::part21::{self, FileFace, StepFacts};

/// How far a face may stray from its surface, in multiples of the tolerance it was
/// tessellated to. Four, because OpenCascade's own meshes reach 3.4 times on a
/// surface of revolution; on the faces measured here it stays within 1.3. For
/// camcraft, which tessellates to a quarter of the job's tolerance, this is the
/// job's tolerance.
pub const SLACK: f64 = 4.0;

/// What a kernel makes of a STEP file: triangles in millimetres, grouped by the
/// B-rep face they came from.
#[derive(Debug, Clone, Default)]
pub struct KernelMesh {
    /// Corner positions, in millimetres.
    pub vertices: Vec<DVec3>,
    /// Three per triangle, the triangles in face order.
    pub indices: Vec<usize>,
    /// How many triangles each face gave, in the kernel's order of faces. The order
    /// should depend only on the file, so a face keeps its number when the
    /// tolerance changes.
    pub face_triangles: Vec<usize>,
}

/// Whether the mesh was checked against the file's own geometry.
#[derive(Debug, Clone, PartialEq)]
pub enum FileCheck {
    /// Both geometric checks ran and passed.
    Passed,
    /// They could not run, and why. Worth telling a person about.
    Skipped(&'static str),
}

/// A mesh that does not match the file it came from.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Problem {
    /// Fewer faces came out of the kernel than the file defines.
    #[error(
        "the file defines {in_file} face(s) but only {meshed} arrived; \
         a missing face is a hole in the part"
    )]
    DroppedFaces {
        /// Faces the file defines.
        in_file: u64,
        /// Faces the kernel gave.
        meshed: u64,
    },
    /// A face came back without a single triangle. A kernel that gives up on a face
    /// may hand it back empty rather than refuse, and the face count still passes.
    #[error("face {face} came back with no triangles; a missing face is a hole in the part")]
    EmptyFace {
        /// The face, in the kernel's order.
        face: usize,
    },
    /// The faces' triangle counts do not add up to the triangles given.
    #[error("the faces account for {counted} triangles but the mesh has {triangles}")]
    Miscounted {
        /// The sum of the faces' counts.
        counted: usize,
        /// Triangles in the mesh.
        triangles: usize,
    },
    /// The mesh extends beyond anything the file describes.
    #[error(
        "the mesh reaches {by:.3} mm outside the file's own geometry on {axis}; \
         the surfaces were evaluated wrongly or the units are not what the file says"
    )]
    OutsideFile {
        /// Which axis and side.
        axis: &'static str,
        /// How far, in millimetres.
        by: f64,
    },
    /// A face's triangles lie off every surface the file defines there.
    #[error(
        "face {face} lies {by:.3} mm off every face of the file it could be; \
         its surface was evaluated wrongly"
    )]
    OffSurface {
        /// The face, in the kernel's order.
        face: usize,
        /// How far off, at worst, in millimetres.
        by: f64,
    },
    /// A face of the file, on a surface measured exactly, has none of the mesh on
    /// it: every face the kernel gave lies on some other surface.
    #[error(
        "nothing of the mesh lies on the file's face #{entity}; \
         a missing face is a hole in the part"
    )]
    Uncovered {
        /// The face's entity in the file.
        entity: u64,
    },
    /// The mesh falls short of the file's own vertices.
    #[error(
        "the mesh stops {by:.3} mm short of the file's vertices on {axis}; \
         the part is smaller than the file says, which is what a unit error looks like"
    )]
    ShortOfVertices {
        /// Which axis and side.
        axis: &'static str,
        /// How far, in millimetres.
        by: f64,
    },
}

/// Hold `mesh`, tessellated to within `tolerance` millimetres, up against what the
/// file says. See the crate docs for the checks.
pub fn check(facts: &StepFacts, mesh: &KernelMesh, tolerance: f64) -> Result<FileCheck, Problem> {
    let meshed = mesh.face_triangles.len() as u64;
    if meshed < facts.faces {
        return Err(Problem::DroppedFaces {
            in_file: facts.faces,
            meshed,
        });
    }
    if let Some(face) = mesh.face_triangles.iter().position(|&n| n == 0) {
        return Err(Problem::EmptyFace { face });
    }
    let counted: usize = mesh.face_triangles.iter().sum();
    if counted * 3 != mesh.indices.len() {
        return Err(Problem::Miscounted {
            counted,
            triangles: mesh.indices.len() / 3,
        });
    }

    if facts.unplaced {
        return Ok(FileCheck::Skipped(
            "geometry is placed by transforms this cannot follow, so the file's raw coordinates bound nothing",
        ));
    }
    let used = mesh.indices.iter().filter_map(|&i| mesh.vertices.get(i));
    let (Some(extent), Some(on_part), Some(got)) = (
        facts.extent,
        part21::bounds(&facts.vertices),
        part21::bounds(used),
    ) else {
        return Ok(FileCheck::Skipped(
            "the file has no vertices to check against",
        ));
    };

    // Room for the tessellation itself and for float noise on a large part.
    let size = (extent.1 - extent.0).max_element().max(1.0);
    let slack = tolerance + size * 1e-9;

    let (lo, hi) = (
        extent.0 - DVec3::splat(slack),
        extent.1 + DVec3::splat(slack),
    );
    if let Some((axis, by)) = worst_excess(got, (lo, hi)) {
        return Err(Problem::OutsideFile { axis, by });
    }

    let (lo, hi) = (got.0 - DVec3::splat(slack), got.1 + DVec3::splat(slack));
    if let Some((axis, by)) = worst_excess(on_part, (lo, hi)) {
        return Err(Problem::ShortOfVertices { axis, by });
    }

    let near = SLACK * tolerance + size * 1e-9;
    let mut covered = vec![false; facts.file_faces.len()];
    let mut first = 0;
    for (face, &n) in mesh.face_triangles.iter().enumerate() {
        let corners = mesh
            .indices
            .get(first * 3..(first + n) * 3)
            .unwrap_or_default();
        first += n;
        let on = on_surface(&facts.file_faces, &mesh.vertices, corners, near);
        for i in on.fits {
            if let Some(c) = covered.get_mut(i) {
                *c = true;
            }
        }
        if let Some(by) = on.off {
            return Err(Problem::OffSurface { face, by });
        }
    }
    // A face the kernel lost can hide behind one it put on the wrong surface:
    // flattened into its neighbour's plane, a cone's triangles all lie on a surface
    // of the file, just not its own.
    if let Some(f) = facts
        .file_faces
        .iter()
        .zip(&covered)
        .find(|(f, covered)| f.surface.is_some() && !**covered)
    {
        return Err(Problem::Uncovered { entity: f.0.entity });
    }
    Ok(FileCheck::Passed)
}

/// Where a kernel's face lies among the file's.
struct OnSurface {
    /// Every face of the file it fits: in its box, and within `near` of its surface.
    fits: Vec<usize>,
    /// How far it lies off every face it could be, if it fits none and none of them
    /// is on a surface this cannot measure.
    off: Option<f64>,
}

/// Which faces of the file a kernel's face, the triangles `corners` names, fits,
/// and how far off them it lies if it fits none.
///
/// It could be any face of the file whose box holds it. If one of those lies on a
/// surface measured exactly, every corner and triangle centre must lie within
/// `near` of it; a face that could instead lie on a surface this cannot measure,
/// such as a B-spline, is taken on its box alone.
fn on_surface(file: &[FileFace], vertices: &[DVec3], corners: &[usize], near: f64) -> OnSurface {
    let nowhere = |off| OnSurface {
        fits: Vec::new(),
        off,
    };
    let mut points = Vec::with_capacity(corners.len() / 3 * 4);
    for t in corners.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|i| vertices.get(i).copied().unwrap_or(DVec3::NAN));
        points.extend([a, b, c, (a + b + c) / 3.0]);
    }
    let Some(held) = part21::bounds(&points) else {
        return nowhere(None);
    };
    let holds = |f: &&FileFace| {
        f.bounds.is_none_or(|(lo, hi)| {
            worst_excess(held, (lo - DVec3::splat(near), hi + DVec3::splat(near))).is_none()
        })
    };
    let could_be: Vec<(usize, &FileFace)> =
        file.iter().enumerate().filter(|(_, f)| holds(f)).collect();
    if could_be.is_empty() {
        // Outside every face's box: how far outside the nearest.
        return nowhere(
            file.iter()
                .filter_map(|f| f.bounds)
                .filter_map(|(lo, hi)| worst_excess(held, (lo, hi)).map(|(_, by)| by))
                .min_by(f64::total_cmp),
        );
    }
    // It could lie on a surface this cannot measure: its box is all there is.
    let unmeasured = could_be.iter().any(|(_, f)| f.surface.is_none());
    // Rank the surfaces on a few triangles spread over the face, then hold every
    // point to them in that order: the first that fits passes the face. A sample
    // can flatter the wrong surface (the triangles it picks may all touch an edge
    // two faces share), so it only orders the work; a face is refused only when no
    // surface fits it all.
    let triangles = points.len() / 4;
    let step = (triangles / 64).max(1);
    let sample = |s: &crate::Surface| {
        points
            .chunks(4)
            .step_by(step)
            .flatten()
            .map(|p| s.distance(*p))
            .fold(0.0, f64::max)
    };
    let mut ranked: Vec<(f64, usize, crate::Surface)> = could_be
        .iter()
        .filter_map(|&(i, f)| f.surface.map(|s| (sample(&s), i, s)))
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut fits = Vec::new();
    let mut nearest = f64::INFINITY;
    for (_, i, s) in ranked {
        let worst = points.iter().map(|p| s.distance(*p)).fold(0.0, f64::max);
        if worst <= near {
            fits.push(i);
        }
        nearest = nearest.min(worst);
    }
    let off = (fits.is_empty() && !unmeasured).then_some(nearest);
    OnSurface { fits, off }
}

/// How far `inner` sticks out of `outer`, at worst, and where.
fn worst_excess(inner: (DVec3, DVec3), outer: (DVec3, DVec3)) -> Option<(&'static str, f64)> {
    let candidates = [
        ("-X", outer.0.x - inner.0.x),
        ("-Y", outer.0.y - inner.0.y),
        ("-Z", outer.0.z - inner.0.z),
        ("+X", inner.1.x - outer.1.x),
        ("+Y", inner.1.y - outer.1.y),
        ("+Z", inner.1.z - outer.1.z),
    ];
    candidates
        .into_iter()
        .filter(|(_, by)| *by > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use glam::dvec3;

    use super::*;

    /// One 10 mm square face, its four corners the file's vertices.
    const SQUARE: &str = "ISO-10303-21;
HEADER; FILE_SCHEMA (('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(10.,0.,0.));
#3=CARTESIAN_POINT('',(10.,10.,0.));
#4=CARTESIAN_POINT('',(0.,10.,0.));
#11=VERTEX_POINT('',#1);
#12=VERTEX_POINT('',#2);
#13=VERTEX_POINT('',#3);
#14=VERTEX_POINT('',#4);
#20=ADVANCED_FACE('',(#21),#22,.T.);
#900=( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#901)) REPRESENTATION_CONTEXT('','3D') );
#901=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );
ENDSEC;
END-ISO-10303-21;
";

    fn square(scale: f64) -> KernelMesh {
        KernelMesh {
            vertices: [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
                .map(|(x, y)| dvec3(x, y, 0.0) * scale)
                .to_vec(),
            indices: vec![0, 1, 2, 0, 2, 3],
            face_triangles: vec![2],
        }
    }

    #[test]
    fn the_right_mesh_passes() {
        let facts = part21::facts(SQUARE).unwrap();
        assert_eq!(check(&facts, &square(1.0), 0.01), Ok(FileCheck::Passed));
    }

    /// Both unit-error directions: 25.4 times too large leaves the file's
    /// geometry, and 25.4 times too small falls short of its vertices.
    #[test]
    fn a_mesh_at_the_wrong_scale_is_refused_either_way() {
        let facts = part21::facts(SQUARE).unwrap();
        assert!(matches!(
            check(&facts, &square(25.4), 0.01),
            Err(Problem::OutsideFile { .. })
        ));
        assert!(matches!(
            check(&facts, &square(1.0 / 25.4), 0.01),
            Err(Problem::ShortOfVertices { .. })
        ));
    }

    #[test]
    fn missing_and_empty_faces_are_refused() {
        let facts = part21::facts(SQUARE).unwrap();
        let none = KernelMesh::default();
        assert_eq!(
            check(&facts, &none, 0.01),
            Err(Problem::DroppedFaces {
                in_file: 1,
                meshed: 0
            })
        );
        let mut empty = square(1.0);
        empty.face_triangles = vec![2, 0];
        assert_eq!(
            check(&facts, &empty, 0.01),
            Err(Problem::EmptyFace { face: 1 })
        );
        let mut miscounted = square(1.0);
        miscounted.face_triangles = vec![1];
        assert!(matches!(
            check(&facts, &miscounted, 0.01),
            Err(Problem::Miscounted { .. })
        ));
    }

    /// step-conformance's generated cylinder: 20 mm across and 30 tall, its base on
    /// the origin, its side bounded only by its two circles.
    const CYLINDER: &str = "ISO-10303-21;
HEADER; FILE_SCHEMA (('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );
#2=( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) );
#3=( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() );
#4=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-7),#1,'distance_accuracy_value','confusion accuracy');
#5=( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#4)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2,#3)) REPRESENTATION_CONTEXT('','3D') );
#6=APPLICATION_CONTEXT('automotive design');
#7=APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#6);
#8=PRODUCT_CONTEXT('',#6,'mechanical');
#9=PRODUCT_DEFINITION_CONTEXT('part definition',#6,'design');
#10=PRODUCT('cylinder','cylinder','',(#8));
#11=PRODUCT_DEFINITION_FORMATION('','',#10);
#12=PRODUCT_DEFINITION('design','',#11,#9);
#13=PRODUCT_DEFINITION_SHAPE('','',#12);
#14=CARTESIAN_POINT('',(0.0,0.0,0.0));
#15=DIRECTION('',(0.0,0.0,1.0));
#16=DIRECTION('',(1.0,0.0,0.0));
#17=AXIS2_PLACEMENT_3D('',#14,#15,#16);
#18=CARTESIAN_POINT('',(0.0,0.0,0.0));
#19=DIRECTION('',(0.0,0.0,1.0));
#20=DIRECTION('',(1.0,0.0,0.0));
#21=AXIS2_PLACEMENT_3D('',#18,#19,#20);
#22=CIRCLE('',#21,10.0);
#23=CARTESIAN_POINT('',(10.0,0.0,0.0));
#24=VERTEX_POINT('',#23);
#25=EDGE_CURVE('',#24,#24,#22,.T.);
#26=CARTESIAN_POINT('',(0.0,0.0,30.0));
#27=DIRECTION('',(0.0,0.0,1.0));
#28=DIRECTION('',(1.0,0.0,0.0));
#29=AXIS2_PLACEMENT_3D('',#26,#27,#28);
#30=CIRCLE('',#29,10.0);
#31=CARTESIAN_POINT('',(10.0,0.0,30.0));
#32=VERTEX_POINT('',#31);
#33=EDGE_CURVE('',#32,#32,#30,.T.);
#34=CARTESIAN_POINT('',(0.0,0.0,0.0));
#35=DIRECTION('',(0.0,0.0,1.0));
#36=DIRECTION('',(1.0,0.0,0.0));
#37=AXIS2_PLACEMENT_3D('',#34,#35,#36);
#38=PLANE('',#37);
#39=ORIENTED_EDGE('',*,*,#25,.F.);
#40=EDGE_LOOP('',(#39));
#41=FACE_OUTER_BOUND('',#40,.T.);
#42=ADVANCED_FACE('',(#41),#38,.F.);
#43=CARTESIAN_POINT('',(0.0,0.0,0.0));
#44=DIRECTION('',(0.0,0.0,1.0));
#45=DIRECTION('',(1.0,0.0,0.0));
#46=AXIS2_PLACEMENT_3D('',#43,#44,#45);
#47=CYLINDRICAL_SURFACE('',#46,10.0);
#48=ORIENTED_EDGE('',*,*,#25,.T.);
#49=EDGE_LOOP('',(#48));
#50=ORIENTED_EDGE('',*,*,#33,.F.);
#51=EDGE_LOOP('',(#50));
#52=FACE_OUTER_BOUND('',#49,.T.);
#53=FACE_BOUND('',#51,.T.);
#54=ADVANCED_FACE('',(#52,#53),#47,.T.);
#55=CARTESIAN_POINT('',(0.0,0.0,30.0));
#56=DIRECTION('',(0.0,0.0,1.0));
#57=DIRECTION('',(1.0,0.0,0.0));
#58=AXIS2_PLACEMENT_3D('',#55,#56,#57);
#59=PLANE('',#58);
#60=ORIENTED_EDGE('',*,*,#33,.T.);
#61=EDGE_LOOP('',(#60));
#62=FACE_OUTER_BOUND('',#61,.T.);
#63=ADVANCED_FACE('',(#62),#59,.T.);
#64=CLOSED_SHELL('',(#42,#54,#63));
#65=MANIFOLD_SOLID_BREP('',#64);
#66=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#17,#65),#5);
#67=SHAPE_DEFINITION_REPRESENTATION(#13,#66);
ENDSEC;
END-ISO-10303-21;
";

    /// The cylinder as a kernel might mesh it: a fan on each end, a strip round
    /// the side, 64 steps round. `side` is the side's radius and `top` the height
    /// of the top face, so either can be put where the file has no surface.
    fn cylinder(side: f64, top: f64) -> KernelMesh {
        let ring = |r: f64, z: f64| {
            (0..64).map(move |k| {
                let t = std::f64::consts::TAU * f64::from(k) / 64.0;
                dvec3(r * t.cos(), r * t.sin(), z)
            })
        };
        let mut m = KernelMesh::default();
        let fan = |m: &mut KernelMesh, z: f64, up: bool| {
            let c = m.vertices.len();
            m.vertices.push(dvec3(0.0, 0.0, z));
            m.vertices.extend(ring(10.0, z));
            for k in 0..64 {
                let (a, b) = (c + 1 + k, c + 1 + (k + 1) % 64);
                m.indices.extend(if up { [c, a, b] } else { [c, b, a] });
            }
            m.face_triangles.push(64);
        };
        fan(&mut m, 0.0, false);
        let base = m.vertices.len();
        m.vertices.extend(ring(side, 0.0));
        m.vertices.extend(ring(side, 30.0));
        for k in 0..64 {
            let (a, b) = (base + k, base + (k + 1) % 64);
            m.indices.extend([a, b, b + 64, a, b + 64, a + 64]);
        }
        m.face_triangles.push(128);
        fan(&mut m, top, true);
        m
    }

    #[test]
    fn a_mesh_on_the_file_s_surfaces_passes() {
        let facts = part21::facts(CYLINDER).unwrap();
        assert_eq!(
            check(&facts, &cylinder(10.0, 30.0), 0.05),
            Ok(FileCheck::Passed)
        );
    }

    /// Surface wrong inside the part: inside every box, so only the surfaces see it.
    #[test]
    fn a_face_off_its_surface_inside_the_part_is_refused() {
        let facts = part21::facts(CYLINDER).unwrap();
        let sunk = check(&facts, &cylinder(9.0, 30.0), 0.05);
        assert!(
            matches!(sunk, Err(Problem::OffSurface { face: 1, by }) if (by - 1.0).abs() < 0.01),
            "{sunk:?}"
        );
        let lifted = check(&facts, &cylinder(10.0, 31.0), 0.05);
        assert!(
            matches!(lifted, Err(Problem::OffSurface { face: 2, .. })),
            "{lifted:?}"
        );
    }

    /// A face flattened into its neighbour's plane: every triangle lies on some
    /// surface of the file, and none on the cylinder's side.
    #[test]
    fn a_face_lost_into_another_s_surface_is_refused() {
        let facts = part21::facts(CYLINDER).unwrap();
        // Its top circle brought down into the base and in to half the radius, the
        // side becomes a ring in the base plane, as solidcraft's cone became a disc.
        // The side's 128 vertices follow the bottom fan's 65, its top circle last.
        let mut flat = cylinder(10.0, 30.0);
        for v in &mut flat.vertices[65 + 64..65 + 128] {
            *v = dvec3(v.x / 2.0, v.y / 2.0, 0.0);
        }
        let got = check(&facts, &flat, 0.05);
        assert!(matches!(got, Err(Problem::Uncovered { .. })), "{got:?}");
    }

    #[test]
    fn excess_names_the_worst_side() {
        let outer = (dvec3(0.0, 0.0, 0.0), dvec3(2.0, 2.0, 2.0));
        // Out by 1.0 on -X and by 1.5 on +Y: the worse one is reported.
        let inner = (dvec3(-1.0, 0.0, 0.0), dvec3(1.0, 3.5, 1.0));
        assert_eq!(worst_excess(inner, outer), Some(("+Y", 1.5)));
        let inside = (dvec3(0.5, 0.5, 0.5), dvec3(1.5, 1.5, 1.5));
        assert_eq!(worst_excess(inside, outer), None);
    }
}
