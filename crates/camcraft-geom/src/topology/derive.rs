//! Faces for a mesh that came without any: an STL file's triangles.
//!
//! A person picking "the floor" or "that wall" means a face as their CAD program
//! drew it. An STL file has thrown that away, so it is found again here from the
//! triangles alone, in two passes:
//!
//! 1. **Planes.** Starting from the largest triangle, grow across neighbours whose
//!    normal agrees within [`DeriveParams::plane_degrees`] and whose corners lie on
//!    the seed's plane. The plane is the seed's and stays fixed, so a gentle curve
//!    cannot drift into one.
//! 2. **Curved faces.** What no plane took is joined across every edge that is not
//!    a crease (sharper than [`DeriveParams::crease_degrees`]).
//!
//! A patch of a tessellated cylinder is itself flat, so pass 1 alone would call
//! every facet a plane. A patch is kept as a plane only if it fills the whole
//! crease-bounded region it sits in (a pocket floor ringed by sharp walls), or if it
//! is [`DeriveParams::plane_dominance`] times larger than any patch it runs smoothly
//! into (a floor that meets its wall through a fillet). A cylinder's facets are all
//! the same size, so they fail both and become one curved face.
//!
//! The result depends only on the triangles, and every order is fixed, so the same
//! file gives the same faces on every run and every machine.

use super::Side;
use crate::mesh::TriMesh;
use glam::DVec3;
use std::collections::VecDeque;

/// How faces are found in a mesh that has none.
///
/// These are part of what a saved pick means: a picked face is found again by a
/// point on it, so a change here moves no pick, but it can change which triangles a
/// face covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeriveParams {
    /// A fold sharper than this, in degrees, is an edge between faces.
    pub crease_degrees: f64,
    /// Triangles within this many degrees of a plane's normal can be part of it.
    pub plane_degrees: f64,
    /// A flat patch that runs smoothly into others is a plane only if it is this
    /// many times larger than the largest of them.
    pub plane_dominance: f64,
}

impl Default for DeriveParams {
    fn default() -> Self {
        Self {
            crease_degrees: 30.0,
            plane_degrees: 0.5,
            plane_dominance: 4.0,
        }
    }
}

const NONE: u32 = u32::MAX;

/// A triangle's neighbour across a shared side.
#[derive(Debug, Clone, Copy)]
struct Link {
    from: u32,
    to: u32,
    /// Whether the two are wound the same way, so their normals compare directly.
    consistent: bool,
    /// Whether the fold between them is a crease.
    crease: bool,
}

/// A face number for every triangle, and how many faces.
pub(super) fn faces(
    mesh: &TriMesh,
    sides: &[Side],
    params: &DeriveParams,
    tol: f64,
    size: f64,
) -> (Vec<u32>, u32) {
    let n = mesh.len();
    let (normals, areas): (Vec<DVec3>, Vec<f64>) = (0..n)
        .map(|t| match mesh.triangle(t) {
            Some([a, b, c]) => {
                let x = (b - a).cross(c - a);
                (x.normalize_or_zero(), x.length() / 2.0)
            }
            None => (DVec3::ZERO, 0.0),
        })
        .unzip();
    let tiny = 1e-12 * size * size;
    let degenerate: Vec<bool> = normals
        .iter()
        .zip(&areas)
        .map(|(n, a)| *n == DVec3::ZERO || a.is_nan() || *a <= tiny)
        .collect();
    let bad = |t: u32| degenerate.get(t as usize).copied().unwrap_or(true);
    let normal = |t: u32| normals.get(t as usize).copied().unwrap_or(DVec3::ZERO);
    let area = |t: u32| areas.get(t as usize).copied().unwrap_or(0.0);

    // Neighbours across manifold sides, both ways, sorted so every walk below
    // visits them in one fixed order.
    let crease_cos = params.crease_degrees.to_radians().cos();
    let mut links = Vec::new();
    for group in sides.chunk_by(|x, y| (x.lo, x.hi) == (y.lo, y.hi)) {
        let [a, b] = group else { continue };
        if a.tri == b.tri || bad(a.tri) || bad(b.tri) {
            continue;
        }
        // Consistent winding runs a shared side in opposite directions.
        let consistent = a.forward != b.forward;
        let nb = if consistent {
            normal(b.tri)
        } else {
            -normal(b.tri)
        };
        let crease = normal(a.tri).dot(nb) < crease_cos;
        links.push(Link {
            from: a.tri,
            to: b.tri,
            consistent,
            crease,
        });
        links.push(Link {
            from: b.tri,
            to: a.tri,
            consistent,
            crease,
        });
    }
    links.sort_unstable_by_key(|l| (l.from, l.to));
    let around = |t: u32| {
        let lo = links.partition_point(|l| l.from < t);
        let hi = links.partition_point(|l| l.from <= t);
        links.get(lo..hi).unwrap_or(&[])
    };

    // Pass 1: flat patches, largest seed first.
    let mut order: Vec<u32> = (0..n)
        .filter_map(|t| u32::try_from(t).ok())
        .filter(|&t| !bad(t))
        .collect();
    order.sort_by(|&a, &b| area(b).total_cmp(&area(a)).then(a.cmp(&b)));
    let plane_cos = params.plane_degrees.to_radians().cos();
    let mut patch = vec![NONE; n];
    let mut patches: Vec<(f64, usize)> = Vec::new(); // (area, triangles)
    let mut queue = VecDeque::new();
    for &seed in &order {
        if patch.get(seed as usize) != Some(&NONE) {
            continue;
        }
        let Some(id) = u32::try_from(patches.len()).ok() else {
            break;
        };
        let n0 = normal(seed);
        let Some([p0, _, _]) = mesh.triangle(seed as usize) else {
            continue;
        };
        let d0 = n0.dot(p0);
        set(&mut patch, seed, id);
        let (mut total, mut count) = (area(seed), 1);
        queue.clear();
        queue.push_back((seed, 1.0));
        while let Some((t, sign)) = queue.pop_front() {
            for l in around(t) {
                if patch.get(l.to as usize) != Some(&NONE) {
                    continue;
                }
                let s = if l.consistent { sign } else { -sign };
                if (normal(l.to) * s).dot(n0) < plane_cos {
                    continue;
                }
                let on = mesh
                    .triangle(l.to as usize)
                    .is_some_and(|c| c.iter().all(|p| (n0.dot(*p) - d0).abs() <= tol));
                if !on {
                    continue;
                }
                set(&mut patch, l.to, id);
                total += area(l.to);
                count += 1;
                queue.push_back((l.to, s));
            }
        }
        patches.push((total, count));
    }

    // The crease-bounded regions, and how many triangles each holds.
    let smooth = |l: &&Link| !l.crease;
    let (region, region_sizes) = components(n, &order, |t| around(t).iter().filter(smooth));

    // Which patches are planes.
    let is_plane: Vec<bool> = {
        let mut largest_neighbour = vec![0.0_f64; patches.len()];
        for l in links.iter().filter(|l| !l.crease) {
            let (Some(&a), Some(&b)) = (patch.get(l.from as usize), patch.get(l.to as usize))
            else {
                continue;
            };
            if a != b
                && let (Some(slot), Some((other, _))) = (
                    largest_neighbour.get_mut(a as usize),
                    patches.get(b as usize),
                )
            {
                *slot = slot.max(*other);
            }
        }
        let mut fills = vec![false; patches.len()];
        for &t in &order {
            let (Some(&p), Some(&r)) = (patch.get(t as usize), region.get(t as usize)) else {
                continue;
            };
            if let (Some(slot), Some((_, count)), Some(size)) = (
                fills.get_mut(p as usize),
                patches.get(p as usize),
                region_sizes.get(r as usize),
            ) {
                *slot = count == size;
            }
        }
        patches
            .iter()
            .zip(&largest_neighbour)
            .zip(&fills)
            .map(|(((area, _), biggest), fills)| {
                *fills || *area >= params.plane_dominance * biggest
            })
            .collect()
    };
    let planar = |t: u32| {
        patch
            .get(t as usize)
            .and_then(|p| is_plane.get(*p as usize))
            .copied()
            .unwrap_or(false)
    };

    // Pass 2: what no plane took, joined across smooth sides.
    let rest: Vec<u32> = order.iter().copied().filter(|&t| !planar(t)).collect();
    let (curved, _) = components(n, &rest, |t| {
        around(t).iter().filter(move |l| !l.crease && !planar(l.to))
    });

    // Labels: a plane's patch, or a curved piece, numbered after the planes.
    let mut label = vec![NONE; n];
    let planes = u32::try_from(patches.len()).unwrap_or(NONE);
    for &t in &order {
        let l = if planar(t) {
            patch.get(t as usize).copied()
        } else {
            curved.get(t as usize).map(|c| c.saturating_add(planes))
        };
        if let Some(l) = l {
            set(&mut label, t, l);
        }
    }

    // Triangles with no area take a neighbour's face, through any side.
    let mut touching: Vec<(u32, u32)> = Vec::new();
    for group in sides.chunk_by(|x, y| (x.lo, x.hi) == (y.lo, y.hi)) {
        for a in group.iter().filter(|s| bad(s.tri)) {
            for b in group {
                if a.tri != b.tri {
                    touching.push((a.tri, b.tri));
                }
            }
        }
    }
    touching.sort_unstable();
    touching.dedup();
    loop {
        let mut changed = false;
        for &(a, b) in &touching {
            if label.get(a as usize) == Some(&NONE)
                && let Some(&l) = label.get(b as usize)
                && l != NONE
            {
                set(&mut label, a, l);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Number faces in order of their first triangle; anything still alone gets
    // its own.
    let mut renumber = std::collections::BTreeMap::new();
    let mut next = 0u32;
    let mut out = Vec::with_capacity(n);
    for l in label {
        let key = if l == NONE {
            // A face of its own: a key no label can take.
            u64::from(next) + (1 << 32)
        } else {
            u64::from(l)
        };
        let id = *renumber.entry(key).or_insert_with(|| {
            let id = next;
            next += 1;
            id
        });
        out.push(id);
    }
    (out, next)
}

fn set(v: &mut [u32], i: u32, value: u32) {
    if let Some(slot) = v.get_mut(i as usize) {
        *slot = value;
    }
}

/// Connected pieces of `members`, joined through `next`, as a piece number per
/// triangle (`NONE` for those not in `members`) and each piece's size.
///
/// Pieces are numbered in the order `members` first reaches them.
fn components<'a, I>(n: usize, members: &[u32], next: impl Fn(u32) -> I) -> (Vec<u32>, Vec<usize>)
where
    I: Iterator<Item = &'a Link>,
{
    let mut piece = vec![NONE; n];
    let mut inside = vec![false; n];
    for &t in members {
        if let Some(slot) = inside.get_mut(t as usize) {
            *slot = true;
        }
    }
    let mut sizes = Vec::new();
    let mut queue = VecDeque::new();
    for &start in members {
        if piece.get(start as usize) != Some(&NONE) {
            continue;
        }
        let Ok(id) = u32::try_from(sizes.len()) else {
            break;
        };
        set(&mut piece, start, id);
        let mut size = 1;
        queue.push_back(start);
        while let Some(t) = queue.pop_front() {
            for l in next(t) {
                let open = inside.get(l.to as usize).copied().unwrap_or(false)
                    && piece.get(l.to as usize) == Some(&NONE);
                if open {
                    set(&mut piece, l.to, id);
                    size += 1;
                    queue.push_back(l.to);
                }
            }
        }
        sizes.push(size);
    }
    (piece, sizes)
}
