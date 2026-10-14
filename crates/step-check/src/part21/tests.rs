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
    assert!(!f.unplaced);
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
    assert!(facts(&t).unwrap().unplaced);
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

/// A part placed in an assembly: the root's representation holds a placement the
/// part's own origin is moved onto, 100 mm along X and turned a quarter about Z.
/// The part's geometry hangs off its representation through a plain relationship,
/// as Fusion writes it.
const ASSEMBLY: &str = "ISO-10303-21;
HEADER; FILE_SCHEMA (('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=APPLICATION_CONTEXT('');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#5=PRODUCT_DEFINITION_CONTEXT('',#1,'design');
#3=PRODUCT('root','root',$,(#2));
#4=PRODUCT_DEFINITION_FORMATION('',$,#3);
#6=PRODUCT_DEFINITION('root','root',#4,#5);
#7=PRODUCT('part','part',$,(#2));
#8=PRODUCT_DEFINITION_FORMATION('',$,#7);
#9=PRODUCT_DEFINITION('part','part',#8,#5);
#10=PRODUCT_DEFINITION_SHAPE('',$,#6);
#11=PRODUCT_DEFINITION_SHAPE('',$,#9);
#20=SHAPE_REPRESENTATION('',(#21,#22),#900);
#21=AXIS2_PLACEMENT_3D('',#23,$,$);
#23=CARTESIAN_POINT('',(0.,0.,0.));
#22=AXIS2_PLACEMENT_3D('',#24,#25,#26);
#24=CARTESIAN_POINT('',(100.,0.,0.));
#25=DIRECTION('',(0.,0.,1.));
#26=DIRECTION('',(0.,1.,0.));
#30=SHAPE_REPRESENTATION('',(#31),#900);
#31=AXIS2_PLACEMENT_3D('',#32,$,$);
#32=CARTESIAN_POINT('',(0.,0.,0.));
#33=SHAPE_REPRESENTATION_RELATIONSHIP('','',#30,#34);
#34=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#35),#900);
#35=MANIFOLD_SOLID_BREP('',#36);
#36=CLOSED_SHELL('',(#37));
#37=ADVANCED_FACE('',(#38),#39,.T.);
#38=FACE_OUTER_BOUND('',#40,.T.);
#40=VERTEX_POINT('',#41);
#41=CARTESIAN_POINT('',(10.,0.,0.));
#39=PLANE('',#42);
#42=AXIS2_PLACEMENT_3D('',#43,$,$);
#43=CARTESIAN_POINT('',(0.,0.,5.));
#50=SHAPE_DEFINITION_REPRESENTATION(#10,#20);
#51=SHAPE_DEFINITION_REPRESENTATION(#11,#30);
#60=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','','',#6,#9,$);
#61=PRODUCT_DEFINITION_SHAPE('',$,#60);
#62=ITEM_DEFINED_TRANSFORMATION('','',#31,#22);
#63=( REPRESENTATION_RELATIONSHIP('','',#30,#20) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#62) SHAPE_REPRESENTATION_RELATIONSHIP() );
#64=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#63,#61);
#900=( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#901)) REPRESENTATION_CONTEXT('','3D') );
#901=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );
ENDSEC;
END-ISO-10303-21;
";

fn near(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-9
}

#[test]
fn a_placed_part_is_checked_where_the_assembly_puts_it() {
    let f = facts(ASSEMBLY).unwrap();
    assert!(!f.unplaced);
    // The part's (10, 0, 0) is turned onto +Y and moved 100 along X.
    assert_eq!(f.vertices.len(), 1);
    assert!(
        near(f.vertices[0], DVec3::new(100.0, 10.0, 0.0)),
        "{:?}",
        f.vertices
    );
    // The plane's origin comes along; the placements themselves are not geometry,
    // so the root's origin does not stretch the box back to 0.
    let (lo, hi) = f.extent.unwrap();
    assert!(near(lo, DVec3::new(100.0, 0.0, 0.0)), "{lo:?}");
    assert!(near(hi, DVec3::new(100.0, 10.0, 5.0)), "{hi:?}");
}

#[test]
fn which_side_is_the_part_comes_from_the_product_structure() {
    // The relationship and the transformation list the two the other way round.
    let swapped = ASSEMBLY
        .replace(
            "REPRESENTATION_RELATIONSHIP('','',#30,#20)",
            "REPRESENTATION_RELATIONSHIP('','',#20,#30)",
        )
        .replace(
            "ITEM_DEFINED_TRANSFORMATION('','',#31,#22)",
            "ITEM_DEFINED_TRANSFORMATION('','',#22,#31)",
        );
    let f = facts(&swapped).unwrap();
    assert!(!f.unplaced);
    assert!(
        near(f.vertices[0], DVec3::new(100.0, 10.0, 0.0)),
        "{:?}",
        f.vertices
    );
}

#[test]
fn a_part_used_twice_is_checked_in_both_places() {
    let twice = ASSEMBLY.replace(
        "#900=",
        "#70=NEXT_ASSEMBLY_USAGE_OCCURRENCE('2','','',#6,#9,$);
#71=PRODUCT_DEFINITION_SHAPE('',$,#70);
#72=AXIS2_PLACEMENT_3D('',#73,$,$);
#73=CARTESIAN_POINT('',(0.,200.,0.));
#74=ITEM_DEFINED_TRANSFORMATION('','',#31,#72);
#75=( REPRESENTATION_RELATIONSHIP('','',#30,#20) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#74) SHAPE_REPRESENTATION_RELATIONSHIP() );
#76=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#75,#71);
#900=",
    );
    let f = facts(&twice).unwrap();
    assert!(!f.unplaced);
    assert_eq!(f.vertices.len(), 2);
    assert!(
        f.vertices
            .iter()
            .any(|v| near(*v, DVec3::new(100.0, 10.0, 0.0)))
    );
    assert!(
        f.vertices
            .iter()
            .any(|v| near(*v, DVec3::new(10.0, 200.0, 0.0)))
    );
}

#[test]
fn a_placement_outside_the_product_structure_is_not_followed() {
    let loose = ASSEMBLY.replace("#64=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#63,#61);", "");
    assert!(facts(&loose).unwrap().unplaced);
}

/// How the sculpted panel is written: the body belongs to the root, and the placed
/// component holds nothing. The body is where the file says, and the component's
/// placement is not stretched into the box.
#[test]
fn geometry_in_the_root_stays_put_while_an_empty_part_is_placed() {
    let root_body = ASSEMBLY.replace(
        "#33=SHAPE_REPRESENTATION_RELATIONSHIP('','',#30,#34);",
        "#33=SHAPE_REPRESENTATION_RELATIONSHIP('','',#20,#34);",
    );
    let f = facts(&root_body).unwrap();
    assert!(!f.unplaced);
    assert!(
        near(f.vertices[0], DVec3::new(10.0, 0.0, 0.0)),
        "{:?}",
        f.vertices
    );
    let (lo, hi) = f.extent.unwrap();
    assert!(
        near(lo, DVec3::new(0.0, 0.0, 0.0)) && near(hi, DVec3::new(10.0, 0.0, 5.0)),
        "{lo:?} {hi:?}"
    );
}

/// A cone 20 mm across at its base on the origin, coming to a point 30 mm up with
/// no vertex there, written as Fusion writes one: placed at the base, its axis
/// pointing down, the way it widens.
const CONE: &str = "ISO-10303-21;
HEADER; FILE_SCHEMA (('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,-1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CONICAL_SURFACE('',#4,10.,0.3217505543966422);
#6=CARTESIAN_POINT('',(10.,0.,0.));
#7=VERTEX_POINT('',#6);
#8=ADVANCED_FACE('',(#9),#5,.T.);
#900=( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#901,#902)) REPRESENTATION_CONTEXT('','3D') );
#901=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );
#902=( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) );
ENDSEC;
END-ISO-10303-21;
";

#[test]
fn a_cone_reaches_its_apex_though_no_point_marks_it() {
    // The box is grown by the largest radius all round, so it reaches past the
    // apex; without the apex it would stop at the base circle, grown, at 10.
    let (_, hi) = facts(CONE).unwrap().extent.unwrap();
    assert!(hi.z >= 30.0 - 1e-9, "{hi:?}");
}

#[test]
fn a_cone_half_angle_in_degrees_is_read_as_degrees() {
    let degrees = CONE
        .replace("0.3217505543966422", "18.43494882292201")
        .replace(
            "#902=( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) );",
            "#902=( CONVERSION_BASED_UNIT('DEGREE',#903) NAMED_UNIT(#905) PLANE_ANGLE_UNIT() );
#903=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.0174532925199433),#904);
#904=( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) );
#905=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);",
        );
    let (_, hi) = facts(&degrees).unwrap().extent.unwrap();
    assert!(hi.z >= 30.0 - 1e-6, "{hi:?}");
}

/// Fusion states its fit as 0.01 mm, written in the file's own unit.
#[test]
fn reads_the_stated_accuracy_in_millimetres() {
    assert_eq!(facts(INCH).unwrap().accuracy, None, "it states none");
    let stated = INCH.replace(
        "ENDSEC;\nEND",
        "#954=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.000393700787401575),#959,\
         'distance_accuracy_value','Maximum Tolerance applied to model');\nENDSEC;\nEND",
    );
    let accuracy = facts(&stated).unwrap().accuracy.unwrap();
    assert!((accuracy - 0.01).abs() < 1e-12, "{accuracy}");
}
