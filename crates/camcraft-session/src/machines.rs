//! The machines and holders camcraft knows without being told ([decision 0021]):
//! a spindle that holds the bit in its own collet, with its nose measured from
//! the maker's drawing, and ATC spindles whose tools sit in holders of their
//! own; and holders from makers' tables.
//!
//! Only what has been measured or read off a maker's drawing is here. A holder
//! whose flange is wider than its nose and whose flange length is not known
//! (BT30 ER16 and ER20, ISO20) is left out until it is, rather than drawn
//! smaller than it is.
//!
//! [decision 0021]: ../../../docs/decisions/0021-shank-and-holder.md

use camcraft_core::job::{Holder, HolderSegment, Machine};

/// The built-in machines, by name.
pub fn builtin_machines() -> Vec<(&'static str, Machine)> {
    vec![
        (
            "80 mm ER20 spindle",
            Machine {
                description: "an 80 mm water-cooled ER20 spindle (GDK80), its nut and body".into(),
                spindle: "ER20".into(),
                holders: false,
                // From the GDK80's drawing: the nut's face is 26 mm below the
                // spindle's boss. The nut is 19.5 of it; the 6.5 above, the
                // spindle's threaded nose, is narrower than the nut and taken
                // at its diameter.
                nose: vec![
                    HolderSegment::cylinder(35.0, 19.5),
                    HolderSegment::cylinder(35.0, 6.5),
                    HolderSegment::cylinder(78.0, 15.0),
                    HolderSegment::cylinder(80.0, 160.0),
                ],
            },
        ),
        ("ISO30 ATC spindle", atc("ISO30")),
        ("BT30 ATC spindle", atc("BT30")),
        ("ISO20 ATC spindle", atc("ISO20")),
    ]
}

/// A spindle whose tools sit in holders of their own.
fn atc(spindle: &str) -> Machine {
    Machine {
        description: format!("an automatic tool changer taking {spindle} holders"),
        spindle: spindle.into(),
        holders: true,
        nose: Vec::new(),
    }
}

/// The built-in machine called `name`.
pub fn builtin_machine(name: &str) -> Option<Machine> {
    builtin_machines()
        .into_iter()
        .find_map(|(n, m)| (n == name).then_some(m))
}

/// The built-in holders.
pub fn builtin_holders() -> Vec<Holder> {
    let one = |name: &str, d: f64, l: f64| Holder {
        name: name.into(),
        segments: vec![HolderSegment::cylinder(d, l)],
    };
    vec![
        // Collet nuts on their own, where a router's spindle or a holder
        // below the nut is not drawn: the standard nuts' outside, by size.
        one("ER11 nut", 19.0, 11.8),
        one("ER16 nut", 27.5, 18.0),
        one("ER20 nut", 35.0, 19.5),
        one("ER25 nut", 42.0, 20.5),
        one("ER32 nut", 50.0, 23.0),
        // Bilz's catalogue: nose diameter by gauge length, from the nut's
        // face to the gauge line. BT30's flange is 46 across: the ER25's
        // nose, 42, is drawn that wide, so the flange is inside it.
        one("BT30 ER25, 70 long", 46.0, 70.0),
        one("BT30 ER32, 70 long", 50.0, 70.0),
        one("ISO30 ER32, 50 long", 50.0, 50.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_checks_clean() {
        for (name, machine) in builtin_machines() {
            for s in &machine.nose {
                assert!(s.length > 0.0 && s.lower_diameter > 0.0, "{name}");
            }
            assert_eq!(builtin_machine(name).as_ref(), Some(&machine));
        }
        for holder in builtin_holders() {
            let tool = camcraft_core::job::ToolSpec {
                holder: Some(holder.clone()),
                ..camcraft_core::job::ToolSpec::default()
            };
            assert!(tool.check().is_empty(), "{}", holder.name);
        }
    }
}
