use super::*;

/// The shape of a Fusion export: three length units defined, one assigned.
const INCH: &str = "ISO-10303-21;
HEADER; FILE_SCHEMA (('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('corner; with a semicolon',(1.,2.,-3.5E-01));
#3=CARTESIAN_POINT('uv',(0.5,0.5));
#4=VERTEX_POINT('',#2);
#5=CIRCLE('',#9,0.25);
#8=CONICAL_SURFACE('',#9,0.125,0.785398);
#10=CARTESIAN_POINT('',(1.E-05,0.,0.));
#6=ADVANCED_FACE('',(#7),#8,.T.);
/* a comment; with a semicolon */
#956=( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#954))
GLOBAL_UNIT_ASSIGNED_CONTEXT((#959,#963,#964)) REPRESENTATION_CONTEXT('','3D') );
#958=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);
#959=( CONVERSION_BASED_UNIT('inch',#962) LENGTH_UNIT() NAMED_UNIT(#958) );
#960=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );
#961=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.) );
#962=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#960);
#963=( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) );
#964=( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() );
ENDSEC;
END-ISO-10303-21;
";

#[test]
fn resolves_the_assigned_unit_not_the_first_one_defined() {
    let f = facts(INCH).unwrap();
    assert_eq!(f.unit.name, "inch");
    assert!((f.unit.mm - 25.4).abs() < 1e-12);
}

#[test]
fn points_are_in_millimetres_and_only_3d() {
    let f = facts(INCH).unwrap();
    assert_eq!(
        point("CARTESIAN_POINT('uv',(0.5,0.5))"),
        None,
        "a 2D parameter-space point is skipped"
    );
    assert_eq!(
        point("CARTESIAN_POINT('',(1.E-05,0.,0.))"),
        Some(DVec3::new(1e-5, 0.0, 0.0)),
        "1.E-05 parses"
    );
    assert_eq!(f.vertices, vec![DVec3::new(25.4, 50.8, -0.35 * 25.4)]);
    // Grown by the circle's radius, not the cone's semi-angle, which is larger.
    let (lo, hi) = f.extent.unwrap();
    let r = 0.25 * 25.4;
    assert!((lo.x - (0.0 - r)).abs() < 1e-9 && (hi.y - (50.8 + r)).abs() < 1e-9);
    assert_eq!(f.faces, 1);
    assert!(!f.transformed);
}

#[test]
fn millimetres_directly() {
    let mm = INCH.replace("#959,#963", "#960,#963");
    assert!((facts(&mm).unwrap().unit.mm - 1.0).abs() < 1e-12);
    let m = INCH.replace("#959,#963", "#961,#963");
    assert!((facts(&m).unwrap().unit.mm - 1000.0).abs() < 1e-12);
}

/// A torus from revolving a small circle whose centre sits 180 from the axis: it
/// reaches 190 on the far side, where the file has no points at all.
#[test]
fn a_revolved_surface_is_bounded_on_the_far_side_of_its_axis() {
    let mm = INCH.replace("#959,#963", "#960,#963").replace(
        "#6=ADVANCED_FACE",
        "#20=CARTESIAN_POINT('',(180.,0.,0.));\n\
             #21=AXIS2_PLACEMENT_3D('',#20,$,$);\n\
             #22=CIRCLE('',#21,10.);\n\
             #23=CARTESIAN_POINT('',(0.,0.,0.));\n\
             #24=AXIS1_PLACEMENT('',#23,$);\n\
             #25=SURFACE_OF_REVOLUTION('',#22,#24);\n\
             #6=ADVANCED_FACE",
    );
    let (lo, hi) = facts(&mm).unwrap().extent.unwrap();
    assert!(lo.x <= -190.0 && lo.y <= -190.0, "far side: {lo:?}");
    assert!(hi.x >= 190.0);
}

/// Never default. A file with no length unit is refused.
#[test]
fn a_missing_unit_is_an_error_not_millimetres() {
    let none = INCH.replace("#959,#963,#964", "#963,#964");
    assert_eq!(facts(&none).unwrap_err(), Part21Error::NoLengthUnit);
}

#[test]
fn an_unknown_unit_is_an_error() {
    let odd = INCH.replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT(.MILLI.,.FURLONG.)");
    assert!(matches!(
        facts(&odd).unwrap_err(),
        Part21Error::UnknownUnit { .. }
    ));
}

#[test]
fn conflicting_contexts_are_an_error() {
    let two = INCH.replace(
            "#958=DIMENSIONAL",
            "#957=( GLOBAL_UNIT_ASSIGNED_CONTEXT((#960)) REPRESENTATION_CONTEXT('','3D') );\n#958=DIMENSIONAL",
        );
    assert!(matches!(
        facts(&two).unwrap_err(),
        Part21Error::ConflictingUnits(_)
    ));
}

#[test]
fn a_cycle_is_an_error_not_a_hang() {
    let cyc = INCH.replace("LENGTH_MEASURE(25.4),#960", "LENGTH_MEASURE(25.4),#959");
    assert!(matches!(
        facts(&cyc).unwrap_err(),
        Part21Error::UnknownUnit { .. }
    ));
}

#[test]
fn transforms_are_noticed() {
    let t = INCH.replace(
        "#6=ADVANCED_FACE",
        "#99=ITEM_DEFINED_TRANSFORMATION('','',#1,#2);\n#6=ADVANCED_FACE",
    );
    assert!(facts(&t).unwrap().transformed);
}

#[test]
fn not_step_is_an_error() {
    assert_eq!(
        facts("solid x\nendsolid x\n").unwrap_err(),
        Part21Error::NoData
    );
}

#[test]
fn references_skip_strings() {
    assert_eq!(refs("VERTEX_POINT('#7 corner',#12)"), vec![12]);
    assert_eq!(refs("X((#1,#23))"), vec![1, 23]);
}

#[test]
fn numbers_skip_references_and_strings() {
    assert_eq!(
        numbers("CIRCLE('r=9',#12,2.5E-01)"),
        vec![0.25],
        "neither the string nor the reference is a number"
    );
}
