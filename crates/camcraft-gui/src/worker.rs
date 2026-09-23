//! Engine work off the UI thread.
//!
//! A STEP file takes seconds to tessellate and a job tens of seconds to run. The
//! window must stay responsive through both, so each runs on its own thread and
//! reports back over a channel, waking the UI when it is done. One piece of work at
//! a time: the UI disables what would start another.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use camcraft_core::job::Job;
use camcraft_session::{Model, Ran, SessionError, Units, load_model, read_job, run};
use eframe::egui;

/// Work for the worker.
#[derive(Debug, Clone)]
pub enum Request {
    /// Load a model to look at, before any job has run.
    Load {
        /// The model file.
        path: PathBuf,
        /// What an STL's numbers mean.
        units: Option<Units>,
        /// Tessellation follows from this machining tolerance.
        tolerance: f64,
    },
    /// Run a job, the way `camcraft job` does: read the job file, load the model at
    /// the job's tolerance, run.
    Run {
        /// The model file.
        model: PathBuf,
        /// What an STL's numbers mean.
        units: Option<Units>,
        /// The job file, read afresh so the file on disk is what runs.
        job: PathBuf,
    },
}

/// What a finished run produced.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The model as loaded for this job.
    pub model: Model,
    /// The job as read.
    pub job: Job,
    /// What running it produced.
    pub ran: Ran,
}

/// What came back.
#[derive(Debug)]
pub enum Reply {
    /// A model to look at.
    Loaded(Result<Model, SessionError>),
    /// A job, run.
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
            Request::Load { .. } => "loading model",
            Request::Run { .. } => "running job",
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

fn execute(request: Request) -> Reply {
    match request {
        Request::Load {
            path,
            units,
            tolerance,
        } => Reply::Loaded(load_model(&path, units, tolerance)),
        Request::Run { model, units, job } => {
            let outcome = read_job(&job).and_then(|job| {
                let model = load_model(&model, units, job.tolerance)?;
                let ran = run(&model, &job)?;
                Ok(Outcome { model, job, ran })
            });
            Reply::Ran(Box::new(outcome))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn fixture(name: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/step")
            .join(name)
    }

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

    const JOB: &str = r#"
tolerance = 0.05
[setup]
stock = { kind = "box", margin = [1.0, 1.0, 1.0] }
[tools.ball]
kind = "ball"
diameter = 3.0
[[operation]]
name = "finish"
strategy = "parallel"
tool = "ball"
passes = { stepover = 1.5, feed = 1500.0 }
"#;

    /// A run through the worker is the CLI's run: the same result as calling the
    /// session directly with the same files.
    #[test]
    fn a_run_through_the_worker_matches_the_session_directly() {
        let dir = std::env::temp_dir().join(format!("camcraft-gui-worker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let job_path = dir.join("job.toml");
        std::fs::write(&job_path, JOB).unwrap();
        let model_path = fixture("hinge_jig.step");

        let mut worker = Worker::default();
        let ctx = egui::Context::default();
        worker.start(
            &ctx,
            Request::Run {
                model: model_path.clone(),
                units: None,
                job: job_path.clone(),
            },
        );
        assert_eq!(worker.busy(), Some("running job"));
        // A second request while busy is ignored, not queued.
        worker.start(
            &ctx,
            Request::Load {
                path: model_path.clone(),
                units: None,
                tolerance: 0.05,
            },
        );
        let Reply::Ran(outcome) = wait(&mut worker) else {
            panic!("expected a run");
        };
        let outcome = (*outcome).unwrap();
        assert!(worker.busy().is_none());

        let job = read_job(&job_path).unwrap();
        let model = load_model(&model_path, None, job.tolerance).unwrap();
        let direct = run(&model, &job).unwrap();
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
            Request::Load {
                path: PathBuf::from("no-units.stl"),
                units: None,
                tolerance: 0.05,
            },
        );
        match wait(&mut worker) {
            Reply::Loaded(Err(SessionError::StlNeedsUnits)) => {}
            other => panic!("expected a units refusal, got {other:?}"),
        }
    }
}
