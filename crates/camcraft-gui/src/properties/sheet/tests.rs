//! What a sheet's panel works out before it draws anything: where the sheet
//! is on the drawing, which part is under the pointer, what is said of how
//! it is laid, and which models its parts card offers.

use camcraft_core::arrange::{CopyLaid, Outcome, Stand};
use camcraft_core::job::{ModelDef, StockDef};
use camcraft_geom::contour::Region;
use eframe::egui;

use super::*;

/// A square ring, 40 across with a 20 hole, its front-left corner at `at`.
fn ring(at: DVec2) -> Region {
    let square = |lo: DVec2, side: f64| {
        Contour::new(vec![
            lo,
            lo + dvec2(side, 0.0),
            lo + dvec2(side, side),
            lo + dvec2(0.0, side),
        ])
        .unwrap()
    };
    Region::new(vec![square(at, 40.0), square(at + dvec2(10.0, 10.0), 20.0)])
}

/// A square `side` across, its front-left corner at `at`.
fn block(at: DVec2, side: f64) -> Region {
    Region::rectangle(at, at + DVec2::splat(side))
}

/// A copy placed, standing where its outline is.
fn placed(outline: Region) -> CopyLaid {
    CopyLaid {
        outcome: Outcome::Placed(Stand::NONE),
        outline: Some(outline),
    }
}

/// A copy left out, for `why`.
fn left(why: Outcome) -> CopyLaid {
    CopyLaid {
        outcome: why,
        outline: None,
    }
}

fn part(model: &str, copies: Vec<CopyLaid>) -> PartLaid {
    PartLaid {
        model: model.into(),
        copies,
        heights: [0.0, 18.0],
    }
}

/// A ring with a block standing in its hole, two more blocks beside it, and
/// a block with no room.
fn parts() -> Vec<PartLaid> {
    vec![
        part("ring", vec![placed(ring(dvec2(10.0, 10.0)))]),
        part(
            "block",
            vec![
                placed(block(dvec2(60.0, 10.0), 10.0)),
                placed(block(dvec2(80.0, 10.0), 10.0)),
                left(Outcome::NoRoom),
            ],
        ),
        part("pin", vec![placed(block(dvec2(27.0, 27.0), 6.0))]),
    ]
}

fn sheet(parts: Vec<PartLaid>) -> SheetLayout {
    SheetLayout {
        stock: "sheet".into(),
        models: parts.iter().map(|p| p.model.clone()).collect(),
        laid: Laid::Done {
            parts,
            used: 1830.0,
            length: 2440.0,
        },
    }
}

fn bed(from: Corner, along: Direction) -> Bed {
    let mut a = Arranged::new([200.0, 100.0, 18.0]);
    a.corner = [500.0, 0.0, 0.0];
    a.from = from;
    a.along = along;
    Bed::of(&a)
}

const CORNERS: [Corner; 4] = [
    Corner::FrontLeft,
    Corner::FrontRight,
    Corner::BackLeft,
    Corner::BackRight,
];

/// The sheet fits the drawing at one scale both ways, centred, its front at
/// the bottom and its left on the left; a point on the drawing is the point
/// of the sheet drawn there. Each corner packing starts from is drawn at
/// that corner of the sheet.
#[test]
fn the_sheet_is_drawn_to_scale_its_start_corner_where_it_is() {
    let b = bed(Corner::FrontLeft, Direction::X);
    let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(300.0, 300.0));
    let fit = Fit::new(b.lo, b.hi(), rect);
    // Twice as long as it is wide: as wide as the drawing, half as tall.
    assert_eq!(fit.scale, 1.5);
    let (lo, hi) = (fit.at(b.lo), fit.at(b.hi()));
    assert_eq!(lo, egui::pos2(10.0, 245.0));
    assert_eq!(hi, egui::pos2(310.0, 95.0));
    assert_eq!(rect.center(), egui::pos2(160.0, 170.0));
    for p in [b.lo, b.hi(), dvec2(560.5, 33.25)] {
        assert!(fit.back(fit.at(p)).distance(p) < 1e-4, "{p}");
    }
    assert!((fit.length(3.0) - 2.0).abs() < 1e-12);

    let screen = |c: Corner| match c {
        Corner::FrontLeft => egui::pos2(lo.x, lo.y),
        Corner::FrontRight => egui::pos2(hi.x, lo.y),
        Corner::BackLeft => egui::pos2(lo.x, hi.y),
        Corner::BackRight => egui::pos2(hi.x, hi.y),
    };
    for c in CORNERS {
        let b = bed(c, Direction::X);
        assert_eq!(fit.at(b.start()), screen(c), "{c:?}");
    }

    // A tall sheet fits by its height.
    let tall = Fit::new(dvec2(0.0, 0.0), dvec2(50.0, 200.0), rect);
    assert_eq!(tall.scale, 1.5);
    assert_eq!(tall.at(dvec2(0.0, 0.0)), egui::pos2(122.5, 320.0));
}

/// How far the parts reach is a line across the sheet that far from the edge
/// packing starts at, along the way it is used up, starting on the side
/// packing starts from.
#[test]
fn the_length_used_is_measured_from_where_packing_starts() {
    let used = 30.0;
    for c in CORNERS {
        for along in [Direction::X, Direction::Y] {
            let b = bed(c, along);
            let [a, z] = b.used_line(used);
            let start = b.start();
            match along {
                Direction::X => {
                    assert_eq!((a.x - start.x).abs(), used, "{c:?}");
                    assert_eq!(a.x, z.x);
                    assert_eq!(a.y, start.y);
                    assert_eq!((z.y - a.y).abs(), 100.0);
                }
                Direction::Y => {
                    assert_eq!((a.y - start.y).abs(), used, "{c:?}");
                    assert_eq!(a.y, z.y);
                    assert_eq!(a.x, start.x);
                    assert_eq!((z.x - a.x).abs(), 200.0);
                }
            }
            // Inside the sheet, from its start.
            for p in [a, z] {
                assert!(
                    p.cmpge(b.lo).all() && p.cmple(b.hi()).all(),
                    "{c:?} {along:?}"
                );
            }
        }
    }
    let b = bed(Corner::BackRight, Direction::Y);
    assert_eq!(b.used_line(30.0), [dvec2(700.0, 70.0), dvec2(500.0, 70.0)]);

    // The border, dashed inside the sheet; none where there is none.
    let line = bed(Corner::FrontLeft, Direction::X).border_line().unwrap();
    assert_eq!(line[0], dvec2(510.0, 10.0));
    assert_eq!(line[2], dvec2(690.0, 90.0));
    let mut none = Arranged::new([200.0, 100.0, 18.0]);
    none.border = 0.0;
    assert_eq!(Bed::of(&none).border_line(), None);
}

/// A point on a copy is on its part, and every copy of a part is found as it:
/// the pin standing in the ring's hole is the part at a point in the hole,
/// the ring the part at a point on it. A point just off a copy's outline is
/// on it within the slack, and nowhere near any is on none.
#[test]
fn a_point_on_the_drawing_is_on_the_part_whose_copy_is_there() {
    let parts = parts();
    assert_eq!(part_at(&parts, dvec2(12.0, 30.0), 0.0), Some(0), "the ring");
    assert_eq!(part_at(&parts, dvec2(30.0, 30.0), 0.0), Some(2), "the pin");
    assert_eq!(part_at(&parts, dvec2(22.0, 30.0), 0.0), None, "the hole");
    for (k, p) in parts.iter().enumerate() {
        for outline in p.copies.iter().filter_map(|c| c.outline.as_ref()) {
            let (lo, hi) = outline.bounds().unwrap();
            // Inside each copy, beside any hole it has.
            let inside = dvec2(lo.x + 1.0, (lo.y + hi.y) / 2.0);
            assert_eq!(part_at(&parts, inside, 0.0), Some(k), "{}", p.model);
        }
    }
    assert_eq!(part_at(&parts, dvec2(71.5, 15.0), 0.0), None);
    assert_eq!(
        part_at(&parts, dvec2(71.5, 15.0), 2.0),
        Some(1),
        "near enough"
    );
    assert_eq!(part_at(&parts, dvec2(150.0, 90.0), 2.0), None);
}

/// The caption counts every copy placed of every one asked for, and says how
/// far along the sheet they reach of its length in the panel's units; a sheet
/// not laid has none.
#[test]
fn the_caption_counts_copies_in_the_panels_units() {
    let laid = sheet(parts());
    assert_eq!(
        caption(&laid, Measure::Metric).as_deref(),
        Some("4 of 5 placed · 1830 of 2440 mm used")
    );
    assert_eq!(
        caption(&laid, Measure::Imperial).as_deref(),
        Some("4 of 5 placed · 72.05 of 96.06 in used")
    );
    let waiting = SheetLayout {
        laid: Laid::Waiting(vec!["ring".into()]),
        ..laid
    };
    assert_eq!(caption(&waiting, Measure::Metric), None);
}

/// Each part says how many of its copies are placed, and why any are left
/// out, as the checks word it.
#[test]
fn each_part_says_how_many_of_its_copies_are_placed() {
    let parts = parts();
    assert_eq!(placed_text(&parts[0]), "1 of 1");
    assert_eq!(placed_text(&parts[1]), "2 of 3");
    assert!(left_out(&parts[0]).is_empty());
    let why: Vec<String> = left_out(&parts[1]).into_iter().map(|(w, _)| w).collect();
    assert_eq!(why, ["no room for 1 of 3"]);

    let alone = part("side", vec![left(Outcome::NoRoom)]);
    assert_eq!(left_out(&alone)[0].0, "no room beside the others");
    let large = part(
        "table",
        vec![left(Outcome::TooLarge), left(Outcome::TooLarge)],
    );
    assert_eq!(placed_text(&large), "0 of 2");
    let (why, detail) = &left_out(&large)[0];
    assert_eq!(why, "too large for the sheet");
    assert!(detail.contains("at any turn it may take"));

    // The sheet lists only those with copies left out.
    let laid = sheet(parts.clone());
    let short: Vec<&str> = laid.left_out().map(|p| p.model.as_str()).collect();
    assert_eq!(short, ["block"]);
}

/// A sheet's parts card offers every model of the job no other sheet holds,
/// and, where those are all there are left, says why it offers nothing.
#[test]
fn add_offers_only_models_no_other_sheet_holds() {
    let mut job = Job::default();
    for name in ["a", "b", "c", "d"] {
        job.models.push(ModelDef {
            name: name.into(),
            path: format!("{name}.stl"),
            units: None,
            transform: Default::default(),
        });
    }
    let sheet_of = |models: &[&str]| {
        let mut a = Arranged::new([200.0, 100.0, 18.0]);
        a.parts = models.iter().map(|m| SheetPart::of(m)).collect();
        StockSpec::Arranged(a)
    };
    job.stocks = vec![
        StockDef {
            name: "first".into(),
            spec: sheet_of(&["a"]),
        },
        StockDef {
            name: "second".into(),
            spec: sheet_of(&["b"]),
        },
        StockDef {
            name: "block".into(),
            spec: StockSpec::default(),
        },
    ];
    let (withheld, why) = on_other_sheets(&job, "first").unwrap();
    assert_eq!(withheld, ["b"]);
    assert_eq!(
        why,
        "The others are on sheet 'second': a model goes on one sheet only."
    );
    let all: Vec<String> = job.models.iter().map(|m| m.name.clone()).collect();
    let names = Names {
        id: egui::Id::new("parts"),
        title: "Parts",
        topic: None,
        all: &all,
        empty: "",
        pick: None,
        more: None,
        withheld: Some((&withheld, &why)),
    };
    let included = vec!["a".to_string()];
    assert_eq!(names.offered(&included), ["c", "d"]);
    let full: Vec<String> = ["a", "c", "d"].map(String::from).to_vec();
    assert!(names.offered(&full).is_empty());
    assert_eq!(names.none_offered(&full), why);

    // From the other sheet, the first's part is the one withheld; from a box,
    // both sheets' are, named.
    assert_eq!(on_other_sheets(&job, "second").unwrap().0, ["a"]);
    let (both, why) = on_other_sheets(&job, "block").unwrap();
    assert_eq!(both, ["a", "b"]);
    assert!(why.contains("sheets 'first', 'second'"), "{why}");
    // With no other sheet, nothing is withheld, and all in is all in.
    job.stocks.truncate(1);
    assert!(on_other_sheets(&job, "first").is_none());
    let open = Names {
        withheld: None,
        ..names
    };
    assert_eq!(open.offered(&included), ["b", "c", "d"]);
    let every: Vec<String> = all.clone();
    assert_eq!(open.none_offered(&every), "All of them are in already.");
}

/// Each part's colour on the drawing comes round again after a few, and
/// follows the part's place among the sheet's parts now, so its row and its
/// copies agree.
#[test]
fn a_parts_colour_follows_its_place_on_the_sheet() {
    assert_eq!(tone(0), tone(TONES));
    assert_ne!(tone(0), tone(1));
    let mut a = Arranged::new([200.0, 100.0, 18.0]);
    a.parts = vec![SheetPart::of("pin"), SheetPart::of("ring")];
    let s = Shown {
        stock: "sheet".into(),
        bed: Bed::of(&a),
        order: a.parts.iter().map(|p| p.model.clone()).collect(),
        now: Now::Lost("stopped".into()),
    };
    assert_eq!(s.tone_of("ring", 0), 1);
    assert_eq!(s.tone_of("gone", 3), 3, "where the sheet no longer has it");
    assert_eq!(s.bounds(), (dvec2(0.0, 0.0), dvec2(200.0, 100.0)));
    assert!(s.current().is_none() && s.parts().is_empty());
}
