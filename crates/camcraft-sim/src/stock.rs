//! Material removal simulation: the stock model and the gouge oracle.
//!
//! These are deliberately the same code. Simulating a toolpath *is* how stock is
//! updated, so rest machining is not a feature — it is what happens when operation
//! two runs after operation one. And because the simulator already knows where the
//! material went, it can answer the questions no geometric check can:
//!
//! - did the tool end up **below the part**, and by how much
//! - did a **rapid touch material**, which is the defect Pngcam ships with
//!
//! # Removal is an erosion, the dual of the cutter-location dilation
//!
//! A tool tip at `(x, y, z)` occupies, at radial distance `d` from its axis, every
//! height at or above `z + h(d)`. So removing it from the stock field is
//!
//! ```text
//! Zstock(px, py) <- min( Zstock(px, py), z + h(d) )
//! ```
//!
//! over the tool footprint — exactly the mirror of how the CL surface is built.
//!
//! # Why this crate defines its own traits
//!
//! `camcraft-core` depends on this crate, not the other way round, so nothing here
//! can name `Toolpath` or `ToolProfile` without creating a cycle. [`SweptTool`] and
//! [`SimMove`] are the minimum the simulator actually needs — a footprint, a profile
//! and whether a move is allowed to touch material — and core implements them for
//! its own types. The restriction is useful rather than merely tolerated: it keeps
//! the simulator a geometry service that cannot quietly grow CAM opinions.

use camcraft_geom::heightmap::{HeightMap, HeightMapError, narrow};
use camcraft_geom::mesh::TriMesh;
use glam::{DVec2, DVec3, dvec2};
use rayon::prelude::*;

/// Upper bound on samples along one swept move.
///
/// A bound rather than a hope: a pathological coordinate must not turn a single
/// move into an unbounded loop.
const MAX_SWEEP_STEPS: u32 = 1_000_000;

/// The minimum a simulator needs to know about a cutter.
pub trait SweptTool: Sync {
    /// Largest radius anywhere on the cutting portion.
    fn max_radius(&self) -> f64;

    /// Height of the tool surface at radial distance `d` above the tip, or `None`
    /// beyond the tool.
    fn height_at(&self, d: f64) -> Option<f64>;
}

/// Whether a move is permitted to be in material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveClass {
    /// Cutting, plunging, ramping or leading. May remove material.
    MayCut,
    /// A traverse at full speed. Must never touch material; if it does, that is a
    /// defect worth stopping for.
    Rapid,
}

/// One move, reduced to what the simulator needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimMove {
    /// Where the tool tip ends up.
    pub to: DVec3,
    /// Whether it may be in material.
    pub class: MoveClass,
}

/// A block of material, as a height field.
#[derive(Debug, Clone, PartialEq)]
pub struct Stock {
    field: HeightMap,
    floor: f64,
}

/// Stock that could not be created.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum StockError {
    /// The grid was unusable.
    #[error("cannot build the stock grid: {0}")]
    Grid(#[from] HeightMapError),
    /// The block had no height.
    #[error("stock top {top} is not above its floor {floor}")]
    Inverted {
        /// Requested top.
        top: f64,
        /// Requested floor.
        floor: f64,
    },
}

/// What simulating a path did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SimReport {
    /// Approximate volume removed, in cubic millimetres.
    pub removed_mm3: f64,
    /// Rapid moves that passed through material.
    ///
    /// Any non-zero value is a defect: a rapid is a full-speed traverse and is not
    /// supposed to be cutting anything.
    pub rapid_contacts: u64,
    /// Deepest a rapid went into material, in millimetres.
    pub deepest_rapid_contact_mm: f64,
}

impl SimReport {
    /// Whether anything here should stop a program being run.
    pub fn is_clean(&self) -> bool {
        self.rapid_contacts == 0
    }
}

/// How the simulated result compares with the part.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VerifyReport {
    /// Columns compared.
    pub compared: u64,
    /// Columns where the simulated surface is below the part.
    pub gouged: u64,
    /// Deepest such excursion, in millimetres.
    pub deepest_gouge_mm: f64,
    /// Columns where material remains well above the part.
    pub leftover: u64,
    /// Most material left anywhere, in millimetres.
    pub tallest_leftover_mm: f64,
}

impl VerifyReport {
    /// Whether the result is within tolerance of the part.
    ///
    /// Leftover does not count: an unfinished region is a planning question, while a
    /// gouge is a ruined part.
    pub fn is_clean(&self) -> bool {
        self.gouged == 0
    }
}

impl Stock {
    /// A rectangular block covering `[min, max]`, its top at `top`.
    pub fn block(
        min: DVec2,
        max: DVec2,
        top: f64,
        floor: f64,
        pitch: f64,
    ) -> Result<Self, StockError> {
        if top <= floor || !top.is_finite() || !floor.is_finite() {
            return Err(StockError::Inverted { top, floor });
        }
        Ok(Self {
            field: HeightMap::covering(min, max, pitch, top)?,
            floor,
        })
    }

    /// The material height field.
    pub fn field(&self) -> &HeightMap {
        &self.field
    }

    /// The bottom of the block.
    pub fn floor(&self) -> f64 {
        self.floor
    }

    /// Height of material at a column, or `None` outside the block.
    pub fn height_at(&self, x: f64, y: f64) -> Option<f64> {
        self.field.sample(x, y)
    }

    /// Sweep a path through the block, removing material.
    ///
    /// The sweep is sampled along each move at no more than half a cell, so
    /// consecutive stamps overlap heavily. Residual error is bounded by how much
    /// `z + h(d)` can change over half a cell, which is well under a cell for any
    /// real cutter.
    pub fn apply<T: SweptTool>(&mut self, moves: &[SimMove], tool: &T) -> SimReport {
        let mut report = SimReport::default();
        let pitch = self.field.pitch();
        let step = (pitch * 0.5).max(1.0e-6);
        let before = self.volume();

        let mut at: Option<DVec3> = None;
        for m in moves {
            let from = match at {
                Some(p) => p,
                None => {
                    at = Some(m.to);
                    continue;
                }
            };
            let span = (m.to - from).length();
            // Bounded: a pathological coordinate cannot turn one move into an
            // unbounded loop.
            // Bounded and clamped, so every later conversion is exact.
            let steps = {
                let n = (span / step).ceil();
                if !n.is_finite() || n < 1.0 {
                    1u32
                } else {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let n = n.min(f64::from(MAX_SWEEP_STEPS)) as u32;
                    n
                }
            };
            let mut worst = 0.0f64;
            for k in 0..=steps {
                let t = f64::from(k) / f64::from(steps);
                let p = from + (m.to - from) * t;
                let cut = self.stamp(p, tool);
                worst = worst.max(cut);
            }
            if m.class == MoveClass::Rapid && worst > pitch * 0.01 {
                report.rapid_contacts += 1;
                report.deepest_rapid_contact_mm = report.deepest_rapid_contact_mm.max(worst);
            }
            at = Some(m.to);
        }

        report.removed_mm3 = (before - self.volume()).max(0.0);
        report
    }

    /// Lower the field to the underside of a tool at `tip`, returning how far the
    /// deepest cell was cut.
    fn stamp<T: SweptTool>(&mut self, tip: DVec3, tool: &T) -> f64 {
        let r = tool.max_radius();
        if !tip.is_finite() || !r.is_finite() || r <= 0.0 {
            return 0.0;
        }
        let pitch = self.field.pitch();
        let origin = self.field.origin();
        let (nx, ny) = (self.field.nx(), self.field.ny());

        // Inclusive bounds. An exclusive upper bound leaves the cell exactly at the
        // cutter's rim unstamped, which under-removes material -- and under-removal
        // is the dangerous direction, because it can hide a gouge.
        let idx = |v: f64, n: u32| -> u32 {
            let last = n.saturating_sub(1);
            if !v.is_finite() || v <= 0.0 {
                0
            } else if v >= f64::from(last) {
                last
            } else {
                // Non-negative and below `last`, so the narrowing is exact.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = v as u32;
                v
            }
        };
        let x0 = idx(((tip.x - r - origin.x) / pitch).floor(), nx);
        let x1 = idx(((tip.x + r - origin.x) / pitch).ceil(), nx);
        let y0 = idx(((tip.y - r - origin.y) / pitch).floor(), ny);
        let y1 = idx(((tip.y + r - origin.y) / pitch).ceil(), ny);

        let floor = self.floor;
        let mut deepest = 0.0f64;
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                let Some(p) = self.field.position(ix, iy) else {
                    continue;
                };
                let d = (p - dvec2(tip.x, tip.y)).length();
                let Some(h) = tool.height_at(d) else { continue };
                let under = (tip.z + h).max(floor);
                if let Some(cur) = self.field.get(ix, iy)
                    && under < cur
                {
                    deepest = deepest.max(cur - under);
                    self.field.set(ix, iy, under);
                }
            }
        }
        deepest
    }

    /// Volume of material remaining, in cubic millimetres.
    pub fn volume(&self) -> f64 {
        let cell = self.field.pitch() * self.field.pitch();
        let floor = self.floor;
        self.field
            .data()
            .par_iter()
            .map(|v| {
                let h = f64::from(*v);
                if h.is_finite() {
                    (h - floor).max(0.0)
                } else {
                    0.0
                }
            })
            .sum::<f64>()
            * cell
    }

    /// Compare the simulated result against the part it was meant to produce.
    ///
    /// This is the oracle. Below the part by more than `tolerance` is a gouge, which
    /// is a ruined part; well above it is leftover, which is only unfinished.
    pub fn verify(&self, part: &PartSurface, tolerance: f64, leftover_limit: f64) -> VerifyReport {
        let mut r = VerifyReport::default();
        for iy in 0..self.field.ny() {
            for ix in 0..self.field.nx() {
                let (Some(p), Some(stock)) = (self.field.position(ix, iy), self.field.get(ix, iy))
                else {
                    continue;
                };
                let Some(target) = part.height_at(p.x, p.y) else {
                    continue;
                };
                r.compared += 1;
                let delta = stock - target;
                if delta < -tolerance {
                    r.gouged += 1;
                    r.deepest_gouge_mm = r.deepest_gouge_mm.max(-delta);
                } else if delta > leftover_limit {
                    r.leftover += 1;
                    r.tallest_leftover_mm = r.tallest_leftover_mm.max(delta);
                }
            }
        }
        r
    }

    /// Write the field as a 16-bit grayscale PGM, for eyeballing.
    ///
    /// Cheap to produce and worth a great deal when a number looks wrong and the
    /// question is *where*.
    pub fn to_pgm(&self) -> String {
        let (nx, ny) = (self.field.nx(), self.field.ny());
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for v in self.field.data() {
            let h = f64::from(*v);
            if h.is_finite() {
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
        let span = if hi > lo { hi - lo } else { 1.0 };
        let mut s = format!("P2\n{nx} {ny}\n65535\n");
        for iy in (0..ny).rev() {
            for ix in 0..nx {
                let v = self.field.get(ix, iy).unwrap_or(lo);
                let t = ((v - lo) / span * 65535.0).clamp(0.0, 65535.0);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let t = t as u32;
                s.push_str(&t.to_string());
                s.push(' ');
            }
            s.push('\n');
        }
        s
    }
}

/// The part's own upper surface, sampled per column.
///
/// Not a cutter-location surface: this is the shape the program is supposed to
/// leave behind, which is what a gouge is measured against.
#[derive(Debug, Clone)]
pub struct PartSurface {
    field: HeightMap,
}

impl PartSurface {
    /// Rasterise the top surface of a mesh over `[min, max]`.
    pub fn from_mesh(
        mesh: &TriMesh,
        min: DVec2,
        max: DVec2,
        pitch: f64,
    ) -> Result<Self, StockError> {
        let mut field = HeightMap::covering(min, max, pitch, f64::NAN)?;
        let origin = field.origin();
        let nx = field.nx() as usize;
        let ny = field.ny();

        // Per-column maximum over the triangles covering it. Parallel over rows,
        // each writing only its own slice, so the result does not depend on thread
        // count.
        field
            .data_mut()
            .par_chunks_mut(nx)
            .zip(0..ny)
            .for_each(|(row, iy)| {
                let y = origin.y + f64::from(iy) * pitch;
                for (cell, ix) in row.iter_mut().zip(0u32..) {
                    let x = origin.x + f64::from(ix) * pitch;
                    *cell = match column_height(mesh, x, y) {
                        Some(z) => narrow(z),
                        None => f32::NAN,
                    };
                }
            });
        Ok(Self { field })
    }

    /// Height of the part at a column, or `None` where the part is absent.
    pub fn height_at(&self, x: f64, y: f64) -> Option<f64> {
        self.field.sample(x, y)
    }

    /// The underlying field.
    pub fn field(&self) -> &HeightMap {
        &self.field
    }
}

/// Highest point of the mesh directly above `(x, y)`.
fn column_height(mesh: &TriMesh, x: f64, y: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for i in 0..mesh.len() {
        let Some(t) = mesh.triangle(i) else { continue };
        if let Some(z) = triangle_height(&t, x, y) {
            best = Some(match best {
                Some(b) if b >= z => b,
                _ => z,
            });
        }
    }
    best
}

/// Height of a triangle above `(x, y)`, or `None` if it does not cover the column.
fn triangle_height(t: &[DVec3; 3], x: f64, y: f64) -> Option<f64> {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac) = (b - a, c - a);
    let det = ab.x * ac.y - ab.y * ac.x;
    if det.abs() < 1.0e-18 {
        // Vertical or degenerate: contributes no column height of its own, and its
        // edges belong to the neighbouring faces.
        return None;
    }
    let (px, py) = (x - a.x, y - a.y);
    let u = (px * ac.y - py * ac.x) / det;
    let v = (ab.x * py - ab.y * px) / det;
    const E: f64 = 1.0e-9;
    if u >= -E && v >= -E && u + v <= 1.0 + E {
        Some(a.z + ab.z * u + ac.z * v)
    } else {
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use camcraft_geom::synthetic;
    use glam::dvec3;

    /// A ball of radius r, standing in for the real cutter.
    struct Ball(f64);
    impl SweptTool for Ball {
        fn max_radius(&self) -> f64 {
            self.0
        }
        fn height_at(&self, d: f64) -> Option<f64> {
            (d <= self.0).then(|| self.0 - (self.0 * self.0 - d * d).max(0.0).sqrt())
        }
    }

    /// A flat mill of radius r.
    struct Flat(f64);
    impl SweptTool for Flat {
        fn max_radius(&self) -> f64 {
            self.0
        }
        fn height_at(&self, d: f64) -> Option<f64> {
            (d <= self.0).then_some(0.0)
        }
    }

    fn block() -> Stock {
        Stock::block(dvec2(-10.0, -10.0), dvec2(10.0, 10.0), 5.0, -5.0, 0.5).unwrap()
    }

    #[test]
    fn rejects_an_inverted_block() {
        let e = Stock::block(dvec2(0.0, 0.0), dvec2(1.0, 1.0), 0.0, 5.0, 0.5);
        assert!(matches!(e, Err(StockError::Inverted { .. })));
    }

    #[test]
    fn a_fresh_block_is_full() {
        let s = block();
        assert_eq!(s.height_at(0.0, 0.0), Some(5.0));
        // 20 x 20 x 10, within a cell of rounding on the grown grid.
        assert!((s.volume() - 4000.0).abs() < 400.0, "got {}", s.volume());
    }

    /// The defining behaviour: cutting removes material down to the tool underside.
    #[test]
    fn a_flat_cut_leaves_a_flat_floor() {
        let mut s = block();
        let moves = vec![
            SimMove {
                to: dvec3(-8.0, 0.0, 1.0),
                class: MoveClass::MayCut,
            },
            SimMove {
                to: dvec3(8.0, 0.0, 1.0),
                class: MoveClass::MayCut,
            },
        ];
        let r = s.apply(&moves, &Flat(2.0));
        assert!(r.is_clean());
        assert!(r.removed_mm3 > 0.0);
        assert_eq!(
            s.height_at(0.0, 0.0),
            Some(1.0),
            "floor should be at the tip"
        );
        assert_eq!(
            s.height_at(0.0, 9.0),
            Some(5.0),
            "away from the cut, untouched"
        );
    }

    /// A ball leaves its own radius behind, not a flat floor.
    #[test]
    fn a_ball_leaves_a_round_bottom() {
        let mut s = Stock::block(dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 5.0, -5.0, 0.1).unwrap();
        let moves = vec![
            SimMove {
                to: dvec3(0.0, 0.0, 0.0),
                class: MoveClass::MayCut,
            },
            SimMove {
                to: dvec3(0.0, 0.0, 0.0),
                class: MoveClass::MayCut,
            },
        ];
        s.apply(&moves, &Ball(3.0));
        assert!(
            (s.height_at(0.0, 0.0).unwrap() - 0.0).abs() < 0.05,
            "tip touches bottom"
        );
        // One radius out, the ball surface is a full radius up.
        let side = s.height_at(3.0, 0.0).unwrap();
        assert!(
            (side - 3.0).abs() < 0.2,
            "expected ~3.0 at the equator, got {side}"
        );
    }

    /// The check the plan singles out, and the defect Pngcam documents.
    #[test]
    fn a_rapid_through_material_is_caught() {
        let mut s = block();
        let moves = vec![
            SimMove {
                to: dvec3(-8.0, 0.0, 1.0),
                class: MoveClass::Rapid,
            },
            SimMove {
                to: dvec3(8.0, 0.0, 1.0),
                class: MoveClass::Rapid,
            },
        ];
        let r = s.apply(&moves, &Flat(2.0));
        assert!(
            !r.is_clean(),
            "a rapid ploughing through stock must not pass"
        );
        assert_eq!(r.rapid_contacts, 1);
        assert!(
            r.deepest_rapid_contact_mm > 3.0,
            "got {}",
            r.deepest_rapid_contact_mm
        );
    }

    /// And the same move above the block is fine, or the check would be useless.
    #[test]
    fn a_rapid_above_the_stock_is_fine() {
        let mut s = block();
        let moves = vec![
            SimMove {
                to: dvec3(-8.0, 0.0, 20.0),
                class: MoveClass::Rapid,
            },
            SimMove {
                to: dvec3(8.0, 0.0, 20.0),
                class: MoveClass::Rapid,
            },
        ];
        let r = s.apply(&moves, &Flat(2.0));
        assert!(r.is_clean());
        assert_eq!(r.removed_mm3, 0.0);
    }

    #[test]
    fn part_surface_reads_a_plane() {
        let mesh = synthetic::plane(2.0, 10.0);
        let p = PartSurface::from_mesh(&mesh, dvec2(-5.0, -5.0), dvec2(5.0, 5.0), 0.5).unwrap();
        assert!((p.height_at(0.0, 0.0).unwrap() - 2.0).abs() < 1e-9);
        assert!((p.height_at(3.3, -2.1).unwrap() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn part_surface_reads_a_step() {
        let mesh = synthetic::step(4.0, 10.0);
        let p = PartSurface::from_mesh(&mesh, dvec2(-5.0, -5.0), dvec2(5.0, 5.0), 0.25).unwrap();
        assert!(
            (p.height_at(-3.0, 0.0).unwrap() - 4.0).abs() < 1e-9,
            "plateau"
        );
        assert!(p.height_at(3.0, 0.0).unwrap().abs() < 1e-9, "shelf");
    }

    /// Cutting too deep must be detected, and cutting to the right depth must not.
    #[test]
    fn verify_distinguishes_a_gouge_from_a_good_cut() {
        let mesh = synthetic::plane(0.0, 12.0);
        let part = PartSurface::from_mesh(&mesh, dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 0.5).unwrap();

        let sweep = |z: f64| {
            let mut s = Stock::block(dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 5.0, -5.0, 0.5).unwrap();
            let mut moves = Vec::new();
            let mut y = -5.0;
            while y <= 5.0 {
                moves.push(SimMove {
                    to: dvec3(-5.0, y, z),
                    class: MoveClass::MayCut,
                });
                moves.push(SimMove {
                    to: dvec3(5.0, y, z),
                    class: MoveClass::MayCut,
                });
                y += 1.0;
            }
            s.apply(&moves, &Flat(1.5));
            s
        };

        let good = sweep(0.0).verify(&part, 0.01, 0.5);
        assert!(
            good.is_clean(),
            "cutting to the surface is not a gouge: {good:?}"
        );
        assert!(good.compared > 100);

        let bad = sweep(-1.0).verify(&part, 0.01, 0.5);
        assert!(!bad.is_clean(), "cutting 1 mm under the part is a gouge");
        assert!(
            (bad.deepest_gouge_mm - 1.0).abs() < 0.05,
            "got {}",
            bad.deepest_gouge_mm
        );
    }

    /// Uncut material is reported as leftover, not as an error.
    #[test]
    fn untouched_stock_is_leftover_not_a_gouge() {
        let mesh = synthetic::plane(0.0, 12.0);
        let part = PartSurface::from_mesh(&mesh, dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 0.5).unwrap();
        let s = Stock::block(dvec2(-6.0, -6.0), dvec2(6.0, 6.0), 5.0, -5.0, 0.5).unwrap();
        let r = s.verify(&part, 0.01, 0.5);
        assert!(r.is_clean(), "leftover is unfinished, not ruined");
        assert!(r.leftover > 100);
        assert!((r.tallest_leftover_mm - 5.0).abs() < 1e-6);
    }

    #[test]
    fn writes_a_readable_pgm() {
        let s = block();
        let p = s.to_pgm();
        assert!(p.starts_with("P2\n"));
        assert!(p.lines().count() > 3);
    }

    #[test]
    fn survives_non_finite_positions_without_panicking() {
        let mut s = block();
        let moves = vec![
            SimMove {
                to: dvec3(0.0, 0.0, 0.0),
                class: MoveClass::MayCut,
            },
            SimMove {
                to: dvec3(f64::NAN, 0.0, 0.0),
                class: MoveClass::MayCut,
            },
        ];
        let _ = s.apply(&moves, &Flat(1.0));
    }
}
