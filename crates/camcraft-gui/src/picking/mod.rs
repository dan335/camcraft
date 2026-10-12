//! Picking in the window: which slot is armed, what is under the pointer, and the
//! work that is kept between frames so the pointer can move freely over a large
//! part.
//!
//! As in Fusion, the view picks only while a slot is armed, by its button in the
//! panel: otherwise the pointer moves over the part and a click does nothing.
//!
//! Three things are worked out once and kept until what they depend on changes:
//! the setup made ready for picking (its models' edges and points in the setup's
//! frame, redone when the layout is), what the selected setup or operation's picks
//! show (redone when the job's picks, the tool or the layout change), and what the
//! thing under the pointer shows (redone when the pointer moves onto something
//! else). Everything else is done as the pointer moves: a ray cast and a look along
//! the edges on the screen.
//!
//! What the pointer hits is found in [`ray`], what a slot takes and what a click
//! does to the job in [`slots`], what the picks made show in [`shown`], what is
//! drawn for what is under the pointer in [`highlight`], and what a pick is
//! called in [`describe`].

pub mod describe;
pub mod highlight;
pub mod ray;
pub mod shown;
pub mod slots;

use std::borrow::Cow;

use camcraft_core::job::{
    self, Boundary, FeatureRef, MachiningBoundary, Operation, Origin, Outline, StockToLeave,
    StrategySpec, ToolSpec,
};
#[cfg(all(test, feature = "ui-snapshots"))]
use eframe::egui;

use crate::document::{Document, Selection, next_key};
use crate::ui::measure::Measure;
use crate::view::viewport::{Content, Highlight, PointerEvent};
use crate::worker::DocId;
use ray::{PickScene, PickStock, Target};
use shown::Shown;
use slots::{Kind, Slot};

/// What the picks shown were worked out from: when any of it changes, they are
/// worked out again, and nothing else makes them. Each frame's is borrowed from
/// the job, to compare with the one kept, and copied only when they differ.
#[derive(Debug, Clone, PartialEq)]
struct ShownFrom<'a> {
    doc: DocId,
    layout: u64,
    setup: Cow<'a, str>,
    op: Option<usize>,
    origin: Cow<'a, Origin>,
    selection: Option<Cow<'a, job::Selection>>,
    /// What the operation's boundary or outline is drawn with, besides its picks.
    drawn: Option<Drawn<'a>>,
    tolerance: u64,
    measure: Measure,
}

/// What an operation's boundary or outline is drawn with besides its picks: only
/// what its kind of strategy draws it with, so an edit to anything else (its
/// stepover, a 3D operation's heights) does not work the picks out again.
#[derive(Debug, Clone, PartialEq)]
enum Drawn<'a> {
    /// Facing: the stock's footprint, grown by this offset (its bits).
    Face(u64),
    /// A 2D operation's outline, drawn at the heights measured from the picks
    /// when they are, with what its tool cuts beside it, which its strategy (a
    /// profile's side) and the stock it leaves decide.
    Outline {
        outline: Outline,
        from_picks: bool,
        strategy: Cow<'a, StrategySpec>,
        leave: StockToLeave,
        tool: Option<Cow<'a, ToolSpec>>,
    },
    /// A 3D operation's machining boundary, where its tool's centre may go against
    /// it, and what the tool cuts outside the part.
    Boundary {
        boundary: MachiningBoundary,
        leave: StockToLeave,
        tool: Option<Cow<'a, ToolSpec>>,
    },
    /// Nothing: a 3D operation kept to the stock.
    Nothing,
}

impl<'a> Drawn<'a> {
    /// What `op`, of `job`, draws its boundary or outline with.
    fn of(job: &'a job::Job, op: &'a Operation) -> Self {
        let tool = || job.tool_for(op).map(Cow::Borrowed);
        if let StrategySpec::Face { stock_offset, .. } = op.strategy {
            return Drawn::Face(stock_offset.to_bits());
        }
        let uses = op.strategy.uses();
        if uses.outline {
            Drawn::Outline {
                outline: op.outline,
                from_picks: op.heights.measured_from_selection(),
                strategy: Cow::Borrowed(&op.strategy),
                leave: op.stock_to_leave(),
                tool: tool(),
            }
        } else if uses.boundary && op.boundary.kind != Boundary::Stock {
            Drawn::Boundary {
                boundary: op.boundary,
                leave: op.stock_to_leave(),
                tool: tool(),
            }
        } else {
            Drawn::Nothing
        }
    }

    /// A copy to keep.
    fn into_owned(self) -> Drawn<'static> {
        let owned = |t: Option<Cow<'_, ToolSpec>>| t.map(|t| Cow::Owned(t.into_owned()));
        match self {
            Drawn::Face(offset) => Drawn::Face(offset),
            Drawn::Outline {
                outline,
                from_picks,
                strategy,
                leave,
                tool,
            } => Drawn::Outline {
                outline,
                from_picks,
                strategy: Cow::Owned(strategy.into_owned()),
                leave,
                tool: owned(tool),
            },
            Drawn::Boundary {
                boundary,
                leave,
                tool,
            } => Drawn::Boundary {
                boundary,
                leave,
                tool: owned(tool),
            },
            Drawn::Nothing => Drawn::Nothing,
        }
    }
}

impl ShownFrom<'_> {
    /// A copy to keep.
    fn into_owned(self) -> ShownFrom<'static> {
        ShownFrom {
            doc: self.doc,
            layout: self.layout,
            setup: Cow::Owned(self.setup.into_owned()),
            op: self.op,
            origin: Cow::Owned(self.origin.into_owned()),
            selection: self.selection.map(|s| Cow::Owned(s.into_owned())),
            drawn: self.drawn.map(Drawn::into_owned),
            tolerance: self.tolerance,
            measure: self.measure,
        }
    }
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
    /// The showing setup, ready to pick from: for which job and layout.
    scene: Option<(DocId, u64, PickScene)>,
    /// What the selection's picks show, with the key its upload goes by.
    shown: Option<(ShownFrom<'static>, Shown, u64)>,
    /// What the thing under the pointer shows, with its upload key.
    hovered: Option<(Target, Highlight, u64)>,
    /// What could be picked into the armed slot, drawn faintly: for which job,
    /// layout and slot, with its upload key.
    ghost: Option<(DocId, u64, Slot, Highlight, u64)>,
    /// Where the view is in the window, as of the last frame. Only the snapshot
    /// harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    view: Option<egui::Rect>,
}

impl Picking {
    /// Start filling `slot` of job `doc`.
    pub fn arm(&mut self, doc: DocId, slot: Slot) {
        self.armed = Some((doc, slot));
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

    /// Click edge `edge` of `model` into the armed slot of `doc`, as a click in the
    /// view does, once the scene is laid out. Whether anything changed. Only the
    /// snapshot harness asks.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn click_edge_for_snapshot(&mut self, doc: &mut Document, model: &str, edge: u32) -> bool {
        let scene = self
            .scene
            .as_ref()
            .filter(|(d, ..)| *d == doc.id)
            .map(|(.., s)| s);
        let (Some(scene), Some(slot)) = (scene, self.armed(doc.id).cloned()) else {
            return false;
        };
        let Some(body) = scene.model(model).map(ray::PickModel::body) else {
            return false;
        };
        let at = body
            .topology()
            .edge_middle(body.mesh(), edge)
            .unwrap_or_default();
        let t = slots::widen(
            scene,
            &slot,
            Target::Edge {
                model: model.to_string(),
                edge,
                at,
            },
        );
        let tolerance = doc.job.tolerance;
        let current = |r: &FeatureRef, kind: Kind| {
            kind.find(scene.model(&r.model)?.body(), r, tolerance).ok()
        };
        slots::apply(doc, &slot, &t, current)
    }

    /// Where the view is in the window: the snapshot harness keeps it up to date.
    #[cfg(all(test, feature = "ui-snapshots"))]
    pub fn view_for_snapshot(&mut self, rect: egui::Rect) {
        self.view = Some(rect);
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

    /// Esc: stop picking. Whether anything was being picked.
    pub fn escape(&mut self) -> bool {
        self.armed.take().is_some()
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
            // The job's models: one undone out of it may still be loaded.
            let loaded = doc.models.iter().filter_map(|(name, slot)| {
                doc.job.model(name)?;
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
                let h = highlight::ghosts(scene, slot);
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
            let s = doc.setup(setup)?;
            let operation = op.and_then(|i| s.operations.get(i));
            Some(ShownFrom {
                doc: doc.id,
                layout,
                setup: Cow::Borrowed(setup.as_str()),
                op,
                origin: Cow::Borrowed(&s.origin),
                selection: operation.map(|o| Cow::Borrowed(&o.selection)),
                drawn: operation.map(|o| Drawn::of(&doc.job, o)),
                tolerance: doc.job.tolerance.to_bits(),
                measure: m,
            })
        });
        match from {
            None => self.shown = None,
            Some(from) if self.shown.as_ref().is_none_or(|(f, ..)| *f != from) => {
                let shown = shown::shown(doc, setup, from.op, prepared, scene, m);
                self.shown = Some((from.into_owned(), shown, next_key()));
            }
            Some(_) => {}
        }

        // What is under the pointer, or under it in the panel's list.
        self.shown_row = self.row.take();
        let row = self.shown_row.and_then(|(kind, i)| {
            let (from, ..) = self.shown.as_ref()?;
            let sel = from.selection.as_ref()?;
            let r = match kind {
                Kind::Face => sel.faces.get(i),
                Kind::Edge => sel.edges.get(i),
            }?;
            row_target(scene, doc, r, kind).map(|t| slots::whole_loop(scene, t))
        });
        let under = row.or_else(|| self.hover.clone());
        match under {
            None => self.hovered = None,
            Some(t) if self.hovered.as_ref().is_none_or(|(h, ..)| *h != t) => {
                let h = highlight::hovered(scene, &t);
                self.hovered = Some((t, h, next_key()));
            }
            Some(_) => {}
        }
    }

    /// Add what picking draws, and says, to what the view draws for job `doc`.
    /// An operation's picks show only while its geometry is being looked at,
    /// `geometry`; a setup's work zero, whenever the setup is selected.
    pub fn decorate<'a>(&'a self, doc: DocId, geometry: bool, content: &mut Content<'a>) {
        if self.scene.as_ref().is_none_or(|(d, ..)| *d != doc) {
            return;
        }
        content.selection = self
            .shown
            .as_ref()
            .filter(|(f, ..)| f.doc == doc && (f.op.is_none() || geometry))
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
    /// it, and what a click there does. Only an armed slot picks. Whether anything
    /// changed that needs the view drawn again.
    pub fn pointer(&mut self, doc: &mut Document, event: Option<PointerEvent>) -> bool {
        let scene = self
            .scene
            .as_ref()
            .filter(|(d, ..)| *d == doc.id)
            .map(|(.., s)| s);
        let (Some(scene), Some(slot)) = (scene, self.armed(doc.id).cloned()) else {
            let changed = self.hover.is_some();
            self.hover = None;
            return changed;
        };
        let target = event
            .and_then(|e| scene.pick(e.ray, e.sight, &e.projector, e.pos, slot.accept()))
            .map(|t| slots::widen(scene, &slot, t));
        let changed = target != self.hover;
        self.hover.clone_from(&target);

        let Some(t) = target.filter(|_| event.is_some_and(|e| e.clicked)) else {
            return changed;
        };
        let tolerance = doc.job.tolerance;
        let current = |r: &FeatureRef, kind: Kind| {
            kind.find(scene.model(&r.model)?.body(), r, tolerance).ok()
        };
        if slots::apply(doc, &slot, &t, current) && slot.single() {
            self.armed = None;
        }
        true
    }
}

/// Every stock of the job where setup `setup`, laid out as `prepared`, has it or
/// would have it: its own first, then the others, which can be picked into it.
fn stocks(
    doc: &Document,
    setup: &str,
    prepared: &camcraft_core::setup::Prepared,
) -> Vec<PickStock> {
    let Some(s) = doc.setup(setup) else {
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
    let index = kind
        .find(scene.model(&r.model)?.body(), r, doc.job.tolerance)
        .ok()?;
    let (model, at) = (r.model.clone(), glam::DVec3::from_array(r.at));
    Some(match kind {
        Kind::Face => Target::Face {
            model,
            face: index,
            at,
        },
        Kind::Edge => Target::Edge {
            model,
            edge: index,
            at,
        },
    })
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;
    use camcraft_core::job::{Height, HeightRef, ProfileSide};

    /// The picks are worked out again only for an edit to what draws them: not a
    /// 3D operation's angle or heights, but its boundary; and a 2D operation's
    /// heights once they are measured from its picks.
    #[test]
    fn picks_are_worked_out_again_only_for_what_draws_them() {
        let mut doc = Document::untitled(1);
        let setup = doc.job.setups[0].name.clone();
        doc.add_operation(&setup, StrategySpec::Parallel { angle: 0.0 });
        doc.add_operation(
            &setup,
            StrategySpec::Profile {
                side: ProfileSide::Outside,
                conventional: false,
            },
        );
        let job = doc.job.clone();
        let mut parallel = job.setups[0].operations[0].clone();
        parallel.boundary.kind = Boundary::Silhouette;
        let profile = job.setups[0].operations[1].clone();
        let drawn = |op: &Operation| Drawn::of(&job, op).into_owned();
        let from_picks = Some(Height {
            reference: HeightRef::Selection,
            offset: 0.0,
        });

        let mut turned = parallel.clone();
        turned.strategy = StrategySpec::Parallel { angle: 45.0 };
        turned.heights.top = from_picks;
        assert_eq!(drawn(&parallel), drawn(&turned));
        let mut kept_in = parallel.clone();
        kept_in.boundary.margin = 2.0;
        assert_ne!(drawn(&parallel), drawn(&kept_in));

        let mut measured = profile.clone();
        measured.heights.top = from_picks;
        assert_ne!(drawn(&profile), drawn(&measured));
        let mut inside = profile.clone();
        inside.strategy = StrategySpec::Profile {
            side: ProfileSide::Inside,
            conventional: false,
        };
        assert_ne!(drawn(&profile), drawn(&inside));
    }
}
