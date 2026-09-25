//! What a pick is called, in words: a face, an edge or a point on a model, or a
//! point on the stock's box, as the panel and the view's card show them.

use camcraft_core::job::{Along, Snap};
use camcraft_geom::topology::{Body, EdgeKind, FaceKind};

use crate::ui::measure::Measure;

/// Where the stock's corner, edge middle or face centre is, in words: "top,
/// front-left corner", "the middle of the top's front edge", "the centre of the
/// top".
pub fn describe_box_point(x: Along, y: Along, z: Along) -> String {
    let side = |a: Along, lo: &'static str, hi: &'static str| match a {
        Along::Min => Some(lo),
        Along::Max => Some(hi),
        Along::Center => None,
    };
    let (sx, sy, sz) = (
        side(x, "left", "right"),
        side(y, "front", "back"),
        side(z, "bottom", "top"),
    );
    match (sz, sy, sx) {
        (Some(z), Some(y), Some(x)) => format!("{z}, {y}-{x} corner"),
        (Some(z), Some(y), None) => format!("middle of the {z}'s {y} edge"),
        (Some(z), None, Some(x)) => format!("middle of the {z}'s {x} edge"),
        (None, Some(y), Some(x)) => format!("middle of the {y}-{x} upright edge"),
        (Some(s), None, None) | (None, Some(s), None) | (None, None, Some(s)) => {
            format!("centre of the {s}")
        }
        (None, None, None) => "centre".to_string(),
    }
}

/// How a round edge is described: "Ø6.35 circle" or "R3.00 arc".
pub fn describe_circle(body: &Body, edge: u32) -> Option<String> {
    match body.topology().edge(edge)?.kind {
        EdgeKind::Circle(c) if c.full => Some(format!("Ø{:.2} circle", c.radius * 2.0)),
        EdgeKind::Circle(c) => Some(format!("R{:.2} arc", c.radius)),
        EdgeKind::Line | EdgeKind::Curve => None,
    }
}

/// "Face 12 · plane · 240.0 mm² · on 'part'".
pub fn describe_face(body: &Body, model: &str, face: u32, m: Measure) -> String {
    match body.topology().face(face) {
        Some(f) => {
            let kind = match f.kind {
                FaceKind::Plane { .. } => "plane",
                FaceKind::Curved => "curved",
            };
            format!("Face {face} · {kind} · {} · on '{model}'", m.area(f.area))
        }
        None => format!("Face {face} on '{model}'"),
    }
}

/// "Edge 3 · Ø6.35 circle · on 'part'", or a line or curve with its length.
pub fn describe_edge(body: &Body, model: &str, edge: u32, m: Measure) -> String {
    let Some(e) = body.topology().edge(edge) else {
        return format!("Edge {edge} on '{model}'");
    };
    let what = match e.kind {
        EdgeKind::Circle(c) if c.full => format!("Ø{} circle", m.number(c.radius * 2.0, 2)),
        EdgeKind::Circle(c) => format!("R{} arc", m.number(c.radius, 2)),
        EdgeKind::Line => format!("line {} long", m.length(e.length, 2)),
        EdgeKind::Curve => format!("curve {} long", m.length(e.length, 2)),
    };
    let closed = if e.closed { ", closed" } else { "" };
    format!("Edge {edge} · {what}{closed} · on '{model}'")
}

/// "Centre of the Ø6.35 circle on 'part'", "Corner 4 on 'part'", "Middle of edge 7
/// on 'part'".
pub fn describe_point(body: &Body, model: &str, snap: Snap, index: u32, m: Measure) -> String {
    match snap {
        Snap::Vertex => format!("Corner {index} on '{model}'"),
        Snap::CircleCenter => match describe_circle(body, index) {
            Some(c) => format!("Centre of the {c} on '{model}'"),
            None => format!("Centre of edge {index} on '{model}'"),
        },
        Snap::EdgeMiddle => {
            let long = body
                .topology()
                .edge(index)
                .map(|e| format!(", {} long", m.length(e.length, 2)))
                .unwrap_or_default();
            format!("Middle of edge {index}{long} on '{model}'")
        }
    }
}
