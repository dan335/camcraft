//! Engine work off the UI thread.
//!
//! A STEP file takes seconds to tessellate and a job tens of seconds to run. The
//! window must stay responsive through both, so each runs on its own thread and
//! reports back over a channel, waking the UI when it is done. One piece of work at
//! a time: the UI disables what would start another.
//!
//! A run takes the job as the app holds it, not as it is on disk, so what runs is
//! what is on screen, saved or not.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use camcraft_core::job::Job;
use camcraft_session::{Model, Models, Ran, SessionError, load_job_model, read_job, run_setup};
use eframe::egui;

/// Work for the worker.
#[derive(Debug, Clone)]
pub enum Request {
    /// Read a job file and load every model it names, at the job's tolerance.
    Open {
        /// The job file.
        path: PathBuf,
    },
    /// Run one setup of a job as it stands, the way `camcraft job` runs it.
    Run {
        /// The job, as it is in the app, saved or not.
        job: Box<Job>,
        /// Which setup.
        setup: usize,
        /// Its models, loaded, by name.
        models: BTreeMap<String, Arc<Model>>,
    },
}

/// A job file, opened.
#[derive(Debug)]
pub struct Opened {
    /// Where it was.
    pub path: PathBuf,
    /// The job.
    pub job: Job,
    /// Each model it names, loaded or why not.
    pub models: BTreeMap<String, Result<Arc<Model>, SessionError>>,
}

/// What a finished run produced.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The job as it was run.
    pub job: Job,
    /// Which setup.
    pub setup: usize,
    /// What running it produced.
    pub ran: Ran,
}

/// What came back.
#[derive(Debug)]
pub enum Reply {
    /// A job file, opened.
    Opened(Box<Result<Opened, SessionError>>),
    /// A setup, run.
    Ran(Box<Result<Outcome, SessionError>>),
    /// The thread stopped without replying: it panicked, most likely inside the
    /// geometry kernel. What it was doing.
    Died(&'static str),
}

/// The one worker.
#[derive(Default)]
pub struct Worker {
    pending: Option<(Receiver<Reply>, &'static str)>,
}

impl Worker {
    /// Whether something is running, and what.
    pub fn busy(&self) -> Option<&'static str> {
        self.pending.as_ref().map(|(_, what)| *what)
    }

    /// Start `request`. Ignored while something else is running; the UI does not
    /// offer it then.
    pub fn start(&mut self, ctx: &egui::Context, request: Request) {
        if self.pending.is_some() {
            return;
        }
        let what = match request {
            Request::Open { .. } => "opening job",
            Request::Run { .. } => "running setup",
        };
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let reply = execute(request);
            // The receiver is gone only if the app is closing; nothing to tell.
            let _ = tx.send(reply);
            ctx.request_repaint();
        });
        self.pending = Some((rx, what));
    }

    /// A reply, if the work has finished.
    pub fn poll(&mut self) -> Option<Reply> {
        let (rx, what) = self.pending.as_ref()?;
        let reply = match rx.try_recv() {
            Ok(reply) => reply,
            Err(TryRecvError::Empty) => return None,
            // Say so rather than wait forever for a reply that cannot come.
            Err(TryRecvError::Disconnected) => Reply::Died(what),
        };
        self.pending = None;
        Some(reply)
    }
}

/// The folder a job's relative paths start from.
pub fn job_dir(path: &std::path::Path) -> PathBuf {
    path.parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), std::path::Path::to_path_buf)
}

fn execute(request: Request) -> Reply {
    match request {
        Request::Open { path } => {
            let opened = read_job(&path).map(|job| {
                let dir = job_dir(&path);
                let models = job
                    .models
                    .iter()
                    .map(|m| {
                        let loaded = load_job_model(&dir, m, job.tolerance).map(Arc::new);
                        (m.name.clone(), loaded)
                    })
                    .collect();
                Opened { path, job, models }
            });
            Reply::Opened(Box::new(opened))
        }
        Request::Run { job, setup, models } => {
            let refs: Models<'_> = models.iter().map(|(n, m)| (n.as_str(), &**m)).collect();
            let outcome = run_setup(&job, setup, &refs).map(|ran| Outcome {
                job: *job,
                setup,
                ran,
            });
            Reply::Ran(Box::new(outcome))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait(worker: &mut Worker) -> Reply {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if let Some(reply) = worker.poll() {
                return reply;
            }
            assert!(Instant::now() < deadline, "the worker never replied");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// A run through the worker is the CLI's run: the same result as calling the
    /// session directly with the same job and models.
    #[test]
    fn a_run_through_the_worker_matches_the_session_directly() {
        let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/rail-jig.camcraft");
        let mut worker = Worker::default();
        let ctx = egui::Context::default();
        worker.start(&ctx, Request::Open { path: example });
        assert_eq!(worker.busy(), Some("opening job"));
        let Reply::Opened(opened) = wait(&mut worker) else {
            panic!("expected an opened job");
        };
        let opened = (*opened).unwrap();
        let models: BTreeMap<String, Arc<Model>> = opened
            .models
            .into_iter()
            .map(|(n, m)| (n, m.unwrap()))
            .collect();

        // Something quicker than the example's roughing: its finish alone.
        let mut job = opened.job;
        job.setups[0].operations.remove(0);
        job.tolerance = 0.1;
        worker.start(
            &ctx,
            Request::Run {
                job: Box::new(job.clone()),
                setup: 0,
                models: models.clone(),
            },
        );
        // A second request while busy is ignored, not queued.
        worker.start(
            &ctx,
            Request::Open {
                path: PathBuf::from("elsewhere.camcraft"),
            },
        );
        let Reply::Ran(outcome) = wait(&mut worker) else {
            panic!("expected a run");
        };
        let outcome = (*outcome).unwrap();
        assert!(worker.busy().is_none());

        let refs: Models<'_> = models.iter().map(|(n, m)| (n.as_str(), &**m)).collect();
        let direct = run_setup(&job, 0, &refs).unwrap();
        let (a, b) = (&outcome.ran.result, &direct.result);
        assert_eq!(a.operations.len(), b.operations.len());
        for (x, y) in a.operations.iter().zip(&b.operations) {
            assert_eq!(x.passes, y.passes);
            assert_eq!(x.path.moves(), y.path.moves(), "{}: paths differ", x.name);
        }
        assert_eq!(a.verify.compared, b.verify.compared);
        assert_eq!(a.is_clean(), b.is_clean());
    }

    #[test]
    fn errors_come_back_as_replies() {
        let mut worker = Worker::default();
        worker.start(
            &egui::Context::default(),
            Request::Open {
                path: PathBuf::from("no-such-job.camcraft"),
            },
        );
        match wait(&mut worker) {
            Reply::Opened(r) if matches!(*r, Err(SessionError::JobIo { .. })) => {}
            other => panic!("expected a read error, got {other:?}"),
        }
    }
}
