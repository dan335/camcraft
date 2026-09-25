//! Posting from the app: whether a job's setups can be posted, rendering their
//! programs into a folder, and writing them, or asking first when something stands
//! in the way.

use std::path::Path;

use camcraft_session::{Posting, plan_posting, write_posting};

use super::App;
use super::prompts::Prompt;
use crate::document::{Document, Level, Status};
use crate::worker::DocId;

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
    /// Render job `id`'s programs for `scope` (one setup, or all) into `dir`, and
    /// write them, or ask first when something stands in the way.
    pub(super) fn prepare_post(&mut self, id: DocId, scope: Option<String>, dir: &Path) {
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
        let entries: Vec<_> = names
            .iter()
            .filter_map(|n| {
                let ran = doc.setups.get(n)?.ran.as_ref()?;
                let setup = ran.job.setups.iter().find(|s| &s.name == n)?;
                Some((setup, &ran.ran))
            })
            .collect();
        match plan_posting(&entries, &doc.dir(), dir, None) {
            Err(e) => doc.note(Level::Error, format!("not posted: {e}")),
            Ok(plan) if plan.needs_confirming() => {
                self.post_plan = Some((id, plan));
                self.post_anyway = false;
                self.prompt = Some(Prompt::Post);
            }
            Ok(plan) => self.write_post(id, &plan),
        }
    }

    pub(super) fn write_post(&mut self, id: DocId, plan: &Posting) {
        let Some(doc) = self.find(id) else {
            return;
        };
        match write_posting(plan) {
            Ok(n) => {
                for (setup, _) in &plan.refused {
                    doc.note(
                        Level::Warn,
                        format!("posted '{setup}' although it did NOT pass its checks"),
                    );
                }
                doc.note(
                    Level::Info,
                    format!("posted {n} program(s) to {}", plan.dir.display()),
                );
            }
            Err(e) => doc.note(Level::Error, format!("posting failed: {e}")),
        }
    }
}
