//! Engine work off the UI thread.
//!
//! A STEP file takes seconds to tessellate and a setup tens of seconds to run. The
//! window must stay responsive through both, so each piece of work runs on its own
//! thread and reports back over a channel, waking the UI when it is done.
//!
//! Several can run at once: each open job loads its own models and runs its own
//! setups. Every piece is tagged with the job and what it is for, so a reply lands
//! where it belongs, and one for a job since closed, or for a run since superseded,
//! is dropped.
//!
//! A run takes the job as the app holds it, not as it is on disk, so what runs is
//! what is on screen, saved or not.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use camcraft_core::job::{Job, ModelDef};
use camcraft_geom::bvh::MeshBvh;
use camcraft_session::{Model, Models, Ran, SessionError, load_job_model, read_job, run_setup};
use eframe::egui;

/// Which open job a piece of work is for.
pub type DocId = u64;

/// What a piece of work is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    /// Opening a job file into a new tab.
    Open,
    /// Loading one of the job's models, by name, as it was asked for then.
    Load {
        /// The model's name.
        name: String,
        /// What was asked for, to tell a reply from one since superseded.
        key: crate::document::ModelKey,
    },
    /// Running a setup, by name.
    Run {
        /// The setup's name.
        setup: String,
        /// Which run of it, to tell a reply from one since superseded.
        seq: u64,
    },
}

/// Work for the worker.
#[derive(Debug, Clone)]
pub enum Request {
    /// Read a job file.
    Open {
        /// The job file.
        path: PathBuf,
    },
    /// Load one of a job's models, found from the job's folder.
    Load {
        /// The job's folder, for a relative path.
        dir: PathBuf,
        /// The model.
        model: ModelDef,
        /// Tessellation follows from the job's machining tolerance.
        tolerance: f64,
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

/// What came back.
#[derive(Debug)]
pub enum Reply {
    /// A job file, read.
    Opened(Box<Result<(PathBuf, Job), SessionError>>),
    /// A model, loaded, with the index picking in the view casts rays against:
    /// built here, off the UI thread, because a large model takes a while.
    Loaded(Result<(Arc<Model>, Arc<MeshBvh>), SessionError>),
    /// A setup, run.
    Ran(Box<Result<Ran, SessionError>>),
    /// The thread stopped without replying: it panicked, most likely inside the
    /// geometry kernel. What it was doing.
    Died(&'static str),
}

struct Pending {
    rx: Receiver<Reply>,
    doc: DocId,
    task: Task,
    what: &'static str,
}

/// Every piece of work in flight.
#[derive(Default)]
pub struct Worker {
    pending: Vec<Pending>,
}

impl Worker {
    /// What is running for job `doc`, if anything: the first piece started.
    pub fn busy(&self, doc: DocId) -> Option<&'static str> {
        self.pending.iter().find(|p| p.doc == doc).map(|p| p.what)
    }

    /// Whether nothing at all is running.
    pub fn idle(&self) -> bool {
        self.pending.is_empty()
    }

    /// Start `request` for `doc`, tagged `task`.
    pub fn start(&mut self, ctx: &egui::Context, doc: DocId, task: Task, request: Request) {
        let what = match request {
            Request::Open { .. } => "opening job",
            Request::Load { .. } => "loading model",
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
        self.pending.push(Pending {
            rx,
            doc,
            task,
            what,
        });
    }

    /// Every reply that has come in, with its job and task.
    pub fn poll(&mut self) -> Vec<(DocId, Task, Reply)> {
        let mut out = Vec::new();
        self.pending.retain(|p| {
            let reply = match p.rx.try_recv() {
                Ok(reply) => reply,
                Err(TryRecvError::Empty) => return true,
                // Say so rather than wait forever for a reply that cannot come.
                Err(TryRecvError::Disconnected) => Reply::Died(p.what),
            };
            out.push((p.doc, p.task.clone(), reply));
            false
        });
        out
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
        Request::Open { path } => Reply::Opened(Box::new(read_job(&path).map(|job| (path, job)))),
        Request::Load {
            dir,
            model,
            tolerance,
        } => Reply::Loaded(load_job_model(&dir, &model, tolerance).map(|m| {
            let bvh = Arc::new(MeshBvh::new(m.mesh()));
            (Arc::new(m), bvh)
        })),
        Request::Run { job, setup, models } => {
            let refs: Models<'_> = models.iter().map(|(n, m)| (n.as_str(), &**m)).collect();
            Reply::Ran(Box::new(run_setup(&job, setup, &refs)))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait(worker: &mut Worker) -> (DocId, Task, Reply) {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if let Some(reply) = worker.poll().pop() {
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
        worker.start(&ctx, 7, Task::Open, Request::Open { path: example });
        assert_eq!(worker.busy(7), Some("opening job"));
        assert_eq!(worker.busy(8), None);
        let (doc, task, Reply::Opened(opened)) = wait(&mut worker) else {
            panic!("expected an opened job");
        };
        assert_eq!((doc, task), (7, Task::Open));
        let (path, job) = (*opened).unwrap();
        let def = job.models[0].clone();
        let key = crate::document::ModelKey::of(&job_dir(&path), &def, job.tolerance);
        worker.start(
            &ctx,
            7,
            Task::Load {
                name: def.name.clone(),
                key,
            },
            Request::Load {
                dir: job_dir(&path),
                model: def.clone(),
                tolerance: job.tolerance,
            },
        );
        let (_, _, Reply::Loaded(model)) = wait(&mut worker) else {
            panic!("expected a model");
        };
        let models = BTreeMap::from([(def.name.clone(), model.unwrap().0)]);

        // Something quicker than the example's roughing: its finish alone.
        let mut job = job;
        job.setups[0].operations.remove(0);
        job.tolerance = 0.1;
        let task = Task::Run {
            setup: "top".into(),
            seq: 1,
        };
        worker.start(
            &ctx,
            7,
            task.clone(),
            Request::Run {
                job: Box::new(job.clone()),
                setup: 0,
                models: models.clone(),
            },
        );
        assert_eq!(worker.busy(7), Some("running setup"));
        let (_, _, Reply::Ran(ran)) = wait(&mut worker) else {
            panic!("expected a run");
        };
        let ran = (*ran).unwrap();
        assert!(worker.idle());

        let refs: Models<'_> = models.iter().map(|(n, m)| (n.as_str(), &**m)).collect();
        let direct = run_setup(&job, 0, &refs).unwrap();
        let (a, b) = (&ran.result, &direct.result);
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
            1,
            Task::Open,
            Request::Open {
                path: PathBuf::from("no-such-job.camcraft"),
            },
        );
        match wait(&mut worker) {
            (1, Task::Open, Reply::Opened(r)) if matches!(*r, Err(SessionError::JobIo { .. })) => {}
            other => panic!("expected a read error, got {other:?}"),
        }
    }
}
