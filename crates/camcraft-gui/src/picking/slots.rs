//! Picking into the job: which property a click in the view fills, what it takes,
//! and what a click does to the job.
//!
//! A property that takes picks is a *slot*: a setup's work zero, its models or its
//! stocks, an operation's faces or its edges. Its Select button arms the view for it, so the view offers
//! only what the slot takes; a click then writes straight into the job, where it is
//! one step to undo like any other edit. A work zero takes one point and is done; a
//! list of faces or edges takes any number, a second click on one taking it out, and
//! is done when the person says so.
//!
//! What the view and the panel show of the picks made is worked out in
//! [`super::shown`].

use camcraft_core::job::{BoxOf, FeatureRef, Origin};
use camcraft_core::setup::features::{self, FeatureError};
use camcraft_geom::topology::Body;
use glam::DVec3;

use crate::document::{Document, Selection};
use crate::picking::ray::{Accept, PickModel, PickScene, Target};
pub use crate::picking::shown::Shown;

/// A property a click in the view can fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    /// Setup `setup`'s work zero: a point on a model or on the stock's box.
    WorkZero {
        /// The setup, by name.
        setup: String,
    },
    /// The models setup `setup` machines. The job's others are shown faintly, to
    /// be clicked in.
    Models {
        /// The setup, by name.
        setup: String,
    },
    /// The stocks setup `setup` starts from, likewise.
    Stocks {
        /// The setup, by name.
        setup: String,
    },
    /// The faces operation `op` of setup `setup` is kept to.
    Faces {
        /// The setup, by name.
        setup: String,
        /// The operation's place in it.
        op: usize,
    },
    /// The edges operation `op` of setup `setup` is kept to.
    Edges {
        /// The setup, by name.
        setup: String,
        /// The operation's place in it.
        op: usize,
    },
}

impl Slot {
    /// What a click may land on while this is being filled.
    pub fn accept(&self) -> Accept {
        match self {
            Slot::WorkZero { .. } => Accept::Points,
            Slot::Models { .. } => Accept::Models,
            Slot::Stocks { .. } => Accept::Stocks,
            Slot::Faces { .. } => Accept::Faces,
            Slot::Edges { .. } => Accept::Edges,
        }
    }

    /// Whether one pick fills it, so it is done after the first click.
    pub fn single(&self) -> bool {
        matches!(self, Slot::WorkZero { .. })
    }

    /// What the view says while it is armed.
    pub fn hint(&self) -> &'static str {
        match self {
            Slot::WorkZero { .. } => {
                "Click a corner, a round edge for its centre, or a point on the stock · Esc to stop"
            }
            Slot::Models { .. } => {
                "Click models to add them to the setup or take them out · Esc or Done to finish"
            }
            Slot::Stocks { .. } => {
                "Click stocks to add them to the setup or take them out · Esc or Done to finish"
            }
            Slot::Faces { .. } => "Click faces to add or remove them · Esc or Done to finish",
            Slot::Edges { .. } => {
                "Click an edge to add the loop it goes round, or take it out · Esc or Done to finish"
            }
        }
    }

    /// Whether it still makes sense to be filling it: its setup and operation are
    /// still there, and still what the panel shows. Undoing past an operation, or
    /// selecting something else, stops picking into it.
    pub fn still_valid(&self, doc: &Document) -> bool {
        match self {
            Slot::WorkZero { setup } | Slot::Models { setup } | Slot::Stocks { setup } => {
                doc.setup(setup).is_some() && doc.selection == Selection::Setup(setup.clone())
            }
            Slot::Faces { setup, op } | Slot::Edges { setup, op } => {
                doc.operation(setup, *op).is_some()
                    && doc.selection == Selection::Operation(setup.clone(), *op)
            }
        }
    }
}

/// Write a click on `target` into the job, for `slot`. Returns whether anything
/// changed: a click on something the slot does not take changes nothing.
///
/// A face or edge already in the list is taken out instead, as picking works in
/// CAD; `current` says which feature a saved pick finds now, so one saved before
/// the model was renumbered is still recognised as the same.
pub fn apply(
    doc: &mut Document,
    slot: &Slot,
    target: &Target,
    current: impl Fn(&FeatureRef, Kind) -> Option<u32>,
) -> bool {
    match (slot, target) {
        (Slot::WorkZero { setup }, Target::Point { point, .. }) => {
            set_origin(doc, setup, |offset| Origin::Point {
                point: point.clone(),
                offset,
            })
        }
        (Slot::WorkZero { setup }, &Target::StockPoint { x, y, z, .. }) => {
            set_origin(doc, setup, |offset| Origin::BoxPoint {
                of: BoxOf::Stock,
                x,
                y,
                z,
                offset,
            })
        }
        (Slot::Models { setup }, Target::Model { model }) => doc
            .setup_mut(setup)
            .is_some_and(|s| toggle_name(&mut s.models, model)),
        (Slot::Stocks { setup }, Target::Stock { stock }) => doc
            .setup_mut(setup)
            .is_some_and(|s| toggle_name(&mut s.stocks, stock)),
        (Slot::Faces { setup, op }, Target::Face { model, face, at }) => {
            let at = own(doc, model, *at);
            doc.operation_mut(setup, *op).is_some_and(|o| {
                toggle(&mut o.selection.faces, model, *face, at, |r| {
                    current(r, Kind::Face)
                })
            })
        }
        (Slot::Edges { setup, op }, Target::Edge { model, edge, at }) => {
            let at = own(doc, model, *at);
            doc.operation_mut(setup, *op).is_some_and(|o| {
                toggle(&mut o.selection.edges, model, *edge, at, |r| {
                    current(r, Kind::Edge)
                })
            })
        }
        (Slot::Edges { setup, op }, Target::Loop { model, edges }) => {
            let edges: Vec<(u32, DVec3)> = edges
                .iter()
                .map(|&(e, at)| (e, own(doc, model, at)))
                .collect();
            doc.operation_mut(setup, *op).is_some_and(|o| {
                toggle_loop(&mut o.selection.edges, model, &edges, |r| {
                    current(r, Kind::Edge)
                })
            })
        }
        _ => false,
    }
}

/// Where `at`, on `model` where the job places it, is in the model's own
/// coordinates: where a pick is saved, so it moves with its model.
fn own(doc: &Document, model: &str, at: DVec3) -> DVec3 {
    doc.models
        .get(model)
        .and_then(|s| s.model())
        .map_or(at, |m| DVec3::from_array(features::unplaced(&m.body, at)))
}

/// What a click on `target` picks into `slot`: for edges, the whole loop the edge
/// goes round ([`edge_loop`]), as they are only ever cut round in loops.
pub fn widen(scene: &PickScene, slot: &Slot, target: Target) -> Target {
    match (slot, target) {
        (Slot::Edges { .. }, t) => whole_loop(scene, t),
        (_, t) => t,
    }
}

/// An edge as the loop it goes round; anything else as it is.
pub fn whole_loop(scene: &PickScene, target: Target) -> Target {
    let Target::Edge { model, edge, at } = target else {
        return target;
    };
    let Some(body) = scene.model(&model).map(PickModel::body) else {
        return Target::Edge { model, edge, at };
    };
    let t = body.topology();
    let edges = edge_loop(scene, &model, edge)
        .into_iter()
        .map(|e| {
            let on = if e == edge {
                at
            } else {
                t.edge_middle(body.mesh(), e).unwrap_or(at)
            };
            (e, on)
        })
        .collect();
    Target::Loop { model, edges }
}

/// The loop edge `edge` of `model` goes round, in order from it: the edges joined
/// to it end to end at its height in the setup, turning least at each corner
/// ([`Topology::level_loop`](camcraft_geom::topology::Topology::level_loop)). So
/// an edge of a pocket's floor or of the part's outline takes the whole of it,
/// however its faces are split, and a hole's rim is its own loop. An edge that is
/// not level, or whose loop does not close, is taken alone.
pub fn edge_loop(scene: &PickScene, model: &str, edge: u32) -> Vec<u32> {
    let Some(pm) = scene.model(model) else {
        return vec![edge];
    };
    let t = pm.body().topology();
    let points = |e: u32| pm.edges.get(e as usize).cloned().unwrap_or_default();
    t.level_loop(edge, points, t.tolerance())
        .unwrap_or_else(|| vec![edge])
}

/// Take a loop out of `list` if all of it is there, or else add what is missing.
fn toggle_loop(
    list: &mut Vec<FeatureRef>,
    model: &str,
    edges: &[(u32, DVec3)],
    current: impl Fn(&FeatureRef) -> Option<u32>,
) -> bool {
    let has = |list: &[FeatureRef], e: u32| {
        list.iter()
            .any(|r| r.model == model && current(r) == Some(e))
    };
    if edges.iter().all(|&(e, _)| has(list, e)) {
        list.retain(|r| !(r.model == model && edges.iter().any(|&(e, _)| current(r) == Some(e))));
    } else {
        for &(e, at) in edges {
            if !has(list, e) {
                list.push(FeatureRef {
                    model: model.to_string(),
                    index: e,
                    at: at.to_array(),
                });
            }
        }
    }
    true
}

/// Take `name` out of `names` if it is there, or else add it.
fn toggle_name(names: &mut Vec<String>, name: &str) -> bool {
    if names.iter().any(|n| n == name) {
        names.retain(|n| n != name);
    } else {
        names.push(name.to_string());
    }
    true
}

/// Whether a saved pick is a face or an edge, for finding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A face.
    Face,
    /// An edge.
    Edge,
}

impl Kind {
    /// Which of `body`'s faces or edges saved pick `r` is now, found as the engine
    /// finds it, in a job machined to `tolerance`.
    pub fn find(self, body: &Body, r: &FeatureRef, tolerance: f64) -> Result<u32, FeatureError> {
        let tol = features::tolerance(tolerance, body);
        match self {
            Kind::Face => features::find_face(body, r, tol),
            Kind::Edge => features::find_edge(body, r, tol),
        }
        .map(|found| found.index)
    }
}

fn set_origin(doc: &mut Document, setup: &str, make: impl FnOnce([f64; 3]) -> Origin) -> bool {
    let Some(s) = doc.setup_mut(setup) else {
        return false;
    };
    // The offset stays: it is the person's, measured from wherever zero is.
    let origin = make(s.origin.offset());
    let changed = s.origin != origin;
    s.origin = origin;
    changed
}

fn toggle(
    list: &mut Vec<FeatureRef>,
    model: &str,
    index: u32,
    at: DVec3,
    current: impl Fn(&FeatureRef) -> Option<u32>,
) -> bool {
    let same = |r: &FeatureRef| r.model == model && current(r) == Some(index);
    if list.iter().any(same) {
        list.retain(|r| !same(r));
    } else {
        list.push(FeatureRef {
            model: model.to_string(),
            index,
            at: at.to_array(),
        });
    }
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests;
