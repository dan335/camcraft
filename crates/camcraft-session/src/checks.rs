//! What must be true before a program is run, checked on the program itself.
//!
//! One list, shared by every front end: the command line prints it when it refuses
//! to post, and the app shows it beside the path. The gate ([`crate::gate`]) is
//! "no failures here", so what the list calls a failure and what refuses to post
//! are the same thing by construction.
//!
//! # Re-derived, not echoed
//!
//! Where it can, a check looks at the path and the part, not at what the strategy
//! or the linker said about their own output. A check that only repeated the
//! producer's opinion would agree with it even when both were wrong. The rapid
//! check drops the job's cutter along every rapid onto the exact mesh; it does not
//! consult the clearance the linker planned with. Only the simulator's verdicts,
//! which need the material as it stood, are reported as the simulator gave them.
//!
//! # Codes
//!
//! `CC1xx` are failures: the program must not be run. `CC2xx` are advisories: look,
//! then decide. The codes are kept stable, so that anything written about them
//! still reads right.

use camcraft_core::allowance::{Allowance, EffectiveTool};
use camcraft_core::dropcutter::{DropCutter, DropTest};
use camcraft_core::job::Job;
use camcraft_core::run::{OperationResult, StrategyDetail};
use camcraft_core::tool::ToolProfile;
use camcraft_core::toolpath::{MoveRole, PLUNGE_RATIO, Toolpath};
use camcraft_geom::mesh::TriMesh;
use camcraft_sim::stock::VerifyReport;
use glam::DVec3;
use rayon::prelude::*;

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// The program must not be run.
    Fail,
    /// Worth a look before deciding.
    Warn,
    /// For information: expected on many parts.
    Note,
    /// A failure checked for and not found.
    Pass,
}

/// One thing found.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// How serious it is.
    pub level: Level,
    /// Its code, `CC101` and so on.
    pub code: &'static str,
    /// What was found, in a sentence.
    pub message: String,
    /// Supporting numbers, when there are any.
    pub detail: String,
    /// The move it is at, as an index into its operation's path, when it is at one.
    pub at_move: Option<usize>,
}

/// Findings for one operation, or for the whole setup.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The operation's index, or `None` for the setup as a whole.
    pub op: Option<usize>,
    /// Its name.
    pub title: String,
    /// What was found, failures first.
    pub findings: Vec<Finding>,
}

/// Everything checked for one setup's run.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Checks {
    /// The setup first, then each operation in order.
    pub groups: Vec<Group>,
}

impl Checks {
    /// Every failure, with the group it is in.
    pub fn failures(&self) -> impl Iterator<Item = (&Group, &Finding)> {
        self.groups
            .iter()
            .flat_map(|g| g.findings.iter().map(move |f| (g, f)))
            .filter(|(_, f)| f.level == Level::Fail)
    }

    /// Whether nothing failed.
    pub fn passed(&self) -> bool {
        self.failures().next().is_none()
    }

    /// Every failure as a line of text, for a log.
    pub fn failure_lines(&self) -> Vec<String> {
        self.failures()
            .map(|(g, f)| {
                let detail = if f.detail.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", f.detail)
                };
                format!("{} {}: {}{detail}", f.code, g.title, f.message)
            })
            .collect()
    }
}

/// Check one setup's run.
///
/// `part` is the part as the setup machined it, in the setup's frame; `setup` is
/// its index in `job`; `model_warnings` are the models' own, reported as they are.
pub fn check(
    job: &Job,
    setup: usize,
    part: &TriMesh,
    operations: &[OperationResult],
    verify: &VerifyReport,
    verify_budget: f64,
    model_warnings: &[String],
) -> Checks {
    let mut groups = Vec::with_capacity(operations.len() + 1);
    groups.push(Group {
        op: None,
        title: job
            .setups
            .get(setup)
            .map_or_else(|| "setup".to_string(), |s| format!("setup {}", s.name)),
        findings: sorted(setup_findings(verify, verify_budget, model_warnings)),
    });
    let ops = job.setups.get(setup).map(|s| &s.operations);
    let cutter = DropCutter::new(part);
    // Only what has paths: an operation left out of the run has nothing to check.
    let per_op: Vec<Group> = operations
        .par_iter()
        .enumerate()
        .filter(|(_, op)| op.made.has_paths())
        .map(|(i, op)| {
            let tool = ops
                .and_then(|o| o.get(i))
                .and_then(|spec| job.tool_for(spec))
                .and_then(|t| t.build().ok())
                .and_then(|t| EffectiveTool::of(&t, Allowance::none()).ok());
            Group {
                op: Some(i),
                title: format!("{}. {}", i + 1, op.name),
                findings: sorted(operation_findings(
                    op,
                    tool.as_ref(),
                    &cutter,
                    job.tolerance,
                )),
            }
        })
        .collect();
    groups.extend(per_op);
    Checks { groups }
}

fn sorted(mut findings: Vec<Finding>) -> Vec<Finding> {
    // Stable, so checks of one level keep their code order.
    findings.sort_by_key(|f| f.level);
    findings
}

fn finding(level: Level, code: &'static str, message: String, detail: String) -> Finding {
    Finding {
        level,
        code,
        message,
        detail,
        at_move: None,
    }
}

fn setup_findings(verify: &VerifyReport, budget: f64, model_warnings: &[String]) -> Vec<Finding> {
    let mut out = Vec::new();
    out.push(if verify.gouged == 0 {
        finding(
            Level::Pass,
            "CC107",
            "simulator: finished stock is nowhere below the part".into(),
            format!(
                "{} column(s) compared, budget {budget:.3} mm",
                thousands(verify.compared)
            ),
        )
    } else {
        finding(
            Level::Fail,
            "CC107",
            format!(
                "simulator: finished stock is below the part in {} column(s)",
                thousands(verify.gouged)
            ),
            format!("deepest {:.3} mm", verify.deepest_gouge_mm),
        )
    });
    if verify.leftover > 0 {
        out.push(finding(
            Level::Note,
            "CC206",
            format!(
                "{} column(s) left more than 1 mm above the part",
                thousands(verify.leftover)
            ),
            format!(
                "tallest {:.2} mm: where the cutter could not reach",
                verify.tallest_leftover_mm
            ),
        ));
    }
    for w in model_warnings {
        out.push(finding(Level::Warn, "CC299", w.clone(), String::new()));
    }
    out
}

fn operation_findings(
    op: &OperationResult,
    tool: Option<&EffectiveTool>,
    cutter: &DropCutter<'_>,
    tolerance: f64,
) -> Vec<Finding> {
    let path = &op.path;
    let moves = path.moves();
    let start = moves.first().map_or(DVec3::ZERO, |m| m.to);
    let mut out = Vec::new();

    // CC101: a link is a hole the linker never filled.
    let links: Vec<usize> = moves
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role.is_unplanned())
        .map(|(i, _)| i)
        .collect();
    out.push(match links.first() {
        None => finding(
            Level::Pass,
            "CC101",
            "no unresolved links".into(),
            String::new(),
        ),
        Some(&i) => Finding {
            at_move: Some(i),
            ..finding(
                Level::Fail,
                "CC101",
                format!(
                    "{} unresolved link(s): the path is not machinable",
                    links.len()
                ),
                String::new(),
            )
        },
    });

    // CC102: the cutter dropped along every rapid, onto the part itself.
    match tool {
        Some(tool) => out.push(rapids_through_part(path, start, tool, cutter, tolerance)),
        None => out.push(finding(
            Level::Fail,
            "CC102",
            "the operation's tool could not be built, so its rapids cannot be checked".into(),
            String::new(),
        )),
    }

    // CC103: going in and coming out are straight down and straight up. Sideways,
    // the cutter drags through the wall it has just cut.
    let mut sideways = Vec::new();
    let mut at = start;
    for (i, m) in moves.iter().enumerate() {
        if matches!(m.role, MoveRole::Plunge | MoveRole::Retract)
            && (m.to.truncate() - at.truncate()).length() > 1e-6
        {
            sideways.push(i);
        }
        at = m.to;
    }
    out.push(match sideways.first() {
        None => finding(
            Level::Pass,
            "CC103",
            "plunges and retracts are vertical".into(),
            String::new(),
        ),
        Some(&i) => Finding {
            at_move: Some(i),
            ..finding(
                Level::Fail,
                "CC103",
                format!(
                    "{} plunge or retract move(s) travel sideways",
                    sideways.len()
                ),
                "drags the cutter through the wall it just cut".into(),
            )
        },
    });

    // CC104: every move that feeds has a feed, and no rapid carries one.
    let bad_feed: Vec<usize> = moves
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            let feeds = m.feed.is_some_and(|f| f.is_finite() && f > 0.0);
            if m.role == MoveRole::Rapid {
                m.feed.is_some()
            } else {
                !feeds
            }
        })
        .map(|(i, _)| i)
        .collect();
    out.push(match bad_feed.first() {
        None => finding(
            Level::Pass,
            "CC104",
            "feed rates are consistent with move roles".into(),
            String::new(),
        ),
        Some(&i) => Finding {
            at_move: Some(i),
            ..finding(
                Level::Fail,
                "CC104",
                format!(
                    "{} move(s) with a feed that does not suit their role",
                    bad_feed.len()
                ),
                "a feeding move without a feed, or a rapid with one".into(),
            )
        },
    });

    // CC105: a plunge at cutting feed, measured as one continuous descent.
    let allowance = op.max_cut_step;
    let deepest = path
        .steep_descents(start, PLUNGE_RATIO)
        .into_iter()
        .filter(|d| d.depth > allowance)
        .max_by(|a, b| a.depth.total_cmp(&b.depth));
    out.push(match deepest {
        None => finding(
            Level::Pass,
            "CC105",
            "no cutting descent plunges past the allowance".into(),
            format!("{allowance:.2} mm allowed"),
        ),
        Some(d) => Finding {
            at_move: Some(d.last),
            ..finding(
                Level::Fail,
                "CC105",
                format!(
                    "a steep cutting descent of {:.2} mm exceeds the {allowance:.2} mm \
                     allowance",
                    d.depth
                ),
                "a plunge at cutting feed breaks cutters".into(),
            )
        },
    });

    // CC106: the simulator's own verdict, which needs the material as it stood.
    out.push(if op.sim.rapid_contacts == 0 {
        finding(
            Level::Pass,
            "CC106",
            "simulator: no rapid touched material".into(),
            String::new(),
        )
    } else {
        finding(
            Level::Fail,
            "CC106",
            format!(
                "simulator: {} rapid(s) pass through material",
                op.sim.rapid_contacts
            ),
            format!("deepest {:.2} mm", op.sim.deepest_rapid_contact_mm),
        )
    });

    // Advisories.
    match &op.detail {
        StrategyDetail::Parallel(r) => {
            if r.refinement_limited > 0 {
                out.push(finding(
                    Level::Warn,
                    "CC201",
                    format!("{} span(s) hit the refinement limit", r.refinement_limited),
                    "the path is coarser than the requested tolerance".into(),
                ));
            }
            if r.discontinuities > 0 {
                out.push(finding(
                    Level::Note,
                    "CC203",
                    format!(
                        "{} span(s) stopped at a surface discontinuity",
                        r.discontinuities
                    ),
                    "expected wherever the part has vertical walls".into(),
                ));
            }
            if r.wall_breaks > 0 {
                out.push(finding(
                    Level::Note,
                    "CC204",
                    format!(
                        "{} pass(es) broken at a wall or a steep face",
                        r.wall_breaks
                    ),
                    "each a plunge turned into a retract and a controlled re-entry".into(),
                ));
            }
        }
        StrategyDetail::ZlevelRough(r) => {
            if r.rings == 0 {
                out.push(finding(
                    Level::Warn,
                    "CC205",
                    "roughing found nothing to clear".into(),
                    "the stock may already be below this operation's top".into(),
                ));
            }
        }
        StrategyDetail::Profile(r) | StrategyDetail::Pocket(r) | StrategyDetail::Face(r) => {
            if r.too_shallow > 0 {
                out.push(finding(
                    Level::Warn,
                    "CC207",
                    format!(
                        "{} area(s) not cut: the floor is not below the top",
                        r.too_shallow
                    ),
                    "set the bottom lower, or the top higher".into(),
                ));
            }
        }
        // A hole the tool does not fit is among the run's own warnings.
        StrategyDetail::Drill(_) => {}
    }
    if op.links.suspicious_plunges > 0 {
        out.push(finding(
            Level::Warn,
            "CC202",
            format!(
                "{} plunge(s) do not start above the surface",
                op.links.suspicious_plunges
            ),
            String::new(),
        ));
    }
    out
}

/// CC102. The cutter, lowered onto the part at points along each rapid, must sit
/// at or below the rapid.
///
/// Points are spaced `2 * sqrt(2 R t)` apart for a cutter of radius `R` and a
/// tolerance `t`. The cutter's discs at neighbouring points then overlap so that
/// what lies between them, at the edge of the swept band, is no wider than `t`: a
/// feature must be narrower than the tolerance to be missed.
fn rapids_through_part(
    path: &Toolpath,
    start: DVec3,
    tool: &EffectiveTool,
    cutter: &DropCutter<'_>,
    tolerance: f64,
) -> Finding {
    let r = tool.max_radius();
    let spacing = if r > tolerance {
        2.0 * (2.0 * r * tolerance).sqrt()
    } else {
        tolerance
    }
    .max(1e-3);
    let mut hits = 0u64;
    let mut deepest = 0.0_f64;
    let mut first = None;
    let mut at = start;
    for (i, m) in path.moves().iter().enumerate() {
        if m.role == MoveRole::Rapid {
            let over = rapid_depth(at, m.to, tool, cutter, spacing);
            if over > 1e-3 {
                hits += 1;
                deepest = deepest.max(over);
                first.get_or_insert(i);
            }
        }
        at = m.to;
    }
    if hits == 0 {
        finding(
            Level::Pass,
            "CC102",
            "no rapid passes through the part".into(),
            format!(
                "cutter ⌀{:.2} mm lowered every {spacing:.2} mm along each rapid",
                2.0 * r
            ),
        )
    } else {
        Finding {
            at_move: first,
            ..finding(
                Level::Fail,
                "CC102",
                format!("{hits} rapid(s) pass through the part"),
                format!("deepest {deepest:.2} mm under the cutter"),
            )
        }
    }
}

/// How far the part stands above the cutter anywhere along one rapid.
fn rapid_depth(
    a: DVec3,
    b: DVec3,
    tool: &impl DropTest,
    cutter: &DropCutter<'_>,
    spacing: f64,
) -> f64 {
    let length = (b.truncate() - a.truncate()).length();
    let steps = (length / spacing).ceil().clamp(1.0, 1.0e6);
    // Clamped, so the narrowing is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = steps as u32;
    (0..=steps)
        .filter_map(|k| {
            let p = a.lerp(b, f64::from(k) / f64::from(steps));
            cutter.drop_at(tool, p.x, p.y).map(|z| z - p.z)
        })
        .fold(0.0, f64::max)
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::test_support::{recheck, run_small};

    fn failed(checks: &Checks) -> Vec<&'static str> {
        checks.failures().map(|(_, f)| f.code).collect()
    }

    /// A clean run passes every failure check, and says so check by check.
    #[test]
    fn a_clean_run_passes_its_checks() {
        let ran = run_small();
        assert!(ran.checks.passed(), "{:?}", ran.checks.failure_lines());
        let passed: Vec<&str> = ran
            .checks
            .groups
            .iter()
            .flat_map(|g| &g.findings)
            .filter(|f| f.level == Level::Pass)
            .map(|f| f.code)
            .collect();
        for code in [
            "CC101", "CC102", "CC103", "CC104", "CC105", "CC106", "CC107",
        ] {
            assert!(passed.contains(&code), "{code} not reported as passed");
        }
    }

    /// Each failure the checks exist for is found, and found at its move, from the
    /// path alone: the engine's own reports still say all is well.
    #[test]
    fn the_checks_find_what_is_wrong_with_a_path() {
        use camcraft_core::toolpath::MoveRole;
        use glam::dvec3;
        let ran = run_small();
        let b = ran.part.bounds().unwrap();
        let mid = b.center();

        // A rapid straight through the middle of the part.
        let c = recheck(&ran, |p| {
            p.rapid_to(dvec3(b.min.x - 10.0, mid.y, mid.z)).unwrap();
            p.rapid_to(dvec3(b.max.x + 10.0, mid.y, mid.z)).unwrap();
        });
        assert_eq!(failed(&c), ["CC102"]);
        // At one of the two added rapids: the first also cuts through the part on
        // its way down.
        let at = c.failures().next().unwrap().1.at_move.unwrap();
        assert!(at >= ran.result.operations[0].path.len(), "{at}");

        // A plunge that travels sideways.
        let c = recheck(&ran, |p| {
            let at = p.moves().last().unwrap().to;
            p.feed_to(at + dvec3(3.0, 0.0, -1.0), MoveRole::Plunge, 300.0)
                .unwrap();
        });
        assert_eq!(failed(&c), ["CC103"]);

        // A hole the linker never filled.
        let c = recheck(&ran, |p| {
            let at = p.moves().last().unwrap().to;
            p.feed_to(at + dvec3(1.0, 0.0, 0.0), MoveRole::Link, 300.0)
                .unwrap();
        });
        assert_eq!(failed(&c), ["CC101"]);

        // A plunge at cutting feed, in short moves.
        let c = recheck(&ran, |p| {
            let at = p.moves().last().unwrap().to;
            for k in 1..=5 {
                let k = f64::from(k);
                p.feed_to(at + dvec3(0.001 * k, 0.0, -0.3 * k), MoveRole::Cut, 300.0)
                    .unwrap();
            }
        });
        assert_eq!(failed(&c), ["CC105"]);
    }
}
