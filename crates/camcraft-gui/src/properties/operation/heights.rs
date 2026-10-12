//! An operation's Heights tab, and its heights drawn in the view as planes
//! while the tab shows.

use camcraft_core::job::{
    self, Height, HeightContext, HeightRef, Heights, Operation, Outline, StrategySpec,
};
use camcraft_core::setup::features;
use camcraft_core::setup::{Bodies, Prepared};
use eframe::egui;
use glam::DVec2;

use super::OperationTab;
use crate::document::{Document, Selection};
use crate::help::topics::heights as ht;
use crate::ui::measure::Measure;
use crate::ui::theme;
use crate::ui::widgets::{self, error, form, hint};
use crate::view::viewport::{Plane, PlaneKind};
use crate::worker::DocId;

/// Whether a 2D operation goes down to its picks without a Bottom: it has some,
/// and they are what it cuts round.
fn down_to_picks(op: &Operation) -> bool {
    op.strategy.uses().outline && !op.selection.is_empty() && op.outline == Outline::Selection
}

pub(super) fn heights(ui: &mut egui::Ui, m: Measure, op: &mut Operation) {
    let uses = op.strategy.uses();
    // Only a top or a bottom is measured from the picks, with some picked, and
    // never a drill's, whose holes each go to their own.
    let picks = op.can_measure_from_picks();
    let bottom = if down_to_picks(op) {
        uses.bottom.map(|_| HeightRef::Selection)
    } else {
        uses.bottom
    };
    form(ui, "heights", |ui| {
        widgets::row(ui, &ht::CLEARANCE, |ui| {
            height_field(ui, m, "clearance", &mut op.heights.clearance, false);
        });
        if uses.stay_down {
            widgets::row(ui, &ht::RETRACT, |ui| {
                height_field(ui, m, "retract", &mut op.heights.retract, false);
            });
        }
        if let Some(unset) = uses.top {
            widgets::row(ui, &ht::TOP, |ui| {
                optional_height(ui, m, "geo-top", &mut op.heights.top, unset, picks);
            });
        }
        if let Some(unset) = bottom {
            widgets::row(ui, &ht::BOTTOM, |ui| {
                optional_height(ui, m, "geo-bottom", &mut op.heights.bottom, unset, picks);
            });
        }
    });
    if let Some(why) = op.cannot_measure_from_picks() {
        error(ui, why);
    }
    ui.add_space(theme::SPACE_3);
    let about = match op.strategy {
        _ if uses.top.is_none() && uses.bottom.is_none() => {
            Some("It follows the part all the way down, so it has no Top or Bottom.")
        }
        StrategySpec::Contour { .. } => Some(
            "Without a Top or Bottom, its levels run from the part's top down to its \
             bottom.",
        ),
        StrategySpec::Flat { .. } => Some(
            "Only the flats between the Top and Bottom are finished; without them, \
             every flat of the part.",
        ),
        StrategySpec::Drill { .. } => Some(
            "Each hole starts at its picked rim. Bottom is how deep a hole goes when \
             its bottom edge is not picked.",
        ),
        StrategySpec::Profile { .. } | StrategySpec::Pocket { .. } => Some(
            "Without a Bottom, each area is cut down to the lowest point picked in \
             it, or to the part's bottom when nothing is picked. From the selection, \
             each area's Top and Bottom are the highest and lowest points picked in \
             it, plus the offset.",
        ),
        StrategySpec::Face { .. } => {
            Some("Without a Bottom, the stock is faced down to the part's top.")
        }
        _ => None,
    };
    if let Some(about) = about {
        hint(ui, about);
    }
    hint(
        ui,
        "The view shows each height as a plane across the stock.",
    );
}

/// The selected operation's heights as planes across the stock, as the run will
/// resolve them, while the tab of it showing is its Heights. Resolving them finds
/// the picks a height may be measured from, which for a pick that is lost means
/// looking through every triangle, so they are worked out again only when
/// something they come from changes.
#[derive(Debug, Default)]
pub struct HeightPlanes {
    /// What they were worked out from, while they show.
    from: Option<PlanesFrom>,
    planes: Vec<Plane>,
}

/// Everything the planes are worked out from: the operation and its setup's
/// layout, the tolerance the picks are found to, and the units they are
/// labelled in.
#[derive(Debug)]
struct PlanesFrom {
    doc: DocId,
    setup: String,
    index: usize,
    strategy: StrategySpec,
    outline: Outline,
    selection: job::Selection,
    heights: Heights,
    layout: u64,
    tolerance: u64,
    measure: Measure,
}

/// An operation whose heights show: its setup and place, itself, and its setup
/// laid out.
struct Shown<'a> {
    setup: &'a str,
    index: usize,
    op: &'a Operation,
    prepared: &'a Prepared,
    layout: u64,
}

impl PlanesFrom {
    fn of(doc: &Document, m: Measure, s: &Shown<'_>) -> Self {
        Self {
            doc: doc.id,
            setup: s.setup.to_string(),
            index: s.index,
            strategy: s.op.strategy,
            outline: s.op.outline,
            selection: s.op.selection.clone(),
            heights: s.op.heights.clone(),
            layout: s.layout,
            tolerance: doc.job.tolerance.to_bits(),
            measure: m,
        }
    }

    /// Whether these are what `s` of `doc` is now, found without copying it:
    /// asked every frame.
    fn is(&self, doc: &Document, m: Measure, s: &Shown<'_>) -> bool {
        self.doc == doc.id
            && self.setup == s.setup
            && self.index == s.index
            && self.strategy == s.op.strategy
            && self.outline == s.op.outline
            && self.selection == s.op.selection
            && self.heights == s.op.heights
            && self.layout == s.layout
            && self.tolerance == doc.job.tolerance.to_bits()
            && self.measure == m
    }
}

impl HeightPlanes {
    /// The planes for `doc`'s selection while `tab`, the tab of it showing, is
    /// its Heights; none otherwise.
    pub fn update(&mut self, doc: &Document, m: Measure, tab: Option<OperationTab>) -> &[Plane] {
        match shown(doc, tab) {
            None => {
                self.from = None;
                self.planes.clear();
            }
            Some(s) if self.from.as_ref().is_some_and(|f| f.is(doc, m, &s)) => {}
            Some(s) => {
                self.planes = planes(doc, m, &s);
                self.from = Some(PlanesFrom::of(doc, m, &s));
            }
        }
        &self.planes
    }

    /// The planes as last worked out.
    pub fn planes(&self) -> &[Plane] {
        &self.planes
    }
}

/// The operation selected in `doc`, when `tab` is its Heights and its setup,
/// the one showing, is laid out.
fn shown(doc: &Document, tab: Option<OperationTab>) -> Option<Shown<'_>> {
    if tab != Some(OperationTab::Heights) {
        return None;
    }
    let Selection::Operation(setup, index) = &doc.selection else {
        return None;
    };
    if doc.active.as_ref() != Some(setup) {
        return None;
    }
    let layout = doc.setups.get(setup)?.layout.as_ref()?;
    Some(Shown {
        setup,
        index: *index,
        op: doc.operation(setup, *index)?,
        prepared: layout.prepared.as_ref().ok()?,
        layout: layout.key,
    })
}

/// `s`'s heights as planes across the stock.
fn planes(doc: &Document, m: Measure, s: &Shown<'_>) -> Vec<Plane> {
    let (op, prepared) = (s.op, s.prepared);
    let stock = prepared.stock_bounds();
    // How high the picks go, when a height is measured from them. One that
    // cannot be found leaves the height where it would be with none.
    let picked = if op.heights.measured_from_selection() || down_to_picks(op) {
        let bodies: Bodies<'_> = doc
            .models
            .iter()
            .filter_map(|(n, slot)| Some((n.as_str(), &slot.model()?.body)))
            .collect();
        features::picks(&op.selection, prepared, &bodies, doc.job.tolerance)
            .ok()
            .flatten()
    } else {
        None
    };
    let ctx = HeightContext {
        picked,
        ..prepared.heights()
    };
    // A little past the stock, so a plane on its top does not hide its edges.
    let grow = 0.04 * (stock.max.x - stock.min.x).max(stock.max.y - stock.min.y);
    let (lo, hi) = (
        stock.min.truncate() - DVec2::splat(grow),
        stock.max.truncate() + DVec2::splat(grow),
    );
    let uses = op.strategy.uses();
    // A 2D operation's, over all its areas: each area's own are drawn with its
    // outline.
    let (top_z, bottom_z) = if uses.outline {
        op.levels_2d(&ctx, ctx.picked)
    } else {
        op.levels_3d(&ctx)
    };
    [
        (
            PlaneKind::Clearance,
            "Clearance",
            Some(op.heights.clearance.resolve(&ctx)),
        ),
        (
            PlaneKind::Retract,
            "Retract",
            uses.stay_down.then(|| op.heights.retract.resolve(&ctx)),
        ),
        (PlaneKind::Top, "Top", uses.top.map(|_| top_z)),
        (PlaneKind::Bottom, "Bottom", uses.bottom.map(|_| bottom_z)),
    ]
    .into_iter()
    .filter_map(|(kind, name, z)| {
        let z = z?;
        Some(Plane {
            kind,
            z,
            lo,
            hi,
            label: format!("{name}  Z {}", m.length(z, 2)),
        })
    })
    .collect()
}

/// A height, an offset from what it is measured from; from the picks too when
/// `picks`. One measured from them already keeps it on the list either way, so
/// it reads as what it is until changed.
fn height_field(ui: &mut egui::Ui, m: Measure, id: &str, h: &mut Height, picks: bool) {
    let mut from = vec![
        HeightRef::StockTop,
        HeightRef::StockBottom,
        HeightRef::ModelTop,
        HeightRef::ModelBottom,
        HeightRef::Absolute,
    ];
    if picks || h.from_selection() {
        from.push(HeightRef::Selection);
    }
    ui.horizontal(|ui| {
        m.signed_length_field(ui, &mut h.offset, 0.1, 10_000.0, "");
        ui.label("from");
        widgets::select(ui, id, &mut h.reference, &from, |r| match r {
            HeightRef::StockTop => "stock top",
            HeightRef::StockBottom => "stock bottom",
            HeightRef::ModelTop => "model top",
            HeightRef::ModelBottom => "model bottom",
            HeightRef::Absolute => "work zero",
            HeightRef::Selection => "selection",
        });
    });
}

/// A top or bottom that need not be given: `default` when it is not, which it
/// starts from when given. `picks` as [`height_field`]'s.
fn optional_height(
    ui: &mut egui::Ui,
    m: Measure,
    id: &str,
    h: &mut Option<Height>,
    default: HeightRef,
    picks: bool,
) {
    let absent = match default {
        HeightRef::StockTop => "the stock's top",
        HeightRef::ModelTop => "the part's top",
        HeightRef::Selection => "the picks",
        _ => "the part's bottom",
    };
    let start = Height {
        reference: default,
        offset: 0.0,
    };
    ui.horizontal(|ui| {
        widgets::optional(ui, h, start, absent, |ui, h| {
            height_field(ui, m, id, h, picks);
        });
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::document::with_plate;

    /// The planes follow the heights as they are edited, and what they are
    /// labelled in, though they are worked out again only when something
    /// changes; with the tab away, there are none.
    #[test]
    fn height_planes_follow_their_heights() {
        let (mut doc, setup) = with_plate();
        let op = doc
            .add_operation(&setup, StrategySpec::named("pocket-clearing").unwrap())
            .unwrap();
        doc.refresh_layout(&setup);
        let heights = Some(OperationTab::Heights);
        let mut planes = HeightPlanes::default();
        let clearance = |p: &[Plane]| {
            p.iter()
                .find(|p| p.kind == PlaneKind::Clearance)
                .unwrap()
                .clone()
        };
        let before = clearance(planes.update(&doc, Measure::Metric, heights));
        assert_eq!(planes.planes().len(), 4, "clearance, retract, top, bottom");
        assert_eq!(
            clearance(planes.update(&doc, Measure::Metric, heights)),
            before
        );

        doc.job.setups[0].operations[op].heights.clearance.offset += 5.0;
        let after = clearance(planes.update(&doc, Measure::Metric, heights));
        assert_eq!(after.z, before.z + 5.0);
        let inches = clearance(planes.update(&doc, Measure::Imperial, heights));
        assert_ne!(inches.label, after.label);

        assert!(
            planes
                .update(&doc, Measure::Metric, Some(OperationTab::Passes))
                .is_empty()
        );
        doc.selection = Selection::Setup(setup);
        assert!(planes.update(&doc, Measure::Metric, heights).is_empty());
    }
}
