//! What the app shows is what the machine runs.
//!
//! These drive the real binary. The app runs a setup through
//! `camcraft_session::run_setup` and draws what comes back; the G-code must come
//! from that same path. So the check is direct: parse every program back into
//! positions and compare them with the paths `run_setup` produces, move for move. A
//! second, approximate path for the picture is exactly the defect this guards
//! against -- a bug could live in only the one nobody looks at.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

mod common;

use std::path::{Path, PathBuf};

use camcraft_core::toolpath::{MoveRole, Toolpath};
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::synthetic;
use camcraft_session::{Models, load_job_model, read_job, run_setup};
use common::scratch;

const JOB: &str = r#"
version = 1
tolerance = 0.1

[tools.rough]
kind = "flat"
diameter = 4.0
number = 1
rpm = 16000

[tools.finish]
kind = "ball"
diameter = 3.0
number = 2

[[model]]
name = "dome"
path = "dome.stl"
units = "mm"

[[stock]]
name = "block"
kind = "around-models"
margin = [1.0, 1.0, 2.0]

[[setup]]
name = "top"
models = ["dome"]
stocks = ["block"]

[[setup.operation]]
name = "rough"
strategy = "zlevel-rough"
tool = "rough"
passes = { stepdown = 2.0, stepover = 2.0, feed = 1800.0, stock_to_leave = { radial = 0.4, axial = 0.4 } }
heights = { clearance = { ref = "stock-top", offset = 5.0 } }

[[setup.operation]]
name = "finish"
strategy = "parallel"
tool = "finish"
passes = { stepover = 1.2, feed = 2000.0 }
heights = { clearance = { ref = "stock-top", offset = 5.0 } }
"#;

/// Write a mesh as binary STL: an 80-byte header, a count, then 50 bytes a facet.
fn write_stl(mesh: &TriMesh, path: &Path) {
    let mut bytes = vec![0u8; 80];
    bytes.extend(u32::try_from(mesh.len()).unwrap().to_le_bytes());
    for i in 0..mesh.len() {
        let t = mesh.triangle(i).unwrap();
        bytes.extend([0u8; 12]); // normal; readers recompute it
        for v in t {
            for c in [v.x, v.y, v.z] {
                #[allow(clippy::cast_possible_truncation)]
                bytes.extend((c as f32).to_le_bytes());
            }
        }
        bytes.extend([0u8; 2]);
    }
    std::fs::write(path, bytes).unwrap();
}

fn dome(dir: &Path) -> PathBuf {
    let path = dir.join("dome.stl");
    write_stl(&synthetic::uv_sphere(6.0, 40, 56), &path);
    path
}

fn camcraft(args: &[&str]) -> std::process::Output {
    let out = common::camcraft(args);
    // 0 is clean and 2 is warnings worth reading; either way output was written.
    // 1 is a refusal, which these jobs must not provoke.
    assert!(
        matches!(out.status.code(), Some(0 | 2)),
        "camcraft {args:?} exited {:?}:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Position as the post writes it: three decimals, and -0 is 0.
fn at(p: glam::DVec3) -> [f64; 3] {
    [p.x, p.y, p.z].map(|x| format!("{x:.3}").parse::<f64>().unwrap() + 0.0)
}

/// Every motion in a Masso program: `(rapid, position)` per block that moves.
///
/// The machine's own moves, up and home (`G28`, `G53`), are not the path's; nor
/// is a block that leaves the position partly unknown, as the first move goes
/// across before it goes down, so the path's first move is where both have gone.
fn motions(program: &str) -> Vec<(bool, [f64; 3])> {
    let mut rapid = true;
    let mut pos = [f64::NAN; 3];
    let mut out = Vec::new();
    for line in program.lines() {
        let line = line.trim();
        if line.starts_with('(') || line.contains("G28") || line.contains("G53") {
            continue;
        }
        let mut moved = false;
        for word in line.split_whitespace() {
            let (letter, value) = word.split_at(1);
            match letter {
                "G" if value == "0" => rapid = true,
                "G" if value == "1" => rapid = false,
                "X" | "Y" | "Z" => {
                    let axis = usize::from(letter.as_bytes()[0] - b'X');
                    pos[axis] = value.parse::<f64>().unwrap() + 0.0;
                    moved = true;
                }
                _ => {}
            }
        }
        if moved && pos.iter().all(|v| v.is_finite()) {
            out.push((rapid, pos));
        }
    }
    out
}

/// The path as the post will have written it: rounded, and without the moves that
/// round to where the tool already is, which the renderer rightly drops.
fn expected(path: &Toolpath) -> Vec<(bool, [f64; 3])> {
    let mut out: Vec<(bool, [f64; 3])> = Vec::new();
    for m in path.moves() {
        let p = at(m.to);
        if out.last().is_some_and(|(_, q)| *q == p) {
            continue;
        }
        out.push((m.role == MoveRole::Rapid, p));
    }
    out
}

/// Setup 0 of the job at `path`, run in-process exactly as the app runs it.
fn run_in_process(path: &Path) -> Vec<(String, Toolpath)> {
    let job = read_job(path).unwrap();
    let dir = path.parent().unwrap();
    let loaded: Vec<_> = job
        .models_for(0)
        .into_iter()
        .map(|m| {
            (
                m.name.clone(),
                load_job_model(dir, m, job.tolerance).unwrap(),
            )
        })
        .collect();
    let models: Models<'_> = loaded.iter().map(|(n, m)| (n.as_str(), m)).collect();
    run_setup(&job, 0, &models)
        .unwrap()
        .result
        .operations
        .into_iter()
        .map(|o| (o.name, o.path))
        .collect()
}

fn assert_same(name: &str, program: &Path, want: &[(bool, [f64; 3])]) {
    let got = motions(&std::fs::read_to_string(program).unwrap());
    assert!(want.len() > 50, "{name}: suspiciously short path");
    assert_eq!(
        got.len(),
        want.len(),
        "{name}: program has {} motions, the run has {}",
        got.len(),
        want.len()
    );
    for (k, (g, w)) in got.iter().zip(want).enumerate() {
        assert_eq!(g, w, "{name}: motion {k} differs");
    }
}

#[test]
fn every_program_is_the_path_the_app_shows() {
    let dir = scratch("every_program_is_the_path_the_app_shows");
    dome(&dir);
    let job = dir.join("job.camcraft");
    std::fs::write(&job, JOB).unwrap();
    let programs = dir.join("out");

    camcraft(&[
        "job",
        job.to_str().unwrap(),
        "--profile",
        "masso",
        "--out",
        programs.to_str().unwrap(),
    ]);

    let ops = run_in_process(&job);
    let names: Vec<&str> = ops.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["rough", "finish"], "every operation, in order");
    for (i, (name, path)) in ops.iter().enumerate() {
        let file = programs.join(format!("top-{:02}-{name}.nc", i + 1));
        assert_same(name, &file, &expected(path));
    }
}

/// `post` is a one-operation job, not a separate engine path.
#[test]
fn post_is_the_same_path_as_the_equivalent_job() {
    let dir = scratch("post_is_the_same_path_as_the_equivalent_job");
    let model = dome(&dir);
    let flags = dir.join("flags.nc");
    camcraft(&[
        "post",
        model.to_str().unwrap(),
        "--units",
        "mm",
        "--tool",
        "ball",
        "--diameter",
        "3",
        "--stepover",
        "1.2",
        "--tolerance",
        "0.1",
        "--clearance",
        "5",
        "--feed",
        "2000",
        "--tool-number",
        "1",
        "--out",
        flags.to_str().unwrap(),
    ]);

    // The same operation, written out as a job file.
    let job = dir.join("job.camcraft");
    std::fs::write(
        &job,
        r#"
version = 1
tolerance = 0.1
[tools.cutter]
kind = "ball"
diameter = 3.0
number = 1
[[model]]
name = "dome"
path = "dome.stl"
units = "mm"
[[stock]]
name = "block"
kind = "around-models"
[[setup]]
name = "finish"
models = ["dome"]
stocks = ["block"]
[[setup.operation]]
name = "finish"
strategy = "parallel"
tool = "cutter"
passes = { stepover = 1.2, feed = 2000.0 }
heights = { clearance = { ref = "model-top", offset = 5.0 } }
linking = { retract_feed = 2000.0, plunge_feed = 500.0 }
"#,
    )
    .unwrap();
    let out = dir.join("out");
    camcraft(&["job", job.to_str().unwrap(), "--out", out.to_str().unwrap()]);

    let from_job = motions(&std::fs::read_to_string(out.join("finish-01-finish.nc")).unwrap());
    assert!(from_job.len() > 50);
    assert_same("post", &flags, &from_job);
}

/// Without `--out`, a job is run and checked, and nothing is written.
#[test]
fn a_job_without_out_writes_nothing() {
    let dir = scratch("a_job_without_out_writes_nothing");
    dome(&dir);
    let job = dir.join("job.camcraft");
    std::fs::write(&job, JOB).unwrap();
    let out = camcraft(&["job", job.to_str().unwrap()]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("checks passed"), "{err}");
    let written: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n != "job.camcraft" && n != "dome.stl")
        })
        .collect();
    assert!(written.is_empty(), "wrote: {written:?}");
}

/// A job's models are found beside it, and an STL among them must say its units.
#[test]
fn a_job_names_its_own_models() {
    let dir = scratch("a_job_names_its_own_models");
    dome(&dir);
    let job = dir.join("job.camcraft");
    std::fs::write(&job, JOB.replace("units = \"mm\"\n", "")).unwrap();
    // Refused, so not through `camcraft`, which insists on success.
    let out = common::camcraft(&["job", job.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("model 'dome'") && err.contains("units"),
        "{err}"
    );

    std::fs::write(&job, JOB.replace("dome.stl", "elsewhere/dome.stl")).unwrap();
    let out = common::camcraft(&["job", job.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("elsewhere/dome.stl"), "{err}");
}
