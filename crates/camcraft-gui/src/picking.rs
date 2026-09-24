//! Picking in the window: which slot is armed, what is under the pointer, what was
//! clicked to look at, and the work that is kept between frames so the pointer can
//! move freely over a large part.
//!
//! Three things are worked out once and kept until what they depend on changes:
//! the setup made ready for picking (its models' edges and points in the setup's
//! frame, redone when the layout is), what the selected setup or operation's picks
//! show (redone when the job's picks, the tool or the layout change), and what the
//! thing under the pointer shows (redone when the pointer moves onto something
//! else). Everything else is done as the pointer moves: a ray cast and a look along
//! the edges on the screen.

use camcraft_core::features;
use camcraft_core::job::{FeatureRef, Geometry, Origin, ToolSpec};
use eframe::egui;

use crate::document::{Document, Selection, next_key};
use crate::measure::Measure;
use crate::pick::{Accept, PickScene, PickStock, Target};
use crate::slots::{self, Kind, Shown, Slot};
use crate::theme;
use crate::viewport::{Content, Highlight, PointerEvent};
use crate::worker::DocId;

/// What the picks shown were worked out from: when any of it changes, they are
/// worked out again.
#[derive(Debug, Clone, PartialEq)]
struct ShownFrom {
    doc: DocId,
    layout: u64,
    setup: String,
    op: Option<usize>,
    origin: Origin,
    geometry: Option<Geometry>,
    tool: Option<ToolSpec>,
    tolerance: u64,
    measure: Measure,
}

/// Picking, across frames.
#[derive(Default)]
pub struct Picking {
    /// The slot a click fills, for which job.
    armed: Option<(DocId, Slot)>,
    /// What is under the pointer.
    hover: Option<Target>,
    /// A row of the panel's list of picks under the pointer, this frame, and
    /// the one last shown.
    row: Option<(Kind, usize)>,
    shown_row: Option<(Kind, usize)>,
    /// What was clicked, to look at, in which job and setup.
    inspect: Option<(DocId, String, Target)>,
    /// The showing setup, ready to pick from: for which job and layout.
    scene: Option<(DocId, u64, PickScene)>,
    /// What the selection's picks show, with the key its upload goes by.
    shown: Option<(ShownFrom, Shown, u64)>,
    /// What the thing under the pointer shows, with its upload key.
    hovered: Option<(Target, Highlight, u64)>,
    /// What could be picked into the armed slot, drawn faintly: for which job,
    /// layout and slot, with its upload key.
    ghost: Option<(DocId, u64, Slot, Highlight, u64)>,
    /// Where the view is in the window, as of the last frame.
    view: Option<egui::Rect>,
}

impl Picking {
    /// Start filling `slot` of job `doc`.
    pub fn arm(&mut self, doc: DocId, slot: Slot) {
        self.armed = Some((doc, slot));
        self.inspect = None;
    }

    /// Stop filling whatever is being filled.
    pub fn disarm(&mut self) {
        self.armed = None;
    }

    /// The slot being filled in job `doc`, if any.
    pub fn armed(&self, doc: DocId) -> Option<&Slot> {
        self.armed
            .as_ref()
            .filter(|(d, _)| *d == doc)
            .map(|(_, s)| s)
    }

    /// What the selection's picks show, in job `doc`.
    pub fn shown(&self, doc: DocId) -> Option<&Shown> {
        self.shown
            .as_ref()
            .filter(|(from, ..)| from.doc == doc)
            .map(|(_, s, _)| s)
    }

    /// Look at `target`, as if it had been clicked. Only the snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn inspect_for_snapshot(&mut self, doc: DocId, setup: String, target: Target) {
        self.inspect = Some((doc, setup, target));
    }

    /// The highest round rim in the showing setup, as the point of it nearest
    /// `eye` and its centre, in the setup's frame, and where the view is. Only the
    /// snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn rim_for_snapshot(
        &self,
        eye: glam::DVec3,
    ) -> Option<(glam::DVec3, glam::DVec3, egui::Rect)> {
        use camcraft_geom::topology::EdgeKind;
        let (_, _, scene) = self.scene.as_ref()?;
        scene
            .models
            .iter()
            .flat_map(|m| {
                let t = m.body().topology();
                t.edges()
                    .iter()
                    .zip(&m.edges)
                    .filter_map(|(e, pts)| match e.kind {
                        EdgeKind::Circle(c) if c.full && c.radius > 2.0 => {
                            // A chord's middle, on the side facing the eye.
                            let near = pts
                                .windows(2)
                                .filter_map(|w| match w {
                                    [a, b] => Some((*a + *b) / 2.0),
                                    _ => None,
                                })
                                .min_by(|x, y| x.distance(eye).total_cmp(&y.distance(eye)))?;
                            Some((near, scene.frame.to_setup(c.center)))
                        }
                        _ => None,
                    })
            })
            .max_by(|x, y| x.1.z.total_cmp(&y.1.z))
            .zip(self.view)
            .map(|((rim, centre), view)| (rim, centre, view))
    }

    /// Show the pick in row `i` of the panel's `kind` list as under the pointer.
    /// The panel is drawn after the view is worked out, so this shows from the
    /// next frame: whether that frame is needed, because the row is a new one.
    pub fn hover_row(&mut self, kind: Kind, i: usize) -> bool {
        self.row = Some((kind, i));
        self.shown_row != self.row
    }

    /// Esc: stop picking, or else stop looking at what was clicked. Whether it did
    /// either.
    pub fn escape(&mut self) -> bool {
        if self.armed.take().is_some() {
            return true;
        }
        self.inspect.take().is_some()
    }

    /// Bring everything up to date with `doc`, the job showing, before the frame
    /// is drawn.
    pub fn refresh(&mut self, doc: Option<&Document>, m: Measure) {
        let Some(doc) = doc else {
            *self = Self::default();
            return;
        };
        if let Some((d, slot)) = &self.armed
            && (*d != doc.id || !slot.still_valid(doc))
        {
            self.armed = None;
        }
        if self
            .inspect
            .as_ref()
            .is_some_and(|(d, s, _)| *d != doc.id || doc.active.as_ref() != Some(s))
        {
            self.inspect = None;
        }

        // The showing setup, laid out, ready to pick from.
        let laid = doc.active.as_ref().and_then(|name| {
            let layout = doc.setups.get(name)?.layout.as_ref()?;
            Some((name, layout.key, layout.prepared.as_ref().ok()?))
        });
        let Some((setup, layout, prepared)) = laid else {
            self.scene = None;
            self.shown = None;
            self.hovered = None;
            self.hover = None;
            return;
        };
        if self
            .scene
            .as_ref()
            .is_none_or(|(d, k, _)| (*d, *k) != (doc.id, layout))
        {
            let loaded = doc.models.iter().filter_map(|(name, slot)| {
                let (model, bvh) = slot.loaded()?;
                Some((name.clone(), model.clone(), bvh.clone()))
            });
            let stocks = stocks(doc, setup, prepared);
            self.scene = Some((doc.id, layout, PickScene::new(prepared, loaded, stocks)));
            self.hovered = None;
            self.hover = None;
        }
        let Some((_, _, scene)) = &self.scene else {
            return;
        };

        // What could be picked in, while models or stocks are being picked.
        match self
            .armed(doc.id)
            .filter(|s| matches!(s, Slot::Models { .. } | Slot::Stocks { .. }))
        {
            None => self.ghost = None,
            Some(slot)
                if self
                    .ghost
                    .as_ref()
                    .is_none_or(|(d, k, s, ..)| (*d, *k, s) != (doc.id, layout, slot)) =>
            {
                let h = slots::ghosts(scene, slot);
                self.ghost = Some((doc.id, layout, slot.clone(), h, next_key()));
            }
            Some(_) => {}
        }

        // The selection's picks: the setup's work zero, or an operation's faces and
        // edges, when they belong to the setup showing.
        let op = match &doc.selection {
            Selection::Setup(s) if s == setup => Some(None),
            Selection::Operation(s, i) if s == setup => Some(Some(*i)),
            _ => None,
        };
        let from = op.and_then(|op| {
            let s = doc.job.setups.get(doc.setup_index(setup)?)?;
            let operation = op.and_then(|i| s.operations.get(i));
            Some(ShownFrom {
                doc: doc.id,
                layout,
                setup: setup.clone(),
                op,
                origin: s.origin.clone(),
                geometry: operation.map(|o| o.geometry.clone()),
                tool: operation.and_then(|o| doc.job.tool_for(o)).cloned(),
                tolerance: doc.job.tolerance.to_bits(),
                measure: m,
            })
        });
        match from {
            None => self.shown = None,
            Some(from) if self.shown.as_ref().is_none_or(|(f, ..)| *f != from) => {
                let shown = slots::shown(doc, setup, from.op, prepared, scene, m);
                self.shown = Some((from, shown, next_key()));
            }
            Some(_) => {}
        }

        // What is under the pointer, or under it in the panel's list.
        self.shown_row = self.row.take();
        let row = self.shown_row.and_then(|(kind, i)| {
            let (from, ..) = self.shown.as_ref()?;
            let g = from.geometry.as_ref()?;
            let r = match kind {
                Kind::Face => g.faces.get(i),
                Kind::Edge => g.edges.get(i),
            }?;
            row_target(scene, doc, r, kind)
        });
        let under = row.or_else(|| self.hover.clone());
        match under {
            None => self.hovered = None,
            Some(t) if self.hovered.as_ref().is_none_or(|(h, ..)| *h != t) => {
                let h = slots::hovered(scene, &t);
                self.hovered = Some((t, h, next_key()));
            }
            Some(_) => {}
        }
    }

    /// Add what picking draws, and says, to what the view draws for job `doc`.
    pub fn decorate<'a>(&'a self, doc: DocId, content: &mut Content<'a>) {
        if self.scene.as_ref().is_none_or(|(d, ..)| *d != doc) {
            return;
        }
        content.selection = self
            .shown
            .as_ref()
            .filter(|(f, ..)| f.doc == doc)
            .map(|(_, s, key)| (*key, &s.highlight));
        content.hover = self.hovered.as_ref().map(|(_, h, key)| (*key, h));
        content.ghost = self
            .ghost
            .as_ref()
            .filter(|(d, ..)| *d == doc)
            .map(|(.., h, key)| (*key, h));
        if let Some(slot) = self.armed(doc) {
            content.hint = Some(slot.hint());
            // What is picked is the part: show it, and not the material over it.
            content.view.part = true;
            content.stock = None;
        }
    }

    /// Where the pointer is over the view this frame, for job `doc`: what is under
    /// it, and what a click there does. Whether anything changed that needs the
    /// view drawn again.
    pub fn pointer(&mut self, doc: &mut Document, event: Option<PointerEvent>) -> bool {
        let Some((_, _, scene)) = self.scene.as_ref().filter(|(d, ..)| *d == doc.id) else {
            let changed = self.hover.is_some();
            self.hover = None;
            return changed;
        };
        let armed = self.armed(doc.id).cloned();
        let accept = armed.as_ref().map_or(Accept::ALL, Slot::accept);
        let target = event.and_then(|e| scene.pick(e.ray, &e.projector, e.pos, accept));
        let mut changed = target != self.hover;
        self.hover.clone_from(&target);

        if !event.is_some_and(|e| e.clicked) {
            return changed;
        }
        changed = true;
        match (armed, target) {
            (Some(slot), Some(t)) => {
                let tolerance = doc.job.tolerance;
                let current = |r: &FeatureRef, kind: Kind| {
                    let body = scene.model(&r.model)?.body();
                    let tol = features::tolerance(tolerance, body);
                    let found = match kind {
                        Kind::Face => features::find_face(body, r, tol),
                        Kind::Edge => features::find_edge(body, r, tol),
                    };
                    found.ok().map(|f| f.index)
                };
                if slots::apply(doc, &slot, &t, current) && slot.single() {
                    self.armed = None;
                }
            }
            (Some(_), None) => {}
            (None, t) => {
                self.inspect = t
                    .zip(doc.active.clone())
                    .map(|(t, setup)| (doc.id, setup, t));
            }
        }
        changed
    }

    /// The card describing what was clicked, over the top left of the view `rect`.
    pub fn card(&mut self, ctx: &egui::Context, doc: DocId, rect: egui::Rect, m: Measure) {
        self.view = Some(rect);
        let Some((d, _, target)) = &self.inspect else {
            return;
        };
        let Some((_, _, scene)) = self.scene.as_ref().filter(|(s, ..)| s == d) else {
            return;
        };
        if *d != doc {
            return;
        }
        let rows = slots::describe(target, scene, m);
        let mut close = false;
        egui::Area::new(egui::Id::new("inspect-card"))
            .fixed_pos(rect.left_top() + egui::vec2(12.0, 12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(rect.width() * 0.5);
                    ui.horizontal(|ui| {
                        ui.strong("Picked");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text("Esc").clicked() {
                                close = true;
                            }
                        });
                    });
                    egui::Grid::new("inspect-rows")
                        .num_columns(2)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            for (name, value) in rows {
                                ui.label(
                                    egui::RichText::new(name)
                                        .color(theme::egui_color(theme::GRAY_7)),
                                );
                                ui.label(value);
                                ui.end_row();
                            }
                        });
                });
            });
        if close {
            self.inspect = None;
        }
    }
}

/// Every stock of the job where setup `setup`, laid out as `prepared`, has it or
/// would have it: its own first, then the others, which can be picked into it.
fn stocks(
    doc: &Document,
    setup: &str,
    prepared: &camcraft_core::setup::Prepared,
) -> Vec<PickStock> {
    let Some(s) = doc.setup_index(setup).and_then(|i| doc.job.setups.get(i)) else {
        return Vec::new();
    };
    let mut out: Vec<PickStock> = s
        .stocks
        .iter()
        .zip(&prepared.stocks)
        .map(|(name, b)| PickStock {
            name: name.clone(),
            bounds: *b,
            in_setup: true,
        })
        .collect();
    let bodies: camcraft_core::setup::Bodies<'_> = doc
        .models
        .iter()
        .filter_map(|(n, slot)| Some((n.as_str(), &slot.model()?.body)))
        .collect();
    for def in doc
        .job
        .stocks
        .iter()
        .filter(|d| !s.stocks.contains(&d.name))
    {
        // One that cannot be placed, around a model not loaded, is left out.
        if let Ok(bounds) =
            camcraft_core::setup::stock_in_frame(&def.spec, &s.models, &prepared.frame, &bodies)
        {
            out.push(PickStock {
                name: def.name.clone(),
                bounds,
                in_setup: false,
            });
        }
    }
    out
}

/// Where a saved pick is now, as if it were under the pointer.
fn row_target(scene: &PickScene, doc: &Document, r: &FeatureRef, kind: Kind) -> Option<Target> {
    let body = scene.model(&r.model)?.body();
    let tol = features::tolerance(doc.job.tolerance, body);
    let at = glam::DVec3::from_array(r.at);
    Some(match kind {
        Kind::Face => Target::Face {
            model: r.model.clone(),
            face: features::find_face(body, r, tol).ok()?.index,
            at,
        },
        Kind::Edge => Target::Edge {
            model: r.model.clone(),
            edge: features::find_edge(body, r, tol).ok()?.index,
            at,
        },
    })
}
