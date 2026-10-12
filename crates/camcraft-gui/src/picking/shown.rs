//! What the view and the panel show of the picks already made: a setup's work
//! zero, and an operation's faces, edges and the boundary or outline they make.
//!
//! What is shown of a pick is found the same way the engine finds it
//! ([`camcraft_core::setup::features`]), so a face lit in the view is the face that will be
//! cut, and a pick that can no longer be found says so here before it stops a run.

use camcraft_core::cutter::allowance::EffectiveTool;
use camcraft_core::cutter::tool::ToolProfile;
use camcraft_core::job::{
    self, Boundary, FeatureRef, MachiningBoundary, Operation, Origin, StrategySpec,
};
use camcraft_core::run::cutting_tool;
use camcraft_core::setup::features::{self, FeatureError};
use camcraft_core::setup::{Bodies, Prepared};
use camcraft_core::strategy::planar::{centre_offset, picked_pieces};
use camcraft_geom::contour::Region;
use glam::{DVec2, DVec3};

use crate::document::Document;
use crate::picking::describe::{describe_edge, describe_face, describe_point};
use crate::picking::highlight::{face_triangles, model_edge_lines};
use crate::picking::ray::{PickModel, PickScene};
use crate::picking::slots::{Kind, edge_loop};
use crate::ui::measure::Measure;
use crate::view::viewport::{Highlight, Outline, OutlineKind};

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
    /// The boundary the picks make, or why they make none.
    pub boundary: Option<Result<(), String>>,
}

/// The picks of `setup`, and of its operation `op` when one is selected, as they
/// stand on the models laid out in `prepared`.
pub fn shown(
    doc: &Document,
    setup: &str,
    op: Option<usize>,
    prepared: &Prepared,
    scene: &PickScene,
    m: Measure,
) -> Shown {
    let mut out = Shown::default();
    let Some(s) = doc.setup(setup) else {
        return out;
    };
    let bodies: Bodies<'_> = scene.setup_models().map(|m| (m.name(), m.body())).collect();
    let tolerance = doc.job.tolerance;
    let to_setup = |p: DVec3| scene.frame.to_setup(p);

    if let Origin::Point { point, .. } = &s.origin {
        out.origin = Some(match bodies.get(point.model.as_str()) {
            Some(body) => {
                match features::find_point(body, point, features::tolerance(tolerance, body)) {
                    Ok((p, found)) => {
                        out.highlight.markers.push(to_setup(p));
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
            None => Err(FeatureError::NotLoaded(point.model.clone()).to_string()),
        });
    }

    let Some(operation) = op.and_then(|i| s.operations.get(i)) else {
        return out;
    };
    let sel = &operation.selection;
    // Each pick found, with the model it is on, or why not.
    let find = |r: &FeatureRef, kind: Kind| {
        let body = bodies
            .get(r.model.as_str())
            .ok_or_else(|| FeatureError::NotLoaded(r.model.clone()).to_string())?;
        let index = kind.find(body, r, tolerance).map_err(|e| e.to_string())?;
        Ok::<_, String>((*body, index))
    };
    for r in &sel.faces {
        out.faces.push(find(r, Kind::Face).map(|(body, face)| {
            out.highlight
                .faces
                .extend(face_triangles(body, face, &to_setup));
            describe_face(body, &r.model, face, m)
        }));
    }
    // Each loop the picked edges go round, by model and its edges, with the picks
    // on it.
    type Round<'a> = (&'a str, Vec<u32>);
    let mut loops: Vec<(Round<'_>, Vec<usize>)> = Vec::new();
    for (i, r) in sel.edges.iter().enumerate() {
        let found = find(r, Kind::Edge);
        if let Ok((_, edge)) = &found {
            if let Some(pm) = scene.model(&r.model) {
                out.highlight
                    .lines
                    .extend(model_edge_lines(pm, *edge, OutlineKind::Selected));
            }
            let mut round = edge_loop(scene, &r.model, *edge);
            round.sort_unstable();
            let key = (r.model.as_str(), round);
            match loops.iter_mut().find(|(k, _)| *k == key) {
                Some((_, picks)) => picks.push(i),
                None => loops.push((key, vec![i])),
            }
        } else {
            loops.push((("", Vec::new()), vec![i]));
        }
        out.edges
            .push(found.map(|(body, edge)| describe_edge(body, &r.model, edge, m)));
    }
    for ((model, round), picks) in loops {
        let what = match (round.len(), picks.first()) {
            (0 | 1, Some(&i)) => out.edges.get(i).cloned().unwrap_or(Ok(String::new())),
            (n, _) if picks.len() < n => Ok(format!(
                "{} of the {n} edges of a loop · on '{model}'",
                picks.len()
            )),
            (n, _) => Ok(format!(
                "Loop of {n} edges{} · on '{model}'",
                scene
                    .model(model)
                    .and_then(|pm| loop_size(pm, &round, m))
                    .unwrap_or_default()
            )),
        };
        out.edge_loops.push((picks, what));
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
    let uses = operation.strategy.uses();
    if uses.outline {
        if found {
            let tool = cutting_tool(&doc.job, operation).ok();
            out.boundary = Some(
                outline_lines(operation, tool.as_ref(), prepared, &bodies, tolerance, z)
                    .map(|lines| out.highlight.lines.extend(lines))
                    .map_err(|e| e.to_string()),
            );
        }
        return out;
    }
    let b = &operation.boundary;
    if uses.boundary && b.kind != Boundary::Stock && found {
        let tool = cutting_tool(&doc.job, operation).ok();
        out.boundary = Some(
            boundary_lines(
                b,
                sel,
                tool.as_ref(),
                (prepared, scene),
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

/// " · 31.80 × 15.80 mm", the size of loop `round` of `pm`'s edges seen from above.
fn loop_size(pm: &PickModel, round: &[u32], m: Measure) -> Option<String> {
    let mut points = round
        .iter()
        .filter_map(|&e| pm.edges.get(e as usize))
        .flatten();
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
    let pocket = matches!(op.strategy, StrategySpec::Pocket { .. });
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
/// outline, so for a wall straight down it is exact, and for one that leans out
/// it is where the tool reaches at the bottom.
fn boundary_lines(
    b: &MachiningBoundary,
    selection: &job::Selection,
    tool: Option<&EffectiveTool>,
    (prepared, scene): (&Prepared, &PickScene),
    bodies: &Bodies<'_>,
    tolerance: f64,
    z: f64,
) -> Result<Vec<Outline>, FeatureError> {
    // The boundary as [`features::boundary_shape`] gives it, but for the part's
    // silhouette, which the scene keeps rather than have it worked out here and
    // again below on every change.
    let other;
    let shape = match b.kind {
        Boundary::Silhouette => {
            let s = scene.silhouette(prepared)?;
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
        let part = scene.silhouette(prepared)?;
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
