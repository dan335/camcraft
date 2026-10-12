//! Posting from the app: whether a job's setups can be posted, rendering their
//! programs into a folder, and writing them, or asking first when something stands
//! in the way. Rendering and writing are the worker's, off the UI thread.

use std::path::Path;

use camcraft_session::{Posting, SessionError};
use eframe::egui;

use super::App;
use super::prompts::Prompt;
use crate::document::{Document, Level, Status};
use crate::worker::{DocId, Request, Task};

/// Why `names` of `doc` cannot be posted at all, if they cannot: each must have a
/// result, finished and up to date. A result that did not pass can be posted, but
/// only once that is confirmed.
pub(super) fn post_blocker(doc: &Document, names: &[String]) -> Option<String> {
    if names.is_empty() {
        return Some("the job has no setups".into());
    }
    names.iter().find_map(|n| match doc.status(n) {
        Status::NotRun => Some(format!("'{n}' has not been generated")),
        Status::Running => Some(format!("'{n}' is still generating")),
        Status::Stale => Some(format!("'{n}' is out of date: generate it again")),
        Status::Verified | Status::Warnings | Status::Refused => None,
    })
}

impl App {
    /// Render job `id`'s programs for `scope` (one setup, or all) into `dir`, to
    /// write them once rendered, or ask first when something stands in the way
    /// ([`App::planned`]).
    pub(super) fn prepare_post(
        &mut self,
        ctx: &egui::Context,
        id: DocId,
        scope: Option<String>,
        dir: &Path,
    ) {
        let Some(doc) = self.find(id) else {
            return;
        };
        let names: Vec<String> = match scope {
            Some(name) => vec![name],
            None => doc.job.setups.iter().map(|s| s.name.clone()).collect(),
        };
        if let Some(why) = post_blocker(doc, &names) {
            doc.note(Level::Warn, format!("not posted: {why}"));
            return;
        }
        // The job each setup ran from: up to date, so the same as the one edited.
        let setups = names
            .iter()
            .filter_map(|n| {
                let ran = doc.setups.get(n)?.ran.as_ref()?;
                let setup = ran.job.setups.iter().find(|s| &s.name == n)?;
                Some((setup.clone(), ran.ran.clone()))
            })
            .collect();
        let request = Request::Post {
            setups,
            job_dir: doc.dir(),
            dir: dir.to_path_buf(),
        };
        self.worker.start(ctx, id, Task::Post, request);
    }

    /// Job `id`'s programs have been rendered, or could not be: write them, or
    /// ask first when something stands in the way.
    pub(super) fn planned(
        &mut self,
        ctx: &egui::Context,
        id: DocId,
        planned: Result<Posting, SessionError>,
    ) {
        match planned {
            Err(e) => {
                if let Some(doc) = self.find(id) {
                    doc.note(Level::Error, format!("not posted: {e}"));
                }
            }
            Ok(plan) if plan.needs_confirming() => {
                self.post_plan = Some((id, plan));
                self.post_anyway = false;
                // Asked now, or once the question already waiting is answered.
                if self.prompt.is_none() {
                    self.prompt = Some(Prompt::Post);
                }
            }
            Ok(plan) => self.write_post(ctx, id, plan),
        }
    }

    /// Write job `id`'s programs, rendered: the worker writes them, and
    /// [`App::written`] says how that went.
    pub(super) fn write_post(&mut self, ctx: &egui::Context, id: DocId, plan: Posting) {
        if self.find(id).is_none() {
            return;
        }
        let task = Task::Write {
            dir: plan.dir.clone(),
            refused: plan.refused.iter().map(|(s, _)| s.clone()).collect(),
        };
        let request = Request::Write {
            posting: Box::new(plan),
        };
        self.worker.start(ctx, id, task, request);
    }

    /// Job `id`'s programs, rendered for `dir`, have been written, or not;
    /// `refused` are the setups among them that did not pass their checks.
    pub(super) fn written(
        &mut self,
        id: DocId,
        (dir, refused): (&Path, &[String]),
        written: Result<usize, SessionError>,
    ) {
        let Some(doc) = self.find(id) else {
            return;
        };
        match written {
            Ok(n) => {
                for setup in refused {
                    doc.note(
                        Level::Warn,
                        format!("posted '{setup}' although it did NOT pass its checks"),
                    );
                }
                doc.note(
                    Level::Info,
                    format!("posted {n} program(s) to {}", dir.display()),
                );
            }
            Err(e) => doc.note(Level::Error, format!("posting failed: {e}")),
        }
    }
}
