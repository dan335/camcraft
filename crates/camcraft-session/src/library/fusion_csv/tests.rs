use super::*;

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/tools")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Bits & Bits' 1/4" up-cut spiral: four rows, one tool, a preset for each,
/// in millimetres.
#[test]
fn a_makers_library_reads_as_one_tool_with_presets() {
    let got = import(&fixture("srf4-250up.csv")).unwrap();
    assert!(got.skipped.is_empty(), "{:?}", got.skipped);
    assert_eq!(got.tools.len(), 1);
    let (name, t) = &got.tools[0];
    assert_eq!(name, "SRF4-250UP");
    assert_eq!(t.kind, ToolKind::Flat);
    assert!(close(t.diameter, 6.35));
    assert_eq!(t.flutes, Some(2));
    assert!(close(t.flute_length.unwrap(), 25.4));
    assert!(close(t.shoulder_length.unwrap(), 25.4254));
    assert!(close(t.shank_diameter.unwrap(), 6.35));
    assert!(close(t.overall_length.unwrap(), 63.5));
    assert_eq!(t.material, Some(ToolMaterial::Carbide));
    assert_eq!(t.vendor.as_deref(), Some("Bitsbits.com"));
    assert_eq!(t.product.as_deref(), Some("SRF4-250UP"));
    assert_eq!(
        t.comment,
        "SRF4-250UP *Astra Coated* 1/4\" Up-Cut Spiral Bit"
    );
    assert_eq!(t.rpm, 16000);
    assert_eq!(t.spindle, SpindleDirection::Clockwise);
    assert_eq!(t.coolant, Coolant::Off);

    let names: Vec<&str> = t.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Wood / Plywood",
            "MDF / Laminate",
            "Aluminum",
            "Plastic / Acrylic"
        ]
    );
    let wood = &t.presets[0];
    assert_eq!(wood.rpm, Some(16000));
    assert!(close(wood.feed.unwrap(), 3810.0));
    assert!(close(wood.plunge_feed.unwrap(), 53.3333 * 25.4));
    assert!(close(wood.ramp_feed.unwrap(), 2286.0));
    // The file sets a stepdown and stepover but does not use them.
    assert_eq!(wood.stepdown, None);
    assert_eq!(wood.stepover, None);
    assert!(close(t.presets[1].feed.unwrap(), 4318.0));
    assert!(t.check().is_empty(), "{:?}", t.check());
}

#[test]
fn a_one_flute_bit_reads_too() {
    let got = import(&fixture("425-up250.csv")).unwrap();
    let (name, t) = &got.tools[0];
    assert_eq!(name, "425-UP250");
    assert_eq!(t.flutes, Some(1));
    assert_eq!(t.presets.len(), 2);
    assert!(close(t.presets[0].feed.unwrap(), 2540.0));
}

/// Columns are found by key, so a comma-separated file with only some of them,
/// in another order, reads.
#[test]
fn columns_are_found_by_key_in_any_order() {
    let text = "\"Preset Name (preset_name)\",\"Unit (tool_unit)\",\"Diameter (tool_diameter)\",\
                    \"Type (tool_type)\",\"Stepover (tool_stepover)\",\"Use Stepover (use_tool_stepover)\"\n\
                    \"Finishing\",\"millimeters\",6,\"ball end mill\",0.4,true\n";
    let got = import(text).unwrap();
    let (name, t) = &got.tools[0];
    assert_eq!(name, "tool");
    assert_eq!(t.kind, ToolKind::Ball);
    assert!(close(t.diameter, 6.0));
    assert_eq!(t.presets[0].name, "Finishing");
    assert!(close(t.presets[0].stepover.unwrap(), 0.4));
}

#[test]
fn what_cannot_be_cut_with_is_skipped_and_said() {
    let text = "\"Type (tool_type)\"\t\"Diameter (tool_diameter)\"\n\
                    \"reamer\"\t3\n\"flat end mill\"\t0\n\"flat end mill\"\t3\n";
    let got = import(text).unwrap();
    assert_eq!(got.tools.len(), 1);
    assert_eq!(got.skipped.len(), 2, "{:?}", got.skipped);
    assert!(got.skipped[0].contains("'reamer'"), "{:?}", got.skipped);
}

/// A chamfer mill's taper is half its included angle, and its tip carries over;
/// a spot drill's tip angle is its whole angle.
#[test]
fn chamfer_mills_and_spot_drills_are_cones() {
    let text = "\"Type (tool_type)\",\"Unit (tool_unit)\",\"Diameter (tool_diameter)\",\
                    \"Taper Angle (tool_taperAngle)\",\"Tip Diameter (tool_tipDiameter)\",\
                    \"Tip Angle (tool_tipAngle)\",\"Product ID (tool_productId)\"\n\
                    \"chamfer mill\",\"millimeters\",12,45,1,,\"C90\"\n\
                    \"spot drill\",\"millimeters\",6,,,120,\"S120\"\n";
    let got = import(text).unwrap();
    assert!(got.skipped.is_empty(), "{:?}", got.skipped);
    let chamfer = &got.tools[0].1;
    assert_eq!(chamfer.kind, ToolKind::Cone);
    assert_eq!(chamfer.angle, Some(90.0));
    assert!(close(chamfer.tip_diameter, 1.0));
    let spot = &got.tools[1].1;
    assert_eq!(spot.angle, Some(120.0));
    assert_eq!(spot.tip_diameter, 0.0);
}

#[test]
fn drills_come_in_with_their_point() {
    let text = "\"Type (tool_type)\",\"Unit (tool_unit)\",\"Diameter (tool_diameter)\",\
                    \"Tip Angle (tool_tipAngle)\"\n\"drill\",\"millimeters\",5,135\n";
    let got = import(text).unwrap();
    let drill = &got.tools[0].1;
    assert_eq!(drill.kind, ToolKind::Drill);
    assert_eq!(drill.angle, Some(135.0));
}

#[test]
fn not_a_tool_library_is_refused() {
    assert!(matches!(
        import("a,b\n1,2\n"),
        Err(ImportError::MissingColumn("tool_type"))
    ));
    assert!(matches!(import(""), Err(ImportError::Empty)));
}

/// Importing a maker's file again, or a second file for a tool already in the
/// folder, adds presets to that tool rather than a twin of it.
#[test]
fn a_tool_already_in_the_folder_gets_the_presets() {
    let mut folder = Folder::default();
    let first = merge(
        &mut folder,
        import(&fixture("srf4-250up.csv")).unwrap().tools,
    );
    assert_eq!(first.added, ["SRF4-250UP"]);
    // The same file again adds nothing.
    let again = merge(
        &mut folder,
        import(&fixture("srf4-250up.csv")).unwrap().tools,
    );
    assert_eq!(again.extended, [("SRF4-250UP".to_string(), 0)]);
    assert_eq!(folder.tools.len(), 1);
    // A person's own preset stays beside the maker's.
    let tool = folder.tools.get_mut("SRF4-250UP").unwrap();
    tool.presets[0].feed = Some(3000.0);
    let changed = merge(
        &mut folder,
        import(&fixture("srf4-250up.csv")).unwrap().tools,
    );
    assert_eq!(changed.extended, [("SRF4-250UP".to_string(), 1)]);
    let names: Vec<&str> = folder.tools["SRF4-250UP"]
        .presets
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names.len(), 5);
    assert_eq!(names[4], "Wood / Plywood 2");
    // Another bit is another tool.
    let other = merge(
        &mut folder,
        import(&fixture("425-up250.csv")).unwrap().tools,
    );
    assert_eq!(other.added, ["425-UP250"]);
}
