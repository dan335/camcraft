//! A job: its models loaded, its setups run, checked and posted.
//!
//! Everything between files on disk and G-code that is not engine work. Both the
//! command line and the desktop app go through here, so they cannot drift apart
//! about what a file means, what runs, or what is allowed to be posted:
//!
//! - which reader a file gets, and what its numbers mean ([`load_model`]),
//! - where a job's model files are ([`fs::resolve`]), and how a job is read and
//!   written ([`read_job`], [`write_job`]),
//! - the one engine path ([`run_setup`]),
//! - the one set of checks ([`checks`]) and the one verification policy
//!   ([`gate`]): no program from a setup with a failure among them,
//! - and how a program is named, rendered and written ([`plan_posting`],
//!   [`program_name`]).
//!
//! Position in the dependency DAG: depends on every library crate, and only the two
//! binaries depend on it. It says nothing about how anything is shown. It returns
//! what happened as data (`notes` and `warnings`), and each front end presents it
//! in its own way.

pub mod checks;
mod error;
mod finish;
pub mod fs;
mod job_file;
pub mod library;
mod model;
mod post;
mod run;
#[cfg(test)]
mod test_support;

pub use error::SessionError;
pub use finish::{FinishSpec, single_operation};
pub use job_file::{job_text, parse_job, read_job, write_job};
pub use model::{Model, Models, load_job_model, load_model, load_models, models_of, units_of};
pub use post::{
    Posting, Program, builtin_posts, load_profile, plan_posting, program, program_name,
    program_spec, setup_profile, write_posting,
};
pub use run::{Passed, Ran, check_runnable, gate, run_setup, run_setup_planned};
