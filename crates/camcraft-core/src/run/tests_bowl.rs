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
    let n = AROUND;
    let mut v = vec![dvec3(0.0, 0.0, FLOOR)];
    // Rings out from the floor's middle, each `n` round.
    let rings: [(usize, f64); 5] = [(0, FLOOR), (1, RIM), (2, RIM), (3, SKIRT), (3, 0.0)];
    for (radius, z) in rings {
        for k in 0..n {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(n);
            let r = radii(theta)[radius];
            v.push(dvec3(r * theta.cos(), r * theta.sin(), z));
        }
    }
    let under = u32::try_from(v.len()).unwrap();
    v.push(dvec3(0.0, 0.0, 0.0));
    let at = |ring: u32, k: u32| 1 + ring * n + k % n;
    let (mut t, mut labels) = (Vec::new(), Vec::new());
    for k in 0..n {
        t.push([0, at(0, k), at(0, k + 1)]);
        labels.push(0);
        // Each strip from a ring to the next out, its triangles wound so
        // their normals face out of the bowl's material.
        for (ring, face) in [(0, 1), (1, 2), (2, 3), (3, 3)] {
            t.push([at(ring, k), at(ring + 1, k), at(ring + 1, k + 1)]);
            t.push([at(ring, k), at(ring + 1, k + 1), at(ring, k + 1)]);
            labels.extend([face, face]);
        }
        t.push([under, at(4, k + 1), at(4, k)]);
        labels.push(4);
    }
    Body::with_faces(TriMesh::new(v, t).unwrap(), labels, 5).unwrap()
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
