//! The System library: twenty common router bits that every library has, so a
//! person can cut something before typing in a tool.
//!
//! It is built here, not read from a file, and is never saved: the manager puts it
//! into the library as the folder [`SYSTEM`] when it loads the user's tools, and
//! [`super::Library::save`] takes it out again. It cannot be changed; a bit is
//! copied into the person's own folders to be changed, as Fusion's sample library
//! is.
//!
//! The bits are inch bits, as sold for hobby routers. Up-cut, down-cut and
//! compression bits are flat end mills, as Fusion and makers' libraries have them;
//! which way the flutes lift the chips is in the name and description. Each bit
//! has a preset for each material it suits, its feed worked out from a
//! conservative chip load, and a stepdown and stepover from its diameter.

use camcraft_core::cutting::feed_for;
use camcraft_core::job::{Preset, ToolKind, ToolMaterial, ToolSpec};

use super::Folder;

/// The System library's folder, at the top of the library.
pub const SYSTEM: &str = "System library";

const MM_PER_INCH: f64 = 25.4;

/// Whether the folder at `path` is the System library or inside it.
pub fn is_system(path: &[String]) -> bool {
    path.first().is_some_and(|f| f == SYSTEM)
}

/// Put the System library into `root`. A folder of the person's own already
/// called that is moved aside under a free name first, and saved there.
pub fn add_system(root: &mut Folder) {
    if let Some(mine) = root.folders.remove(SYSTEM) {
        let name = (1..)
            .map(|n| {
                if n == 1 {
                    format!("{SYSTEM} (mine)")
                } else {
                    format!("{SYSTEM} (mine {n})")
                }
            })
            .find(|n| !root.folders.contains_key(n))
            .unwrap_or_default();
        root.folders.insert(name, mine);
    }
    root.folders.insert(SYSTEM.into(), system_library());
}

/// What a bit is cut in: each a preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Material {
    Softwood,
    Hardwood,
    Mdf,
    Acrylic,
    Hdpe,
    Aluminum,
    Foam,
}

use Material::{Acrylic, Aluminum, Foam, Hardwood, Hdpe, Mdf, Softwood};

const WOOD: [Material; 3] = [Softwood, Hardwood, Mdf];

impl Material {
    fn name(self) -> &'static str {
        match self {
            Softwood => "Softwood",
            Hardwood => "Hardwood",
            Mdf => "MDF / plywood",
            Acrylic => "Acrylic",
            Hdpe => "HDPE",
            Aluminum => "Aluminum",
            Foam => "Sign foam / HDU",
        }
    }

    /// Chip load, in inches a tooth, for a bit of size `size`.
    fn chip_load(self, size: Size) -> f64 {
        use Size::{Large, Quarter, Small, Tiny};
        match (self, size) {
            (Softwood | Mdf, Tiny) => 0.0005,
            (Softwood | Mdf, Small) => 0.0015,
            (Softwood | Mdf, Quarter) => 0.0025,
            (Softwood | Mdf, Large) => 0.004,
            (Hardwood, Tiny) => 0.0004,
            (Hardwood, Small) => 0.001,
            (Hardwood, Quarter) => 0.002,
            (Hardwood, Large) => 0.003,
            (Acrylic, Tiny | Small) => 0.002,
            (Acrylic, Quarter | Large) => 0.004,
            (Hdpe, Tiny | Small) => 0.003,
            (Hdpe, Quarter | Large) => 0.006,
            (Aluminum, Tiny | Small) => 0.0005,
            (Aluminum, Quarter | Large) => 0.001,
            (Foam, _) => Softwood.chip_load(size) * 1.5,
        }
    }

    /// How deep a pass goes, as a share of the bit's diameter.
    fn stepdown(self) -> f64 {
        match self {
            Softwood | Mdf | Acrylic | Hdpe => 0.5,
            Hardwood => 0.25,
            Foam => 1.0,
            Aluminum => 0.1,
        }
    }
}

/// Which row of chip loads a bit reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Size {
    /// 1/16" and smaller.
    Tiny,
    /// 1/8", and the V-bits, which cut with little of their width.
    Small,
    /// 1/4".
    Quarter,
    /// 1/2" and the surfacing bits.
    Large,
}

/// How a bit's passes are spaced.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Passes {
    /// A share of the diameter apart, as deep as the material allows.
    Across(f64),
    /// Surfacing: a share of the diameter apart, a fixed millimetres deep.
    Skim(f64, f64),
    /// V-carving: the depth and spacing follow the artwork.
    Carve,
}

/// One bit of the library.
struct Bit {
    name: &'static str,
    folder: &'static str,
    kind: ToolKind,
    /// Across, in inches.
    diameter: f64,
    /// Included angle, for a V-bit.
    angle: Option<f64>,
    /// Across the tip, in inches, for a V-bit with a flat.
    tip: f64,
    flutes: u32,
    /// In inches, as are the rest.
    shank: f64,
    flute_length: f64,
    overall: f64,
    rpm: u32,
    size: Size,
    passes: Passes,
    materials: &'static [Material],
    comment: &'static str,
}

const END_MILLS: &str = "End mills";
const BALL: &str = "Ball";
const V_BITS: &str = "V-bits";
const SURFACING: &str = "Surfacing";

const UPCUT_SMALL: &[Material] = &[Softwood, Hardwood, Mdf, Acrylic, Hdpe, Foam];
const UPCUT: &[Material] = &[Softwood, Hardwood, Mdf, Foam];
const O_FLUTE: &[Material] = &[Acrylic, Hdpe, Aluminum];
const CARVING: &[Material] = &[Softwood, Hardwood, Mdf, Foam];

/// A flat end mill.
#[allow(clippy::too_many_arguments)]
const fn flat(
    name: &'static str,
    diameter: f64,
    flutes: u32,
    shank: f64,
    flute_length: f64,
    overall: f64,
    size: Size,
    materials: &'static [Material],
    comment: &'static str,
) -> Bit {
    Bit {
        name,
        folder: END_MILLS,
        kind: ToolKind::Flat,
        diameter,
        angle: None,
        tip: 0.0,
        flutes,
        shank,
        flute_length,
        overall,
        rpm: if shank > 0.25 { 16000 } else { 18000 },
        size,
        passes: Passes::Across(0.4),
        materials,
        comment,
    }
}

/// A ball-nose bit, on a 1/4" shank.
const fn ball(
    name: &'static str,
    diameter: f64,
    flute_length: f64,
    overall: f64,
    size: Size,
    comment: &'static str,
) -> Bit {
    Bit {
        name,
        folder: BALL,
        kind: ToolKind::Ball,
        diameter,
        angle: None,
        tip: 0.0,
        flutes: 2,
        shank: 0.25,
        flute_length,
        overall,
        rpm: 18000,
        size,
        passes: Passes::Across(0.1),
        materials: CARVING,
        comment,
    }
}

/// A V-bit on a 1/4" shank, its flutes as long as its cone.
const fn v_bit(
    name: &'static str,
    angle: f64,
    diameter: f64,
    tip: f64,
    comment: &'static str,
) -> Bit {
    Bit {
        name,
        folder: V_BITS,
        kind: ToolKind::Cone,
        diameter,
        angle: Some(angle),
        tip,
        flutes: 2,
        shank: 0.25,
        flute_length: 0.0,
        overall: 2.0,
        rpm: 18000,
        size: Size::Small,
        passes: Passes::Carve,
        materials: CARVING,
        comment,
    }
}

/// A surfacing bit, skimming 0.5 mm a pass.
const fn surfacing(
    name: &'static str,
    diameter: f64,
    shank: f64,
    flute_length: f64,
    overall: f64,
    comment: &'static str,
) -> Bit {
    Bit {
        name,
        folder: SURFACING,
        kind: ToolKind::Flat,
        diameter,
        angle: None,
        tip: 0.0,
        flutes: 2,
        shank,
        flute_length,
        overall,
        rpm: 16000,
        size: Size::Large,
        passes: Passes::Skim(0.6, 0.5),
        materials: &WOOD,
        comment,
    }
}

/// The bits, in tool-number order.
#[rustfmt::skip]
const BITS: [Bit; 20] = [
    flat("1/16 upcut", 0.0625, 2, 0.25, 0.1875, 1.5, Size::Tiny, UPCUT,
        "1/16\" up-cut spiral, 2 flute, for fine detail"),
    flat("1/8 upcut", 0.125, 2, 0.25, 0.5, 2.0, Size::Small, UPCUT_SMALL,
        "1/8\" up-cut spiral, 2 flute: lifts chips out of the cut"),
    flat("1/8 downcut", 0.125, 2, 0.25, 0.5, 2.0, Size::Small, &WOOD,
        "1/8\" down-cut spiral, 2 flute: a clean top edge"),
    flat("1/4 upcut", 0.25, 2, 0.25, 0.75, 2.5, Size::Quarter, UPCUT_SMALL,
        "1/4\" up-cut spiral, 2 flute: the everyday roughing bit"),
    flat("1/4 downcut", 0.25, 2, 0.25, 0.75, 2.5, Size::Quarter, &WOOD,
        "1/4\" down-cut spiral, 2 flute: a clean top edge, chips packed in the cut"),
    flat("1/4 compression", 0.25, 2, 0.25, 0.875, 2.5, Size::Quarter, &WOOD,
        "1/4\" compression spiral, 2 flute: clean top and bottom edges in sheet \
         goods, once cut deeper than its up-cut tip"),
    flat("1/8 O-flute", 0.125, 1, 0.25, 0.5, 2.0, Size::Small, O_FLUTE,
        "1/8\" single O-flute, for plastics and aluminum"),
    flat("1/4 O-flute", 0.25, 1, 0.25, 0.75, 2.5, Size::Quarter, O_FLUTE,
        "1/4\" single O-flute, for plastics and aluminum"),
    flat("1/2 upcut", 0.5, 2, 0.5, 1.25, 3.0, Size::Large, UPCUT,
        "1/2\" up-cut spiral, 2 flute, 1/2\" shank"),
    flat("1/2 compression", 0.5, 2, 0.5, 1.125, 3.0, Size::Large, &WOOD,
        "1/2\" compression spiral, 2 flute, 1/2\" shank: clean edges in sheet goods"),
    ball("1/32 ball", 0.03125, 0.125, 1.5, Size::Tiny,
        "1/32\" ball nose, 2 flute, for the finest 3D detail"),
    ball("1/16 ball", 0.0625, 0.25, 1.5, Size::Tiny,
        "1/16\" ball nose, 2 flute, for 3D finishing"),
    ball("1/8 ball", 0.125, 0.5, 2.0, Size::Small,
        "1/8\" ball nose, 2 flute, for 3D finishing"),
    ball("1/4 ball", 0.25, 0.75, 2.5, Size::Quarter,
        "1/4\" ball nose, 2 flute, for 3D roughing and large finishing"),
    v_bit("30° engraving", 30.0, 0.25, 0.005,
        "30° engraving bit, 0.005\" tip, 1/4\" shank: fine lettering"),
    v_bit("60° V-bit", 60.0, 0.5, 0.0,
        "60° V-bit, 1/2\" across, 1/4\" shank: V-carving and deep lettering"),
    v_bit("90° V-bit", 90.0, 0.5, 0.0,
        "90° V-bit, 1/2\" across, 1/4\" shank: V-carving, chamfers and signs"),
    v_bit("120° V-bit", 120.0, 0.5, 0.0,
        "120° V-bit, 1/2\" across, 1/4\" shank: shallow, wide lettering"),
    surfacing("1-inch surfacing", 1.0, 0.25, 0.25, 2.0,
        "1\" spoilboard surfacing bit, 1/4\" shank"),
    surfacing("2-inch surfacing", 2.0, 0.5, 0.375, 2.5,
        "2\" spoilboard surfacing bit, 1/2\" shank"),
];

/// The System library: its folders of bits, numbered T1 to T20.
pub fn system_library() -> Folder {
    let mut library = Folder::default();
    for (number, bit) in (1..).zip(&BITS) {
        library
            .folders
            .entry(bit.folder.to_string())
            .or_default()
            .tools
            .insert(bit.name.to_string(), tool(bit, number));
    }
    library
}

fn tool(bit: &Bit, number: u32) -> ToolSpec {
    let d = bit.diameter * MM_PER_INCH;
    // A cone's flutes run the length of the cone.
    let flute_length = match bit.angle {
        Some(a) => (bit.diameter - bit.tip) / 2.0 / (a.to_radians() / 2.0).tan(),
        None => bit.flute_length,
    };
    ToolSpec {
        kind: bit.kind,
        diameter: d,
        angle: bit.angle,
        tip_diameter: bit.tip * MM_PER_INCH,
        number,
        rpm: bit.rpm,
        comment: bit.comment.into(),
        flutes: Some(bit.flutes),
        flute_length: Some(flute_length * MM_PER_INCH),
        shank_diameter: Some(bit.shank * MM_PER_INCH),
        overall_length: Some(bit.overall * MM_PER_INCH),
        // Half an inch of shank out of the collet beyond the flutes.
        stickout: Some((flute_length + 0.5) * MM_PER_INCH),
        material: Some(ToolMaterial::Carbide),
        presets: bit.materials.iter().map(|&m| preset(bit, m)).collect(),
        ..ToolSpec::default()
    }
}

fn preset(bit: &Bit, material: Material) -> Preset {
    let d = bit.diameter * MM_PER_INCH;
    let fz = material.chip_load(bit.size);
    let feed = feed_for(fz * MM_PER_INCH, f64::from(bit.rpm), bit.flutes);
    let (stepdown, stepover) = match bit.passes {
        Passes::Across(over) => (Some(material.stepdown() * d), Some(over * d)),
        Passes::Skim(over, down) => (Some(down), Some(over * d)),
        Passes::Carve => (None, None),
    };
    Preset {
        name: material.name().into(),
        comment: format!(
            "{} in/tooth at {} rev/min: a conservative start for a hobby router; \
             adjust to your machine",
            inches(fz),
            thousands(bit.rpm)
        ),
        rpm: Some(bit.rpm),
        feed: Some(feed),
        plunge_feed: Some(feed / 3.0),
        ramp_feed: None,
        stepdown,
        stepover,
        stock_to_leave: None,
    }
}

/// A chip load as a maker charts it: `0.00075`, not `0.0007500000000000001`.
fn inches(v: f64) -> String {
    let s = format!("{v:.5}");
    s.trim_end_matches('0').to_string()
}

/// `18000` as `18,000`.
fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9 * a.abs().max(b.abs()).max(1.0)
    }

    fn bit<'a>(lib: &'a Folder, folder: &str, name: &str) -> &'a ToolSpec {
        &lib.folders[folder].tools[name]
    }

    fn preset_names(tool: &ToolSpec) -> Vec<&str> {
        tool.presets.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn every_system_bit_checks_clean() {
        for (folder, name, tool) in system_library().walk() {
            assert!(
                tool.check().is_empty(),
                "{}/{name}: {:?}",
                folder.join("/"),
                tool.check()
            );
        }
    }

    #[test]
    fn twenty_bits_numbered_once_each() {
        let lib = system_library();
        let numbers: BTreeSet<u32> = lib.walk().iter().map(|(_, _, t)| t.number).collect();
        assert_eq!(lib.count(), 20);
        assert_eq!(numbers, (1..=20).collect());
        assert_eq!(
            lib.folders.keys().collect::<Vec<_>>(),
            ["Ball", "End mills", "Surfacing", "V-bits"]
        );
    }

    /// A 1/4" two-flute bit at 0.002 in a tooth and 18,000 rev/min feeds at
    /// 72 in/min.
    #[test]
    fn feeds_come_from_the_chip_load() {
        let lib = system_library();
        let tool = bit(&lib, END_MILLS, "1/4 downcut");
        let hard = tool.preset("Hardwood").unwrap();
        assert_eq!(hard.rpm, Some(18000));
        assert!(close(hard.feed.unwrap(), 72.0 * 25.4), "{:?}", hard.feed);
        assert!(close(hard.plunge_feed.unwrap(), 24.0 * 25.4));
        assert!(close(hard.stepdown.unwrap(), 0.0625 * 25.4));
        assert!(close(hard.stepover.unwrap(), 0.1 * 25.4));
        for (_, _, t) in lib.walk() {
            for p in &t.presets {
                let fz =
                    p.feed.unwrap() / (f64::from(p.rpm.unwrap()) * f64::from(t.flutes.unwrap()));
                assert!(fz > 0.0 && fz < 0.01 * 25.4, "{}: {fz}", p.name);
            }
        }
    }

    #[test]
    fn each_bit_has_the_materials_it_suits() {
        let lib = system_library();
        let wood = ["Softwood", "Hardwood", "MDF / plywood"];
        assert_eq!(preset_names(bit(&lib, END_MILLS, "1/4 compression")), wood);
        assert_eq!(
            preset_names(bit(&lib, END_MILLS, "1/4 upcut")),
            [
                "Softwood",
                "Hardwood",
                "MDF / plywood",
                "Acrylic",
                "HDPE",
                "Sign foam / HDU"
            ]
        );
        assert_eq!(
            preset_names(bit(&lib, END_MILLS, "1/8 O-flute")),
            ["Acrylic", "HDPE", "Aluminum"]
        );
        let v = bit(&lib, V_BITS, "90° V-bit");
        assert_eq!(
            preset_names(v),
            ["Softwood", "Hardwood", "MDF / plywood", "Sign foam / HDU"]
        );
        assert!(
            v.presets
                .iter()
                .all(|p| p.stepdown.is_none() && p.stepover.is_none())
        );
        let skim = bit(&lib, SURFACING, "1-inch surfacing");
        assert_eq!(preset_names(skim), wood);
        assert!(skim.presets.iter().all(|p| p.stepdown == Some(0.5)));
    }

    /// A V-bit's flutes run the length of its cone: a 90° bit 1/2" across is
    /// 1/4" tall.
    #[test]
    fn a_v_bit_is_as_tall_as_its_cone() {
        let lib = system_library();
        let v = bit(&lib, V_BITS, "90° V-bit");
        assert!(close(v.flute_length.unwrap(), 0.25 * 25.4));
        assert!(v.build().is_ok());
    }

    #[test]
    fn a_folder_of_the_same_name_is_moved_aside() {
        let mut root = Folder::default();
        root.create(&[], SYSTEM).unwrap();
        root.at_mut(&[SYSTEM.into()])
            .unwrap()
            .tools
            .insert("mine".into(), ToolSpec::default());
        root.create(&[], "System library (mine)").unwrap();
        add_system(&mut root);
        assert_eq!(root.folders["System library (mine 2)"].tools.len(), 1);
        assert!(root.folders["System library (mine)"].tools.is_empty());
        assert_eq!(root.folders[SYSTEM], system_library());
        assert!(is_system(&[SYSTEM.into(), "Ball".into()]));
        assert!(!is_system(&["System library (mine)".into()]));
        assert!(!is_system(&[]));
    }

    #[test]
    fn chip_loads_read_as_charted() {
        assert_eq!(inches(0.0005 * 1.5), "0.00075");
        assert_eq!(inches(0.002), "0.002");
    }

    #[test]
    fn thousands_are_grouped() {
        assert_eq!(thousands(18000), "18,000");
        assert_eq!(thousands(900), "900");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
