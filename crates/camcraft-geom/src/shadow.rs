//! The shadow a mesh casts straight down from above a height.
//!
//! `S(t)` is every XY position with some part of the surface strictly above `t`
//! over it: the union of the projections of every triangle, clipped to `Z > t`.
//! It is the exact, mesh-derived answer to "where would a shank standing on `t`
//! hit something?", which is what the cutter-location region at a level is built
//! from (see `camcraft_core::reach`).
//!
//! # Surfaces, not solids
//!
//! A cutter's shank is unbounded above, so *any* surface point above the level
//! blocks its column, whichever way the triangle faces and whether or not the mesh
//! is closed. So every triangle counts, down-facing ones included, and each is
//! rewound before the union so a triangle seen from below cannot cancel one seen
//! from above.
//!
//! # Triangles seen edge-on
//!
//! A vertical triangle projects to a segment, with no area, and a union would drop
//! it. In a closed mesh its neighbours' shadow runs right along it, so it adds
//! nothing; but a single vertical sheet in an open mesh is a real obstacle that
//! would vanish. So a triangle whose projection is narrower than
//! [`EDGE_ON_WIDTH`] is kept as a segment, and whatever of those segments lies off
//! the shadow's area is kept as a [`Shadow::sheets`] line, to be grown with round
//! ends. Giving such triangles a sliver of area instead would be simpler, and would
//! put a sawtooth a micron deep along every wall of every closed part.
//!
//! # Levels and flats
//!
//! A face lying exactly on the level does not block it: a cutter may sit on a floor.
//! Heights within [`LEVEL_EPSILON`] above the level count as on it, so a flat face
//! that the tessellator placed a rounding error high is still a floor.

use crate::contour::{Contour, Region, RegionError};
use crate::mesh::TriMesh;
use glam::{DVec2, DVec3, dvec2};

/// How far above a level a point may sit and still count as lying on it, in
/// millimetres.
pub const LEVEL_EPSILON: f64 = 1.0e-6;

/// A triangle whose projection is narrower than this is seen edge-on, and kept as
/// a segment, in millimetres. The segment may miss the projection by that much.
pub const EDGE_ON_WIDTH: f64 = 5.0e-4;

/// A segment within this of the shadow's area counts as lying on it, in
/// millimetres.
pub const ON_AREA: f64 = 1.0e-3;

/// What a mesh casts from above a height.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shadow {
    area: Region,
    sheets: Vec<Vec<DVec2>>,
}

impl Shadow {
    /// The area under the surface.
    pub fn area(&self) -> &Region {
        &self.area
    }

    /// Surface seen edge-on and standing clear of the area: lines, with no width.
    pub fn sheets(&self) -> &[Vec<DVec2>] {
        &self.sheets
    }

    /// Whether nothing at all is cast.
    pub fn is_empty(&self) -> bool {
        self.area.is_empty() && self.sheets.is_empty()
    }

    /// Everything within `radius`, erring outward by at most `arc_tolerance`.
    pub fn grown(&self, radius: f64, arc_tolerance: f64) -> Result<Region, RegionError> {
        let area = self.area.offset(radius, arc_tolerance);
        if self.sheets.is_empty() {
            return Ok(area);
        }
        area.union(&Region::around_polylines(
            &self.sheets,
            radius,
            arc_tolerance,
        ))
    }
}

/// A triangle's projection: an area, or a segment when seen edge-on.
enum Footprint {
    Area(Contour),
    Edge(DVec2, DVec2),
}

/// Shadows of one mesh at successively lower heights.
///
/// Asking in descending order is incremental: triangles wholly above the previous
/// height are unioned once and kept, and only those straddling the new height are
/// clipped afresh. Asking for a height above the last one starts again from the
/// top, which is correct but repeats the work.
#[derive(Debug)]
pub struct Shadows<'a> {
    mesh: &'a TriMesh,
    /// Triangle indices, highest lowest-vertex first.
    by_bottom: Vec<usize>,
    /// Triangle indices, highest highest-vertex first.
    by_top: Vec<usize>,
    /// Union of every triangle wholly above the last height asked.
    whole: Region,
    /// The edge-on ones among them.
    whole_edges: Vec<Vec<DVec2>>,
    /// How many of `by_bottom` are in `whole`.
    whole_count: usize,
    /// How many of `by_top` have reached above the last height asked.
    started_count: usize,
    /// The last height asked, or `None` before the first.
    last: Option<f64>,
}

impl<'a> Shadows<'a> {
    /// Prepare to cast shadows of `mesh`.
    pub fn new(mesh: &'a TriMesh) -> Self {
        let n = mesh.len();
        let bottom = |i: usize| z_range(mesh, i).0;
        let top = |i: usize| z_range(mesh, i).1;
        let mut by_bottom: Vec<usize> = (0..n).collect();
        by_bottom.sort_by(|&a, &b| bottom(b).total_cmp(&bottom(a)).then(a.cmp(&b)));
        let mut by_top: Vec<usize> = (0..n).collect();
        by_top.sort_by(|&a, &b| top(b).total_cmp(&top(a)).then(a.cmp(&b)));
        Self {
            mesh,
            by_bottom,
            by_top,
            whole: Region::default(),
            whole_edges: Vec::new(),
            whole_count: 0,
            started_count: 0,
            last: None,
        }
    }

    /// The shadow of everything strictly above `t`.
    pub fn above(&mut self, t: f64) -> Result<Shadow, RegionError> {
        if !t.is_finite() {
            return Ok(Shadow::default());
        }
        if self.last.is_some_and(|last| t > last) {
            self.whole = Region::default();
            self.whole_edges.clear();
            self.whole_count = 0;
            self.started_count = 0;
        }
        self.last = Some(t);
        let cut = t + LEVEL_EPSILON;

        // Triangles now wholly above join the kept union.
        let mut fresh = Vec::new();
        while let Some(&i) = self.by_bottom.get(self.whole_count) {
            if z_range(self.mesh, i).0 <= cut {
                break;
            }
            if let Some(tri) = self.mesh.triangle(i) {
                fresh.push(footprint(&tri));
            }
            self.whole_count += 1;
        }
        if !fresh.is_empty() {
            let (areas, edges) = split(fresh.into_iter().flatten());
            self.whole_edges.extend(edges);
            self.whole = self.whole.with_polygons(&areas)?;
        }

        // Triangles reaching above `t` but not wholly above it, clipped at `t`.
        while let Some(&i) = self.by_top.get(self.started_count) {
            if z_range(self.mesh, i).1 <= cut {
                break;
            }
            self.started_count += 1;
        }
        let (areas, mut edges) = split(
            self.by_top
                .iter()
                .take(self.started_count)
                .filter(|&&i| z_range(self.mesh, i).0 <= cut)
                .filter_map(|&i| self.mesh.triangle(i))
                .filter_map(|tri| footprint(&clip_above(&tri, cut))),
        );
        let area = if areas.is_empty() {
            self.whole.clone()
        } else {
            self.whole.with_polygons(&areas)?
        };
        edges.extend(self.whole_edges.iter().cloned());
        let sheets = if edges.is_empty() {
            Vec::new()
        } else {
            area.grow_covering(ON_AREA).polylines_outside(&edges)?
        };
        Ok(Shadow { area, sheets })
    }
}

/// The shadow of everything in `mesh` strictly above `t`, from scratch.
pub fn shadow_above(mesh: &TriMesh, t: f64) -> Result<Shadow, RegionError> {
    Shadows::new(mesh).above(t)
}

fn z_range(mesh: &TriMesh, i: usize) -> (f64, f64) {
    mesh.triangle(i)
        .map_or((f64::NEG_INFINITY, f64::NEG_INFINITY), |t| {
            (
                t[0].z.min(t[1].z).min(t[2].z),
                t[0].z.max(t[1].z).max(t[2].z),
            )
        })
}

/// The part of a triangle strictly above `cut`, as a polygon of up to four points.
///
/// Each crossing is computed from the edge's endpoints taken in a fixed order, so
/// two triangles sharing an edge produce bit-identical crossings and their
/// shadows meet without a hairline gap.
fn clip_above(tri: &[DVec3; 3], cut: f64) -> Vec<DVec3> {
    let mut out = Vec::with_capacity(4);
    for (&a, &b) in tri.iter().zip(tri.iter().cycle().skip(1)) {
        let (a_in, b_in) = (a.z > cut, b.z > cut);
        if a_in {
            out.push(a);
        }
        if a_in != b_in {
            let (p, q) = if (a.x, a.y, a.z) <= (b.x, b.y, b.z) {
                (a, b)
            } else {
                (b, a)
            };
            let s = (cut - p.z) / (q.z - p.z);
            out.push(p + (q - p) * s);
        }
    }
    out
}

fn split(footprints: impl Iterator<Item = Footprint>) -> (Vec<Contour>, Vec<Vec<DVec2>>) {
    let mut areas = Vec::new();
    let mut edges = Vec::new();
    for f in footprints {
        match f {
            Footprint::Area(c) => areas.push(c),
            Footprint::Edge(a, b) => edges.push(vec![a, b]),
        }
    }
    (areas, edges)
}

/// A convex polygon's projection onto XY.
///
/// `None` for fewer than two distinct points. Narrower than [`EDGE_ON_WIDTH`], it is
/// its diameter.
fn footprint(poly: &[DVec3]) -> Option<Footprint> {
    let pts: Vec<DVec2> = poly.iter().map(|p| dvec2(p.x, p.y)).collect();
    if pts.len() < 2 {
        return None;
    }
    // The polygon's diameter: its two farthest-apart points.
    let (length, a, b) = pts
        .iter()
        .enumerate()
        .flat_map(|(i, &a)| pts.iter().skip(i + 1).map(move |&b| (a.distance(b), a, b)))
        .fold((0.0_f64, DVec2::ZERO, DVec2::ZERO), |best, c| {
            if c.0 > best.0 { c } else { best }
        });
    if length <= 0.0 {
        return None;
    }
    let area = Contour::new(pts.clone()).map_or(0.0, |c| c.signed_area().abs());
    // The polygon is convex, so the triangle on its diameter and its farthest point
    // lies inside it: every point is within 2A/L of the diameter's line, and no
    // point projects past either end of a diameter.
    if 2.0 * area / length > EDGE_ON_WIDTH {
        return Contour::new(pts).map(Footprint::Area);
    }
    Some(Footprint::Edge(a, b))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::synthetic;
    use glam::dvec3;

    /// A closed box as a mesh.
    fn cuboid(min: DVec3, max: DVec3) -> TriMesh {
        let v = |x: bool, y: bool, z: bool| {
            dvec3(
                if x { max.x } else { min.x },
                if y { max.y } else { min.y },
                if z { max.z } else { min.z },
            )
        };
        let verts: Vec<DVec3> = (0..8)
            .map(|i| v(i & 1 != 0, i & 2 != 0, i & 4 != 0))
            .collect();
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        let tris = quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect();
        TriMesh::new(verts, tris).unwrap()
    }

    #[test]
    fn a_box_casts_its_own_footprint() {
        let m = cuboid(dvec3(-2.0, -1.0, 0.0), dvec3(3.0, 4.0, 5.0));
        let s = shadow_above(&m, 2.0).unwrap();
        // Exactly 5 x 5: the walls lie along its edge, and add nothing.
        assert!(
            (s.area().area() - 25.0).abs() < 1e-6,
            "got {}",
            s.area().area()
        );
        assert!(
            s.sheets().is_empty(),
            "walls of a closed box are not sheets"
        );
        assert!(
            shadow_above(&m, 5.0).unwrap().is_empty(),
            "nothing above the top"
        );
    }

    /// A floor is somewhere to stand. A face exactly on the level must not count.
    #[test]
    fn a_face_on_the_level_casts_nothing() {
        let floor = synthetic::plane(0.0, 10.0);
        assert!(shadow_above(&floor, 0.0).unwrap().is_empty());
        assert!(
            shadow_above(&floor, -2.0 * LEVEL_EPSILON)
                .unwrap()
                .area()
                .area()
                > 399.0
        );
    }

    /// Down-facing triangles wind the other way once projected. Unioned naively
    /// they cancel the top, and a solid block casts no shadow at all.
    #[test]
    fn faces_seen_from_below_do_not_cancel() {
        let m = cuboid(dvec3(0.0, 0.0, 0.0), dvec3(4.0, 4.0, 4.0));
        let below = shadow_above(&m, -1.0).unwrap();
        assert!(
            (below.area().area() - 16.0).abs() < 1e-6,
            "got {}",
            below.area().area()
        );
    }

    /// A lone vertical sheet has no area from above, and is still an obstacle.
    #[test]
    fn a_vertical_sheet_still_casts_a_shadow() {
        let fin = TriMesh::new(
            vec![
                dvec3(0.0, 0.0, 0.0),
                dvec3(10.0, 0.0, 0.0),
                dvec3(10.0, 0.0, 5.0),
                dvec3(0.0, 0.0, 5.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        )
        .unwrap();
        let s = shadow_above(&fin, 1.0).unwrap();
        assert!(!s.is_empty(), "the sheet vanished");
        assert!(s.area().is_empty(), "a sheet has no area");
        let length: f64 = s
            .sheets()
            .iter()
            .map(|l| l.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>())
            .sum();
        assert!(length >= 10.0 - 1e-6, "sheet length {length}");
        // Grown by 1 with round ends: a 10 x 2 stadium.
        let grown = s.grown(1.0, 0.001).unwrap().area();
        let stadium = 20.0 + std::f64::consts::PI;
        assert!(grown >= stadium && grown < stadium + 0.05, "{grown}");
    }

    /// The shadow of a sphere above `t` is a disc of radius sqrt(r^2 - t^2),
    /// within the tessellation's own error.
    #[test]
    fn a_sphere_casts_the_disc_its_slice_says() {
        let r = 10.0;
        let m = synthetic::uv_sphere(r, 120, 180);
        let sag = synthetic::uv_sphere_max_sag(r, 120, 180);
        for t in [-5.0, 0.0, 3.0, 7.5] {
            let s = shadow_above(&m, t).unwrap().area().clone();
            // Below the equator the shadow is the whole equator disc.
            let rho = if t < 0.0 { r } else { (r * r - t * t).sqrt() };
            let ideal = std::f64::consts::PI * rho * rho;
            let slack = 2.0 * std::f64::consts::PI * rho * (sag * 4.0 + 0.01);
            assert!(
                (s.area() - ideal).abs() < slack,
                "t {t}: area {} vs {ideal}",
                s.area()
            );
        }
    }

    /// Descending queries reuse work. They must also give the same answer as
    /// starting afresh, including after a query that goes back up.
    #[test]
    fn incremental_matches_from_scratch() {
        let m = synthetic::uv_sphere(8.0, 40, 60);
        let mut shadows = Shadows::new(&m);
        for t in [7.0, 5.0, 2.5, 2.5, 0.0, -3.0, 4.0, 1.0] {
            let a = shadows.above(t).unwrap().area().clone();
            let b = shadow_above(&m, t).unwrap().area().clone();
            assert!(
                (a.area() - b.area()).abs() < 1e-6,
                "t {t}: {} vs {}",
                a.area(),
                b.area()
            );
        }
    }
}
