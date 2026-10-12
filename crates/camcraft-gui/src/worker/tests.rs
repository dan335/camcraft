use super::*;
use camcraft_session::fs::job_dir;
use std::time::{Duration, Instant};

fn wait(worker: &mut Worker) -> (DocId, Task, Reply) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        // Progress on the way is not the answer.
        if let Some(reply) = worker
            .poll()
            .into_iter()
            .find(|(_, _, r)| !matches!(r, Reply::Progress(_)))
        {
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
    let example =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rail-jig.camcraft");
    let mut worker = Worker::default();
    let ctx = egui::Context::default();
    worker.start(&ctx, 7, Task::Open, Request::Open { path: example });
    assert!(!worker.idle());
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
            plans: Vec::new(),
        },
    );
    assert!(!worker.idle());
    let (_, _, Reply::Ran(ran)) = wait(&mut worker) else {
        panic!("expected a run");
    };
    let (ran, scene) = (*ran).unwrap();
    assert!(worker.idle());

    let refs: Models<'_> = models.iter().map(|(n, m)| (n.as_str(), &**m)).collect();
    let direct = camcraft_session::run_setup(&job, 0, &refs).unwrap();
    let (a, b) = (&ran.result, &direct.result);
    assert_eq!(a.operations.len(), b.operations.len());
    for (x, y) in a.operations.iter().zip(&b.operations) {
        assert_eq!(x.passes, y.passes);
        assert_eq!(x.path.moves(), y.path.moves(), "{}: paths differ", x.name);
    }
    assert_eq!(a.verify.compared, b.verify.compared);
    assert_eq!(a.is_clean(), b.is_clean());
    // Its toolpaths come laid out for drawing, as the run made them.
    let drawn = Scene::build(&job, &job.setups[0], a);
    assert_eq!(scene.segments, drawn.segments);
    assert!(!scene.segments.is_empty());

    // Posted through the worker: rendered, as the session renders them,
    // then written.
    let dir = std::env::temp_dir().join(format!("camcraft-worker-post-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    worker.start(
        &ctx,
        7,
        Task::Post,
        Request::Post {
            setups: vec![(job.setups[0].clone(), ran.clone())],
            job_dir: job_dir(&path),
            dir: dir.clone(),
        },
    );
    let (_, _, Reply::Planned(planned)) = wait(&mut worker) else {
        panic!("expected programs");
    };
    let plan = (*planned).unwrap();
    let direct =
        camcraft_session::plan_posting(&[(&job.setups[0], &ran)], &job_dir(&path), &dir, None)
            .unwrap();
    let files: Vec<(PathBuf, String)> = plan
        .programs
        .iter()
        .map(|p| (p.path.clone(), p.text.clone()))
        .collect();
    assert!(!files.is_empty());
    assert!(
        files
            .iter()
            .map(|(p, _)| p)
            .eq(direct.programs.iter().map(|p| &p.path))
    );
    let task = Task::Write {
        dir: dir.clone(),
        refused: Vec::new(),
    };
    worker.start(
        &ctx,
        7,
        task,
        Request::Write {
            posting: Box::new(plan),
        },
    );
    let (_, _, Reply::Written(written)) = wait(&mut worker) else {
        panic!("expected programs written");
    };
    assert_eq!(written.unwrap(), files.len());
    for (path, text) in &files {
        assert_eq!(&std::fs::read_to_string(path).unwrap(), text);
    }
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
