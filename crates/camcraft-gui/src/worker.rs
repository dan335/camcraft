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
//! what is on screen, saved or not. Its toolpaths are laid out for drawing here
//! too, and a program is rendered and written here, so a long one never holds
//! the window up.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use camcraft_core::job::{Job, ModelDef, Setup, Transform};
use camcraft_core::run::{Plan, Stage};
use camcraft_geom::bvh::MeshBvh;
use camcraft_session::{
    Model, Models, Posting, Ran, SessionError, load_job_model, plan_posting, read_job,
    run_setup_planned, write_posting,
};
use eframe::egui;

use crate::view::scene::Scene;

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
    /// Rendering programs, to write them or ask first.
    Post,
    /// Writing programs rendered.
    Write {
        /// Where to.
        dir: PathBuf,
        /// The setups among them that did not pass their checks.
        refused: Vec<String>,
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
    /// Place a loaded model again, without reading its file.
    Place {
        /// The model, as loaded.
        model: Arc<Model>,
        /// Where it goes.
        transform: Transform,
    },
    /// Run one setup of a job as it stands, the way `camcraft job` runs it.
    Run {
        /// The job, as it is in the app, saved or not.
        job: Box<Job>,
        /// Which setup.
        setup: usize,
        /// Its models, loaded, by name.
        models: BTreeMap<String, Arc<Model>>,
        /// What to do with each operation; none, to work them all out.
        plans: Vec<Plan>,
    },
    /// Render the programs of setups into a folder, writing nothing, as
    /// [`plan_posting`] does.
    Post {
        /// Each setup, as it ran, with what running it made.
        setups: Vec<(Setup, Ran)>,
        /// The job's folder, where a machine profile that is a file is found.
        job_dir: PathBuf,
        /// Where the programs go.
        dir: PathBuf,
    },
    /// Write programs rendered, all or none.
    Write {
        /// What to write, and where.
        posting: Box<Posting>,
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
    /// A setup, run, with its toolpaths laid out for drawing: built here, off
    /// the UI thread, because a long program takes a while.
    Ran(Box<Result<(Ran, Scene), SessionError>>),
    /// Programs rendered, and not written yet.
    Planned(Box<Result<Posting, SessionError>>),
    /// Programs written: how many.
    Written(Result<usize, SessionError>),
    /// Where a run has got to: sent as it goes, before [`Reply::Ran`].
    Progress(Stage),
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
    /// Whether nothing at all is running.
    pub fn idle(&self) -> bool {
        self.pending.is_empty()
    }

    /// Start `request` for `doc`, tagged `task`.
    pub fn start(&mut self, ctx: &egui::Context, doc: DocId, task: Task, request: Request) {
        let what = match request {
            Request::Open { .. } => "opening job",
            Request::Load { .. } => "loading model",
            Request::Place { .. } => "placing model",
            Request::Run { .. } => "running setup",
            Request::Post { .. } => "rendering programs",
            Request::Write { .. } => "writing programs",
        };
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let reply = execute(request, &mut |stage| {
                // The receiver is gone only if the app is closing; nothing to tell.
                let _ = tx.send(Reply::Progress(stage));
                ctx.request_repaint();
            });
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

    /// Every reply that has come in, with its job and task: any progress, then
    /// the answer, which ends the piece of work.
    pub fn poll(&mut self) -> Vec<(DocId, Task, Reply)> {
        let mut out = Vec::new();
        self.pending.retain(|p| {
            loop {
                let reply = match p.rx.try_recv() {
                    Ok(reply) => reply,
                    Err(TryRecvError::Empty) => return true,
                    // Say so rather than wait forever for a reply that cannot come.
                    Err(TryRecvError::Disconnected) => Reply::Died(p.what),
                };
                let done = !matches!(reply, Reply::Progress(_));
                out.push((p.doc, p.task.clone(), reply));
                if done {
                    return false;
                }
            }
        });
        out
    }
}

fn execute(request: Request, progress: &mut dyn FnMut(Stage)) -> Reply {
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
        Request::Place { model, transform } => Reply::Loaded(model.placed(&transform).map(|m| {
            let bvh = Arc::new(MeshBvh::new(m.mesh()));
            (Arc::new(m), bvh)
        })),
        Request::Run {
            job,
            setup,
            models,
            plans,
        } => {
            let refs: Models<'_> = models.iter().map(|(n, m)| (n.as_str(), &**m)).collect();
            let ran = run_setup_planned(&job, setup, &refs, &plans, progress);
            Reply::Ran(Box::new(ran.map(|ran| {
                let scene = job
                    .setups
                    .get(setup)
                    .map(|s| Scene::build(&job, s, &ran.result))
                    .unwrap_or_default();
                (ran, scene)
            })))
        }
        Request::Post {
            setups,
            job_dir,
            dir,
        } => {
            let setups: Vec<(&Setup, &Ran)> = setups.iter().map(|(s, r)| (s, r)).collect();
            Reply::Planned(Box::new(plan_posting(&setups, &job_dir, &dir, None)))
        }
        Request::Write { posting } => Reply::Written(write_posting(&posting)),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests;
