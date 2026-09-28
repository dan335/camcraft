//! A 2D contour round the lego brick, as Fusion posts it and as camcraft does.
//!
//! `fixtures/nc/lego_2d_contour_01.nc` is Fusion's 2D Contour round the brick's
//! outline, ramped into at three depths, posted for a Masso: up before the tool
//! change, across then down, Fusion's three-piece lead onto the ring once (a
//! vertical arc, a straight, a level arc), then one spiral: down along the ring
//! to a depth, once round there, down again, and so on, never lifting; the lead
//! off once, and home at the end. camcraft's program for the same contour must
//! have the same shape. The two are not in the same place or units, as their
//! setups differ, so each program is reduced to what each block does.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::Path;

use common::{camcraft, fixture, scratch};

/// What a block does, as far as its shape goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    /// Straight up out of everything, before a tool change or at the end.
    Up,
    /// Home across.
    Home,
    /// A rapid across, at the height the tool is.
    Across,
    /// A rapid up or down.
    RapidZ,
    /// A feed straight down.
    Plunge,
    /// A feed straight up.
    Retract,
    /// A straight feed across, level.
    Line,
    /// A straight feed across and down: a ramp.
    Ramp,
    /// An arc in the level plane going down: a helix.
    Helix,
    /// An arc in an upright plane.
    UprightArc,
    /// An arc in the level plane.
    LevelArc,
    /// The spindle's stop, or the tool change.
    Machine,
}

/// Each block of a program, reduced to what it does.
fn blocks(program: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let (mut motion, mut plane) = (0u8, 17u8);
    let mut z: Option<f64> = None;
    for line in program.lines() {
        let words: Vec<&str> = line
            .split_whitespace()
            .filter(|w| !w.starts_with('N') && !w.starts_with('('))
            .collect();
        if words.is_empty() {
            continue;
        }
        if words.contains(&"G28") {
            out.push(if words.iter().any(|w| w.starts_with('X')) {
                Block::Home
            } else {
                Block::Up
            });
            z = None;
            continue;
        }
        if words.iter().any(|w| matches!(*w, "M5" | "M6")) {
            out.push(Block::Machine);
            continue;
        }
        let mut axes = (false, false);
        let mut to_z = None;
        for w in &words {
            let (letter, value) = w.split_at(1);
            match letter {
                "G" => match value.trim_end_matches('.') {
                    "0" | "00" => motion = 0,
                    "1" | "01" => motion = 1,
                    "2" | "02" => motion = 2,
                    "3" | "03" => motion = 3,
                    "17" => plane = 17,
                    "18" => plane = 18,
                    "19" => plane = 19,
                    _ => {}
                },
                "X" | "Y" => axes.0 = true,
                "Z" => {
                    axes.1 = true;
                    to_z = value.parse::<f64>().ok();
                }
                _ => {}
            }
        }
        if !axes.0 && !axes.1 {
            continue;
        }
        let block = match motion {
            0 if axes.0 => Block::Across,
            0 => Block::RapidZ,
            1 if axes.0 && axes.1 => Block::Ramp,
            1 if axes.0 => Block::Line,
            1 => match (z, to_z) {
                (Some(a), Some(b)) if b > a => Block::Retract,
                _ => Block::Plunge,
            },
            _ if plane == 17 && axes.1 => Block::Helix,
            _ if plane == 17 => Block::LevelArc,
            _ => Block::UprightArc,
        };
        if to_z.is_some() {
            z = to_z;
        }
        out.push(block);
    }
    // A move made in two blocks is the same move.
    out.dedup_by(|a, b| a == b && matches!(a, Block::RapidZ | Block::Plunge));
    out
}

/// The contour, set as Fusion's is: round the brick's outline, outside it, at
/// three depths, ramped into, with Fusion's leads.
fn job(dir: &Path) -> String {
    let text = format!(
        r#"
version = 1
tolerance = 0.05

[tools.quarter]
kind = "flat"
diameter = 6.35
number = 2
rpm = 16000

[[model]]
name = "lego"
path = "{}"

[[stock]]
name = "block"
kind = "around-models"
margin = [50.8, 12.7, 5.08]

[[setup]]
name = "top"
models = ["lego"]
stocks = ["block"]

[setup.origin]
kind = "box-point"
of = "stock"
x = "min"
y = "min"
z = "min"

[[setup.operation]]
name = "contour"
strategy = "profile"
tool = "quarter"
heights = {{ bottom = {{ ref = "model-bottom", offset = 1.0 }} }}
passes = {{ feed = 3810.0, stepdown = 15.0 }}
linking = {{ entry = "ramp" }}
"#,
        fixture("lego_4x2_01.step").replace('\\', "/")
    );
    let path = dir.join("lego.camcraft");
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn a_contour_is_the_shape_of_fusions() {
    let fusion = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/nc/lego_2d_contour_01.nc"),
    )
    .unwrap();
    let dir = scratch("fusion-contour");
    let out = dir.join("out");
    let run = camcraft(&["job", &job(&dir), "--out", &out.to_string_lossy()]);
    assert!(
        run.status.code() == Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let program = std::fs::read_to_string(out.join("top-01-contour.nc")).unwrap();

    use Block::*;
    // A run of moves going down is ramping, a run of level ones is going round
    // (the lead's straight and arc onto the ring count with the lap after);
    // what matters is the order of them, not how many sides each has.
    let shape = |blocks: Vec<Block>| {
        let mut out: Vec<Block> = Vec::new();
        for b in blocks {
            // camcraft feeds up out of the cut to the safe distance before it
            // rapids, where Fusion rapids straight out; and it rapids down to
            // the lead where the lead is clear of the material, where Fusion
            // feeds the last of the way: the differences kept, on purpose.
            if matches!(b, Retract | Plunge) {
                continue;
            }
            let b = match b {
                Ramp | Helix => Ramp,
                Line | LevelArc => Line,
                b => b,
            };
            if out.last() != Some(&b) || !matches!(b, Ramp | Line | RapidZ) {
                out.push(b);
            }
        }
        out
    };
    let expected = [
        Across, RapidZ, // over the start, and down
        UprightArc, Line, // Fusion's lead onto it
        Ramp, Line, Ramp, Line, Ramp, Line, // down to each depth, and round
        UprightArc, RapidZ, // off, once, and out
    ];
    for (name, text) in [("Fusion's", &fusion), ("camcraft's", &program)] {
        let got = shape(blocks(text));
        let start = got.iter().position(|b| *b == Across).unwrap();
        assert_eq!(
            got[start..start + expected.len()],
            expected,
            "{name} program:\n{text}"
        );
        // Up before the tool change, and raised and home at the end.
        let up = got[..start].iter().position(|b| *b == Up);
        let change = got[..start].iter().rposition(|b| *b == Machine);
        assert!(up.is_some() && up < change, "{name} program:\n{text}");
        assert_eq!(got[got.len() - 2..], [Up, Home], "{name} program:\n{text}");
    }
}
