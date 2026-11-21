//! Scallop finishing round a bowl: one family of rings from the edge of the
//! floor it avoids, up the inside, over the rim and down the outside, then
//! level rings down the skirt below where the tool last rests on the part
//! from above, to the Bottom (decision 0035).

use std::collections::HashMap;

use super::*;
use crate::path::toolpath::MoveRole;
use camcraft_geom::mesh::{TriMesh, closest_point_on_segment};
use camcraft_geom::topology::Body;
use glam::{DVec3, dvec2, dvec3};

/// Points round each ring of the bowl.
const AROUND: u32 = 180;

/// The bowl's radii at `theta` from +X: its floor's edge, the top of its
/// inside wall, the outside of its rim and its skirt. Asymmetric, and its
/// inside wall 0.6 wide at its steepest, 87°, and 6 at its gentlest, 59°: it
/// is steeper than a plunge, 84°, round part of it.
fn radii(theta: f64) -> [f64; 4] {
    let floor = 12.0 + 3.0 * theta.cos() + 1.5 * (2.0 * theta).sin();
    let wall = floor + 0.6 + 2.7 * (1.0 + theta.sin());
    let rim = wall + 3.0;
    [floor, wall, rim, rim + 4.0]
}

/// The bowl's heights: its floor, rim, the top of its skirt and its base.
const FLOOR: f64 = 2.0;
const RIM: f64 = 12.0;
const SKIRT: f64 = 4.0;

/// A bowl standing on Z0 round the origin, with its faces named: the floor
/// 0, the inside wall 1, the rim 2, the outside wall 3, down a slope and then
/// an upright skirt, and its base 4.
fn bowl() -> Body {
    let ring = |i: usize| move |theta: f64| radii(theta)[i];
    let (a, b, c, d) = (ring(0), ring(1), ring(2), ring(3));
    ringed(
        &[(&a, FLOOR), (&b, RIM), (&c, RIM), (&d, SKIRT), (&d, 0.0)],
        &[1, 2, 3, 3],
        4,
    )
}

/// A body of rings round the origin, ring `i` [`AROUND`] points `rings[i].0`
/// of `theta` from it at height `rings[i].1`, out from the first: closed over
/// from the middle of the first on face 0, each strip from a ring to the
/// next on face `faces[i]`, and closed under from below the last on face
/// `base`. The triangles wound so their normals face out of the material.
fn ringed(rings: &[(&dyn Fn(f64) -> f64, f64)], faces: &[u32], base: u32) -> Body {
    let n = AROUND;
    let mut v = vec![dvec3(0.0, 0.0, rings[0].1)];
    for (radius, z) in rings {
        for k in 0..n {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(n);
            let r = radius(theta);
            v.push(dvec3(r * theta.cos(), r * theta.sin(), *z));
        }
    }
    let under = u32::try_from(v.len()).unwrap();
    v.push(dvec3(0.0, 0.0, rings[rings.len() - 1].1));
    let at = |ring: u32, k: u32| 1 + ring * n + k % n;
    let last = u32::try_from(rings.len() - 1).unwrap();
    let (mut t, mut labels) = (Vec::new(), Vec::new());
    for k in 0..n {
        t.push([0, at(0, k), at(0, k + 1)]);
        labels.push(0);
        for (ring, &face) in faces.iter().enumerate() {
            let ring = u32::try_from(ring).unwrap();
            t.push([at(ring, k), at(ring + 1, k), at(ring + 1, k + 1)]);
            t.push([at(ring, k), at(ring + 1, k + 1), at(ring, k + 1)]);
            labels.extend([face, face]);
        }
        t.push([under, at(last, k + 1), at(last, k)]);
        labels.push(base);
    }
    let count = base.max(faces.iter().copied().max().unwrap_or(0)) + 1;
    Body::with_faces(TriMesh::new(v, t).unwrap(), labels, count).unwrap()
}

/// Its 4 mm ball, a stepover of 0.8 inside out from the floor's edge, the
/// floor avoided, down to a millimetre above the base.
const JOB: &str = r#"
version = 1
tolerance = 0.05

[tools.ball]
kind = "ball"
diameter = 4.0
number = 1

[[model]]
name = "bowl"
path = "bowl.stl"
units = "mm"

[[stock]]
name = "block"
kind = "around-models"
margin = [6.0, 6.0, 0.0]

[[setup]]
name = "top"
models = ["bowl"]
stocks = ["block"]

[[setup.operation]]
name = "finish"
strategy = "scallop"
order = "inside-out"
tool = "ball"
passes = { stepover = 0.8, feed = 2000.0 }
heights = { clearance = { ref = "stock-top", offset = 6.0 }, bottom = { ref = "model-bottom", offset = 1.0 } }

[[setup.operation.surfaces.group]]
action = "avoid"

[[setup.operation.surfaces.group.faces]]
model = "bowl"
index = 0
at = [1.0, 0.0, 2.0]
"#;

const STEPOVER: f64 = 0.8;
const BOTTOM: f64 = 1.0;

/// The cuts of `path`, each a straight piece, filed by where they are, and
/// the pass each is of: a pass runs from one move that is not a cut to the
/// next.
struct Cuts {
    cells: HashMap<(i64, i64, i64), Vec<usize>>,
    pieces: Vec<(DVec3, DVec3)>,
    pass: Vec<usize>,
}

impl Cuts {
    const CELL: f64 = 1.0;

    fn of(path: &crate::path::toolpath::Toolpath) -> Self {
        let (mut pieces, mut pass) = (Vec::new(), Vec::new());
        let mut at = path.moves()[0].to;
        let (mut passes, mut cutting) = (0, false);
        for m in path.moves() {
            if m.role == MoveRole::Cut {
                pieces.push((at, m.to));
                pass.push(passes);
            } else if cutting {
                passes += 1;
            }
            cutting = m.role == MoveRole::Cut;
            at = m.to;
        }
        let key = |p: DVec3| {
            let c = (p / Self::CELL).floor();
            (c.x as i64, c.y as i64, c.z as i64)
        };
        let mut cells: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
        for (i, &(a, b)) in pieces.iter().enumerate() {
            let (lo, hi) = (key(a.min(b)), key(a.max(b)));
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    for z in lo.2..=hi.2 {
                        cells.entry((x, y, z)).or_default().push(i);
                    }
                }
            }
        }
        Self {
            cells,
            pieces,
            pass,
        }
    }

    /// How far `p` is from the nearest cut among `which`, looked for within
    /// two cells, or infinitely far.
    fn nearest(&self, p: DVec3, which: impl Fn(usize) -> bool) -> f64 {
        let c = (p / Self::CELL).floor();
        let (cx, cy, cz) = (c.x as i64, c.y as i64, c.z as i64);
        let mut best = f64::INFINITY;
        for x in cx - 2..=cx + 2 {
            for y in cy - 2..=cy + 2 {
                for z in cz - 2..=cz + 2 {
                    for &i in self.cells.get(&(x, y, z)).into_iter().flatten() {
                        if !which(i) {
                            continue;
                        }
                        let (a, b) = self.pieces[i];
                        best = best.min(p.distance(closest_point_on_segment(p, a, b)));
                    }
                }
            }
        }
        best
    }
}

/// Round the bowl, from the floor's edge out to the Bottom: one family of
/// rings a stepover apart along the surface, steep ground and all; nothing of
/// what it machines further than half a stepover from a cut, the skirt below
/// where the tool rests from above included, so no gap between the last ring
/// laid over the part and the first down the skirt is wider than a stepover;
/// each ring of the field started on one straight seam; few lifts; no gouge
/// and no rapid into the material; and the same on one thread or eight.
#[test]
fn a_bowl_is_finished_in_one_family_of_rings() {
    let body = bowl();
    let bodies: Bodies<'_> = [("bowl", &body)].into_iter().collect();
    let job: Job = toml::from_str(JOB).unwrap();
    let on = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| run_setup(&job, 0, &bodies).unwrap())
    };
    let result = on(8);
    assert_eq!(on(1).operations[0].path, result.operations[0].path);
    let op = &result.operations[0];
    let path = &op.path;
    assert_eq!(result.verify.gouged, 0);
    assert_eq!(op.sim.rapid_contacts, 0);
    let StrategyDetail::Scallop(report) = &op.detail else {
        panic!("{:?}", op.detail);
    };
    assert!(report.walls > 0, "no rings down the skirt: {report:?}");

    // What the part looks like to the tool, to look for what is left.
    let prepared = crate::setup::prepare(&job, 0, &bodies).unwrap();
    let surfaces = part_surfaces(&job, &job.setups[0].operations[0], &prepared, &bodies).unwrap();
    let tool = cutting_tool(&job, &job.setups[0].operations[0]).unwrap();
    let seat = surfaces.under(&tool).unwrap();
    let cuts = Cuts::of(path);

    // Over the ground it machines, from above, the floor's edge drawn where
    // the tool comes within half the tolerance of the wall, as the field's
    // is; and beside the skirt, below where the tool would rest from above,
    // down to the Bottom.
    let tol = job.tolerance;
    let mut furthest = (0.0_f64, DVec3::ZERO);
    let mut look = |p: DVec3| {
        let d = cuts.nearest(p, |_| true);
        if d > furthest.0 {
            furthest = (d, p);
        }
    };
    let mut x = -32.0;
    while x < 32.0 {
        let mut y = -32.0;
        while y < 32.0 {
            if let Some(rest) = seat.rest_at(x, y, 0.5 * tol)
                && rest.machined
                && rest.z > BOTTOM + tol
            {
                look(dvec3(x, y, rest.z));
            }
            y += 0.5;
        }
        x += 0.5;
    }
    for k in 0..120 {
        let theta = std::f64::consts::TAU * f64::from(k) / 120.0;
        let out = dvec2(theta.cos(), theta.sin()) * (radii(theta)[3] + 2.0);
        let mut z = BOTTOM;
        while z < SKIRT - 2.0 {
            look(out.extend(z));
            z += 0.25;
        }
    }

    // Up the steep inside wall, each ring a stepover along the surface from
    // the next one out, the next pass.
    let inside = |p: DVec3| {
        let theta = p.y.atan2(p.x);
        p.z > FLOOR + 1.0 && p.z < RIM - 1.0 && p.truncate().length() < radii(theta)[1]
    };
    let mut gaps = Vec::new();
    for (i, &(_, p)) in cuts.pieces.iter().enumerate().step_by(7) {
        if inside(p) {
            let next = cuts.pass[i] + 1;
            gaps.push((cuts.nearest(p, |j| cuts.pass[j] == next), p));
        }
    }
    gaps.retain(|g| g.0.is_finite());
    let least = gaps.iter().copied().fold(
        (f64::MAX, DVec3::ZERO),
        |a, g| if g.0 < a.0 { g } else { a },
    );
    let most = gaps
        .iter()
        .copied()
        .fold((0.0, DVec3::ZERO), |a, g| if g.0 > a.0 { g } else { a });

    // Each ring over the part starts on one line seen from above: where the
    // way from the last ring onto it ends.
    let starts: Vec<DVec3> = path
        .moves()
        .windows(2)
        .filter(|w| {
            w[0].role == MoveRole::Transition && w[1].role == MoveRole::Cut && w[0].to.z > SKIRT
        })
        .map(|w| w[0].to)
        .collect();
    let (first, last) = (starts[0].truncate(), starts[starts.len() - 1].truncate());
    let along = (last - first).normalize();
    let off = starts
        .iter()
        .map(|p| (p.truncate() - first).perp_dot(along).abs())
        .fold(0.0, f64::max);

    // A lift is a rapid straight up.
    let mut lifts = 0;
    let mut at = path.moves()[0].to;
    for m in path.moves() {
        if m.role == MoveRole::Rapid && m.to.z > at.z + 1e-9 && m.to.truncate() == at.truncate() {
            lifts += 1;
        }
        at = m.to;
    }
    // Half a stepover, or, along the outer boundary, where the field's lines
    // are kept half a stepover in from it (`scallop`), half of a stepover and
    // a half: 0.60 of the 0.8, measured, beside the skirt's top.
    assert!(
        furthest.0 < 0.75 * STEPOVER + 2.0 * tol,
        "{furthest:?} from the nearest cut"
    );
    assert!(gaps.len() > 100, "{} looked at", gaps.len());
    assert!(
        least.0 > 0.95 * STEPOVER && most.0 < 1.05 * STEPOVER,
        "rings {least:?} to {most:?} apart up the inside"
    );
    assert!(starts.len() > 20, "{} ring starts", starts.len());
    assert!(off < 0.01, "a ring starts {off} off the seam: {starts:?}");
    assert!(lifts <= 8, "{lifts} lifts");
}

/// A ring's radius at an angle from +X.
type Radius = Box<dyn Fn(f64) -> f64>;

/// How far a bulging bowl's outside swells out at half its height.
const BULGE: f64 = 3.0;

/// A bowl as the first, inside and rim, whose outside swells out from its rim
/// by [`BULGE`] at half its height and tucks back in to its base: below its
/// widest the wall is an undercut a ball from above cannot reach.
fn bulging() -> Body {
    const STEPS: u32 = 24;
    let ring = |i: usize| move |theta: f64| radii(theta)[i];
    let (floor, wall, rim) = (ring(0), ring(1), ring(2));
    let outside: Vec<(Radius, f64)> = (1..=STEPS)
        .map(|i| {
            let z = RIM * (1.0 - f64::from(i) / f64::from(STEPS));
            let swell = BULGE * (std::f64::consts::PI * (RIM - z) / RIM).sin();
            let radius: Radius = Box::new(move |t| radii(t)[2] + swell);
            (radius, z)
        })
        .collect();
    let mut rings: Vec<(&dyn Fn(f64) -> f64, f64)> =
        vec![(&floor, FLOOR), (&wall, RIM), (&rim, RIM)];
    rings.extend(outside.iter().map(|(r, z)| (r.as_ref(), *z)));
    let mut faces = vec![1, 2];
    faces.extend((1..=STEPS).map(|_| 3));
    ringed(&rings, &faces, 4)
}

/// The cuts of `path` crossing the upright half-plane out from the bowl's
/// middle at `theta`, beyond `from` from it: where the tool's tip is, seen
/// in that plane as how far out and how high, highest first.
fn section(path: &crate::path::toolpath::Toolpath, theta: f64, from: f64) -> Vec<DVec3> {
    let dir = dvec2(theta.cos(), theta.sin());
    let side = |p: DVec3| dir.perp_dot(p.truncate());
    let mut out = Vec::new();
    let mut at = path.moves()[0].to;
    for m in path.moves() {
        if m.role == MoveRole::Cut {
            let (a, b) = (side(at), side(m.to));
            if a * b < 0.0 {
                let p = at.lerp(m.to, a / (a - b));
                if p.truncate().dot(dir) > from {
                    out.push(p);
                }
            }
        }
        at = m.to;
    }
    out.sort_by(|a, b| b.z.total_cmp(&a.z));
    out
}

/// Down the outside of a bowl that bulges and tucks in under its widest:
/// the rings over the part are no more than a stepover apart along the
/// surface the tool rides, right down to where the tool leaves it, where the
/// field's grid ended a cell short and the area's edge a tolerance short of
/// it, leaving a band unfinished; their last runs along
/// that line, at the height where the tool's side meets the bulge's widest
/// all round, smoothly; and the level rings below start within a stepover of
/// it and go on a stepover apart to the Bottom.
#[test]
fn down_a_bulge_the_rings_reach_where_the_tool_leaves_it() {
    // Inside out from the floor's edge, the line the tool leaves the part
    // along is the field's far end; outside in, an edge it is measured from.
    down_a_bulge("inside-out");
    down_a_bulge("outside-in");
}

/// [`down_a_bulge_the_rings_reach_where_the_tool_leaves_it`], cut `order`.
fn down_a_bulge(order: &str) {
    let body = bulging();
    let bodies: Bodies<'_> = [("bowl", &body)].into_iter().collect();
    let job: Job = toml::from_str(&JOB.replace("inside-out", order)).unwrap();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let op = &result.operations[0];
    assert_eq!(result.verify.gouged, 0);
    assert_eq!(op.sim.rapid_contacts, 0);
    let StrategyDetail::Scallop(report) = &op.detail else {
        panic!("{:?}", op.detail);
    };
    let radius = 2.0;
    // Where the tool's side meets the bulge's widest: its tip a radius below
    // half the bowl's height.
    let widest = 0.5 * RIM - radius;
    let leaves = report.leaves_at.unwrap();
    assert!(
        (leaves - widest).abs() < 0.05,
        "leaves the part at {leaves}"
    );
    let tol = job.tolerance;
    let mut worst_field = 0.0f64;
    let mut worst_join = 0.0f64;
    let mut last_rings = Vec::new();
    for k in 0..24 {
        let theta = std::f64::consts::TAU * f64::from(k) / 24.0;
        let out = radii(theta)[2] + BULGE + radius;
        let cut = section(&op.path, theta, radii(theta)[2] + 0.5);
        // The rings over the part, down to where the tool leaves it, and the
        // level rings below.
        let (over, below): (Vec<DVec3>, Vec<DVec3>) = cut.iter().partition(|p| p.z > widest - 0.1);
        // How far apart the rings are, where the tool's tip is.
        for w in over.windows(2) {
            worst_field = worst_field.max(w[0].distance(w[1]));
        }
        let last = *over.last().unwrap();
        last_rings.push((last.z, last.truncate().length() - out));
        let first_below = below.first().unwrap();
        worst_join = worst_join.max(last.z - first_below.z);
        for w in below.windows(2) {
            assert!(
                w[0].z - w[1].z < STEPOVER + tol,
                "{theta}: level rings {} apart",
                w[0].z - w[1].z
            );
        }
        assert!((below.last().unwrap().z - BOTTOM).abs() < tol);
    }
    // Over the part, a stepover apart along the surface the tool rides.
    assert!(worst_field < 1.05 * STEPOVER, "rings {worst_field} apart");
    // The last ring over the part along where the tool leaves it, all round:
    // as high to within a few hundredths, the tool's side on the widest.
    let (lo, hi) = last_rings
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), &(z, _)| {
            (lo.min(z), hi.max(z))
        });
    assert!(
        hi - lo < 0.05,
        "the last ring over the part from {lo} to {hi} high"
    );
    // Seen from above on the bulge's widest offset by the tool, to what its
    // chords keep to: inside it where the outline bulges out, outside where
    // it curves in, 0.052 measured.
    for &(_, off) in &last_rings {
        assert!(
            off.abs() < 2.0 * tol,
            "the last ring over the part {off} off the widest"
        );
    }
    // And the level rings start within a stepover of it.
    assert!(
        worst_join < STEPOVER + tol,
        "{worst_join} from the last ring to the first level one"
    );
}

/// A cup, its floor avoided, its wall straight up from it at `degrees` to a
/// sharp rim: upright at 90°.
fn cup(degrees: f64) -> Body {
    let lean = (RIM - FLOOR) / degrees.to_radians().tan();
    let (floor, top, rim) = (
        |_: f64| CUP,
        move |_: f64| CUP + lean,
        move |_: f64| CUP + lean + 3.0,
    );
    ringed(
        &[(&floor, FLOOR), (&top, RIM), (&rim, RIM), (&rim, 0.0)],
        &[1, 2, 3],
        4,
    )
}

/// The cup's floor's radius.
const CUP: f64 = 10.0;

/// Up a pocket's wall from a floor avoided to a sharp rim, upright or a few
/// degrees off it, the rings run a stepover apart on the wall from its foot
/// to its top, each whole and level. Beside an upright wall the tool's path
/// stands upright, its foot where it rests on the floor and its top where it
/// starts rolling over the rim: dropped there it rests at one or the other,
/// and the wall is seen only from beside it.
#[test]
fn up_an_upright_wall_the_rings_reach_its_top() {
    let r = 2.0;
    for degrees in [90.0, 89.0, 87.0, 85.0] {
        let body = cup(degrees);
        let bodies: Bodies<'_> = [("bowl", &body)].into_iter().collect();
        let job: Job = toml::from_str(JOB).unwrap();
        let result = run_setup(&job, 0, &bodies).unwrap();
        let op = &result.operations[0];
        assert_eq!(result.verify.gouged, 0, "{degrees}°");
        let mut passes: Vec<Vec<DVec3>> = Vec::new();
        let mut cutting = false;
        for m in op.path.moves() {
            if m.role == MoveRole::Cut {
                if !cutting {
                    passes.push(Vec::new());
                }
                if let Some(p) = passes.last_mut() {
                    p.push(m.to);
                }
            }
            cutting = m.role == MoveRole::Cut;
        }
        // The passes up the wall: above the floor, below the rim, inside the
        // cup.
        let lean = (RIM - FLOOR) / degrees.to_radians().tan();
        let on_wall = |q: &DVec3| {
            q.z > FLOOR + 0.01 && q.z < RIM - 0.01 && q.truncate().length() < CUP + lean
        };
        let rings: Vec<&Vec<DVec3>> = passes.iter().filter(|p| p.iter().all(on_wall)).collect();
        // A ball touches a wall at `degrees` its radius times one less its
        // cosine above its tip; above where that is the rim, it rolls over
        // the rim's edge.
        let above = r * (1.0 - degrees.to_radians().cos());
        let rolls = RIM - above;
        let mut heights = Vec::new();
        for p in &rings {
            let (lo, hi) = p
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| {
                    (lo.min(q.z), hi.max(q.z))
                });
            if lo >= rolls - 0.01 {
                continue;
            }
            // Level to the field's placing up a cliff, from its edges'
            // profiles, some across the grid and some aslant it, as down a
            // wall: a tenth or two at its foot.
            assert!(
                hi - lo < 0.25 * STEPOVER,
                "{degrees}°: a ring up the wall from {lo} to {hi}"
            );
            let around: f64 = p.windows(2).map(|w| w[0].distance(w[1])).sum();
            assert!(
                around > 0.95 * std::f64::consts::TAU * (CUP - r),
                "{degrees}°: a ring up the wall {around} round"
            );
            heights.push((0.5 * (lo + hi) + above).min(RIM));
        }
        heights.sort_by(f64::total_cmp);
        // How far up the wall each touches, from the floor's touch to the rim.
        let up = |z: f64| (z - FLOOR) / degrees.to_radians().sin();
        let mut along: Vec<f64> = heights.iter().map(|&z| up(z)).collect();
        along.insert(0, up(FLOOR + above));
        along.push(up(RIM));
        for w in along.windows(2) {
            assert!(
                w[1] - w[0] <= STEPOVER + 2.0 * job.tolerance,
                "{degrees}°: rings {} apart up the wall: {along:.2?}",
                w[1] - w[0]
            );
        }
    }
}
