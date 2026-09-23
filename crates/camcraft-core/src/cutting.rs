//! Surface speed and chip load: what a cutter's speed and feed mean at its edge.
//!
//! Makers chart their cutters in these, not in rev/min and mm/min, because they
//! hold across diameters: the same surface speed suits a material whatever the
//! cutter's size, and the same chip load suits a cutter whatever it is spun at.
//! Nothing here is kept; each is worked out from a tool's diameter and flutes and
//! a preset's spindle speed and feed, so it cannot go stale.

use std::f64::consts::PI;

/// Surface speed, in metres per minute, of a cutter `diameter` millimetres across
/// at `rpm`.
pub fn surface_speed(diameter: f64, rpm: f64) -> f64 {
    PI * diameter * rpm / 1000.0
}

/// The spindle speed that gives surface speed `vc`, in metres per minute, at
/// `diameter` millimetres.
pub fn rpm_for(diameter: f64, vc: f64) -> f64 {
    vc * 1000.0 / (PI * diameter)
}

/// Chip load, in millimetres per tooth, at `feed` mm/min and `rpm` with `flutes`
/// cutting edges.
pub fn chip_load(feed: f64, rpm: f64, flutes: u32) -> f64 {
    feed / (rpm * f64::from(flutes))
}

/// The feed, in mm/min, that gives chip load `fz` millimetres per tooth at `rpm`
/// with `flutes` cutting edges.
pub fn feed_for(fz: f64, rpm: f64, flutes: u32) -> f64 {
    fz * rpm * f64::from(flutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
    }

    /// A 1/4" cutter at 16000 rev/min, the Bits & Bits up-cut spiral: its chart
    /// gives 1047.2 ft/min, and at 150 in/min with two flutes 0.0046875 in a
    /// tooth.
    #[test]
    fn a_makers_chart_agrees() {
        let d = 6.35;
        let vc = surface_speed(d, 16000.0);
        assert!((vc / 0.3048 - 1047.2).abs() < 0.05, "{vc}");
        let fz = chip_load(150.0 * 25.4, 16000.0, 2);
        assert!(close(fz / 25.4, 0.004_687_5), "{fz}");
    }

    #[test]
    fn each_is_the_other_backwards() {
        for (d, rpm) in [(3.0, 24000.0), (6.35, 16000.0), (12.0, 9000.0)] {
            assert!(close(rpm_for(d, surface_speed(d, rpm)), rpm));
            for flutes in [1, 2, 4] {
                let feed = 2500.0;
                assert!(close(
                    feed_for(chip_load(feed, rpm, flutes), rpm, flutes),
                    feed
                ));
            }
        }
    }
}
