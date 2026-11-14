//! Another CAM system's program for an operation, cut from the same material
//! with the same tool as camcraft's, and the two compared.
//!
//! Neither program is taken at its word: both are read back as a controller
//! reads them ([`readback::read`]), so one reader measures both, and the other
//! one is cut by camcraft's simulator from the material camcraft's run had
//! before that operation, with the tool the operation names. What each leaves
//! is then compared column by column, and against the part.
//!
//! This is how camcraft's operations are held to Fusion's: the same part, the
//! same settings, both programs posted for the same machine, and the material
//! each leaves measured the same way.

use camcraft_core::job::Job;
use camcraft_core::run::{LEFTOVER_LIMIT, cutting_tool};
use camcraft_geom::arc::Arc;
use camcraft_post::readback::{self, Action, Convention, Motion, Problem, Step};
use camcraft_sim::stock::{MoveClass, SimMove, SimReport, Stock, VerifyReport};
use glam::{DVec3, dvec3};

use crate::{Ran, SessionError, program, setup_profile};

/// Why two programs could not be compared.
#[derive(Debug, thiserror::Error)]
pub enum CompareError {
    /// No operation of that name ran in the setup.
    #[error("no operation '{op}' ran in setup '{setup}'")]
    NoOperation {
        /// The operation asked for.
        op: String,
        /// The setup.
        setup: String,
    },
    /// The other program says something that cannot be read.
    #[error("the other program for '{op}': {problem}")]
    Unreadable {
        /// The operation.
        op: String,
        /// What could not be read.
        problem: Problem,
    },
    /// The material the run kept could not be unpacked.
    #[error("the material before '{op}' could not be unpacked: {why}")]
    Material {
        /// The operation.
        op: String,
        /// Why.
        why: String,
    },
    /// Something the session does went wrong.
    #[error(transparent)]
    Session(#[from] SessionError),
}

/// What a program does, in numbers, as read back.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Figures {
    /// Blocks that move the tool, where it is known where from.
    pub moves: usize,
    /// Of them, arcs and helices.
    pub arcs: usize,
    /// How far the tool feeds, in millimetres.
    pub fed: f64,
    /// How far it goes at rapid, in millimetres.
    pub rapid: f64,
    /// Minutes spent feeding, at the feeds the program asks for.
    pub feed_minutes: f64,
    /// Feed moves straight down.
    pub plunges: usize,
    /// Rapid moves straight up.
    pub lifts: usize,
    /// The lowest the tool's tip goes, in millimetres.
    pub lowest: f64,
}

/// How the material two programs leave differs, from the same start.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Difference {
    /// The grid's pitch, in millimetres.
    pub pitch: f64,
    /// Columns either program cut.
    pub cut: usize,
    /// Of them, where the two leave the material within the tolerance of each
    /// other.
    pub agree: usize,
    /// Columns camcraft cut lower than the other did, and the most.
    pub ours_lower: (usize, f64),
    /// Columns the other cut lower than camcraft did, and the most.
    pub theirs_lower: (usize, f64),
    /// How much each removed, in cubic millimetres: camcraft's, the other's.
    pub removed: (f64, f64),
    /// The difference, either way, at the 50th, 95th and 99th percentiles and
    /// the most, over the columns either cut.
    pub quantiles: [f64; 4],
    /// Material each left above the part, in cubic millimetres, over the
    /// columns either cut: camcraft's, the other's.
    pub left: (f64, f64),
    /// The difference seen from above, a character a patch: ` ` neither cut,
    /// `.` both alike, `+` camcraft cut lower, `x` the other cut lower, `#`
    /// both. +Y up, +X right.
    pub map: Vec<String>,
}

/// One operation of a setup, camcraft's program and another, compared.
#[derive(Debug, Clone)]
pub struct Compared {
    /// The operation.
    pub op: String,
    /// What camcraft's program does.
    pub ours: Figures,
    /// What the other program does.
    pub theirs: Figures,
    /// What camcraft's cut did, as the run simulated it.
    pub ours_cut: SimReport,
    /// What the other program's cut did.
    pub theirs_cut: SimReport,
    /// The material camcraft's cut leaves, against the part, as the run checks
    /// its finished material: gouges are what it would call one.
    pub ours_verify: VerifyReport,
    /// The material the other program leaves, checked the same way.
    pub theirs_verify: VerifyReport,
    /// How the material each left differs.
    pub difference: Difference,
}

/// The arc a motion turns, as the simulator takes one.
fn arc_of(m: &Motion) -> Option<Arc> {
    m.arc.map(|t| Arc {
        centre: t.centre,
        axis: t.normal,
        sweep: t.sweep,
    })
}

fn known(p: &readback::Position) -> Option<DVec3> {
    match *p {
        [Some(x), Some(y), Some(z)] => Some(dvec3(x, y, z)),
        _ => None,
    }
}

/// What `steps` do, in numbers.
pub fn figures(steps: &[Step]) -> Figures {
    let mut f = Figures {
        lowest: f64::INFINITY,
        ..Figures::default()
    };
    for step in steps {
        let Action::Move(m) = &step.action else {
            continue;
        };
        let (Some(a), Some(b)) = (known(&m.from), known(&m.to)) else {
            continue;
        };
        f.moves += 1;
        f.lowest = f.lowest.min(b.z).min(a.z);
        let arc = arc_of(m);
        let length = arc.map_or_else(|| a.distance(b), |arc| arc.length(a, b));
        let level = a.truncate().distance(b.truncate());
        if arc.is_some() {
            f.arcs += 1;
        }
        match m.feed {
            Some(feed) => {
                f.fed += length;
                if feed > 0.0 {
                    f.feed_minutes += length / feed;
                }
                if arc.is_none() && level < 1e-6 && b.z < a.z {
                    f.plunges += 1;
                }
            }
            None => {
                f.rapid += length;
                if arc.is_none() && level < 1e-6 && b.z > a.z {
                    f.lifts += 1;
                }
            }
        }
    }
    if !f.lowest.is_finite() {
        f.lowest = 0.0;
    }
    f
}

/// What `steps` do, as the simulator takes it: runs of moves from where the
/// tool is known to be, each starting where it is first known. Where it goes
/// out of knowing (a `G28`, a move in machine coordinates), a run ends.
pub fn sim_runs(steps: &[Step]) -> Vec<Vec<SimMove>> {
    let mut runs: Vec<Vec<SimMove>> = Vec::new();
    let mut run: Vec<SimMove> = Vec::new();
    for step in steps {
        let Action::Move(m) = &step.action else {
            continue;
        };
        let Some(to) = known(&m.to) else {
            if !run.is_empty() {
                runs.push(std::mem::take(&mut run));
            }
            continue;
        };
        // A run's first move only places the tool.
        match known(&m.from) {
            Some(from) => {
                if run.is_empty() {
                    run.push(SimMove {
                        to: from,
                        class: MoveClass::Rapid,
                        arc: None,
                    });
                }
            }
            None => {
                // Where it starts is not known: it is known from where it ends.
                if !run.is_empty() {
                    runs.push(std::mem::take(&mut run));
                }
                run.push(SimMove {
                    to,
                    class: MoveClass::Rapid,
                    arc: None,
                });
                continue;
            }
        }
        run.push(SimMove {
            to,
            class: if m.feed.is_some() {
                MoveClass::MayCut
            } else {
                MoveClass::Rapid
            },
            arc: arc_of(m),
        });
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
}

/// How the material `ours` and `theirs` leave differs, both cut from
/// `start`, and each against the part's heights `part`.
pub fn difference(
    start: &Stock,
    ours: &Stock,
    theirs: &Stock,
    part: impl Fn(f64, f64) -> Option<f64>,
    tolerance: f64,
    map_width: usize,
) -> Difference {
    let field = start.field();
    let (nx, ny) = (field.nx(), field.ny());
    let pitch = field.pitch();
    let area = pitch * pitch;
    let mut d = Difference {
        pitch,
        ..Difference::default()
    };
    // Each patch is `patch` cells across and twice that down, as a character
    // is about twice as tall as it is wide.
    let map_w = u32::try_from(map_width).unwrap_or(u32::MAX).max(1);
    let patch = (nx / map_w).max(1);
    let map_h = ny.div_ceil(2 * patch).max(1);
    let index = |px: u32, py: u32| usize::try_from(py * map_w + px).unwrap_or(usize::MAX);
    // Per patch: anyone cut, ours lower, theirs lower.
    let mut patches = vec![[false; 3]; index(0, map_h)];
    let mut gaps: Vec<f64> = Vec::new();
    for iy in 0..ny {
        for ix in 0..nx {
            let (Some(s), Some(a), Some(b)) = (
                start.field().get(ix, iy),
                ours.field().get(ix, iy),
                theirs.field().get(ix, iy),
            ) else {
                continue;
            };
            d.removed.0 += (s - a).max(0.0) * area;
            d.removed.1 += (s - b).max(0.0) * area;
            let touched = s - a > tolerance || s - b > tolerance;
            if !touched {
                continue;
            }
            d.cut += 1;
            let gap = a - b;
            gaps.push(gap.abs());
            let px = (ix / patch).min(map_w - 1);
            let py = (iy / (2 * patch)).min(map_h - 1);
            let cell = patches.get_mut(index(px, py));
            if gap.abs() <= tolerance {
                d.agree += 1;
            } else if gap < 0.0 {
                d.ours_lower.0 += 1;
                d.ours_lower.1 = d.ours_lower.1.max(-gap);
            } else {
                d.theirs_lower.0 += 1;
                d.theirs_lower.1 = d.theirs_lower.1.max(gap);
            }
            if let Some(pos) = field.position(ix, iy)
                && let Some(p) = part(pos.x, pos.y)
            {
                d.left.0 += (a - p).max(0.0) * area;
                d.left.1 += (b - p).max(0.0) * area;
            }
            if let Some(c) = cell {
                c[0] = true;
                c[1] |= gap < -tolerance;
                c[2] |= gap > tolerance;
            }
        }
    }
    gaps.sort_by(f64::total_cmp);
    // The difference at `percent` of the way through them, from the least.
    let at = |percent: usize| -> f64 {
        let i = gaps.len().saturating_sub(1) * percent / 100;
        gaps.get(i).copied().unwrap_or(0.0)
    };
    d.quantiles = [at(50), at(95), at(99), at(100)];
    d.map = (0..map_h)
        .rev()
        .map(|py| {
            (0..map_w)
                .map(|px| match patches.get(index(px, py)) {
                    Some([true, true, true]) => '#',
                    Some([true, true, false]) => '+',
                    Some([true, false, true]) => 'x',
                    Some([true, false, false]) => '.',
                    _ => ' ',
                })
                .collect()
        })
        .collect();
    d
}

/// Operation `op` of setup `setup` as `ran` made it, compared with `theirs`,
/// another program for it, cut from the same material with the same tool.
/// Differences within `tolerance` are alike.
pub fn compare(
    job: &Job,
    setup: usize,
    job_dir: &std::path::Path,
    ran: &Ran,
    op: &str,
    theirs: &str,
    tolerance: f64,
) -> Result<Compared, CompareError> {
    let s = job
        .setups
        .get(setup)
        .ok_or_else(|| CompareError::NoOperation {
            op: op.to_string(),
            setup: setup.to_string(),
        })?;
    let missing = || CompareError::NoOperation {
        op: op.to_string(),
        setup: s.name.clone(),
    };
    let index = ran
        .result
        .operations
        .iter()
        .position(|o| o.name == op)
        .ok_or_else(missing)?;
    let made = ran.result.operations.get(index).ok_or_else(missing)?;
    let spec = s
        .operations
        .iter()
        .find(|o| o.name == op && o.enabled)
        .ok_or_else(missing)?;
    let unpack = |i: usize| -> Result<Stock, CompareError> {
        let material = |why: String| CompareError::Material {
            op: op.to_string(),
            why,
        };
        ran.result
            .keyframes
            .get(i)
            .ok_or_else(|| material("the run kept none".into()))?
            .unpack()
            .map_err(|e| material(e.to_string()))
    };
    let start = unpack(index)?;
    let ours = unpack(index + 1)?;
    let tool = cutting_tool(job, spec).map_err(SessionError::from)?;

    let steps = readback::read_as(theirs, Convention::Fanuc).map_err(|problem| {
        CompareError::Unreadable {
            op: op.to_string(),
            problem,
        }
    })?;
    let mut cut = start.clone();
    let mut theirs_cut = SimReport::default();
    for run in sim_runs(&steps) {
        let r = cut.apply(&run, &tool);
        theirs_cut.removed_mm3 += r.removed_mm3;
        theirs_cut.rapid_contacts += r.rapid_contacts;
        theirs_cut.deepest_rapid_contact_mm = theirs_cut
            .deepest_rapid_contact_mm
            .max(r.deepest_rapid_contact_mm);
    }

    let profile = setup_profile(s, job_dir, None)?;
    let (text, _) = program(made, s.wcs, &profile, op)?;
    let ours_steps = readback::read(&text).map_err(|problem| SessionError::ReadsBackWrong {
        name: op.to_string(),
        problem,
    })?;

    let part = &ran.result.part;
    let check = |m: &Stock| m.verify(part, ran.result.verify_budget, LEFTOVER_LIMIT);
    let (ours_verify, theirs_verify) = (check(&ours), check(&cut));
    let difference = difference(
        &start,
        &ours,
        &cut,
        |x, y| part.height_at(x, y),
        tolerance,
        100,
    );
    Ok(Compared {
        op: op.to_string(),
        ours: figures(&ours_steps),
        theirs: figures(&steps),
        ours_cut: made.sim,
        theirs_cut,
        ours_verify,
        theirs_verify,
        difference,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_engrave;

#[cfg(test)]
mod tests_ramp;
