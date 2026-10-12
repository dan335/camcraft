//! A part placed in an assembly, through the real kernel.
//!
//! The file's checks have to be made where the assembly puts the part, or they
//! either pass a mesh that is somewhere else or refuse one that is right. So the
//! hinge jig is wrapped in an assembly that turns it a quarter about Z and moves
//! it, and the kernel, which places it, is the judge of where it goes: the checks
//! must pass, and the mesh must be the plain part's, moved.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::indexing_slicing
)]

use std::path::Path;

use camcraft_import::step::{FileCheck, load_step};
use glam::{DAffine3, DVec3, dvec3};
use step_check::part21;

/// Tessellation tolerance, in millimetres.
const TOLERANCE: f64 = 0.01;

/// The id of the entity a line of the file defines, for a line that starts with
/// `#<id>=<kind>(`.
fn id_of(text: &str, kind: &str) -> u64 {
    text.lines()
        .find_map(|l| {
            let (id, body) = l.split_once('=')?;
            body.starts_with(&format!("{kind}("))
                .then(|| id.trim_start_matches('#').parse().ok())?
        })
        .unwrap_or_else(|| panic!("no {kind}"))
}

/// `text`, a single part, made the one component of a new root product placed by
/// the transform taking the part's own placement onto (4, 2, 1) in, turned so the
/// part's X runs along Y.
fn wrapped(text: &str) -> String {
    let pd = id_of(text, "PRODUCT_DEFINITION");
    let pd_context = id_of(text, "PRODUCT_DEFINITION_CONTEXT");
    let context = id_of(text, "PRODUCT_CONTEXT");
    let rep = id_of(text, "SHAPE_REPRESENTATION");
    // SHAPE_REPRESENTATION('',(#own),#units)
    let rep_line = text
        .lines()
        .find(|l| l.starts_with(&format!("#{rep}=")))
        .unwrap();
    let ids: Vec<u64> = rep_line
        .split('#')
        .skip(2)
        .map(|s| {
            s.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap()
        })
        .collect();
    let (own, units) = (ids[0], ids[1]);
    let n = text
        .lines()
        .filter_map(|l| l.strip_prefix('#')?.split('=').next()?.parse::<u64>().ok())
        .max()
        .unwrap();
    let e = |k: u64| n + k;
    let added = format!(
        "#{p}=PRODUCT('asm','asm',$,(#{context}));
#{f}=PRODUCT_DEFINITION_FORMATION('',$,#{p});
#{d}=PRODUCT_DEFINITION('asm','asm',#{f},#{pd_context});
#{s}=PRODUCT_DEFINITION_SHAPE('',$,#{d});
#{r}=SHAPE_REPRESENTATION('',(#{o},#{at}),#{units});
#{o}=AXIS2_PLACEMENT_3D('',#{op},$,$);
#{op}=CARTESIAN_POINT('',(0.,0.,0.));
#{at}=AXIS2_PLACEMENT_3D('',#{ap},#{az},#{ax});
#{ap}=CARTESIAN_POINT('',(4.,2.,1.));
#{az}=DIRECTION('',(0.,0.,1.));
#{ax}=DIRECTION('',(0.,1.,0.));
#{sdr}=SHAPE_DEFINITION_REPRESENTATION(#{s},#{r});
#{u}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','hinge','',#{d},#{pd},$);
#{us}=PRODUCT_DEFINITION_SHAPE('',$,#{u});
#{t}=ITEM_DEFINED_TRANSFORMATION('','',#{own},#{at});
#{rr}=( REPRESENTATION_RELATIONSHIP('','',#{rep},#{r}) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{t}) SHAPE_REPRESENTATION_RELATIONSHIP() );
#{c}=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{rr},#{us});
ENDSEC;",
        p = e(1),
        f = e(2),
        d = e(3),
        s = e(4),
        r = e(5),
        o = e(6),
        op = e(7),
        at = e(8),
        ap = e(9),
        az = e(10),
        ax = e(11),
        sdr = e(12),
        u = e(13),
        us = e(14),
        t = e(15),
        rr = e(16),
        c = e(17),
    );
    // The DATA section's end; the header's ENDSEC comes first.
    let end = text.rfind("ENDSEC;").unwrap();
    format!("{}{added}{}", &text[..end], &text[end + "ENDSEC;".len()..])
}

#[test]
fn a_part_in_an_assembly_is_checked_where_the_assembly_puts_it() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/step/hinge_jig.step");
    let text = std::fs::read_to_string(&path).unwrap();
    let dir = std::env::temp_dir().join(format!("camcraft-assembly-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let placed_path = dir.join("hinge_in_assembly.step");
    let placed_text = wrapped(&text);
    std::fs::write(&placed_path, &placed_text).unwrap();

    assert!(!part21::facts(&placed_text).unwrap().unplaced);
    let (plain, _) = load_step(&path, TOLERANCE).unwrap();
    let (placed, report) = load_step(&placed_path, TOLERANCE).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(report.check, FileCheck::Passed);

    // The part's own placement is at its origin, so the move is the target's.
    let moved = DAffine3::from_cols(DVec3::Y, -DVec3::X, DVec3::Z, dvec3(4.0, 2.0, 1.0) * 25.4);
    let b = plain.mesh().bounds().unwrap();
    let corners = (0..8).map(|i| {
        let pick = |bit: usize, lo: f64, hi: f64| if i & bit == 0 { lo } else { hi };
        moved.transform_point3(dvec3(
            pick(1, b.min.x, b.max.x),
            pick(2, b.min.y, b.max.y),
            pick(4, b.min.z, b.max.z),
        ))
    });
    let (lo, hi) = part21::bounds(&corners.collect::<Vec<_>>()).unwrap();
    let got = placed.mesh().bounds().unwrap();
    assert!(
        (got.min - lo).abs().max_element() < 1e-6 && (got.max - hi).abs().max_element() < 1e-6,
        "placed at {:?}..{:?}, expected {lo:?}..{hi:?}",
        got.min,
        got.max
    );
}
