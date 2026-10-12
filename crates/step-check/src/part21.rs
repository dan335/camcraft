//! Just enough of STEP's text format (ISO 10303-21) to check the kernel's answer.
//!
//! The kernel reads the geometry and converts it to millimetres. That conversion is
//! the single most expensive thing to get wrong -- an inch part read as millimetres
//! is 25.4 times too small -- so the unit is not taken on trust. This reads the
//! unit context from the file independently, along with the raw points and the
//! face count, so the importer can hold the kernel's mesh up against the file it
//! came from.
//!
//! This is not a STEP reader. It resolves entity references only as far as the
//! length unit and an assembly's placements need, and treats everything else as
//! text.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DAffine3, DVec3};

use crate::surface::Surface;

/// Most placements of one representation followed before giving up, so a file
/// that instances a sub-assembly many times over cannot run away.
const MAX_PLACEMENTS: usize = 100_000;

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
///
/// Points are where the kernel puts them: in millimetres, and where the file's
/// assembly places them, each component's points moved by its placement and a
/// component used twice counted twice.
#[derive(Debug, Clone)]
pub struct StepFacts {
    /// The length unit the geometry is stated in.
    pub unit: LengthUnit,
    /// Every point a `VERTEX_POINT` names, in millimetres. These lie on the part.
    pub vertices: Vec<DVec3>,
    /// A box the geometry cannot leave, in millimetres, or `None` if the file has
    /// no points at all.
    ///
    /// Not simply the box around the points. A circle leaves that box by up to its
    /// radius; a surface of revolution can land on the far side of its axis from
    /// every point in the file; an extrusion reaches as far as its vector. Each is
    /// bounded here by the geometry that defines it. Points that only place a
    /// representation or a component are not geometry, and are left out.
    pub extent: Option<(DVec3, DVec3)>,
    /// `ADVANCED_FACE` and `FACE_SURFACE` entities.
    pub faces: u64,
    /// Whether some geometry is placed in a way this cannot follow: a mapped item,
    /// or a transform not tied to an assembly's product structure. Raw points
    /// then bound nothing.
    pub unplaced: bool,
    /// Every face, once for each place the assembly puts it.
    pub file_faces: Vec<FileFace>,
}

/// A face of the file, where the file puts it.
#[derive(Debug, Clone)]
pub struct FileFace {
    /// Its entity in the file: `#id`.
    pub entity: u64,
    /// A box it cannot leave, in millimetres: the box around everything that
    /// bounds it and the surface it lies on, grown by their largest radius. `None`
    /// when its surface is one this cannot bound, such as a surface of revolution.
    pub bounds: Option<(DVec3, DVec3)>,
    /// The surface it lies on, when distance to it is exact: a plane, cylinder,
    /// cone, sphere or torus.
    pub surface: Option<Surface>,
}

/// Read the facts out of a STEP file's text.
pub fn facts(text: &str) -> Result<StepFacts, Part21Error> {
    let entities = entities(text)?;
    let unit = length_unit(&entities)?;
    let s = unit.mm;
    let radians = angle_unit(&entities);
    let placements = Placements::read(&entities, s);

    let mut points: BTreeMap<u64, Vec<DVec3>> = BTreeMap::new();
    let mut max_radius: f64 = 0.0;
    let mut sweep: f64 = 0.0;
    let mut revolved: Vec<(DVec3, f64)> = Vec::new();
    let mut apexes = Vec::new();
    let mut faces = 0;
    for (&id, body) in &entities {
        match kind(body) {
            "CARTESIAN_POINT" => {
                if !placements.placing.contains(&id)
                    && let Some(p) = point(body)
                {
                    points.insert(id, placements.place(id, p * s));
                }
            }
            "ADVANCED_FACE" | "FACE_SURFACE" => faces += 1,
            "CIRCLE"
            | "CYLINDRICAL_SURFACE"
            | "SPHERICAL_SURFACE"
            | "TOROIDAL_SURFACE"
            | "ELLIPSE" => {
                max_radius = max_radius.max(radius(body) * s);
            }
            // (name, position, radius, semi-angle). A cone reaches its apex, which no
            // point in the file need mark: a face can come to a point there with no
            // vertex, as a cone does from Fusion.
            "CONICAL_SURFACE" => {
                max_radius = max_radius.max(radius(body) * s);
                let n = numbers(body);
                if let (Some(&at), [r, angle, ..]) = (refs(body).first(), n.as_slice())
                    && let Some(frame) = placement(&entities, at, s)
                {
                    let t = (*angle * radians).tan();
                    if t.abs() > 1e-12 {
                        let apex = frame.translation - (*r * s / t) * frame.matrix3.z_axis;
                        apexes.extend(placements.place(id, apex));
                    }
                }
            }
            // (name, swept curve, axis). Rotation about an axis through A keeps
            // every point at its distance from A, so the surface lies within a ball
            // around A as large as the farthest reach of its profile.
            "SURFACE_OF_REVOLUTION" => {
                let r = refs(body);
                if let (Some(&curve), Some(&axis)) = (r.first(), r.get(1)) {
                    let centre = refs(entities.get(&axis).map_or("", String::as_str))
                        .first()
                        .and_then(|&c| Some((c, point(entities.get(&c)?)?)));
                    if let Some((c, a)) = centre {
                        let a = a * s;
                        let (profile, curve_radius) = reach(&entities, curve);
                        let far = profile
                            .iter()
                            .map(|p| (*p * s - a).length())
                            .fold(0.0, f64::max);
                        // The ball is about the axis wherever the surface is placed.
                        for a in placements.place(c, a) {
                            revolved.push((a, far + curve_radius * s));
                        }
                    }
                }
            }
            // (name, swept curve, vector): as far as the vector reaches.
            "SURFACE_OF_LINEAR_EXTRUSION" => {
                if let Some(v) = refs(body).get(1).and_then(|v| entities.get(v)) {
                    sweep = sweep.max(numbers(v).last().map_or(0.0, |m| m.abs()) * s);
                }
            }
            _ => {}
        }
    }

    let extent = bounds(points.values().flatten().chain(&apexes)).map(|(lo, hi)| {
        let grow = DVec3::splat(max_radius + sweep);
        revolved
            .iter()
            .fold((lo - grow, hi + grow), |(lo, hi), (a, r)| {
                (lo.min(*a - DVec3::splat(*r)), hi.max(*a + DVec3::splat(*r)))
            })
    });

    let mut file_faces = Vec::new();
    for (&id, body) in &entities {
        if matches!(kind(body), "ADVANCED_FACE" | "FACE_SURFACE") {
            file_faces.extend(file_face(&entities, id, body, s, radians, &placements));
        }
    }

    let mut vertices = Vec::new();
    for body in entities.values() {
        if kind(body) == "VERTEX_POINT"
            && let Some(id) = refs(body).first()
            && let Some(p) = points.get(id)
        {
            vertices.extend(p);
        }
    }

    Ok(StepFacts {
        unit,
        vertices,
        extent,
        faces,
        unplaced: placements.unplaced,
        file_faces,
    })
}

/// The face `id`, once for each place the file puts it.
///
/// A face lies inside the box around everything it refers to: its edges, their
/// vertices and curves, and its surface, whose control points hold a B-spline. The
/// box is grown by the largest radius among them, and reaches a cone's apex. A
/// surface of revolution or an extrusion is not bounded by its points, so a face on
/// one gets no box.
fn file_face(
    entities: &BTreeMap<u64, String>,
    id: u64,
    body: &str,
    s: f64,
    radians: f64,
    placements: &Placements,
) -> Vec<FileFace> {
    // (name, bounds, surface, same sense)
    let Some(surface_body) = refs(body).last().and_then(|r| entities.get(r)) else {
        return Vec::new();
    };
    let surface = Surface::read(entities, surface_body, s, radians);
    let bounded = surface.is_some() || surface_body.contains("B_SPLINE_SURFACE");
    let local = bounded
        .then(|| {
            let (points, radius) = reach(entities, id);
            let mut points: Vec<DVec3> = points.into_iter().map(|p| p * s).collect();
            points.extend(surface.and_then(|f| f.apex()));
            let grow = DVec3::splat(radius * s);
            bounds(&points).map(|(lo, hi)| (lo - grow, hi + grow))
        })
        .flatten();
    placements
        .transforms(id)
        .iter()
        .map(|w| FileFace {
            entity: id,
            bounds: local.map(|(lo, hi)| {
                let corner = |i: u8| {
                    DVec3::new(
                        if i & 1 == 0 { lo.x } else { hi.x },
                        if i & 2 == 0 { lo.y } else { hi.y },
                        if i & 4 == 0 { lo.z } else { hi.z },
                    )
                };
                let placed: Vec<DVec3> = (0..8).map(|i| w.transform_point3(corner(i))).collect();
                bounds(&placed).unwrap_or((lo, hi))
            }),
            surface: surface.map(|f| f.moved(w)),
        })
        .collect()
}

/// Where the file puts each piece of its geometry.
///
/// An assembly states each component's geometry in the component's own frame, and
/// places the component in its parent with a pair of axis placements
/// (`ITEM_DEFINED_TRANSFORMATION`): the one in the component's frame is moved onto
/// the one in the parent's. Which representation is the component is read from the
/// product structure (`NEXT_ASSEMBLY_USAGE_OCCURRENCE`), not from the order the
/// relationship happens to list them in. A plain relationship between two
/// representations keeps them in one frame.
#[derive(Debug, Default)]
struct Placements {
    /// The frames an entity is reached from, by index into `world`. An entity in
    /// none is in the root frame.
    owners: BTreeMap<u64, Vec<usize>>,
    /// Every transform taking a frame to the root, in millimetres: one for a
    /// component used once, more for one used several times.
    world: Vec<Vec<DAffine3>>,
    /// Points that only place a representation or a component.
    placing: BTreeSet<u64>,
    /// Some geometry is placed in a way this cannot follow.
    unplaced: bool,
}

impl Placements {
    fn read(entities: &BTreeMap<u64, String>, s: f64) -> Self {
        let mut out = Self::default();
        let mut frames = Frames::default();
        let mut shape_reps = BTreeSet::new();
        // (relationship, first representation, second, transformation)
        let mut transformed = Vec::new();
        let mut reps_of: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        let mut usage_of: BTreeMap<u64, u64> = BTreeMap::new();
        let mut transformations = BTreeSet::new();

        for (&id, body) in entities {
            let k = kind(body);
            let r = refs(body);
            if body.contains("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION") {
                if let [a, b, t] = r[..] {
                    transformed.push((id, a, b, t));
                    shape_reps.extend([a, b]);
                } else {
                    out.unplaced = true;
                }
            } else if (k.ends_with("REPRESENTATION_RELATIONSHIP")
                || (k.is_empty() && body.contains("REPRESENTATION_RELATIONSHIP(")))
                && let [a, b, ..] = r[..]
            {
                frames.join(a, b);
                shape_reps.extend([a, b]);
            }
            match k {
                "SHAPE_DEFINITION_REPRESENTATION" => {
                    if let [pds, rep, ..] = r[..]
                        && let Some(pd) = entities.get(&pds).and_then(|b| refs(b).first().copied())
                    {
                        reps_of.entry(pd).or_default().push(rep);
                        shape_reps.insert(rep);
                    }
                }
                "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION" => {
                    if let [rel, pds, ..] = r[..] {
                        usage_of.insert(rel, pds);
                    }
                }
                "ITEM_DEFINED_TRANSFORMATION" => {
                    transformations.insert(id);
                }
                "MAPPED_ITEM" | "CARTESIAN_TRANSFORMATION_OPERATOR_3D" => out.unplaced = true,
                _ => {}
            }
        }

        // A representation's own axis placements place it, and are not geometry.
        for rep in &shape_reps {
            let items = entities.get(rep).map(|b| refs(b)).unwrap_or_default();
            for item in items {
                out.placing.extend(location(entities, item));
            }
        }

        // Each placed component: child frame -> (parent frame, transform).
        let mut up: BTreeMap<u64, Vec<(u64, DAffine3)>> = BTreeMap::new();
        for &(rel, a, b, t) in &transformed {
            let edge = (|| {
                let usage = refs(entities.get(usage_of.get(&rel)?)?).first().copied()?;
                let [parent, child, ..] = refs(entities.get(&usage)?)[..] else {
                    return None;
                };
                let frame_of = |pd: u64| -> BTreeSet<u64> {
                    reps_of
                        .get(&pd)
                        .into_iter()
                        .flatten()
                        .map(|&r| frames.find(r))
                        .collect()
                };
                let (children, parents) = (frame_of(child), frame_of(parent));
                let (fa, fb) = (frames.find(a), frames.find(b));
                let [item_a, item_b] = refs(entities.get(&t)?)[..] else {
                    return None;
                };
                let (pa, pb) = (
                    placement(entities, item_a, s)?,
                    placement(entities, item_b, s)?,
                );
                out.placing.extend(location(entities, item_a));
                out.placing.extend(location(entities, item_b));
                if fa != fb && children.contains(&fa) && parents.contains(&fb) {
                    Some((fa, fb, pb * pa.inverse()))
                } else if fa != fb && children.contains(&fb) && parents.contains(&fa) {
                    Some((fb, fa, pa * pb.inverse()))
                } else {
                    None
                }
            })();
            match edge {
                Some((child, parent, moved)) => {
                    up.entry(child).or_default().push((parent, moved));
                    transformations.remove(&t);
                }
                None => out.unplaced = true,
            }
        }
        // A transformation that places nothing this could follow.
        out.unplaced |= !transformations.is_empty();
        if up.is_empty() {
            return out;
        }

        // Every frame's ways to the root, then which frame each entity is in.
        let every_frame: BTreeSet<u64> = shape_reps.iter().map(|&r| frames.find(r)).collect();
        let mut memo = BTreeMap::new();
        for frame in every_frame {
            let Some(world) = to_root(frame, &up, &mut memo, 0) else {
                out.unplaced = true;
                return out;
            };
            let index = out.world.len();
            out.world.push(world);
            let mut seen = BTreeSet::new();
            let mut stack: Vec<u64> = shape_reps
                .iter()
                .copied()
                .filter(|&r| frames.find(r) == frame)
                .collect();
            while let Some(id) = stack.pop() {
                if !seen.insert(id) {
                    continue;
                }
                out.owners.entry(id).or_default().push(index);
                if let Some(body) = entities.get(&id) {
                    stack.extend(refs(body));
                }
            }
        }
        out
    }

    /// Every transform taking entity `id` to where the file puts it: one for each
    /// placement of the frames it is in, or none at all if it is in the root.
    fn transforms(&self, id: u64) -> Vec<DAffine3> {
        match self.owners.get(&id) {
            None => vec![DAffine3::IDENTITY],
            Some(frames) => frames
                .iter()
                .filter_map(|&f| self.world.get(f))
                .flatten()
                .copied()
                .collect(),
        }
    }

    /// Where the point `p` of entity `id`, in millimetres, ends up: once for each
    /// placement of the frames it is in.
    fn place(&self, id: u64, p: DVec3) -> Vec<DVec3> {
        match self.owners.get(&id) {
            None => vec![p],
            Some(frames) => frames
                .iter()
                .filter_map(|&f| self.world.get(f))
                .flatten()
                .map(|w| w.transform_point3(p))
                .collect(),
        }
    }
}

/// Every transform taking `frame` to the root, or `None` if the placements loop,
/// nest too deeply or multiply past [`MAX_PLACEMENTS`].
fn to_root(
    frame: u64,
    up: &BTreeMap<u64, Vec<(u64, DAffine3)>>,
    memo: &mut BTreeMap<u64, Vec<DAffine3>>,
    depth: u32,
) -> Option<Vec<DAffine3>> {
    if let Some(w) = memo.get(&frame) {
        return Some(w.clone());
    }
    if depth > 64 {
        return None;
    }
    let mut out = Vec::new();
    match up.get(&frame) {
        None => out.push(DAffine3::IDENTITY),
        Some(parents) => {
            for &(parent, moved) in parents {
                for w in to_root(parent, up, memo, depth + 1)? {
                    out.push(w * moved);
                }
                if out.len() > MAX_PLACEMENTS {
                    return None;
                }
            }
        }
    }
    memo.insert(frame, out.clone());
    Some(out)
}

/// Representations kept in one frame by plain relationships.
#[derive(Debug, Default)]
struct Frames(BTreeMap<u64, u64>);

impl Frames {
    fn find(&self, mut id: u64) -> u64 {
        // Bounded: joins only ever point a root at a root.
        for _ in 0..self.0.len() + 1 {
            match self.0.get(&id) {
                Some(&up) if up != id => id = up,
                _ => break,
            }
        }
        id
    }

    fn join(&mut self, a: u64, b: u64) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0.insert(a.max(b), a.min(b));
        }
    }
}

/// The point an `AXIS2_PLACEMENT_3D` is at, if `id` is one.
fn location(entities: &BTreeMap<u64, String>, id: u64) -> Option<u64> {
    let body = entities.get(&id)?;
    (kind(body) == "AXIS2_PLACEMENT_3D").then(|| refs(body).first().copied())?
}

/// The frame an `AXIS2_PLACEMENT_3D` stands for, with its origin in millimetres.
/// An axis or reference direction left out (`$`) is the standard one.
pub(crate) fn placement(entities: &BTreeMap<u64, String>, id: u64, s: f64) -> Option<DAffine3> {
    let body = entities.get(&id)?;
    if kind(body) != "AXIS2_PLACEMENT_3D" {
        return None;
    }
    let a = args(body);
    let at = |i: usize| {
        a.get(i)
            .and_then(|arg| refs(arg).first().copied())
            .and_then(|r| point(entities.get(&r)?))
    };
    let origin = at(1)? * s;
    let z = at(2).unwrap_or(DVec3::Z).try_normalize()?;
    let x = at(3).unwrap_or(DVec3::X);
    let x = (x - z * x.dot(z))
        .try_normalize()
        .unwrap_or_else(|| z.any_orthonormal_vector());
    Some(DAffine3::from_cols(x, z.cross(x), z, origin))
}

/// The top-level arguments of a simple instance, as text.
fn args(body: &str) -> Vec<&str> {
    let Some(open) = body.find('(') else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let (mut depth, mut start, mut in_string) = (0usize, open + 1, false);
    for (i, c) in body.char_indices().skip_while(|&(i, _)| i <= open) {
        match c {
            '\'' => in_string = !in_string,
            _ if in_string => {}
            '(' => depth += 1,
            ')' if depth == 0 => {
                out.push(body[start..i].trim());
                break;
            }
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(body[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out
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
pub(crate) fn reach(entities: &BTreeMap<u64, String>, id: u64) -> (Vec<DVec3>, f64) {
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
            "CIRCLE"
            | "ELLIPSE"
            | "CYLINDRICAL_SURFACE"
            | "SPHERICAL_SURFACE"
            | "CONICAL_SURFACE"
            | "TOROIDAL_SURFACE" => r = r.max(radius(body)),
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
pub(crate) fn kind(body: &str) -> &str {
    body.split('(').next().map_or("", str::trim)
}

/// Every `#n` reference outside strings, in order.
pub(crate) fn refs(body: &str) -> Vec<u64> {
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
pub(crate) fn numbers(body: &str) -> Vec<f64> {
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
pub(crate) fn point(body: &str) -> Option<DVec3> {
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

/// Radians per plane angle unit: one unless a context assigns another. Only a cone's
/// half-angle needs it, so an angle unit this cannot read is taken as radians.
fn angle_unit(entities: &BTreeMap<u64, String>) -> f64 {
    for body in entities.values() {
        let Some(at) = body.find("GLOBAL_UNIT_ASSIGNED_CONTEXT") else {
            continue;
        };
        let tail = &body[at..];
        let group = tail
            .find('(')
            .and_then(|o| tail[o..].find(')').map(|c| &tail[o..o + c]))
            .unwrap_or("");
        for id in refs(group) {
            let Some(unit) = entities.get(&id) else {
                continue;
            };
            if !unit.contains("PLANE_ANGLE_UNIT") {
                continue;
            }
            // ( CONVERSION_BASED_UNIT('DEGREE',#m) ... ), with #m
            // PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.0174532925),#radian)
            if unit.contains("CONVERSION_BASED_UNIT")
                && let Some(m) = refs(unit).first().and_then(|m| entities.get(m))
                && let Some(&f) = numbers(m).first()
                && f > 0.0
            {
                return f;
            }
            return 1.0;
        }
    }
    1.0
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
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
