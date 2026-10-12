use super::*;
use crate::test_jobs::run_setup_zero;
use camcraft_core::job::Job;

#[test]
fn thresholds_sit_where_the_verifier_puts_them() {
    let budget = 0.45;
    assert_eq!(classify(10.0, None, budget), Class::OffPart);
    assert_eq!(
        classify(10.0 - 0.45, Some((10.0, 0.45)), budget),
        Class::OnPart
    );
    assert_eq!(
        classify(10.0 - 0.46, Some((10.0, 0.46)), budget),
        Class::Gouge
    );
    assert_eq!(
        classify(10.0 + LEFTOVER_LIMIT, Some((10.0, -LEFTOVER_LIMIT)), budget),
        Class::OnPart
    );
    assert_eq!(
        classify(
            10.0 + LEFTOVER_LIMIT + 0.01,
            Some((10.0, -LEFTOVER_LIMIT - 0.01)),
            budget
        ),
        Class::Leftover
    );
}

/// Beside a wall, what counts is how far below the lowest nearby part the
/// material is, not how far below the column's own height.
#[test]
fn beside_a_wall_the_floor_decides() {
    assert_eq!(classify(0.0, Some((10.0, 0.0)), 0.45), Class::OnPart);
    assert_eq!(classify(-0.5, Some((10.0, 0.5)), 0.45), Class::Gouge);
}

#[test]
fn leftover_deepens_with_what_is_left() {
    let faint = cell_color(ColorMode::Compare, Class::Leftover, LEFTOVER_LIMIT + 0.01);
    let strong = cell_color(ColorMode::Compare, Class::Leftover, 50.0);
    assert_eq!(
        strong,
        theme::AMBER_5,
        "saturates at the design system's amber"
    );
    let brightness = |c: [u8; 3]| c.iter().map(|v| u32::from(*v)).sum::<u32>();
    assert!(brightness(faint) < brightness(strong));
}

/// The picture agrees with the verdict: on a real run, the cells drawn red and
/// amber are exactly the columns the verifier counted as gouged and left over.
#[test]
fn the_picture_counts_what_the_verdict_counts() {
    let job: Job = toml::from_str(
            r#"
version = 1
tolerance = 0.1
[tools.flat]
kind = "flat"
diameter = 6.0
[[model]]
name = "hinge"
path = "fixtures/step/hinge_jig.step"
[[stock]]
name = "block"
kind = "around-models"
margin = [2.0, 2.0, 2.0]
[[setup]]
name = "top"
models = ["hinge"]
stocks = ["block"]
[[setup.operation]]
name = "rough"
strategy = "pocket-clearing"
tool = "flat"
passes = { stepdown = 4.0, stepover = 3.0, feed = 1800.0, stock_to_leave = { radial = 0.3, axial = 0.3 } }
"#,
        )
        .unwrap();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let result = run_setup_zero(&job, &repo).result;

    let finished = result.keyframes.last().unwrap().unpack().unwrap();
    let image = StockImage::build_shaded(&finished, &result.part, result.verify_budget).0;
    assert_eq!(image.count(Class::Gouge) as u64, result.verify.gouged);
    assert_eq!(image.count(Class::Leftover) as u64, result.verify.leftover);
    assert!(
        result.verify.leftover > 0,
        "a roughing pass that leaves stock should leave some over the limit"
    );
    let compared =
        image.count(Class::Gouge) + image.count(Class::Leftover) + image.count(Class::OnPart);
    assert_eq!(compared as u64, result.verify.compared);
    assert_eq!(image.heights.len(), (image.nx * image.ny) as usize);
    assert_eq!(image.codes.len(), image.heights.len());
}

/// A code keeps its class, and a leftover's shade, and nothing else collides
/// with the code for no cell.
#[test]
fn a_code_is_a_class_and_a_shade() {
    for class in CLASSES {
        for delta in [0.0, 0.5, 2.0, 50.0] {
            let c = code(class, delta);
            assert_eq!(class_of(c), Some(class), "{class:?} {delta}");
            assert_ne!(c, NO_CELL);
        }
    }
    assert!(code(Class::Leftover, 50.0) > code(Class::Leftover, LEFTOVER_LIMIT + 0.1));
    assert_eq!(class_of(NO_CELL), None);
}

#[test]
fn material_is_one_colour_but_a_gouge_is_red() {
    for class in CLASSES {
        let want = if class == Class::Gouge {
            theme::ERR
        } else {
            theme::MATERIAL
        };
        assert_eq!(cell_color(ColorMode::Material, class, 3.0), want);
    }
}

/// Patching only the cells a sweep changed gives the image built afresh: the
/// same heights, codes and counts.
#[test]
fn a_patch_is_a_rebuild_of_what_changed() {
    use camcraft_geom::synthetic;
    use camcraft_sim::stock::{MoveClass, SimMove, SweptTool};
    use glam::{dvec2, dvec3};

    struct Flat(f64);
    impl SweptTool for Flat {
        fn max_radius(&self) -> f64 {
            self.0
        }
        fn height_at(&self, d: f64) -> Option<f64> {
            (d <= self.0).then_some(0.0)
        }
    }
    let (lo, hi) = (dvec2(-10.0, -10.0), dvec2(10.0, 10.0));
    let part = PartSurface::from_mesh(&synthetic::plane(0.0, 12.0), lo, hi, 0.5).unwrap();
    {
        let mut stock = Stock::block(lo, hi, 5.0, -5.0, 0.5).unwrap();
        let mut image = StockImage::build_shaded(&stock, &part, 0.1).0;
        for (a, b, z) in [
            ((-8.0, -2.0), (8.0, -2.0), 1.0),
            ((0.0, -8.0), (0.0, 3.0), -1.0),
        ] {
            let moves = [
                SimMove {
                    to: dvec3(a.0, a.1, z),
                    class: MoveClass::MayCut,
                    arc: None,
                },
                SimMove {
                    to: dvec3(b.0, b.1, z),
                    class: MoveClass::MayCut,
                    arc: None,
                },
            ];
            let rect = stock.sweep(&moves, &Flat(1.5)).touched.unwrap();
            // Once as one rectangle, once as tiles in parallel.
            let mut tiled = image.clone();
            image.patch_many(&stock, &part, 0.1, &[rect]);
            let mut tiles = crate::stock::dirty::DirtyTiles::new(image.nx, image.ny);
            tiles.mark(rect);
            tiled.patch_many(&stock, &part, 0.1, &tiles.rects());
            let fresh = StockImage::build_shaded(&stock, &part, 0.1).0;
            for got in [&image, &tiled] {
                assert_eq!(got.heights, fresh.heights);
                assert_eq!(got.codes, fresh.codes);
                assert_eq!(got.counts, fresh.counts);
            }
        }
        assert!(
            image.count(Class::Gouge) > 0,
            "the second cut goes under the part"
        );
    }
}
