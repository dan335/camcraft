//! The simulated material as an image: one height and one colour per stock cell.
//!
//! Built on the CPU from the simulator's height field, and uploaded to the GPU as
//! two textures that a static grid is displaced by. So showing a different stage is
//! a texture write, and later, during playback, so is each step: only the cells the
//! tool touched.
//!
//! Every cell is classified by the verifier's own rule, with the verifier's own
//! lookup of the part and its own thresholds (the job's gouge budget and
//! [`LEFTOVER_LIMIT`]). What is red here is exactly what the verdict counted as
//! gouged, and what is amber is what it counted as left over.

use camcraft_core::run::LEFTOVER_LIMIT;
use camcraft_geom::heightmap::narrow;
use camcraft_sim::stock::{PartSurface, Stock};
use glam::DVec2;

use crate::theme;

/// How a column of material compares with the part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Below the part by more than the budget: a ruined part.
    Gouge,
    /// On the part, within the budget and the leftover limit.
    OnPart,
    /// Further above the part than the leftover limit: where the cutter could not
    /// reach, or has not yet.
    Leftover,
    /// Material the part does not cover.
    OffPart,
}

/// Every class, in the order the legend lists them.
pub const CLASSES: [Class; 4] = [Class::Gouge, Class::Leftover, Class::OnPart, Class::OffPart];

/// Where the leftover colour stops getting stronger, in millimetres above the part.
const LEFTOVER_SATURATES: f64 = 5.0;

/// Classify one column: `stock` height against the part's, if it has one.
pub fn classify(stock: f64, part: Option<f64>, budget: f64) -> Class {
    let Some(part) = part else {
        return Class::OffPart;
    };
    let delta = stock - part;
    if delta < -budget {
        Class::Gouge
    } else if delta > LEFTOVER_LIMIT {
        Class::Leftover
    } else {
        Class::OnPart
    }
}

/// A class's name, for the legend.
pub fn class_name(class: Class) -> &'static str {
    match class {
        Class::Gouge => "below the part",
        Class::OnPart => "on the part",
        Class::Leftover => "left above the part",
        Class::OffPart => "outside the part",
    }
}

/// A class's colour for the legend: leftover at its strongest.
pub fn class_color(class: Class) -> [u8; 3] {
    match class {
        Class::Gouge => theme::ERR,
        Class::OnPart => theme::GRAY_8,
        Class::Leftover => theme::AMBER_5,
        Class::OffPart => theme::GRAY_5,
    }
}

/// A cell's colour: its class, with leftover shaded by how much is left.
pub fn cell_color(class: Class, delta: f64) -> [u8; 3] {
    match class {
        Class::Leftover => {
            let t =
                ((delta - LEFTOVER_LIMIT) / (LEFTOVER_SATURATES - LEFTOVER_LIMIT)).clamp(0.0, 1.0);
            let mix = |a: u8, b: u8| {
                let (a, b) = (f64::from(a), f64::from(b));
                // Between two bytes, so within a byte.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = (a + (b - a) * t).round() as u8;
                v
            };
            let ([r0, g0, b0], [r1, g1, b1]) = (theme::AMBER_7, theme::AMBER_5);
            [mix(r0, r1), mix(g0, g1), mix(b0, b1)]
        }
        other => class_color(other),
    }
}

/// The material, ready to upload: a grid of heights and colours.
#[derive(Debug, Clone)]
pub struct StockImage {
    /// Columns.
    pub nx: u32,
    /// Rows.
    pub ny: u32,
    /// Position of cell (0, 0).
    pub origin: DVec2,
    /// Cell size.
    pub pitch: f64,
    /// The bottom of the block, which the skirt walls go down to.
    pub floor: f64,
    /// Height per cell, row by row.
    pub heights: Vec<f32>,
    /// sRGB colour per cell, row by row, with full alpha.
    pub colors: Vec<[u8; 4]>,
    /// Cells per class, in [`CLASSES`] order.
    pub counts: [usize; 4],
}

impl StockImage {
    /// Classify and colour `stock` against `part`, with the job's gouge `budget`.
    pub fn build(stock: &Stock, part: &PartSurface, budget: f64) -> Self {
        let field = stock.field();
        let (nx, ny) = (field.nx(), field.ny());
        let cells = nx as usize * ny as usize;
        let mut heights = Vec::with_capacity(cells);
        let mut colors = Vec::with_capacity(cells);
        let mut counts = [0usize; 4];
        for iy in 0..ny {
            for ix in 0..nx {
                let (Some(p), Some(h)) = (field.position(ix, iy), field.get(ix, iy)) else {
                    heights.push(narrow(stock.floor()));
                    colors.push([0, 0, 0, 255]);
                    continue;
                };
                // The verifier's own lookup, so the counts are its counts.
                let target = part.height_at(p.x, p.y);
                let class = classify(h, target, budget);
                let delta = target.map_or(0.0, |t| h - t);
                if let Some(slot) = CLASSES.iter().position(|c| *c == class)
                    && let Some(n) = counts.get_mut(slot)
                {
                    *n += 1;
                }
                let [r, g, b] = cell_color(class, delta);
                heights.push(narrow(h));
                colors.push([r, g, b, 255]);
            }
        }
        Self {
            nx,
            ny,
            origin: field.origin(),
            pitch: field.pitch(),
            floor: stock.floor(),
            heights,
            colors,
            counts,
        }
    }

    /// Cells of `class`.
    pub fn count(&self, class: Class) -> usize {
        CLASSES
            .iter()
            .position(|c| *c == class)
            .and_then(|i| self.counts.get(i))
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests {
    use super::*;
    use camcraft_session::{load_model, read_job, run};

    #[test]
    fn thresholds_sit_where_the_verifier_puts_them() {
        let budget = 0.45;
        assert_eq!(classify(10.0, None, budget), Class::OffPart);
        assert_eq!(classify(10.0 - 0.45, Some(10.0), budget), Class::OnPart);
        assert_eq!(classify(10.0 - 0.46, Some(10.0), budget), Class::Gouge);
        assert_eq!(
            classify(10.0 + LEFTOVER_LIMIT, Some(10.0), budget),
            Class::OnPart
        );
        assert_eq!(
            classify(10.0 + LEFTOVER_LIMIT + 0.01, Some(10.0), budget),
            Class::Leftover
        );
    }

    #[test]
    fn leftover_deepens_with_what_is_left() {
        let faint = cell_color(Class::Leftover, LEFTOVER_LIMIT + 0.01);
        let strong = cell_color(Class::Leftover, 50.0);
        assert_eq!(
            strong,
            theme::AMBER_5,
            "saturates at the design system's amber"
        );
        let brightness = |c: [u8; 3]| c.iter().map(|v| u32::from(*v)).sum::<u32>();
        assert!(brightness(faint) < brightness(strong));
    }

    /// The picture agrees with the verdict: on a real run, the cells drawn red and
    /// amber are exactly the columns the verifier counted as gouged and left over.
    #[test]
    fn the_picture_counts_what_the_verdict_counts() {
        let dir = std::env::temp_dir().join(format!("camcraft-gui-stock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let job_path = dir.join("job.toml");
        std::fs::write(
            &job_path,
            r#"
tolerance = 0.1
[setup]
stock = { kind = "box", margin = [2.0, 2.0, 2.0] }
[tools.flat]
kind = "flat"
diameter = 6.0
[[operation]]
name = "rough"
strategy = "zlevel-rough"
tool = "flat"
stepdown = 4.0
passes = { stepover = 3.0, feed = 1800.0, stock_to_leave = { radial = 0.3, axial = 0.3 } }
"#,
        )
        .unwrap();
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let job = read_job(&job_path).unwrap();
        let model = load_model(
            &repo.join("fixtures/step/hinge_jig.step"),
            None,
            job.tolerance,
        )
        .unwrap();
        let result = run(&model, &job).unwrap().result;

        let image = StockImage::build(&result.stock, &result.part, result.verify_budget);
        assert_eq!(image.count(Class::Gouge) as u64, result.verify.gouged);
        assert_eq!(image.count(Class::Leftover) as u64, result.verify.leftover);
        assert!(
            result.verify.leftover > 0,
            "a roughing pass that leaves stock should leave some over the limit"
        );
        let compared =
            image.count(Class::Gouge) + image.count(Class::Leftover) + image.count(Class::OnPart);
        assert_eq!(compared as u64, result.verify.compared);
        assert_eq!(image.heights.len(), (image.nx * image.ny) as usize);
        assert_eq!(image.colors.len(), image.heights.len());
    }
}
