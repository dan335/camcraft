//! Just enough of STEP's text format (ISO 10303-21) to check the kernel's answer.
//!
//! OpenCascade reads the geometry and converts it to millimetres. That conversion is
//! the single most expensive thing to get wrong -- an inch part read as millimetres
//! is 25.4 times too small -- so the unit is not taken on trust. This reads the
//! unit context from the file independently, along with the raw points and the
//! face count, so the importer can hold the kernel's mesh up against the file it
//! came from.
//!
//! This is not a STEP reader. It resolves entity references only as far as the
//! length unit needs, and treats everything else as text.

use std::collections::BTreeMap;

use glam::DVec3;

/// Something in the file that this cannot make sense of.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Part21Error {
    /// No `DATA;` section.
    #[error("no DATA section; this is not a STEP file")]
    NoData,
    /// The file names no length unit for its geometry.
    #[error("the file declares no length unit for its geometry")]
    NoLengthUnit,
    /// Two geometry contexts disagree about what a length is.
    #[error("the file declares conflicting length units: {0}")]
    ConflictingUnits(String),
    /// A length unit this does not recognise.
    #[error("unrecognised length unit in entity #{id}: {text}")]
    UnknownUnit {
        /// The entity.
        id: u64,
        /// What it said.
        text: String,
    },
    /// A reference to an entity that does not exist.
    #[error("entity #{0} is referenced but not defined")]
    Dangling(u64),
}

/// The file's length unit.
#[derive(Debug, Clone, PartialEq)]
pub struct LengthUnit {
    /// What the file calls it: `millimetre`, `inch`, and so on.
    pub name: String,
    /// Millimetres per unit.
    pub mm: f64,
}

/// What the file says, independently of any kernel.
#[derive(Debug, Clone)]
pub struct StepFacts {
    /// The length unit the geometry is stated in.
    pub unit: LengthUnit,
    /// Every 3D `CARTESIAN_POINT`, in millimetres.
    pub points: Vec<DVec3>,
    /// Every point a `VERTEX_POINT` names, in millimetres. These lie on the part.
    pub vertices: Vec<DVec3>,
    /// A box the geometry cannot leave, in millimetres, or `None` if the file has
    /// no points at all.
    ///
    /// Not simply the box around the points. A circle leaves that box by up to its
    /// radius; a surface of revolution can land on the far side of its axis from
    /// every point in the file; an extrusion reaches as far as its vector. Each is
    /// bounded here by the geometry that defines it.
    pub extent: Option<(DVec3, DVec3)>,
    /// `ADVANCED_FACE` and `FACE_SURFACE` entities.
    pub faces: u64,
    /// Whether geometry is placed by transforms, as in an assembly. Raw points are
    /// then in local frames and bound nothing.
    pub transformed: bool,
}

/// Read the facts out of a STEP file's text.
pub fn facts(text: &str) -> Result<StepFacts, Part21Error> {
    let entities = entities(text)?;
    let unit = length_unit(&entities)?;
    let s = unit.mm;

    let mut points = BTreeMap::new();
    let mut max_radius: f64 = 0.0;
    let mut sweep: f64 = 0.0;
    let mut revolved: Vec<(DVec3, f64)> = Vec::new();
    let mut faces = 0;
    let mut transformed = false;
    for (&id, body) in &entities {
        match kind(body) {
            "CARTESIAN_POINT" => {
                if let Some(p) = point(body) {
                    points.insert(id, p * s);
                }
            }
            "ADVANCED_FACE" | "FACE_SURFACE" => faces += 1,
            "CIRCLE"
            | "CYLINDRICAL_SURFACE"
            | "SPHERICAL_SURFACE"
            | "CONICAL_SURFACE"
            | "TOROIDAL_SURFACE"
            | "ELLIPSE" => {
                max_radius = max_radius.max(radius(body) * s);
            }
            // (name, swept curve, axis). Rotation about an axis through A keeps
            // every point at its distance from A, so the surface lies within a ball
            // around A as large as the farthest reach of its profile.
            "SURFACE_OF_REVOLUTION" => {
                let r = refs(body);
                if let (Some(&curve), Some(&axis)) = (r.first(), r.get(1)) {
                    let centre = refs(entities.get(&axis).map_or("", String::as_str))
                        .first()
                        .and_then(|c| entities.get(c))
                        .and_then(|c| point(c));
                    if let Some(a) = centre {
                        let a = a * s;
                        let (profile, curve_radius) = reach(&entities, curve);
                        let far = profile
                            .iter()
                            .map(|p| (*p * s - a).length())
                            .fold(0.0, f64::max);
                        revolved.push((a, far + curve_radius * s));
                    }
                }
            }
            // (name, swept curve, vector): as far as the vector reaches.
            "SURFACE_OF_LINEAR_EXTRUSION" => {
                if let Some(v) = refs(body).get(1).and_then(|v| entities.get(v)) {
                    sweep = sweep.max(numbers(v).last().map_or(0.0, |m| m.abs()) * s);
                }
            }
            "ITEM_DEFINED_TRANSFORMATION"
            | "MAPPED_ITEM"
            | "CARTESIAN_TRANSFORMATION_OPERATOR_3D" => {
                transformed = true;
            }
            _ => {}
        }
        if body.contains("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION") {
            transformed = true;
        }
    }

    let extent = bounds(points.values()).map(|(lo, hi)| {
        let grow = DVec3::splat(max_radius + sweep);
        revolved
            .iter()
            .fold((lo - grow, hi + grow), |(lo, hi), (a, r)| {
                (lo.min(*a - DVec3::splat(*r)), hi.max(*a + DVec3::splat(*r)))
            })
    });

    let mut vertices = Vec::new();
    for body in entities.values() {
        if kind(body) == "VERTEX_POINT"
            && let Some(id) = refs(body).first()
            && let Some(p) = points.get(id)
        {
            vertices.push(*p);
        }
    }

    Ok(StepFacts {
        unit,
        points: points.into_values().collect(),
        vertices,
        extent,
        faces,
        transformed,
    })
}

/// The box around some points.
pub fn bounds<'a>(points: impl IntoIterator<Item = &'a DVec3>) -> Option<(DVec3, DVec3)> {
    let mut it = points.into_iter();
    let first = *it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))))
}

/// A circle, cylinder, sphere or cone is `(name, placement, radius, ...)`, so its
/// radius is its first number; a cone's second is its semi-angle. A torus and an
/// ellipse are bounded by the sum of their two.
fn radius(body: &str) -> f64 {
    let n = numbers(body);
    match kind(body) {
        "TOROIDAL_SURFACE" | "ELLIPSE" => n.iter().take(2).map(|v| v.abs()).sum(),
        _ => n.first().map_or(0.0, |v| v.abs()),
    }
}

/// Every 3D point reachable from `id` through references, in file units, and the
/// largest radius of any circle-like entity on the way.
///
/// Bounded in depth and by a visited set, so a malformed file cannot make it spin.
fn reach(entities: &BTreeMap<u64, String>, id: u64) -> (Vec<DVec3>, f64) {
    let mut seen = std::collections::BTreeSet::new();
    let mut stack = vec![(id, 0u32)];
    let mut points = Vec::new();
    let mut r: f64 = 0.0;
    while let Some((id, depth)) = stack.pop() {
        if depth > 16 || !seen.insert(id) {
            continue;
        }
        let Some(body) = entities.get(&id) else {
            continue;
        };
        match kind(body) {
            "CARTESIAN_POINT" => points.extend(point(body)),
            "CIRCLE" | "ELLIPSE" => r = r.max(radius(body)),
            _ => {}
        }
        stack.extend(refs(body).into_iter().map(|n| (n, depth + 1)));
    }
    (points, r)
}

/// Every entity in the DATA section, by id, with comments removed and whitespace
/// collapsed. Semicolons and comment markers inside strings are respected.
fn entities(text: &str) -> Result<BTreeMap<u64, String>, Part21Error> {
    let start = text.find("DATA;").ok_or(Part21Error::NoData)? + "DATA;".len();
    let data = &text[start..];

    let mut out = BTreeMap::new();
    let mut current = String::new();
    let mut chars = data.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            current.push(c);
            if c == '\'' {
                // '' is an escaped quote, not the end of the string.
                if chars.peek() == Some(&'\'') {
                    current.push('\'');
                    chars.next();
                } else {
                    in_string = false;
                }
            }
            continue;
        }
        match c {
            '\'' => {
                in_string = true;
                current.push(c);
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for d in chars.by_ref() {
                    if prev == '*' && d == '/' {
                        break;
                    }
                    prev = d;
                }
            }
            ';' => {
                let stmt = current.split_whitespace().collect::<Vec<_>>().join(" ");
                current.clear();
                if stmt.starts_with("ENDSEC") {
                    break;
                }
                if let Some((id, body)) = stmt.split_once('=')
                    && let Some(n) = id.trim().strip_prefix('#')
                    && let Ok(n) = n.trim().parse::<u64>()
                {
                    out.insert(n, body.trim().to_string());
                }
            }
            _ => current.push(c),
        }
    }
    Ok(out)
}

/// The entity type of a simple instance, or empty for a complex one.
fn kind(body: &str) -> &str {
    body.split('(').next().map_or("", str::trim)
}

/// Every `#n` reference outside strings, in order.
fn refs(body: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut in_string = false;
    let mut digits: Option<String> = None;
    for c in body.chars() {
        if c == '\'' {
            in_string = !in_string;
        }
        if let Some(d) = digits.as_mut() {
            if c.is_ascii_digit() {
                d.push(c);
                continue;
            }
            if let Ok(n) = d.parse() {
                out.push(n);
            }
            digits = None;
        }
        if c == '#' && !in_string {
            digits = Some(String::new());
        }
    }
    if let Some(n) = digits.and_then(|d| d.parse().ok()) {
        out.push(n);
    }
    out
}

/// Every numeric literal outside strings, in order. References are not numbers.
fn numbers(body: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut in_string = false;
    let mut token = String::new();
    let mut after_hash = false;
    let flush = |token: &mut String, out: &mut Vec<f64>, skip: bool| {
        if !token.is_empty()
            && !skip
            && let Ok(v) = token.parse::<f64>()
        {
            out.push(v);
        }
        token.clear();
    };
    for c in body.chars() {
        if c == '\'' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'E' | 'e') {
            token.push(c);
        } else {
            flush(&mut token, &mut out, after_hash);
            after_hash = c == '#';
        }
    }
    flush(&mut token, &mut out, after_hash);
    out
}

/// A 3D `CARTESIAN_POINT`. Two-coordinate points live in a surface's parameter
/// space, not in the model, and are skipped.
fn point(body: &str) -> Option<DVec3> {
    let open = body.rfind('(')?;
    let close = body[open..].find(')')? + open;
    let coords: Vec<f64> = body[open + 1..close]
        .split(',')
        .map(|v| v.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    match coords.as_slice() {
        [x, y, z] => Some(DVec3::new(*x, *y, *z)),
        _ => None,
    }
}

/// The length unit of every geometric context, which must all agree.
fn length_unit(entities: &BTreeMap<u64, String>) -> Result<LengthUnit, Part21Error> {
    let mut found: Option<LengthUnit> = None;
    for body in entities.values() {
        let Some(at) = body.find("GLOBAL_UNIT_ASSIGNED_CONTEXT") else {
            continue;
        };
        // The unit list is the first parenthesised group after the keyword.
        let tail = &body[at..];
        let group = tail
            .find('(')
            .and_then(|o| tail[o..].find(')').map(|c| &tail[o..o + c]))
            .unwrap_or("");
        for id in refs(group) {
            let unit_body = entities.get(&id).ok_or(Part21Error::Dangling(id))?;
            if !unit_body.contains("LENGTH_UNIT") {
                continue;
            }
            let unit = resolve(entities, id, 0)?;
            match &found {
                None => found = Some(unit),
                Some(prev) if (prev.mm - unit.mm).abs() <= prev.mm * 1e-9 => {}
                Some(prev) => {
                    return Err(Part21Error::ConflictingUnits(format!(
                        "{} and {}",
                        prev.name, unit.name
                    )));
                }
            }
        }
    }
    found.ok_or(Part21Error::NoLengthUnit)
}

/// Millimetres per unit for the length unit entity `id`.
fn resolve(
    entities: &BTreeMap<u64, String>,
    id: u64,
    depth: u32,
) -> Result<LengthUnit, Part21Error> {
    let body = entities.get(&id).ok_or(Part21Error::Dangling(id))?;
    let unknown = || Part21Error::UnknownUnit {
        id,
        text: body.clone(),
    };
    // Conversion chains are one or two links long in practice. Bound it anyway, so
    // a cycle in a malformed file is an error rather than a hang.
    if depth > 8 {
        return Err(unknown());
    }

    if let Some(at) = body.find("SI_UNIT(") {
        let args = &body[at + "SI_UNIT(".len()..];
        let args = args.split(')').next().unwrap_or("");
        let mut parts = args.split(',').map(str::trim);
        let prefix = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("");
        if name != ".METRE." {
            return Err(unknown());
        }
        let (mm, label) = match prefix {
            "$" => (1000.0, "metre"),
            ".DECI." => (100.0, "decimetre"),
            ".CENTI." => (10.0, "centimetre"),
            ".MILLI." => (1.0, "millimetre"),
            ".MICRO." => (0.001, "micrometre"),
            ".KILO." => (1_000_000.0, "kilometre"),
            _ => return Err(unknown()),
        };
        return Ok(LengthUnit {
            name: label.into(),
            mm,
        });
    }

    if let Some(at) = body.find("CONVERSION_BASED_UNIT(") {
        let args = &body[at + "CONVERSION_BASED_UNIT(".len()..];
        let name = args
            .split('\'')
            .nth(1)
            .map_or_else(String::new, |n| n.to_lowercase());
        let measure_id = *refs(args).first().ok_or_else(unknown)?;
        let measure = entities
            .get(&measure_id)
            .ok_or(Part21Error::Dangling(measure_id))?;
        // LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4), #base)
        let factor = *numbers(measure).first().ok_or_else(unknown)?;
        let base_id = *refs(measure).first().ok_or_else(unknown)?;
        let base = resolve(entities, base_id, depth + 1)?;
        let mm = factor * base.mm;
        if !(mm.is_finite() && mm > 0.0) {
            return Err(unknown());
        }
        return Ok(LengthUnit { name, mm });
    }

    Err(unknown())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, clippy::panic)]
mod tests {
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
        assert_eq!(f.points.len(), 3, "the 2D parameter-space point is skipped");
        assert!(
            f.points.contains(&DVec3::new(1e-5 * 25.4, 0.0, 0.0)),
            "1.E-05 parses"
        );
        assert!(f.points.contains(&DVec3::new(25.4, 50.8, -0.35 * 25.4)));
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

    /// Never default. A file with no length unit is refused.
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
}
