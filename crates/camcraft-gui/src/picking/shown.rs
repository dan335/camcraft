//! What the view and the panel show of the picks already made: a setup's work
//! zero, and an operation's faces, edges and the boundary or outline they make,
//! and its surface groups' faces with the part tinted by what it does to each.
//!
//! What is shown of a pick is found the same way the engine finds it
//! ([`camcraft_core::setup::features`]), so a face lit in the view is the face that will be
//! cut, and a pick that can no longer be found says so here before it stops a run.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use camcraft_core::cutter::allowance::EffectiveTool;
use camcraft_core::cutter::tool::ToolProfile;
use camcraft_core::job::{
    self, Boundary, Curves, FeatureRef, MachiningBoundary, Operation, Origin, Outline as Cut,
    StrategySpec, SurfaceAction, Surfaces,
};
use camcraft_core::run::cutting_tool;
use camcraft_core::setup::features::{self, FeatureError};
use camcraft_core::setup::{Bodies, PrepareError, Prepared};
use camcraft_core::strategy::planar::{centre_offset, picked_pieces};
use camcraft_geom::contour::Region;
use camcraft_geom::mesh::TriMesh;
use camcraft_geom::topology::Body;
use glam::{DVec2, DVec3, Vec3};

use crate::document::Document;
use crate::picking::describe::{describe_edge, describe_face, describe_point};
use crate::picking::highlight::{face_triangles, model_edge_lines};
use crate::picking::ray::{PickModel, PickScene};
use crate::picking::slots::{Kind, chamfer_run, edge_loop, smooth_chain};
use crate::ui::measure::Measure;
use crate::view::viewport::{FaceTint, Highlight, Outline, OutlineKind};

/// What the view and the panel show of the picks for what is selected: a setup's
/// picked work zero, or an operation's faces, edges and the boundary they make.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shown {
    /// Drawn over the part.
    pub highlight: Highlight,
    /// The work zero's point: what it is, or why it cannot be found.
    pub origin: Option<Result<String, String>>,
    /// Each picked face, in the job's order: what it is, or why it cannot be
    /// found.
    pub faces: Vec<Result<String, String>>,
    /// Each picked edge, likewise.
    pub edges: Vec<Result<String, String>>,
    /// The picked edges as the panel lists them: each loop once, as its picks'
    /// places in the job's order and what it is, and each pick not found alone.
    pub edge_loops: Vec<(Vec<usize>, Result<String, String>)>,
    /// Each surface group's faces, in the job's order: what each is, or why it
    /// cannot be found or is refused.
    pub groups: Vec<Vec<Result<String, String>>>,
    /// The part's faces tinted by what the operation does to them, where it
    /// does anything but machine them all.
    pub tint: Option<FaceTint>,
    /// The boundary the picks make, or why they make none.
    pub boundary: Option<Result<(), String>>,
    /// The boundary is drawn from the part's silhouette, which is still being
    /// worked out: it is drawn once it is.
    pub waiting: bool,
}

/// The picks of `setup`, and of its operation `op` when one is selected, as they
/// stand on the models laid out in `prepared`, whose part's silhouette is
/// `silhouette`, or `Ok(None)` while it is cast ([`Layout::silhouette`]). The
/// faces drawn as picked are those of what is being picked into: surface group
/// `group` while it is, else the operation's own.
///
/// [`Layout::silhouette`]: crate::document::Layout::silhouette
pub fn shown(
    doc: &Document,
    (setup, op, group): (&str, Option<usize>, Option<usize>),
    (prepared, silhouette): (&Prepared, Result<Option<&Region>, String>),
    scene: &PickScene,
    m: Measure,
) -> Shown {
    let mut out = Shown::default();
    let Some(s) = doc.setup(setup) else {
        return out;
    };
    // The setup's models, one its sheet had no room for among them: a pick on
    // it is on nothing, and says so.
    let bodies = Bodies::standing(
        scene
            .models
            .iter()
            .filter(|m| m.in_setup || s.models.iter().any(|n| n == m.name()))
            .map(|m| (m.name(), m.body(), m.stands())),
    );
    let tolerance = doc.job.tolerance;

    if let Origin::Point { point, .. } = &s.origin {
        // The work zero is one place, so it is found on a model only where the
        // model stands once, as laying the setup out finds it.
        out.origin = Some(match bodies.standing_once(&point.model) {
            Ok((body, stand)) => {
                match features::find_point(body, point, features::tolerance(tolerance, body)) {
                    Ok((p, found)) => {
                        out.highlight
                            .markers
                            .push(scene.frame.to_setup_as(stand, p));
                        Ok(describe_point(
                            body,
                            &point.model,
                            point.snap,
                            found.index,
                            m,
                        ))
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            // The pick's own words: the row it is shown in says it is the work
            // zero's.
            Err(PrepareError::Feature(e)) => Err(e.to_string()),
            Err(e) => Err(e.to_string()),
        });
    }

    let Some(operation) = op.and_then(|i| s.operations.get(i)) else {
        return out;
    };
    let sel = &operation.selection;
    // Each pick found, with the model it is on, or why not.
    let find = |r: &FeatureRef, kind: Kind| {
        let copies = bodies
            .get(&r.model)
            .ok_or_else(|| FeatureError::NotLoaded(r.model.clone()).to_string())?;
        if copies.stands.is_empty() {
            return Err(FeatureError::NotPlaced(r.model.clone()).to_string());
        }
        let body = copies.body;
        let index = kind.find(body, r, tolerance).map_err(|e| e.to_string())?;
        Ok::<_, String>((body, index))
    };
    let uses = operation.strategy.uses();
    let curves = uses.curves;
    for r in &sel.faces {
        out.faces.push(find(r, Kind::Face).map(|(body, face)| {
            if group.is_none() {
                every_copy(&mut out.highlight, scene, &bodies, (&r.model, body, face));
            }
            // A chamfer's face is its edges, each chamfered, and an
            // engraving's its outline, engraved: drawn as picked.
            if matches!(curves, Some(Curves::Chamfered | Curves::Engraved))
                && let Some(pm) = scene.model(&r.model)
            {
                for &e in body.topology().face(face).map_or(&[][..], |f| &f.edges) {
                    out.highlight
                        .lines
                        .extend(model_edge_lines(pm, e, OutlineKind::Selected));
                }
            }
            describe_face(body, &r.model, face, m)
        }));
    }
    // Each loop the picked edges go round, by model and its edges, with the picks
    // on it; a trace's, each chain they run on along; a chamfer's, each loop,
    // or each chain where they close no loop.
    type Round<'a> = (&'a str, Vec<u32>);
    let mut loops: Vec<(Round<'_>, bool, Vec<usize>)> = Vec::new();
    for (i, r) in sel.edges.iter().enumerate() {
        let found = find(r, Kind::Edge);
        if let Ok((_, edge)) = &found {
            if let Some(pm) = scene.model(&r.model) {
                out.highlight
                    .lines
                    .extend(model_edge_lines(pm, *edge, OutlineKind::Selected));
            }
            let (mut round, closed) = match curves {
                Some(Curves::Traced) => (smooth_chain(scene, &r.model, *edge), false),
                Some(Curves::Chamfered) => chamfer_run(scene, &r.model, *edge),
                _ => (edge_loop(scene, &r.model, *edge), true),
            };
            round.sort_unstable();
            let key = (r.model.as_str(), round);
            match loops.iter_mut().find(|(k, ..)| *k == key) {
                Some((.., picks)) => picks.push(i),
                None => loops.push((key, closed, vec![i])),
            }
        } else {
            loops.push((("", Vec::new()), true, vec![i]));
        }
        out.edges
            .push(found.map(|(body, edge)| describe_edge(body, &r.model, edge, m)));
    }
    for ((model, round), closed, picks) in loops {
        let chains = !closed;
        let what = match (round.len(), picks.first()) {
            (0 | 1, Some(&i)) => out.edges.get(i).cloned().unwrap_or(Ok(String::new())),
            (n, _) if picks.len() < n => Ok(format!(
                "{} of the {n} edges of a {} · on '{model}'",
                picks.len(),
                if chains { "chain" } else { "loop" }
            )),
            (n, _) if chains => Ok(format!("Chain of {n} edges · on '{model}'")),
            (n, _) => Ok(format!(
                "Loop of {n} edges{} · on '{model}'",
                scene
                    .model(model)
                    .and_then(|pm| loop_size(scene, pm, &round, m))
                    .unwrap_or_default()
            )),
        };
        out.edge_loops.push((picks, what));
    }

    // Each surface group's faces, found as the run finds them, and the group
    // each face found is in: the first, one in two refused as the run
    // refuses it. The group being picked into is drawn as picked.
    if uses.surfaces {
        let surfaces = &operation.surfaces;
        let mut grouped: BTreeMap<(&str, u32), usize> = BTreeMap::new();
        for (k, g) in surfaces.groups.iter().enumerate() {
            let mut rows = Vec::with_capacity(g.faces.len());
            for r in &g.faces {
                rows.push(find(r, Kind::Face).and_then(|(body, face)| {
                    match grouped.entry((r.model.as_str(), face)) {
                        Entry::Occupied(first) if *first.get() != k => {
                            return Err(FeatureError::TwoGroups {
                                model: r.model.clone(),
                                face,
                                first: first.get() + 1,
                                second: k + 1,
                            }
                            .to_string());
                        }
                        Entry::Occupied(_) => {}
                        Entry::Vacant(v) => {
                            v.insert(k);
                        }
                    }
                    if group == Some(k) {
                        every_copy(&mut out.highlight, scene, &bodies, (&r.model, body, face));
                    }
                    Ok(describe_face(body, &r.model, face, m))
                }));
            }
            out.groups.push(rows);
        }
        if !surfaces.is_whole() {
            out.tint = tint(surfaces, &grouped, prepared, &bodies);
        }
    }

    // The boundary, drawn at the stock's top so it reads over everything. Worked
    // out as the run will work it out, with the operation's own tool.
    let found = out.faces.iter().chain(&out.edges).all(Result::is_ok);
    let z = prepared.stock_bounds().max.z;
    // Facing works from the stock alone: what it faces is its footprint and the
    // stock offset.
    if let StrategySpec::Face { stock_offset, .. } = operation.strategy {
        let b = prepared.stock_bounds();
        let grow = DVec2::splat(stock_offset);
        let lo = b.min.truncate() - grow;
        let hi = b.max.truncate() + grow;
        out.boundary = Some(if hi.x > lo.x && hi.y > lo.y {
            let area = Region::rectangle(lo, hi);
            out.highlight
                .lines
                .extend(rings_at(&area, z, OutlineKind::Boundary));
            Ok(())
        } else {
            Err("the stock offset leaves nothing to face".into())
        });
        return out;
    }
    // What is drawn from the part's silhouette waits for it to be cast,
    // elsewhere, rather than casting it while a frame is drawn.
    let tool = cutting_tool(&doc.job, operation).ok();
    let b = &operation.boundary;
    let needs_silhouette = if uses.outline {
        operation.outline == Cut::Silhouette
    } else {
        uses.boundary
            && b.kind != Boundary::Stock
            && (b.kind == Boundary::Silhouette || tool.is_some())
    };
    let part = match silhouette {
        Ok(Some(part)) => Some(part),
        Ok(None) if needs_silhouette => {
            out.waiting = found;
            return out;
        }
        Err(why) if needs_silhouette => {
            out.boundary = found.then_some(Err(why));
            return out;
        }
        _ => None,
    };
    if uses.outline {
        if found {
            out.boundary = Some(
                outline_lines(operation, tool.as_ref(), prepared, &bodies, tolerance, z)
                    .map(|lines| out.highlight.lines.extend(lines))
                    .map_err(|e| e.to_string()),
            );
        }
        return out;
    }
    if uses.boundary && b.kind != Boundary::Stock && found {
        out.boundary = Some(
            boundary_lines(
                b,
                sel,
                tool.as_ref(),
                (prepared, part),
                &bodies,
                tolerance,
                z,
            )
            .map(|lines| out.highlight.lines.extend(lines))
            .map_err(|e| e.to_string()),
        );
    }
    out
}

/// Face `face` of `model`, whose body is `body`, drawn into `h` on every copy
/// of the model among `bodies`: a pick is found once on the body, and means
/// the face on every copy.
fn every_copy(
    h: &mut Highlight,
    scene: &PickScene,
    bodies: &Bodies<'_>,
    (model, body, face): (&str, &Body, u32),
) {
    for stand in bodies.get(model).map_or(&[][..], |c| c.stands) {
        let to_setup = |p: DVec3| scene.frame.to_setup_as(stand, p);
        h.faces.extend(face_triangles(body, face, &to_setup));
    }
}

/// The part laid out as `prepared` from `bodies`, its faces sorted by what
/// `surfaces` does to them, as the run sorts them: a face in a group, by its
/// model and number in `grouped`, as its group does; every other face as the
/// model does. Each triangle is the part's own, so it lies exactly on it.
fn tint(
    surfaces: &Surfaces,
    grouped: &BTreeMap<(&str, u32), usize>,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
) -> Option<FaceTint> {
    let action = |k: Option<&usize>| {
        k.and_then(|&k| surfaces.groups.get(k))
            .map_or(surfaces.model.action, |g| g.action)
    };
    let meshes = prepared
        .by_face(bodies, 2, |piece, face| {
            let k = face.and_then(|f| grouped.get(&(piece.model.as_str(), f)));
            Some(usize::from(action(k) == SurfaceAction::Avoid))
        })
        .ok()?;
    let triangles = |mesh: &TriMesh| -> Vec<[Vec3; 3]> {
        (0..mesh.len())
            .filter_map(|t| mesh.triangle(t))
            .map(|t| t.map(DVec3::as_vec3))
            .collect()
    };
    match meshes.as_slice() {
        [machined, avoided] => Some(FaceTint {
            machined: triangles(machined),
            avoided: triangles(avoided),
        }),
        _ => None,
    }
}

/// " · 31.80 × 15.80 mm", the size of loop `round` of `pm`'s edges seen from
/// above, on its body as its own transform places it: the loop is the same
/// loop on every copy.
fn loop_size(scene: &PickScene, pm: &PickModel, round: &[u32], m: Measure) -> Option<String> {
    let mut points = round.iter().flat_map(|&e| scene.body_edge(pm, e));
    let first = points.next()?.truncate();
    let (lo, hi) = points.fold((first, first), |(lo, hi), p| {
        (lo.min(p.truncate()), hi.max(p.truncate()))
    });
    let size = hi - lo;
    Some(format!(
        " · {} × {}",
        m.number(size.x, 2),
        m.length(size.y, 2)
    ))
}

/// A 2D operation's outline as lines, and hatched, what the tool cuts away beside
/// it: a band along it for a profile, all inside it for a pocket. Each piece of it
/// is drawn at the lowest of the picks in it, so it lies on them; with nothing
/// picked in it, at `top`, the stock's top. The stock's footprint when that is the
/// boundary. Nothing is hatched without a tool.
fn outline_lines(
    op: &Operation,
    tool: Option<&EffectiveTool>,
    prepared: &Prepared,
    bodies: &Bodies<'_>,
    tolerance: f64,
    top: f64,
) -> Result<Vec<Outline>, FeatureError> {
    let region = |e: camcraft_geom::contour::RegionError| FeatureError::Region(e.to_string());
    let (shape, picked) = features::outline(op, prepared, bodies, tolerance)?;
    let shape = shape.unwrap_or_else(|| {
        let b = prepared.stock_bounds();
        Region::rectangle(b.min.truncate(), b.max.truncate())
    });
    let arc = tolerance / 2.0;
    let radius = tool.map_or(0.0, EffectiveTool::cutter_radius);
    let offset = centre_offset(op, radius);
    let pocket = matches!(
        op.strategy,
        StrategySpec::Pocket { .. } | StrategySpec::Adaptive2d { .. }
    );
    let mut lines = Vec::new();
    let mut room = false;
    for (piece, picks) in picked_pieces(&shape, arc, &picked) {
        let z = picks.map_or(top, |p| p.lowest);
        lines.extend(rings_at(&piece, z, OutlineKind::Boundary));
        if radius <= 0.0 {
            room = true;
            continue;
        }
        let centres = piece.offset(offset, arc);
        if centres.area() <= 1e-9 {
            continue;
        }
        room = true;
        let reach = centres.offset(radius, arc);
        let cut = if pocket {
            reach
        } else {
            reach
                .difference(&centres.offset(-radius, arc))
                .map_err(region)?
        };
        lines.extend(hatched(&cut, z).map_err(region)?);
    }
    if !room {
        return Err(FeatureError::TooWide {
            diameter: radius * 2.0,
        });
    }
    Ok(lines)
}

/// An operation's machining boundary as lines at height `z`: the boundary as
/// chosen, where the tool's centre may go against it, and hatched, what the tool
/// cuts outside the part. That last is worked out against the part's whole
/// outline, `part`, so for a wall straight down it is exact, and for one that
/// leans out it is where the tool reaches at the bottom. `part` is there
/// whenever the boundary is the silhouette or there is a tool.
fn boundary_lines(
    b: &MachiningBoundary,
    selection: &job::Selection,
    tool: Option<&EffectiveTool>,
    (prepared, part): (&Prepared, Option<&Region>),
    bodies: &Bodies<'_>,
    tolerance: f64,
    z: f64,
) -> Result<Vec<Outline>, FeatureError> {
    let silhouette =
        || part.ok_or_else(|| FeatureError::Region("the part's silhouette is not cast".into()));
    // The boundary as [`features::boundary_shape`] gives it, but for the part's
    // silhouette, which the layout keeps rather than have it worked out here and
    // again below on every change.
    let other;
    let shape = match b.kind {
        Boundary::Silhouette => {
            let s = silhouette()?;
            if s.area() <= 1e-6 {
                return Err(FeatureError::NoArea);
            }
            s
        }
        kind => match features::boundary_shape(kind, selection, prepared, bodies, tolerance)? {
            Some(s) => {
                other = s;
                &other
            }
            None => return Ok(Vec::new()),
        },
    };
    let radius = tool.map_or(0.0, EffectiveTool::cutter_radius);
    let arc = tolerance / 2.0;
    let limit = shape.offset(features::containment_offset(b, radius), arc);
    if limit.area() <= 1e-9 {
        return Err(FeatureError::NoRoom {
            diameter: radius * 2.0,
        });
    }
    let mut lines = rings_at(shape, z, OutlineKind::Boundary);
    lines.extend(rings_at(&limit, z, OutlineKind::Limit));
    if let Some(tool) = tool {
        let region = |e: camcraft_geom::contour::RegionError| FeatureError::Region(e.to_string());
        let part = silhouette()?;
        let centres = limit
            .difference(&part.offset(tool.max_radius(), arc))
            .map_err(region)?;
        let cut = centres
            .offset(radius, arc)
            .difference(part)
            .map_err(region)?;
        lines.extend(hatched(&cut, z).map_err(region)?);
    }
    Ok(lines)
}

/// `region` hatched at height `z`.
fn hatched(region: &Region, z: f64) -> Result<Vec<Outline>, camcraft_geom::contour::RegionError> {
    let mut lines = Vec::new();
    for line in region.clip_polylines(&hatch(region))? {
        lines.extend(line.windows(2).filter_map(|w| match w {
            [a, b] => Some(Outline {
                a: DVec3::new(a.x, a.y, z),
                b: DVec3::new(b.x, b.y, z),
                kind: OutlineKind::Channel,
            }),
            _ => None,
        }));
    }
    Ok(lines)
}

/// Each ring of `region` as closed lines at height `z`.
fn rings_at(region: &Region, z: f64, kind: OutlineKind) -> Vec<Outline> {
    let mut out = Vec::new();
    for ring in region.rings() {
        let pts = ring.points();
        let next = pts.iter().skip(1).chain(pts.first());
        out.extend(pts.iter().zip(next).map(|(a, b)| Outline {
            a: DVec3::new(a.x, a.y, z),
            b: DVec3::new(b.x, b.y, z),
            kind,
        }));
    }
    out
}

/// Diagonal lines across `region`'s bounds, to be clipped to it: some sixty
/// across its longer side, and no closer than a millimetre and a half.
fn hatch(region: &Region) -> Vec<Vec<DVec2>> {
    let Some((lo, hi)) = region.bounds() else {
        return Vec::new();
    };
    let size = hi - lo;
    let gap = (size.x.max(size.y) / 60.0).max(1.5);
    // Lines x - y = c, from the lowest c to the highest.
    let (from, to) = (lo.x - hi.y, hi.x - lo.y);
    let mut out = Vec::new();
    let mut c = from + gap / 2.0;
    while c < to {
        out.push(vec![DVec2::new(lo.y + c, lo.y), DVec2::new(hi.y + c, hi.y)]);
        c += gap;
    }
    out
}
