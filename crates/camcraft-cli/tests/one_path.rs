//! What the inspector shows is what the machine runs.
//!
//! These drive the real binary. The inspection document and the G-code must come
//! from one engine path, so the check is direct: parse every program back into
//! positions and compare them with the path the inspection document holds, move for
//! move. A second, approximate path for the picture is exactly the defect this
//! guards against -- a bug could live in only the one nobody looks at.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use camcraft_geom::mesh::TriMesh;
use camcraft_geom::synthetic;
use serde_json::Value;

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
stepdown = 2.0
passes = { stepover = 2.0, feed = 1800.0, stock_to_leave = { radial = 0.4, axial = 0.4 } }
heights = { clearance = { ref = "stock-top", offset = 5.0 } }

[[setup.operation]]
name = "finish"
strategy = "parallel"
tool = "finish"
passes = { stepover = 1.2, feed = 2000.0 }
heights = { clearance = { ref = "stock-top", offset = 5.0 } }
"#;

/// A fresh directory per test, under the target directory Cargo provides.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

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
    let out = Command::new(env!("CARGO_BIN_EXE_camcraft"))
        .args(args)
        .output()
        .unwrap();
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

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Position as the post writes it: three decimals, and -0 is 0.
fn at(v: &Value) -> [f64; 3] {
    let a = v.as_array().unwrap();
    std::array::from_fn(|i| {
        let x = a.get(i).and_then(Value::as_f64).unwrap();
        format!("{x:.3}").parse::<f64>().unwrap() + 0.0
    })
}

/// Every motion in a Masso program: `(rapid, position)` per block that moves.
fn motions(program: &str) -> Vec<(bool, [f64; 3])> {
    let mut rapid = true;
    let mut pos = [f64::NAN; 3];
    let mut out = Vec::new();
    for line in program.lines() {
        let line = line.trim();
        if line.starts_with('(') {
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
        if moved {
            out.push((rapid, pos));
        }
    }
    out
}

/// The path as the post will have written it: rounded, and without the moves that
/// round to where the tool already is, which the renderer rightly drops.
fn expected(path: &Value) -> Vec<(bool, [f64; 3])> {
    let mut out: Vec<(bool, [f64; 3])> = Vec::new();
    for m in path.as_array().unwrap() {
        let p = at(&m["to"]);
        if out.last().is_some_and(|(_, q)| *q == p) {
            continue;
        }
        out.push((m["role"] == "rapid", p));
    }
    out
}

#[test]
fn every_program_is_the_path_the_inspector_shows() {
    let dir = scratch("every_program_is_the_path_the_inspector_shows");
    dome(&dir);
    let job = dir.join("job.camcraft");
    std::fs::write(&job, JOB).unwrap();
    let json = dir.join("inspect.json");
    let programs = dir.join("out");

    camcraft(&[
        "job",
        job.to_str().unwrap(),
        "--profile",
        "masso",
        "--inspect",
        json.to_str().unwrap(),
        "--out",
        programs.to_str().unwrap(),
    ]);

    let doc = read_json(&json);
    assert_eq!(doc["format"], 2);
    let ops = doc["operations"].as_array().unwrap();
    let names: Vec<&str> = ops.iter().map(|o| o["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["rough", "finish"], "every operation, in order");

    for (i, op) in ops.iter().enumerate() {
        let name = op["name"].as_str().unwrap();
        let file = programs.join(format!("top-{:02}-{name}.nc", i + 1));
        let got = motions(&std::fs::read_to_string(&file).unwrap());
        let want = expected(&op["path"]);
        assert!(want.len() > 50, "{name}: suspiciously short path");
        assert_eq!(
            got.len(),
            want.len(),
            "{name}: program has {} motions, inspection shows {}",
            got.len(),
            want.len()
        );
        for (k, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(g, w, "{name}: motion {k} differs");
        }
    }
}

/// `inspect` is a one-operation job, not a separate engine path.
#[test]
fn inspect_is_the_same_path_as_the_equivalent_job() {
    let dir = scratch("inspect_is_the_same_path_as_the_equivalent_job");
    let model = dome(&dir);
    let flags = dir.join("flags.json");
    camcraft(&[
        "inspect",
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
    let file = dir.join("file.json");
    camcraft(&[
        "job",
        job.to_str().unwrap(),
        "--inspect",
        file.to_str().unwrap(),
    ]);

    let (a, b) = (read_json(&flags), read_json(&file));
    let (pa, pb) = (&a["operations"][0]["path"], &b["operations"][0]["path"]);
    assert!(pa.as_array().unwrap().len() > 50);
    assert!(pa == pb, "inspect and job produced different paths");
    assert_eq!(a["job"]["verify"], b["job"]["verify"]);
}

/// Inspecting alone writes nothing a machine can read.
#[test]
fn job_with_only_inspect_writes_no_program() {
    let dir = scratch("job_with_only_inspect_writes_no_program");
    dome(&dir);
    let job = dir.join("job.camcraft");
    std::fs::write(&job, JOB).unwrap();
    let json = dir.join("inspect.json");
    camcraft(&[
        "job",
        job.to_str().unwrap(),
        "--inspect",
        json.to_str().unwrap(),
    ]);
    assert!(json.exists());
    let written: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "nc"))
        .collect();
    assert!(written.is_empty(), "wrote programs: {written:?}");
}

/// Neither output asked for is a mistake, and is refused before any work is done.
#[test]
fn job_with_no_output_is_refused() {
    let out = Command::new(env!("CARGO_BIN_EXE_camcraft"))
        .args(["job", "job.camcraft"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--out") && err.contains("--inspect"), "{err}");
}

/// A job's models are found beside it, and an STL among them must say its units.
#[test]
fn a_job_names_its_own_models() {
    let dir = scratch("a_job_names_its_own_models");
    dome(&dir);
    let job = dir.join("job.camcraft");
    std::fs::write(&job, JOB.replace("units = \"mm\"\n", "")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_camcraft"))
        .args(["job", job.to_str().unwrap(), "--inspect", "x.json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("model 'dome'") && err.contains("units"),
        "{err}"
    );

    std::fs::write(&job, JOB.replace("dome.stl", "elsewhere/dome.stl")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_camcraft"))
        .args(["job", job.to_str().unwrap(), "--inspect", "x.json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("elsewhere/dome.stl"), "{err}");
}
