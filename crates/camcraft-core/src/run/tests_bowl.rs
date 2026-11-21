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
    // Where the tool rolls over the rim's sharp edges its contact stays on
    // the edge, which the rings either side cut (decision 0039).
    let mut scratch = Vec::new();
    let mut x = -32.0;
    while x < 32.0 {
        let mut y = -32.0;
        while y < 32.0 {
            if let Some(rest) = seat.rest_at(x, y, 0.5 * tol)
                && rest.machined
                && rest.z > BOTTOM + tol
                && !seat
                    .rest_in(x, y, &mut scratch)
                    .is_some_and(|(_, rolling)| rolling)
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

/// The radii of a bowl as [`radii`]'s, its inside wall wider: 3.5 mm at its
/// steepest, 71°, and 8.9 at its gentlest, 48°, seen from above. A wall
/// nearly upright is too narrow, seen from above, for the strip at its top
/// to break the ground (`ground::BESIDE`).
fn wide(theta: f64) -> [f64; 4] {
    let floor = 12.0 + 3.0 * theta.cos() + 1.5 * (2.0 * theta).sin();
    let wall = floor + 3.5 + 2.7 * (1.0 + theta.sin());
    let rim = wall + 3.0;
    [floor, wall, rim, rim + 4.0]
}

/// The bowl with the wider wall, named as [`bowl`]'s.
fn wide_bowl() -> Body {
    let ring = |i: usize| move |theta: f64| wide(theta)[i];
    let (a, b, c, d) = (ring(0), ring(1), ring(2), ring(3));
    ringed(
        &[(&a, FLOOR), (&b, RIM), (&c, RIM), (&d, SKIRT), (&d, 0.0)],
        &[1, 2, 3, 3],
        4,
    )
}

/// The wider bowl's profile at `theta` from +X, out from its middle, as how
/// far out and how high: its floor's edge, the inside wall's top, the rim's
/// outside, the skirt's top and its foot.
fn profile(theta: f64) -> [DVec2; 5] {
    let [floor, wall, rim, skirt] = wide(theta);
    [
        dvec2(floor, FLOOR),
        dvec2(wall, RIM),
        dvec2(rim, RIM),
        dvec2(skirt, SKIRT),
        dvec2(skirt, 0.0),
    ]
}

/// Inside out round the wider bowl, whose rim is flat between two sharp edges
/// and whose inside wall is longer on some sides than others, so the rings up
/// it meet the rim at a slant: they stop where the tool starts rolling over
/// the rim's inner edge and start again where it has rolled over, and the
/// same at its outer edge (decision 0039). So no pass crosses either edge
/// rolling over it, where with the strips no break 1 to 3 crossed the inner
/// edge at each of 36 sections and 2 or 3 the outer; and either side of each,
/// where the tool touches the part, the rings are no further apart than a
/// stepover.
#[test]
fn inside_out_no_ring_rolls_over_the_rim() {
    no_ring_rolls_over_the_rim("inside-out");
}

/// Outside in round the wider bowl, the mirror: the zones are cut from the
/// outermost in, the inside's measured from the rim's inner edge and blended
/// into the floor's edge (decision 0039). No pass crosses either edge rolling
/// over it; and either side of each, the rings are no further apart than a
/// stepover.
#[test]
fn outside_in_no_ring_rolls_over_the_rim() {
    no_ring_rolls_over_the_rim("outside-in");
}

/// [`inside_out_no_ring_rolls_over_the_rim`], cut `order`.
fn no_ring_rolls_over_the_rim(order: &str) {
    let body = wide_bowl();
    let bodies: Bodies<'_> = [("bowl", &body)].into_iter().collect();
    let job: Job = toml::from_str(&JOB.replace("inside-out", order)).unwrap();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let op = &result.operations[0];
    assert_eq!(result.verify.gouged, 0);
    let radius = 2.0;
    // A tool rolling over an edge strictly: three degrees past where it
    // touches either face.
    let margin = 3f64.to_radians().cos();
    let (mut rolling, mut widest) = (Vec::new(), (0.0f64, 0.0f64));
    for k in 0..36 {
        let theta = std::f64::consts::TAU * f64::from(k) / 36.0;
        let dir = dvec2(theta.cos(), theta.sin());
        let at = profile(theta);
        let lengths: Vec<f64> = at.windows(2).map(|w| w[0].distance(w[1])).collect();
        // Where each crossing's tool touches the part, as how far along the
        // profile; and over which edge, the wall's top 1 or the rim's
        // outside 2, it rolls strictly.
        let mut touches = Vec::new();
        for q in section(&op.path, theta, wide(theta)[0] + 0.5) {
            let centre = dvec2(q.truncate().dot(dir), q.z + radius);
            let (mut best, mut along, mut vertex) = (f64::INFINITY, 0.0, None);
            let mut start = 0.0;
            for (i, w) in at.windows(2).enumerate() {
                let (a, b) = (w[0], w[1]);
                let t = ((centre - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
                let d = centre.distance(a.lerp(b, t));
                if d < best {
                    best = d;
                    along = start + t * lengths[i];
                    vertex = if t <= 0.0 {
                        Some(i)
                    } else if t >= 1.0 {
                        Some(i + 1)
                    } else {
                        None
                    };
                }
                start += lengths[i];
            }
            // The outward normals of the faces either side of a vertex: the
            // profile turned left.
            let normal = |i: usize| (at[i + 1] - at[i]).normalize().perp();
            if let Some(v) = vertex.filter(|&v| v == 1 || v == 2) {
                let u = (centre - at[v]).normalize();
                if u.dot(normal(v - 1)) < margin && u.dot(normal(v)) < margin {
                    rolling.push((theta, v, q));
                }
            }
            touches.push(along);
        }
        touches.sort_by(f64::total_cmp);
        // Either side of the two edges: from 3 below the wall's top to 3
        // past the rim's outside.
        let (from, to) = (lengths[0] - 3.0, lengths[0] + lengths[1] + 3.0);
        for w in touches.windows(2) {
            if w[0] > from && w[1] < to && w[1] - w[0] > widest.0 {
                widest = (w[1] - w[0], theta);
            }
        }
    }
    assert!(
        rolling.is_empty(),
        "{order}: rolling over the rim: {rolling:?}"
    );
    assert!(
        widest.0 < 1.1 * STEPOVER,
        "{order}: rings {widest:?} apart where the tool touches the rim"
    );
}

/// [`JOB`] with nothing avoided: the floor machined as the rest.
fn no_group() -> Job {
    let text = JOB
        .split("[[setup.operation.surfaces.group]]")
        .next()
        .unwrap();
    toml::from_str(text).unwrap()
}

/// How high the egg bowl's floor is: 4 under its rim.
const EGG_FLOOR: f64 = RIM - 4.0;

/// A shallow bowl, its floor an egg `long` from end to end and half as wide,
/// its blunt end at +X, rising from it with no crease, as a parabola 3 out
/// and 4 up, to a sharp flat rim, then down to its skirt as [`bowl`]'s.
fn egg_bowl(long: f64) -> Body {
    const STEPS: u32 = 8;
    let floor = move |t: f64| {
        let (a, b) = (0.5 * long, 0.25 * long);
        let r = 1.0 / ((t.cos() / a).powi(2) + (t.sin() / b).powi(2)).sqrt();
        r * (1.0 + 0.1 * t.cos())
    };
    let wall: Vec<(Radius, f64)> = (0..=STEPS)
        .map(|i| {
            let s = 3.0 * f64::from(i) / f64::from(STEPS);
            let radius: Radius = Box::new(move |t| floor(t) + s);
            (radius, EGG_FLOOR + 4.0 * (s / 3.0).powi(2))
        })
        .collect();
    let rim = move |t: f64| floor(t) + 6.0;
    let skirt = move |t: f64| floor(t) + 10.0;
    let mut rings: Vec<(&dyn Fn(f64) -> f64, f64)> =
        wall.iter().map(|(r, z)| (r.as_ref(), *z)).collect();
    rings.extend([
        (&rim as &dyn Fn(f64) -> f64, RIM),
        (&skirt, SKIRT),
        (&skirt, 0.0),
    ]);
    let mut faces: Vec<u32> = (0..STEPS).map(|_| 1).collect();
    faces.extend([2, 3, 3]);
    ringed(&rings, &faces, 4)
}
/// A cup as [`cup`]'s, but square seen from above, `half` across, its wall
/// leaning 3 out over its height to a sharp flat rim.
fn square_cup(half: f64) -> Body {
    let square = |h: f64| move |t: f64| h / t.cos().abs().max(t.sin().abs());
    let (floor, top, rim) = (square(half), square(half + 3.0), square(half + 6.0));
    ringed(
        &[(&floor, FLOOR), (&top, RIM), (&rim, RIM), (&rim, 0.0)],
        &[1, 2, 3],
        4,
    )
}

/// Each closed pass of `path` over the part, in the order cut: its points.
fn closed_passes(path: &crate::path::toolpath::Toolpath) -> Vec<Vec<DVec3>> {
    let mut out = Vec::new();
    let mut pass: Vec<DVec3> = Vec::new();
    let mut at = path.moves()[0].to;
    for m in path.moves() {
        if m.role == MoveRole::Cut {
            if pass.is_empty() {
                pass.push(at);
            }
            pass.push(m.to);
        } else if !pass.is_empty() {
            if pass[0].distance(pass[pass.len() - 1]) < 1e-6 && pass.len() > 3 {
                out.push(std::mem::take(&mut pass));
            }
            pass.clear();
        }
        at = m.to;
    }
    out
}

/// The sharpest turn of closed `ring`, seen from above, between the way in
/// over the half millimetre before a point and the way out over the half
/// after, in degrees.
fn sharpest(ring: &[DVec3]) -> f64 {
    // Resampled a tenth of a millimetre apart.
    let mut flat: Vec<DVec2> = Vec::new();
    for w in ring.windows(2) {
        let (a, b) = (w[0].truncate(), w[1].truncate());
        let n = (a.distance(b) / 0.1).ceil().max(1.0) as usize;
        for k in 0..n {
            flat.push(a.lerp(b, k as f64 / n as f64));
        }
    }
    let n = flat.len();
    (0..n)
        .filter_map(|i| {
            let (a, b, c) = (flat[(i + n - 5) % n], flat[i], flat[(i + 5) % n]);
            let (u, v) = ((b - a).try_normalize()?, (c - b).try_normalize()?);
            Some(u.dot(v).clamp(-1.0, 1.0).acos().to_degrees())
        })
        .fold(0.0, f64::max)
}

/// Inside out round an egg-shaped bowl with nothing avoided, the rings in
/// from the rim are offsets of it, and deeper than its ends are round they
/// came to points there and kept them to the middle. Inside the last ring
/// with no corner they go out from a spine instead (decision 0040): no ring
/// but the first two round it turns more than 30° over a millimetre, the
/// rings on the floor are no further apart than a stepover, and nothing is
/// gouged.
#[test]
fn an_egg_bowl_is_ringed_out_from_a_spine() {
    let body = egg_bowl(40.0);
    let bodies: Bodies<'_> = [("bowl", &body)].into_iter().collect();
    let job = no_group();
    let result = run_setup(&job, 0, &bodies).unwrap();
    let op = &result.operations[0];
    assert_eq!(result.verify.gouged, 0);
    let StrategyDetail::Scallop(report) = &op.detail else {
        panic!("{:?}", op.detail);
    };
    assert!(report.blended > 0, "no spine: {report:?}");
    let inside: Vec<Vec<DVec3>> = closed_passes(&op.path)
        .into_iter()
        .filter(|r| r.iter().all(|p| p.z < EGG_FLOOR + 1.0))
        .collect();
    assert!(inside.len() > 10, "{} rings inside", inside.len());
    let sharp: Vec<(usize, f64)> = inside
        .iter()
        .map(|r| sharpest(r))
        .enumerate()
        .skip(2)
        .filter(|&(_, turn)| turn > 30.0)
        .collect();
    assert!(sharp.is_empty(), "rings turning sharply: {sharp:?}");
    // On the floor, where the tool's tip is where it touches, the rings are
    // a stepover apart: across the spine's middle, and on along its line
    // past each of its ends. It is the first pass, open.
    let spine: Vec<DVec3> = op
        .path
        .moves()
        .iter()
        .skip_while(|m| m.role != MoveRole::Cut)
        .take_while(|m| m.role == MoveRole::Cut)
        .map(|m| m.to)
        .collect();
    let (west, east) = spine.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        (lo.min(p.x), hi.max(p.x))
    });
    assert!(east - west > 5.0, "the first pass is no spine: {spine:?}");
    let middle = 0.5 * (west + east);
    for (theta, from) in [
        (std::f64::consts::FRAC_PI_2, 0.0),
        (-std::f64::consts::FRAC_PI_2, 0.0),
        (0.0, east),
        (std::f64::consts::PI, -west),
    ] {
        let mut cut: Vec<DVec3> = section_from(
            &op.path,
            dvec2(middle, 0.0),
            theta,
            from - middle * theta.cos(),
        )
        .into_iter()
        .filter(|p| (p.z - EGG_FLOOR).abs() < 0.01)
        .collect();
        cut.sort_by(|a, b| {
            a.x.mul_add(theta.cos(), a.y * theta.sin())
                .total_cmp(&b.x.mul_add(theta.cos(), b.y * theta.sin()))
        });
        for w in cut.windows(2) {
            let gap = w[0].distance(w[1]);
            assert!(
                gap < 1.05 * STEPOVER,
                "{theta}: rings {gap} apart on the floor at {:?}",
                w[0]
            );
        }
    }
}

/// The cuts of `path` crossing the upright half-plane out from `centre` at
/// `theta`, beyond `from` from it: where the tool's tip is.
fn section_from(
    path: &crate::path::toolpath::Toolpath,
    centre: DVec2,
    theta: f64,
    from: f64,
) -> Vec<DVec3> {
    let dir = dvec2(theta.cos(), theta.sin());
    let side = |p: DVec3| dir.perp_dot(p.truncate() - centre);
    let mut out = Vec::new();
    let mut at = path.moves()[0].to;
    for m in path.moves() {
        if m.role == MoveRole::Cut {
            let (a, b) = (side(at), side(m.to));
            if a * b < 0.0 {
                let p = at.lerp(m.to, a / (a - b));
                if (p.truncate() - centre).dot(dir) > from {
                    out.push(p);
                }
            }
        }
        at = m.to;
    }
    out
}
/// A round bowl and a square pocket, nothing avoided, inside out: the rings
/// inside the rim are as they were. The round bowl's innermost rings come
/// to a point but its spine is nought long; the square's outermost ring has
/// corners already, so it has no last smooth ring. No spine, so no open
/// pass, and nothing blended inside.
#[test]
fn a_round_bowl_and_a_square_pocket_keep_their_rings() {
    let round = |p: DVec3| p.truncate().length() < CUP + 1.5;
    let square = |p: DVec3| p.x.abs().max(p.y.abs()) < 15.0;
    let bodies_and_rims: [(Body, &dyn Fn(DVec3) -> bool); 2] =
        [(cup(80.0), &round), (square_cup(12.0), &square)];
    for (body, within) in &bodies_and_rims {
        let bodies: Bodies<'_> = [("bowl", body)].into_iter().collect();
        let result = run_setup(&no_group(), 0, &bodies).unwrap();
        let op = &result.operations[0];
        assert_eq!(result.verify.gouged, 0);
        let StrategyDetail::Scallop(report) = &op.detail else {
            panic!("{:?}", op.detail);
        };
        // The cup's inside is its own zone, its rim a strip; outside the
        // rim only where the tool leaves the part is blended into.
        let opened: Vec<DVec3> = {
            let mut out = Vec::new();
            let (mut start, mut at) = (None, op.path.moves()[0].to);
            for m in op.path.moves() {
                if m.role == MoveRole::Cut {
                    start.get_or_insert(at);
                } else if let Some(s) = start.take()
                    && s.distance(at) > 1e-6
                    && within(s)
                {
                    out.push(s);
                }
                at = m.to;
            }
            out
        };
        assert!(
            opened.is_empty(),
            "open passes inside: {opened:?} ({report:?})"
        );
    }
}

/// How far a block's outline is from the origin at `theta` from +X: its
/// sides at X -16 and 34 and Y ±18, the pocket in it off centre.
fn block(theta: f64) -> f64 {
    let (c, s) = (theta.cos(), theta.sin());
    let x = if c > 0.0 { 34.0 / c } else { -16.0 / c };
    let y = 18.0 / s.abs();
    x.min(y)
}

/// A block 50 by 36, [`RIM`] high, with a round pocket of radius [`CUP`] round
/// the origin down to [`FLOOR`], its wall upright to a sharp rim: 6 mm of the
/// block's top beside it at -X, and 24 at +X. The floor 0, the pocket's wall
/// 1, the top 2, the block's sides 3, its base 4.
fn pocketed_block() -> Body {
    let (pocket, outline) = (|_: f64| CUP, block);
    ringed(
        &[
            (&pocket, FLOOR),
            (&pocket, RIM),
            (&outline, RIM),
            (&outline, 0.0),
        ],
        &[1, 2, 3],
        4,
    )
}

/// Inside out over a block with a pocket off centre in it, nothing avoided:
/// the pocket's rim is a strip, but the block's top round it, measured out
/// from it, is too much wider on one side than the other for its rings to
/// blend into where the tool leaves the block. So the ground is cut as one,
/// its strips ground again, as before zones (decision 0039): no ring ends in
/// an arc on the block's edge, where with the zones 20 ran from its top down
/// its sides; one pass over the part is open, a sliver on the pocket's wall,
/// where 33 were; and it lifts 4 times, where 12.
#[test]
fn a_block_with_a_pocket_off_centre_is_cut_as_one() {
    let body = pocketed_block();
    let bodies: Bodies<'_> = [("bowl", &body)].into_iter().collect();
    let result = run_setup(&no_group(), 0, &bodies).unwrap();
    let op = &result.operations[0];
    assert_eq!(result.verify.gouged, 0);
    // The passes over the part that do not end where they start: the level
    // rings at the Bottom beside the block's sides, cut in pieces, aside.
    let (mut start, mut at, mut open) = (None, op.path.moves()[0].to, Vec::new());
    for m in op.path.moves() {
        if m.role == MoveRole::Cut {
            start.get_or_insert(at);
        } else if let Some(s) = start.take()
            && s.distance(at) > 1e-6
            && s.z.max(at.z) > BOTTOM + 1e-3
        {
            open.push((s, at));
        }
        at = m.to;
    }
    // Inside the pocket: within its radius and a half of the tool's of its
    // middle, seen from above.
    let pocket = |p: DVec3| p.truncate().length() < CUP + 2.5;
    let outside: Vec<_> = open
        .iter()
        .filter(|(a, b)| !pocket(*a) || !pocket(*b))
        .collect();
    assert!(outside.is_empty(), "arcs ending on the block: {outside:?}");
    assert!(open.len() <= 1, "open passes: {open:?}");
    let mut lifts = 0;
    let mut at = op.path.moves()[0].to;
    for m in op.path.moves() {
        if m.role == MoveRole::Rapid && m.to.z > at.z + 1e-9 && m.to.truncate() == at.truncate() {
            lifts += 1;
        }
        at = m.to;
    }
    assert!(lifts <= 4, "{lifts} lifts");
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

/// How far the widest of a tilted bulge stands above half the bowl's height
/// at +X, and below it at -X.
const TILT: f64 = 1.5;

/// A bowl as the bulging one, but widest [`TILT`] higher at +X than half its
/// height and as much lower at -X: the line where the tool leaves the part
/// runs round it at a slant to the rings, which are a fixed distance from
/// the floor's edge.
fn tilted() -> Body {
    const STEPS: u32 = 48;
    let ring = |i: usize| move |theta: f64| radii(theta)[i];
    let (floor, wall, rim) = (ring(0), ring(1), ring(2));
    let outside: Vec<(Radius, f64)> = (1..=STEPS)
        .map(|i| {
            let z = RIM * (1.0 - f64::from(i) / f64::from(STEPS));
            let radius: Radius = Box::new(move |t| {
                let widest = 0.5 * RIM + TILT * t.cos();
                let up = if z >= widest {
                    (RIM - z) / (RIM - widest)
                } else {
                    z / widest
                };
                radii(t)[2] + BULGE * (std::f64::consts::FRAC_PI_2 * up).sin()
            });
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

/// Down a bowl widest higher on one side than the other, the rings meet the
/// line the tool leaves the part along at a slant: they blend into it, and
/// rings below it go down to the first level ring (decision 0038). Every pass
/// off the floor's edge is a closed ring, no gouge, no rapid into the
/// material. Inside out, from the floor's edge, the rings along the outside
/// are no further apart than a stepover and a twentieth where the tool's tip
/// is (1.03, measured), and it lifts no more than 3 times, where it lifted at
/// every arc's end and every piece of a level ring.
#[test]
fn rings_blend_into_where_the_tool_leaves_a_tilted_bulge() {
    for order in ["inside-out", "outside-in"] {
        let body = tilted();
        let bodies: Bodies<'_> = [("bowl", &body)].into_iter().collect();
        let job: Job = toml::from_str(&JOB.replace("inside-out", order)).unwrap();
        let result = run_setup(&job, 0, &bodies).unwrap();
        let op = &result.operations[0];
        assert_eq!(result.verify.gouged, 0, "{order}");
        assert_eq!(op.sim.rapid_contacts, 0, "{order}");
        let StrategyDetail::Scallop(report) = &op.detail else {
            panic!("{:?}", op.detail);
        };
        assert!(report.blended > 0, "{order}: {report:?}");
        // Each run of cuts off the floor's edge ends where it started; the
        // floor's edge, where the floor avoided meets the wall, is cut out
        // in places where the tool touches only the floor.
        let (mut start, mut at, mut open) = (None, op.path.moves()[0].to, Vec::new());
        for m in op.path.moves() {
            if m.role == MoveRole::Cut {
                start.get_or_insert(at);
            } else if let Some(s) = start.take()
                && s.distance(at) > 1e-6
                && s.z.min(at.z) > FLOOR + 0.1
            {
                open.push((s, at));
            }
            at = m.to;
        }
        assert!(open.is_empty(), "{order}: passes not closed {open:?}");
        // Along where the tool leaves the part and below it, no ring bobs up
        // and down: no two turns in height of more than a tenth of a stepover
        // within the tool's radius along it. The line's height there is the
        // tool's at the bulge's widest, which the facets would have it jump
        // by from one grid edge to the next.
        let mut ring: Vec<DVec3> = Vec::new();
        let mut cutting = false;
        let mut bobbing = Vec::new();
        for m in op.path.moves() {
            if m.role == MoveRole::Cut {
                ring.push(m.to);
            } else if cutting {
                let leaving = |p: &DVec3| 0.5 * RIM + TILT * p.y.atan2(p.x).cos() - 2.0;
                let outside = ring.iter().all(|p| {
                    p.truncate().length() > radii(p.y.atan2(p.x))[2] + 0.5
                        && p.z < leaving(p) + 0.05
                });
                if outside && let Some(at) = turns_within(&ring, 0.1 * STEPOVER, 2.0) {
                    bobbing.push(at);
                }
                ring.clear();
            }
            cutting = m.role == MoveRole::Cut;
        }
        assert!(
            bobbing.is_empty(),
            "{order}: rings bob up and down at {bobbing:?}"
        );
        if order == "outside-in" {
            continue;
        }
        let mut worst = 0.0f64;
        for k in 0..24 {
            let theta = std::f64::consts::TAU * f64::from(k) / 24.0;
            let cut = section(&op.path, theta, radii(theta)[2] + 0.5);
            for w in cut.windows(2) {
                worst = worst.max(w[0].distance(w[1]));
            }
        }
        assert!(worst < 1.05 * STEPOVER, "rings {worst} apart");
        let mut lifts = 0;
        let mut at = op.path.moves()[0].to;
        for m in op.path.moves() {
            if m.role == MoveRole::Rapid && m.to.z > at.z + 1e-9 && m.to.truncate() == at.truncate()
            {
                lifts += 1;
            }
            at = m.to;
        }
        assert!(lifts <= 3, "{lifts} lifts");
    }
}
/// Where along `points` two turns in height, each more than `size` from the
/// last, come within `within` of each other along the way, if anywhere.
fn turns_within(points: &[DVec3], size: f64, within: f64) -> Option<DVec3> {
    let mut along = 0.0;
    let mut extremes: Vec<(f64, DVec3)> = vec![(0.0, *points.first()?)];
    let mut rising: Option<bool> = None;
    for (a, b) in points.iter().zip(points.iter().skip(1)) {
        along += a.distance(*b);
        let last = extremes[extremes.len() - 1].1.z;
        let (higher, lower) = (b.z > last + size, b.z < last - size);
        match rising {
            None if higher || lower => {
                rising = Some(higher);
                extremes.push((along, *b));
            }
            Some(up) if (up && b.z > last) || (!up && b.z < last) => {
                let n = extremes.len() - 1;
                extremes[n] = (along, *b);
            }
            Some(up) if (up && lower) || (!up && higher) => {
                rising = Some(!up);
                extremes.push((along, *b));
            }
            _ => {}
        }
    }
    // The turns: every extreme but the start and the one still running.
    let turns = &extremes[1..extremes.len().saturating_sub(1).max(1)];
    turns
        .windows(2)
        .find(|w| w[1].0 - w[0].0 < within)
        .map(|w| w[1].1)
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
