//! Linking: turning a strategy's holes into real traverses.
//!
//! A strategy emits [`MoveRole::Link`] moves meaning "get here, somehow". This
//! resolves each into a retract, a traverse at a safe height, and a plunge. A
//! program that still contains links is not machinable, and [`Toolpath::has_unplanned_links`]
//! makes that checkable rather than assumed.
//!
//! # Clearance is a field, not a plane
//!
//! [`Clearance`] asks for the safe height *over a given traverse*, not for a single
//! number. In 0.1 the only implementation is a constant plane, which is all a flat
//! spoilboard needs. But fixture and clamp avoidance, "keep tool down", and
//! stay-down distance all require the answer to vary with position, and retrofitting
//! that would mean touching every call site. The seam costs nothing now.
//!
//! # Retract, traverse, plunge
//!
//! Deliberately conservative for 0.1. The tool comes straight up out of the cut at
//! feed rate, crosses at the safe height, and descends straight down. Rapid retract
//! from inside material is how tools get snapped, and "keep tool down" optimisation
//! is a later `Toolpath -> Toolpath` transform, not something to improvise here.

use camcraft_geom::heightmap::HeightMap;
use glam::{DVec2, DVec3, dvec3};

use crate::toolpath::{MoveRole, Toolpath, ToolpathError};

/// How high the tool must be to cross safely.
pub trait Clearance {
    /// Safe tool-tip Z for a traverse from `from` to `to` with a tool of the given
    /// radius.
    ///
    /// The radius is supplied so an implementation can account for the tool's
    /// footprint sweeping past an obstacle, not just its axis.
    fn safe_z(&self, from: DVec2, to: DVec2, tool_radius: f64) -> f64;
}

/// A single horizontal plane, high enough to clear everything.
///
/// Sufficient when the only thing to avoid is the part itself on a flat bed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClearancePlane {
    z: f64,
}

impl ClearancePlane {
    /// A plane at absolute height `z`.
    pub fn new(z: f64) -> Result<Self, LinkError> {
        z.is_finite()
            .then_some(Self { z })
            .ok_or(LinkError::BadParameter {
                name: "clearance",
                value: z,
            })
    }

    /// The plane height.
    pub fn z(&self) -> f64 {
        self.z
    }
}

impl Clearance for ClearancePlane {
    fn safe_z(&self, _from: DVec2, _to: DVec2, _tool_radius: f64) -> f64 {
        self.z
    }
}

/// Clearance taken from a height field, plus a margin.
///
/// This is what [`Clearance`] existed for. A plane must clear the tallest thing on
/// the table everywhere, so every traverse flies at the height of the worst obstacle
/// on the part. A field answers per traverse, so the tool stays low where it can.
///
/// The height is sampled **along** the traverse and across the tool's own width,
/// because the cutter is not a point: a rapid whose axis misses a boss can still put
/// the cutter's shoulder through it.
#[derive(Debug, Clone)]
pub struct ClearanceField {
    field: HeightMap,
    margin: f64,
    fallback: f64,
}

impl ClearanceField {
    /// Clear `field` by `margin`, falling back to `fallback` where the field has no
    /// data.
    ///
    /// The fallback matters: an unsampled column is an unknown height, and flying
    /// low over an unknown is exactly the wrong guess.
    pub fn new(field: HeightMap, margin: f64, fallback: f64) -> Result<Self, LinkError> {
        if !margin.is_finite() || margin < 0.0 {
            return Err(LinkError::BadParameter {
                name: "margin",
                value: margin,
            });
        }
        if !fallback.is_finite() {
            return Err(LinkError::BadParameter {
                name: "fallback",
                value: fallback,
            });
        }
        Ok(Self {
            field,
            margin,
            fallback,
        })
    }

    /// Samples taken along a traverse. Fixed, so the answer cannot vary with input.
    const SAMPLES: u32 = 24;
}

impl Clearance for ClearanceField {
    fn safe_z(&self, from: DVec2, to: DVec2, tool_radius: f64) -> f64 {
        let r = if tool_radius.is_finite() && tool_radius > 0.0 {
            tool_radius
        } else {
            0.0
        };
        let mut worst = f64::MIN;
        let mut unknown = false;

        for i in 0..=Self::SAMPLES {
            let t = f64::from(i) / f64::from(Self::SAMPLES);
            let p = from.lerp(to, t);
            // Sample the tool's footprint, not just its axis: a rapid whose centre
            // misses a boss can still catch it with the cutter's shoulder.
            let probes = [
                p,
                p + DVec2::new(r, 0.0),
                p - DVec2::new(r, 0.0),
                p + DVec2::new(0.0, r),
                p - DVec2::new(0.0, r),
            ];
            for q in probes {
                match self.field.sample(q.x, q.y) {
                    Some(h) => worst = worst.max(h),
                    None => unknown = true,
                }
            }
        }

        let base = if worst > f64::MIN { worst } else { f64::MIN };
        let from_field = if base > f64::MIN {
            base + self.margin
        } else {
            f64::MIN
        };
        // Anywhere the field is silent, the fallback governs. Never fly under an
        // unknown.
        if unknown || from_field <= f64::MIN {
            from_field.max(self.fallback)
        } else {
            from_field
        }
    }
}

/// How to resolve a link.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkParams {
    /// Feed rate for coming up out of a cut, in mm/min.
    pub retract_feed: f64,
    /// Feed rate for descending into material, in mm/min.
    pub plunge_feed: f64,
    /// Tool radius, passed to the clearance source.
    pub tool_radius: f64,
}

/// Linking could not proceed.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LinkError {
    /// A parameter was outside its usable range.
    #[error("{name} must be finite and greater than zero, got {value}")]
    BadParameter {
        /// Which parameter.
        name: &'static str,
        /// What it was.
        value: f64,
    },
    /// The clearance source returned a height that is not usable.
    #[error("clearance height is not finite at {at:?}")]
    BadClearance {
        /// Where it was asked about.
        at: DVec2,
    },
    /// A move could not be added.
    #[error(transparent)]
    Toolpath(#[from] ToolpathError),
}

/// What linking did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkReport {
    /// How many links were resolved.
    pub links_resolved: u64,
    /// How many plunges start at or below the surface they descend into.
    ///
    /// Always zero for a plane clearance; meaningful once clearance varies.
    pub suspicious_plunges: u64,
}

/// Replace every [`MoveRole::Link`] with a retract, a traverse and a plunge.
///
/// Cutting moves pass through byte for byte: linking decides how to travel between
/// cuts and must never alter the cuts themselves.
pub fn link<C: Clearance>(
    path: &Toolpath,
    clearance: &C,
    params: &LinkParams,
) -> Result<(Toolpath, LinkReport), LinkError> {
    check("retract_feed", params.retract_feed)?;
    check("plunge_feed", params.plunge_feed)?;
    check("tool_radius", params.tool_radius)?;

    let mut out = Toolpath::new();
    let mut report = LinkReport::default();
    let mut at: Option<DVec3> = None;

    for m in path.moves() {
        if m.role != MoveRole::Link {
            out.push(*m)?;
            at = Some(m.to);
            continue;
        }

        let target = m.to;
        let from_xy = at.map_or(target.truncate(), |p| p.truncate());
        let safe = clearance.safe_z(from_xy, target.truncate(), params.tool_radius);
        if !safe.is_finite() {
            return Err(LinkError::BadClearance { at: from_xy });
        }

        // Straight up out of the cut, at feed. Nothing rapids out of material.
        if let Some(cur) = at
            && safe > cur.z
        {
            out.feed_to(
                dvec3(cur.x, cur.y, safe),
                MoveRole::Retract,
                params.retract_feed,
            )?;
        }

        // Across at the safe height.
        out.rapid_to(dvec3(target.x, target.y, safe))?;

        // Straight down onto the start of the next cut.
        if safe > target.z {
            out.feed_to(target, MoveRole::Plunge, params.plunge_feed)?;
        } else {
            report.suspicious_plunges += 1;
            out.feed_to(target, MoveRole::Plunge, params.plunge_feed)?;
        }

        report.links_resolved += 1;
        at = Some(target);
    }

    Ok((out, report))
}

fn check(name: &'static str, value: f64) -> Result<(), LinkError> {
    (value.is_finite() && value > 0.0)
        .then_some(())
        .ok_or(LinkError::BadParameter { name, value })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
    use super::*;
    use crate::toolpath::Move;
    use glam::dvec2;

    fn params() -> LinkParams {
        LinkParams {
            retract_feed: 800.0,
            plunge_feed: 300.0,
            tool_radius: 3.0,
        }
    }

    /// Two short passes with a link in front of each, as a strategy would emit.
    fn strategy_output() -> Toolpath {
        let mut t = Toolpath::new();
        t.feed_to(dvec3(0.0, 0.0, -2.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 0.0, -2.0), MoveRole::Cut, 1000.0)
            .unwrap();
        t.feed_to(dvec3(5.0, 1.0, -3.0), MoveRole::Link, 1000.0)
            .unwrap();
        t.feed_to(dvec3(0.0, 1.0, -3.0), MoveRole::Cut, 1000.0)
            .unwrap();
        t
    }

    #[test]
    fn rejects_unusable_parameters() {
        let p = ClearancePlane::new(10.0).unwrap();
        for bad in [
            LinkParams {
                retract_feed: 0.0,
                ..params()
            },
            LinkParams {
                plunge_feed: f64::NAN,
                ..params()
            },
            LinkParams {
                tool_radius: -1.0,
                ..params()
            },
        ] {
            assert!(link(&strategy_output(), &p, &bad).is_err());
        }
        assert!(ClearancePlane::new(f64::INFINITY).is_err());
    }

    /// The whole point: a linked path is machinable, and can prove it.
    #[test]
    fn resolves_every_link() {
        let input = strategy_output();
        assert!(input.has_unplanned_links());

        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, report) = link(&input, &plane, &params()).unwrap();

        assert!(!out.has_unplanned_links(), "output must be machinable");
        assert_eq!(report.links_resolved, 2);
    }

    /// Linking decides how to travel between cuts. It must not touch the cuts.
    #[test]
    fn cutting_moves_pass_through_unchanged() {
        let input = strategy_output();
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, _) = link(&input, &plane, &params()).unwrap();

        let before: Vec<Move> = input
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Cut)
            .copied()
            .collect();
        let after: Vec<Move> = out
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Cut)
            .copied()
            .collect();
        assert_eq!(before, after, "cuts were altered by linking");
    }

    /// Every traverse must happen at the safe height, not somewhere convenient.
    #[test]
    fn traverses_happen_at_clearance() {
        let plane = ClearancePlane::new(12.5).unwrap();
        let (out, _) = link(&strategy_output(), &plane, &params()).unwrap();

        let rapids: Vec<&Move> = out
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Rapid)
            .collect();
        assert_eq!(rapids.len(), 2);
        for r in rapids {
            assert_eq!(r.to.z, 12.5, "rapid at {:?} is not at clearance", r.to);
        }
    }

    /// Retracts go straight up and plunges straight down. A diagonal exit drags the
    /// cutter sideways through the wall it just cut.
    #[test]
    fn retracts_and_plunges_are_vertical() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, _) = link(&strategy_output(), &plane, &params()).unwrap();

        let moves = out.moves();
        let mut at = DVec3::ZERO;
        let mut seen_retract = 0;
        let mut seen_plunge = 0;
        for (i, m) in moves.iter().enumerate() {
            if i > 0 {
                match m.role {
                    MoveRole::Retract => {
                        assert_eq!(m.to.truncate(), at.truncate(), "retract moved sideways");
                        assert!(m.to.z > at.z, "retract must go up");
                        seen_retract += 1;
                    }
                    MoveRole::Plunge => {
                        assert_eq!(m.to.truncate(), at.truncate(), "plunge moved sideways");
                        assert!(m.to.z < at.z, "plunge must go down");
                        seen_plunge += 1;
                    }
                    _ => {}
                }
            }
            at = m.to;
        }
        assert_eq!(seen_plunge, 2);
        assert_eq!(seen_retract, 1, "only the second link follows a cut");
    }

    /// Nothing has been cut yet at the first link, so there is nothing to retract
    /// from. Emitting one would be a move from an unknown position.
    #[test]
    fn the_first_link_does_not_retract() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, _) = link(&strategy_output(), &plane, &params()).unwrap();
        let first = out.moves().first().unwrap();
        assert_eq!(
            first.role,
            MoveRole::Rapid,
            "a program must open by moving to clearance, not by retracting from nowhere"
        );
    }

    /// Nothing rapids out of material.
    ///
    /// Counted, not just filtered: an earlier version of this test only iterated
    /// over retract moves and asserted their feed, so replacing the retract with a
    /// rapid left zero of them and the test passed vacuously.
    #[test]
    fn retracts_use_feed_not_rapid() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, _) = link(&strategy_output(), &plane, &params()).unwrap();

        let retracts: Vec<&Move> = out
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Retract)
            .collect();
        assert_eq!(
            retracts.len(),
            1,
            "the second link follows a cut and must retract from it"
        );
        assert_eq!(retracts.first().unwrap().feed, Some(800.0));

        let plunges: Vec<&Move> = out
            .moves()
            .iter()
            .filter(|m| m.role == MoveRole::Plunge)
            .collect();
        assert_eq!(plunges.len(), 2);
        assert!(plunges.iter().all(|m| m.feed == Some(300.0)));
    }

    /// The property behind the previous test, stated directly: a rapid may only
    /// begin once the tool is already clear. Anything else is a rapid out of the
    /// work regardless of how it is labelled.
    #[test]
    fn no_rapid_begins_below_clearance() {
        let z = 10.0;
        let plane = ClearancePlane::new(z).unwrap();
        let (out, _) = link(&strategy_output(), &plane, &params()).unwrap();

        let mut checked = 0;
        for (prev, cur) in out.moves().iter().zip(out.moves().iter().skip(1)) {
            if cur.role != MoveRole::Rapid {
                continue;
            }
            assert!(
                prev.to.z >= z - 1e-9,
                "rapid begins at {:?}, below clearance {z}",
                prev.to
            );
            checked += 1;
        }
        assert!(checked > 0, "expected at least one rapid after a retract");
    }

    /// A clearance below the work cannot produce a safe plunge, and the report has
    /// to say so rather than quietly emitting it.
    #[test]
    fn a_clearance_below_the_work_is_reported() {
        let plane = ClearancePlane::new(-50.0).unwrap();
        let (_, report) = link(&strategy_output(), &plane, &params()).unwrap();
        assert_eq!(report.suspicious_plunges, 2);
    }

    /// The clearance source is asked about the traverse, not about a fixed height.
    /// This is what lets a varying field drop in later without touching callers.
    #[test]
    fn clearance_is_queried_per_traverse() {
        struct Recording(std::cell::RefCell<Vec<(DVec2, DVec2, f64)>>);
        impl Clearance for Recording {
            fn safe_z(&self, from: DVec2, to: DVec2, r: f64) -> f64 {
                self.0.borrow_mut().push((from, to, r));
                20.0
            }
        }
        let rec = Recording(std::cell::RefCell::new(Vec::new()));
        let (_, _) = link(&strategy_output(), &rec, &params()).unwrap();

        let calls = rec.0.borrow();
        assert_eq!(calls.len(), 2, "one query per link");
        let second = calls.get(1).unwrap();
        assert_eq!(
            second.0,
            dvec2(5.0, 0.0),
            "asked from the end of the last cut"
        );
        assert_eq!(second.1, dvec2(5.0, 1.0), "asked to the start of the next");
        assert_eq!(second.2, 3.0, "tool radius is passed through");
    }

    fn stepped_field() -> HeightMap {
        // Flat at 0, with a 20 mm boss in the middle.
        let mut f = HeightMap::filled(dvec2(-10.0, -10.0), 1.0, 21, 21, 0.0).unwrap();
        for iy in 8..13 {
            for ix in 8..13 {
                f.set(ix, iy, 20.0);
            }
        }
        f
    }

    #[test]
    fn rejects_an_unusable_field() {
        let f = stepped_field();
        assert!(ClearanceField::new(f.clone(), -1.0, 0.0).is_err());
        assert!(ClearanceField::new(f, 5.0, f64::NAN).is_err());
    }

    /// The whole point of a field over a plane: stay low where it is safe to.
    #[test]
    fn a_traverse_clear_of_the_boss_flies_low() {
        let c = ClearanceField::new(stepped_field(), 5.0, 0.0).unwrap();
        let away = c.safe_z(dvec2(-9.0, -9.0), dvec2(-7.0, -9.0), 0.5);
        assert!(
            (away - 5.0).abs() < 1e-9,
            "clear of the boss it should be flat + margin, got {away}"
        );
    }

    /// And rise where it is not.
    #[test]
    fn a_traverse_over_the_boss_rises() {
        let c = ClearanceField::new(stepped_field(), 5.0, 0.0).unwrap();
        let over = c.safe_z(dvec2(-9.0, 0.0), dvec2(9.0, 0.0), 0.5);
        assert!(
            (over - 25.0).abs() < 1e-9,
            "crossing the boss it should be 20 + margin, got {over}"
        );
    }

    /// The cutter is not a point. A rapid whose axis misses a boss can still put
    /// the tool's shoulder through it.
    #[test]
    fn the_tool_width_is_accounted_for() {
        let c = ClearanceField::new(stepped_field(), 5.0, 0.0).unwrap();
        // The boss spans cell centres -2..2. A traverse at y = -4 clears it by 2 mm
        // on the axis, but a 3 mm cutter does not.
        let thin = c.safe_z(dvec2(-9.0, -4.0), dvec2(9.0, -4.0), 0.1);
        let wide = c.safe_z(dvec2(-9.0, -4.0), dvec2(9.0, -4.0), 3.0);
        assert!(
            thin < 25.0,
            "a thin tool passes beside the boss, got {thin}"
        );
        assert!(
            (wide - 25.0).abs() < 1e-9,
            "a wide tool must clear it, got {wide}"
        );
    }

    /// An unsampled column is an unknown height. Flying low over an unknown is
    /// exactly the wrong guess.
    #[test]
    fn unknown_ground_falls_back_rather_than_flying_low() {
        let c = ClearanceField::new(stepped_field(), 5.0, 99.0).unwrap();
        let outside = c.safe_z(dvec2(500.0, 500.0), dvec2(520.0, 500.0), 1.0);
        assert_eq!(outside, 99.0, "off the field, the fallback governs");
    }

    /// Fields and planes must be interchangeable at the call site, since that is the
    /// entire reason clearance is a trait.
    #[test]
    fn a_field_substitutes_for_a_plane() {
        let field = ClearanceField::new(stepped_field(), 5.0, 0.0).unwrap();
        let (out, report) = link(&strategy_output(), &field, &params()).unwrap();
        assert!(!out.has_unplanned_links());
        assert_eq!(report.links_resolved, 2);
    }

    #[test]
    fn an_empty_path_links_to_an_empty_path() {
        let plane = ClearancePlane::new(10.0).unwrap();
        let (out, report) = link(&Toolpath::new(), &plane, &params()).unwrap();
        assert!(out.is_empty());
        assert_eq!(report, LinkReport::default());
    }
}
