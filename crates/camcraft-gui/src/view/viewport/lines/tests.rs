use super::*;
use glam::DVec3;

/// An arc is drawn as lines along it, all of them belonging to its move: the
/// playhead, which counts moves, shows them together or not at all.
#[test]
fn an_arc_is_drawn_as_lines_along_it() {
    use crate::view::scene::{Scene, Segment};
    use camcraft_core::path::toolpath::{Arc, MoveKind, MoveRole};
    let straight = Segment {
        a: DVec3::new(-1.0, 0.0, 0.0),
        b: DVec3::new(0.0, 0.0, 0.0),
        kind: MoveKind::Linear,
        role: MoveRole::Cut,
        op: 0,
        feed: Some(600.0),
        ends_at: 0.0,
    };
    let half = Segment {
        a: DVec3::ZERO,
        b: DVec3::new(10.0, 0.0, 0.0),
        kind: MoveKind::Arc(Arc {
            centre: DVec3::new(5.0, 0.0, 0.0),
            axis: DVec3::Z,
            sweep: -std::f64::consts::PI,
        }),
        ..straight
    };
    let scene = Scene {
        segments: vec![straight, half, straight],
        ..Scene::default()
    };
    let (lines, ends) = line_instances(&scene);
    let arc = (ends[1] - ends[0]) as usize;
    assert!(arc > 8, "{arc} lines for a half turn");
    for l in &lines[1..=arc] {
        for p in [l.a, l.b] {
            let r = ((p[0] - 5.0).powi(2) + p[1].powi(2)).sqrt();
            assert!((r - 5.0).abs() < 1e-4, "{p:?} is off the arc");
        }
    }
    assert_eq!(lines_shown(&ends, 0), 0);
    assert_eq!(lines_shown(&ends, 1), 1);
    assert_eq!(lines_shown(&ends, 2), ends[1]);
    assert_eq!(lines_shown(&ends, 9), ends[2]);
    // Half way along in time is half way round.
    let mid = half.point_at(0.5);
    assert!((mid - DVec3::new(5.0, 5.0, 0.0)).length() < 1e-9, "{mid}");
    assert!((half.length() - 5.0 * std::f64::consts::PI).abs() < 1e-9);
}

/// The instance buffer is the scene in order, so the GPU's instance index is
/// the move's place in the program, which is what the playhead compares against.
#[test]
fn line_instances_keep_machine_order_and_roles() {
    use crate::view::scene::{Scene, Segment};
    use camcraft_core::path::toolpath::MoveRole;
    let seg = |x: f64, role, op| Segment {
        a: DVec3::new(x, 0.0, 0.0),
        b: DVec3::new(x + 1.0, 0.0, 0.0),
        kind: camcraft_core::path::toolpath::MoveKind::Linear,
        role,
        op,
        feed: None,
        ends_at: 0.0,
    };
    let scene = Scene {
        segments: vec![
            seg(0.0, MoveRole::Rapid, 0),
            seg(1.0, MoveRole::Cut, 0),
            seg(2.0, MoveRole::Plunge, 1),
        ],
        ..Scene::default()
    };
    let (moves, ends) = line_instances(&scene);
    let got: Vec<(f32, u32, u32)> = moves.iter().map(|m| (m.a[0], m.kind, m.op)).collect();
    assert_eq!(got, vec![(0.0, 1, 0), (1.0, 0, 0), (2.0, 2, 1)]);
    assert_eq!(ends, [1, 2, 3]);
    let outline = outline_instances(&[
        Outline {
            a: DVec3::ZERO,
            b: DVec3::X,
            kind: OutlineKind::Stock,
        },
        Outline {
            a: DVec3::ZERO,
            b: DVec3::Z,
            kind: OutlineKind::AxisZ,
        },
    ]);
    let kinds: Vec<u32> = outline.iter().map(|o| o.kind).collect();
    assert_eq!(
        kinds,
        [kind_of(OutlineKind::Stock), kind_of(OutlineKind::AxisZ)]
    );
}

/// Every kind of line that is not a move has a kind of its own after the
/// moves', within the 32 a mask holds, and its colour at that place.
#[test]
fn every_line_has_a_kind_and_a_colour_of_its_own() {
    let mut all = vec![
        OutlineKind::Stock,
        OutlineKind::AxisX,
        OutlineKind::AxisY,
        OutlineKind::AxisZ,
        OutlineKind::Selected,
        OutlineKind::Hover,
        OutlineKind::Boundary,
        OutlineKind::Limit,
        OutlineKind::Channel,
        OutlineKind::Ghost,
    ];
    all.extend(PlaneKind::ALL.map(OutlineKind::Plane));
    assert_eq!(all.len(), LINE_KINDS.len());
    let mut kinds: Vec<u32> = all.iter().map(|o| kind_of(*o)).collect();
    let colors = kind_colors();
    for (o, k) in all.iter().zip(&kinds) {
        assert!(*k as usize >= MOVE_KINDS && (*k as usize) < KINDS, "{o:?}");
        let want = LINE_KINDS.iter().find(|l| l.outline == *o).unwrap().color;
        assert_eq!(colors[*k as usize], linear_rgba(want, 1.0), "{o:?}");
    }
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds.len(), all.len(), "a kind each");
    for (i, role) in theme::ROLES.iter().enumerate() {
        assert_eq!(theme::role_index(*role) as usize, i);
        assert_eq!(colors[i], linear_rgba(theme::role_srgb(*role), 1.0));
    }
}

/// The stock's outline and the axes show as ticked, picks and planes always,
/// and moves by role; picks and the boundary are drawn wide.
#[test]
fn what_shows_is_what_is_ticked() {
    let bit = |o: OutlineKind| 1u32 << kind_of(o);
    let view = View {
        roles: 0b101,
        stock: false,
        origin: true,
        ..View::default()
    };
    let shown = kinds_shown(&view);
    assert_eq!(shown & 0xff, 0b101);
    assert_eq!(shown & bit(OutlineKind::Stock), 0);
    assert_ne!(shown & bit(OutlineKind::AxisY), 0);
    for o in [
        OutlineKind::Selected,
        OutlineKind::Channel,
        OutlineKind::Plane(PlaneKind::Retract),
    ] {
        assert_ne!(shown & bit(o), 0, "{o:?}");
    }
    let all = kinds_shown(&View {
        stock: true,
        ..view
    });
    assert_ne!(all & bit(OutlineKind::Stock), 0);

    let constants = wgsl_constants();
    let wide = bit(OutlineKind::Selected) | bit(OutlineKind::Hover) | bit(OutlineKind::Boundary);
    assert!(
        constants.contains(&format!("WIDE_KINDS: u32 = {wide}u;")),
        "{constants}"
    );
    assert!(constants.contains(&format!("LINE_KINDS: u32 = {KINDS}u;")));
    assert!(constants.contains("MOVE_KINDS: u32 = 8u;"));
}

/// An operation shown alone is passed as one more than its index, however
/// far down the setup it is, and none as zero.
#[test]
fn an_operation_shown_alone_is_passed_whole() {
    let alone = |only| {
        only_op(&View {
            only,
            ..View::default()
        })
    };
    assert_eq!(alone(None), 0);
    assert_eq!(alone(Some(0)), 1);
    assert_eq!(alone(Some(40)), 41);
}
