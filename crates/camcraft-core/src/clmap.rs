//! Cached cutter-location maps.
//!
//! # There is no single `Zcl`
//!
//! A CL map is a function of **(effective tool profile, grid pitch, region)**.
//! Roughing with a 6 mm flat and finishing with a 3 mm ball are two different
//! surfaces over the same part. Treating `Zcl` as one global field is a latent bug,
//! so maps carry a [`ClMapKey`] and are looked up by it.
//!
//! # The grid is an index, not an answer
//!
//! Nodes are filled from the exact [`DropCutter`] query, so at a node the grid *is*
//! the truth. Between nodes it is an interpolation, and interpolation dips below a
//! convex surface — which is a gouge. Path points come from the exact query; the grid
//! answers areal questions like "which region is above this level".
//!
//! # Determinism
//!
//! The fill is parallel over rows, each row writing only its own slice, and every
//! cell is an independent exact query. No reduction, no shared accumulator, so the
//! result is bit-identical at any thread count.

use std::collections::HashMap;
use std::sync::Arc;

use camcraft_geom::heightmap::{HeightMap, HeightMapError, narrow};
use glam::DVec2;
use rayon::prelude::*;

use crate::dropcutter::{DropCutter, DropTest};
use crate::tool::ToolKey;

/// Identifies a CL map exactly: which tool, at what resolution, over what region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClMapKey {
    /// The cutter geometry this map was computed for.
    pub tool: ToolKey,
    /// Grid pitch, as an exact bit pattern.
    pub pitch: u64,
    /// Region lower corner, as exact bit patterns.
    pub min: [u64; 2],
    /// Region upper corner, as exact bit patterns.
    pub max: [u64; 2],
}

impl ClMapKey {
    fn new(tool: ToolKey, min: DVec2, max: DVec2, pitch: f64) -> Self {
        Self {
            tool,
            pitch: pitch.to_bits(),
            min: [min.x.to_bits(), min.y.to_bits()],
            max: [max.x.to_bits(), max.y.to_bits()],
        }
    }
}

/// A cutter-location surface sampled on a regular grid.
#[derive(Debug, Clone)]
pub struct ClMap {
    key: ClMapKey,
    grid: HeightMap,
}

/// A CL map that could not be computed.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ClMapError {
    /// The requested grid was unusable.
    #[error("cannot build the sampling grid: {0}")]
    Grid(#[from] HeightMapError),
}

impl ClMap {
    /// Sample the CL surface for `tool` over `[min, max]` at the given pitch.
    ///
    /// Cells with no geometry beneath them are left unset rather than zeroed, so
    /// "open stock" and "the surface is at Z=0" stay distinguishable.
    pub fn compute<T: DropTest + Sync>(
        cutter: &DropCutter<'_>,
        tool: &T,
        min: DVec2,
        max: DVec2,
        pitch: f64,
    ) -> Result<Self, ClMapError> {
        let mut grid = HeightMap::covering(min, max, pitch, f64::NAN)?;
        let origin = grid.origin();
        let nx = grid.nx() as usize;
        let ny = grid.ny();

        grid.data_mut().par_chunks_mut(nx).zip(0..ny).for_each_init(
            Vec::new,
            |scratch, (row, iy)| {
                let y = origin.y + f64::from(iy) * pitch;
                for (cell, ix) in row.iter_mut().zip(0u32..) {
                    // Positions must match HeightMap::position exactly, or the grid
                    // would disagree with the exact query at its own nodes.
                    let x = origin.x + f64::from(ix) * pitch;
                    *cell = match cutter.drop_at_with(tool, x, y, scratch) {
                        Some(z) => narrow(z),
                        None => f32::NAN,
                    };
                }
            },
        );

        Ok(Self {
            key: ClMapKey::new(tool.geometry_key(), min, max, pitch),
            grid,
        })
    }

    /// What this map was computed for.
    pub fn key(&self) -> &ClMapKey {
        &self.key
    }

    /// The sampled grid.
    pub fn grid(&self) -> &HeightMap {
        &self.grid
    }
}

/// Grid pitch at which bilinear interpolation stays within `tolerance`.
///
/// Linear interpolation of a surface with curvature `1/R` over a span `h` errs by at
/// most `h^2 / (8R)`; across the diagonal of a bilinear cell the span is `h * sqrt(2)`,
/// giving `h^2 / (4R)`. Requiring that to stay within `tolerance`:
///
/// ```text
/// pitch <= 2 * sqrt(R * tolerance)
/// ```
///
/// `min_convex_radius` comes from [`ToolProfile::min_convex_radius`]. Only *convex*
/// curvature matters: there the interpolant passes below the true surface, which
/// gouges. In concave regions it passes above, which merely leaves material.
///
/// Returns `None` when the tool has no such bound — a flat mill crossing a step
/// leaves a genuine slope discontinuity, so no pitch makes interpolation accurate.
/// Size that grid from how precisely you need region boundaries instead, and never
/// interpolate a path point from it.
///
/// [`ToolProfile::min_convex_radius`]: crate::tool::ToolProfile::min_convex_radius
pub fn pitch_for_tolerance(tolerance: f64, min_convex_radius: Option<f64>) -> Option<f64> {
    let r = min_convex_radius?;
    (tolerance.is_finite() && tolerance > 0.0 && r.is_finite() && r > 0.0)
        .then(|| 2.0 * (r * tolerance).sqrt())
}

/// Memoises CL maps by [`ClMapKey`].
///
/// Sampling a dense grid is the expensive part of every 3D strategy, and successive
/// operations routinely ask for the same one. Keyed rather than global, so two tools
/// cannot silently share a surface.
#[derive(Debug, Default)]
pub struct ClMapCache {
    entries: HashMap<ClMapKey, Arc<ClMap>>,
}

impl ClMapCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many maps are held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache holds nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Fetch the map for these parameters, computing it if absent.
    pub fn get_or_compute<T: DropTest + Sync>(
        &mut self,
        cutter: &DropCutter<'_>,
        tool: &T,
        min: DVec2,
        max: DVec2,
        pitch: f64,
    ) -> Result<Arc<ClMap>, ClMapError> {
        let key = ClMapKey::new(tool.geometry_key(), min, max, pitch);
        if let Some(hit) = self.entries.get(&key) {
            return Ok(Arc::clone(hit));
        }
        let map = Arc::new(ClMap::compute(cutter, tool, min, max, pitch)?);
        self.entries.insert(key, Arc::clone(&map));
        Ok(map)
    }

    /// Drop everything, for when the model or setup changes.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use crate::tool::{Ball, Flat, ToolProfile};
    use camcraft_geom::synthetic;
    use glam::dvec2;

    const LO: DVec2 = DVec2::new(-6.0, -6.0);
    const HI: DVec2 = DVec2::new(6.0, 6.0);

    /// The invariant tying the two representations together: at a node, the grid is
    /// the exact query. Anything else means the cached layer has drifted from truth.
    #[test]
    fn nodes_match_the_exact_query() {
        let mesh = synthetic::uv_sphere(10.0, 60, 90);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(2.0).unwrap();
        let map = ClMap::compute(&dc, &tool, LO, HI, 0.5).unwrap();
        let grid = map.grid();

        let mut checked = 0;
        for iy in 0..grid.ny() {
            for ix in 0..grid.nx() {
                let p = grid.position(ix, iy).unwrap();
                let exact = dc.drop_at(&tool, p.x, p.y);
                match (grid.get(ix, iy), exact) {
                    (Some(g), Some(e)) => {
                        assert!(
                            (g - e).abs() <= 1e-5 * e.abs().max(1.0),
                            "node ({ix},{iy}) grid {g} vs exact {e}"
                        );
                        checked += 1;
                    }
                    (None, None) => {}
                    (g, e) => panic!("node ({ix},{iy}) disagrees on coverage: {g:?} vs {e:?}"),
                }
            }
        }
        assert!(checked > 100, "expected real coverage, checked {checked}");
    }

    /// Between nodes, interpolation error must stay inside the bound that
    /// `pitch_for_tolerance` was derived from — otherwise that derivation is fiction.
    #[test]
    fn interpolation_stays_within_the_derived_bound() {
        let tolerance = 0.02;
        let tool = Ball::new(2.0).unwrap();
        let pitch = pitch_for_tolerance(tolerance, tool.min_convex_radius()).unwrap();

        let mesh = synthetic::uv_sphere(10.0, 240, 360);
        let dc = DropCutter::new(&mesh);
        let map = ClMap::compute(&dc, &tool, LO, HI, pitch).unwrap();
        let grid = map.grid();

        let mut worst: f64 = 0.0;
        let mut samples = 0;
        for i in 0..40 {
            for j in 0..40 {
                // Deliberately off-node, where interpolation actually does work.
                let x = -5.0 + f64::from(i) * 0.25 + pitch * 0.37;
                let y = -5.0 + f64::from(j) * 0.25 + pitch * 0.61;
                if let (Some(g), Some(e)) = (grid.sample(x, y), dc.drop_at(&tool, x, y)) {
                    worst = worst.max((g - e).abs());
                    samples += 1;
                }
            }
        }
        assert!(samples > 500, "expected real coverage, got {samples}");
        // Tessellation sag rides on top of the interpolation bound.
        let budget = tolerance + synthetic::uv_sphere_max_sag(10.0, 240, 360) + 1e-9;
        assert!(
            worst <= budget,
            "worst interpolation error {worst} over budget {budget}"
        );
    }

    #[test]
    fn pitch_is_derived_only_where_a_bound_exists() {
        let ball = Ball::new(3.0).unwrap();
        let p = pitch_for_tolerance(0.01, ball.min_convex_radius()).unwrap();
        assert!((p - 2.0 * 0.03_f64.sqrt()).abs() < 1e-12);
        // Finer tolerance demands a finer grid.
        assert!(pitch_for_tolerance(0.001, ball.min_convex_radius()).unwrap() < p);

        // A flat mill leaves sharp convex edges, so no such pitch exists.
        let flat = Flat::new(3.0).unwrap();
        assert_eq!(flat.min_convex_radius(), None);
        assert_eq!(pitch_for_tolerance(0.01, flat.min_convex_radius()), None);

        // Nonsense inputs do not produce a nonsense grid.
        assert_eq!(pitch_for_tolerance(0.0, Some(1.0)), None);
        assert_eq!(pitch_for_tolerance(f64::NAN, Some(1.0)), None);
        assert_eq!(pitch_for_tolerance(0.01, Some(-1.0)), None);
    }

    #[test]
    fn open_stock_stays_unset_rather_than_zero() {
        let mesh = synthetic::plane(3.0, 2.0);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(0.5).unwrap();
        let map = ClMap::compute(&dc, &tool, dvec2(-9.0, -9.0), dvec2(9.0, 9.0), 1.0).unwrap();
        let grid = map.grid();
        assert_eq!(grid.get(0, 0), None, "corner is off the plane entirely");
        let mid = grid.sample(0.0, 0.0).unwrap();
        assert!(
            (mid - 3.0).abs() < 1e-6,
            "over the plane we get its height, got {mid}"
        );
    }

    /// Each row writes only its own slice and every cell is an independent query, so
    /// thread count cannot change a single bit.
    #[test]
    fn fill_is_bit_identical_regardless_of_threads() {
        let mesh = synthetic::uv_sphere(8.0, 50, 70);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(1.5).unwrap();

        let run = |threads: usize| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(|| ClMap::compute(&dc, &tool, LO, HI, 0.3).unwrap())
        };
        let a = run(1);
        let b = run(8);
        assert_eq!(a.grid().data().len(), b.grid().data().len());
        for (i, (x, y)) in a.grid().data().iter().zip(b.grid().data()).enumerate() {
            assert!(
                x.to_bits() == y.to_bits(),
                "cell {i} differs between 1 and 8 threads: {x} vs {y}"
            );
        }
    }

    #[test]
    fn cache_reuses_by_key_and_separates_by_tool() {
        let mesh = synthetic::plane(1.0, 20.0);
        let dc = DropCutter::new(&mesh);
        let ball = Ball::new(2.0).unwrap();
        let other = Ball::new(4.0).unwrap();
        let mut cache = ClMapCache::new();
        assert!(cache.is_empty());

        let first = cache.get_or_compute(&dc, &ball, LO, HI, 1.0).unwrap();
        let again = cache.get_or_compute(&dc, &ball, LO, HI, 1.0).unwrap();
        assert!(Arc::ptr_eq(&first, &again), "same key must hit the cache");
        assert_eq!(cache.len(), 1);

        // A different tool is a different surface, never a cache hit.
        let bigger = cache.get_or_compute(&dc, &other, LO, HI, 1.0).unwrap();
        assert!(!Arc::ptr_eq(&first, &bigger));
        assert_eq!(cache.len(), 2);
        assert_ne!(first.key(), bigger.key());

        // So is a different pitch, or a different region.
        cache.get_or_compute(&dc, &ball, LO, HI, 0.5).unwrap();
        cache
            .get_or_compute(&dc, &ball, LO, dvec2(7.0, 7.0), 1.0)
            .unwrap();
        assert_eq!(cache.len(), 4);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn refuses_an_absurd_grid_rather_than_exhausting_memory() {
        let mesh = synthetic::plane(0.0, 1.0);
        let dc = DropCutter::new(&mesh);
        let tool = Ball::new(1.0).unwrap();
        let err = ClMap::compute(&dc, &tool, dvec2(-500.0, -500.0), dvec2(500.0, 500.0), 1e-4);
        assert!(matches!(
            err,
            Err(ClMapError::Grid(HeightMapError::TooLarge { .. }))
        ));
    }
}
