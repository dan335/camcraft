//! camcraft's own results on the parts made for its operations, held to the
//! figures a run of them gave when they were recorded.
//!
//! Each part in `fixtures/step` made for operations (`ops_block`, `ops_2d`,
//! `ops_engrave`, `ops_3d`) has its jobs here: the operations made as the app
//! makes a new one ([`Operation::fresh`](camcraft_core::job::Operation::fresh)),
//! with the settings each test names, run, checked and posted. Every check must
//! pass, but where a test says which fail and why; the material each operation
//! leaves must not be cut into the part, nor any rapid go through material; and
//! what each operation does is held to what it did when its figures were
//! recorded ([`Recorded`]): how often its program lifts, exactly, how far it
//! feeds, to half a percent, how low it goes, and what it leaves on the part, the
//! most anywhere and in all.
//!
//! A change that moves a figure is not wrong for that: it is a change in what
//! camcraft cuts, to be looked at, said in its commit and recorded anew here.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::path::Path;

use camcraft_core::job::Job;
use camcraft_core::run::LEFTOVER_LIMIT;
use camcraft_post::readback;
use camcraft_sim::stock::{Stock, VerifyReport};

use crate::compare::figures;
use crate::{Ran, load_models, models_of, program, run_setup, setup_profile};

mod ops_2d;
mod ops_3d;
mod ops_block;
mod ops_engrave;

/// What an operation's run did, as recorded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Recorded {
    /// Rapid moves straight up in its program.
    pub lifts: usize,
    /// How far its program feeds, in millimetres.
    pub fed: f64,
    /// The lowest its tool's tip goes, in millimetres.
    pub lowest: f64,
    /// The most material left anywhere on the part after it, in millimetres.
    pub most_left: f64,
    /// All the material left above the part after it, in cubic millimetres.
    pub left: f64,
}

/// What an operation's run did, as measured: its program read back, and the
/// material after it judged against the part as the run's own check judges it.
pub(crate) struct Measured {
    /// The operation.
    pub op: String,
    /// The figures [`Recorded`] holds.
    pub figures: Recorded,
    /// The material after it, against the part.
    pub verify: VerifyReport,
    /// Rapid moves that went through material, as the run simulated them.
    pub rapid_contacts: u64,
    /// The material after it.
    pub material: Stock,
}

/// The directory the jobs here are run from.
pub(crate) fn here() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// `job`'s first setup run, its models loaded.
pub(crate) fn run(job: &Job) -> Ran {
    let loaded = load_models(job, here(), &[0]).unwrap();
    run_setup(job, 0, &models_of(&loaded)).unwrap()
}

/// `ran` passed every check.
pub(crate) fn passed(ran: &Ran) {
    assert!(ran.checks.failures().count() == 0, "{:?}", ran.checks);
}

/// The material above the part in `material`, in cubic millimetres, judged
/// cell by cell against `ran`'s part as its check judges it.
fn left_on(ran: &Ran, material: &Stock) -> f64 {
    let field = material.field();
    let area = field.pitch() * field.pitch();
    let part = ran.result.part.lookup(field);
    let mut left = 0.0;
    for iy in 0..field.ny() {
        for ix in 0..field.nx() {
            let (Some(p), Some(h)) = (field.position(ix, iy), field.get(ix, iy)) else {
                continue;
            };
            if let Some((top, _)) = part(ix, iy, p, h, ran.result.verify_budget) {
                left += (h - top).max(0.0) * area;
            }
        }
    }
    left
}

/// Operation `op` of `job`'s first setup as `ran` made it, measured.
pub(crate) fn measure(job: &Job, ran: &Ran, op: &str) -> Measured {
    let index = ran
        .result
        .operations
        .iter()
        .position(|o| o.name == op)
        .unwrap_or_else(|| panic!("no operation {op} ran"));
    let made = &ran.result.operations[index];
    let material = ran.result.keyframes[index + 1].unpack().unwrap();
    let setup = &job.setups[0];
    let profile = setup_profile(setup, here(), None).unwrap();
    let (text, _) = program(made, setup.wcs, &profile, op).unwrap();
    let posted = figures(&readback::read(&text).unwrap());
    let verify = material.verify(&ran.result.part, ran.result.verify_budget, LEFTOVER_LIMIT);
    Measured {
        op: op.to_string(),
        figures: Recorded {
            lifts: posted.lifts,
            fed: posted.fed,
            lowest: posted.lowest,
            most_left: verify.tallest_leftover_mm,
            left: left_on(ran, &material),
        },
        verify,
        rapid_contacts: made.sim.rapid_contacts,
        material,
    }
}

impl Measured {
    /// The figures as a [`Recorded`] is written, to record them anew.
    fn written(&self) -> String {
        let f = &self.figures;
        format!(
            "{}: Recorded {{ lifts: {}, fed: {:.1}, lowest: {:.4}, most_left: {:.4}, left: {:.1} }}",
            self.op, f.lifts, f.fed, f.lowest, f.most_left, f.left
        )
    }

    /// Cut nothing of the part, and no rapid through material.
    pub(crate) fn clean(&self) {
        assert_eq!(self.verify.gouged, 0, "{}: {:?}", self.op, self.verify);
        assert_eq!(self.rapid_contacts, 0, "{}", self.op);
    }

    /// The figures as `recorded`: the lifts exactly, what it feeds to half a
    /// percent, the lowest to a micron, the most left to 5 microns, and all it
    /// leaves to half a percent.
    pub(crate) fn as_recorded(&self, recorded: &Recorded) {
        let (got, want) = (&self.figures, recorded);
        let say = format!("{}\nrecorded {want:?}", self.written());
        assert_eq!(got.lifts, want.lifts, "lifts: {say}");
        assert!((got.fed - want.fed).abs() <= 0.005 * want.fed, "fed: {say}");
        assert!((got.lowest - want.lowest).abs() <= 1e-3, "lowest: {say}");
        assert!(
            (got.most_left - want.most_left).abs() <= 0.005,
            "most left: {say}"
        );
        assert!(
            (got.left - want.left).abs() <= 0.005 * want.left.max(1.0),
            "left: {say}"
        );
    }
}

/// `job` run, every check passed, and each of `ops` clean and as recorded.
/// Every operation's figures are written out before any is held, so a change
/// shows them all at once. The run, for what more a test asks of it.
pub(crate) fn holds(job: &Job, ops: &[(&str, Recorded)]) -> Ran {
    let ran = run(job);
    passed(&ran);
    let measured: Vec<Measured> = ops.iter().map(|(op, _)| measure(job, &ran, op)).collect();
    for m in &measured {
        eprintln!("{}", m.written());
    }
    for (m, (_, recorded)) in measured.iter().zip(ops) {
        m.clean();
        m.as_recorded(recorded);
    }
    ran
}
