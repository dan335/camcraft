//! Where the tool travels, and the levels it cuts between: Fusion's Heights tab.

use serde::{Deserialize, Serialize};

use super::{Operation, Outline};

/// Where the tool travels, and the levels it cuts between: Fusion's Heights tab.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Heights {
    /// The height the tool arrives at the operation and leaves it at.
    #[serde(default = "default_clearance")]
    pub clearance: Height,
    /// The height it goes up to between cuts with a full retraction, and never
    /// above with a minimum one: Fusion's retract height.
    #[serde(
        default = "default_retract",
        skip_serializing_if = "is_default_retract"
    )]
    pub retract: Height,
    /// Highest level to cut at; the strategy's own when not given
    /// ([`Uses::top`](super::Uses::top)): the stock's top, or the models' for
    /// contour and flat finishing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Height>,
    /// Lowest level to cut at; the strategy's own when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<Height>,
}

impl Default for Heights {
    fn default() -> Self {
        Self {
            clearance: default_clearance(),
            retract: default_retract(),
            top: None,
            bottom: None,
        }
    }
}

impl Heights {
    /// Whether its top or bottom is measured from the selection.
    pub fn measured_from_selection(&self) -> bool {
        [self.top, self.bottom]
            .iter()
            .flatten()
            .any(Height::from_selection)
    }
}

fn default_clearance() -> Height {
    Height {
        reference: HeightRef::StockTop,
        offset: 10.0,
    }
}

/// Fusion's: 5 mm above the stock.
fn default_retract() -> Height {
    Height {
        reference: HeightRef::StockTop,
        offset: 5.0,
    }
}

// Serde's `skip_serializing_if` hands the field by reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_retract(h: &Height) -> bool {
    *h == default_retract()
}

/// A height, stated relative to something that can be measured.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Height {
    /// What the offset is measured from.
    #[serde(rename = "ref", default = "default_ref")]
    pub reference: HeightRef,
    /// Millimetres above the reference.
    #[serde(default)]
    pub offset: f64,
}

fn default_ref() -> HeightRef {
    HeightRef::StockTop
}

/// What a height is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HeightRef {
    /// Top of the block.
    StockTop,
    /// Bottom of the block.
    StockBottom,
    /// Highest point of the model.
    ModelTop,
    /// Lowest point of the model.
    ModelBottom,
    /// The offset is the height, in setup coordinates.
    Absolute,
    /// The operation's picked faces and edges: their highest point for a top,
    /// their lowest for a bottom. A 2D operation measures each of its areas from
    /// the picks in it, as Fusion's "selected contours". Only a top or a bottom
    /// can be, and only with something picked.
    Selection,
}

/// What a height resolves against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeightContext {
    /// Top of the block.
    pub stock_top: f64,
    /// Bottom of the block.
    pub stock_bottom: f64,
    /// Highest point of the model.
    pub model_top: f64,
    /// Lowest point of the model.
    pub model_bottom: f64,
    /// How high the operation's picks go, when it has any and a height is
    /// measured from them.
    pub picked: Option<Picks>,
}

/// The lowest and highest of some picked points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Picks {
    /// The lowest.
    pub lowest: f64,
    /// The highest.
    pub highest: f64,
}

impl Picks {
    /// How high `zs` go; `None` for none.
    pub fn of(zs: impl IntoIterator<Item = f64>) -> Option<Picks> {
        zs.into_iter().fold(None, |p, z| {
            Some(p.map_or(
                Picks {
                    lowest: z,
                    highest: z,
                },
                |p: Picks| Picks {
                    lowest: p.lowest.min(z),
                    highest: p.highest.max(z),
                },
            ))
        })
    }
}

impl HeightContext {
    /// How high `reference` is, with the picks at `picked`.
    fn at(&self, reference: HeightRef, picked: f64) -> f64 {
        match reference {
            HeightRef::StockTop => self.stock_top,
            HeightRef::StockBottom => self.stock_bottom,
            HeightRef::ModelTop => self.model_top,
            HeightRef::ModelBottom => self.model_bottom,
            HeightRef::Absolute => 0.0,
            HeightRef::Selection => picked,
        }
    }
}

impl Height {
    /// Resolve to a height in setup coordinates, as a clearance. A clearance is
    /// never measured from the picks ([`Operation::check`] turns that away); one
    /// that was would read them as a top.
    pub fn resolve(&self, ctx: &HeightContext) -> f64 {
        self.resolve_top(ctx)
    }

    /// Resolve as a top: the picks read by their highest point, or the model's
    /// top with none.
    pub fn resolve_top(&self, ctx: &HeightContext) -> f64 {
        self.measured_from(ctx, ctx.picked.map_or(ctx.model_top, |p| p.highest))
    }

    /// Resolve as a bottom: the picks read by their lowest point, or the model's
    /// bottom with none.
    pub fn resolve_bottom(&self, ctx: &HeightContext) -> f64 {
        self.measured_from(ctx, ctx.picked.map_or(ctx.model_bottom, |p| p.lowest))
    }

    fn measured_from(&self, ctx: &HeightContext, picked: f64) -> f64 {
        ctx.at(self.reference, picked) + self.offset
    }

    /// Whether it is measured from the picks.
    pub fn from_selection(&self) -> bool {
        self.reference == HeightRef::Selection
    }
}

impl Operation {
    /// A 2D operation's top and bottom over one piece of its outline with
    /// `picks` in it, if any. A height measured from the selection reads the
    /// piece's picks, or all of them, `ctx.picked`, where it has none. Without a
    /// top, the stock's; without a bottom, the piece's lowest pick when the picks
    /// are the outline, or else the models' bottom.
    pub fn levels_2d(&self, ctx: &HeightContext, picks: Option<Picks>) -> (f64, f64) {
        let own = HeightContext {
            picked: picks.or(ctx.picked),
            ..*ctx
        };
        let h = &self.heights;
        let top = h.top.map_or(ctx.stock_top, |h| h.resolve_top(&own));
        let bottom = match h.bottom {
            Some(h) => h.resolve_bottom(&own),
            None if self.outline == Outline::Selection => {
                picks.map_or(ctx.model_bottom, |p| p.lowest)
            }
            None => ctx.model_bottom,
        };
        (top, bottom)
    }

    /// A 3D operation's top and bottom: its own, or where its strategy has them
    /// without ([`Uses::top`](super::Uses::top), [`Uses::bottom`](super::Uses::bottom));
    /// the models' where it has none.
    pub fn levels_3d(&self, ctx: &HeightContext) -> (f64, f64) {
        let uses = self.strategy.uses();
        let h = &self.heights;
        let top = uses.top.map_or(ctx.model_top, |r| ctx.at(r, ctx.model_top));
        let bottom = uses
            .bottom
            .map_or(ctx.model_bottom, |r| ctx.at(r, ctx.model_bottom));
        (
            h.top.map_or(top, |h| h.resolve_top(ctx)),
            h.bottom.map_or(bottom, |h| h.resolve_bottom(ctx)),
        )
    }

    /// Whether its top or bottom can be measured from its picks: it has some,
    /// and they are not holes ([`Uses::holes`](super::Uses::holes)), each of
    /// which goes to its own bottom.
    pub fn can_measure_from_picks(&self) -> bool {
        !self.strategy.uses().holes && !self.selection.is_empty()
    }

    /// Why a height of this operation cannot be measured from its picks, if one
    /// is and cannot: a clearance never is, a drill's holes each go to their own
    /// bottom, and with nothing picked there is nothing to measure from.
    pub fn cannot_measure_from_picks(&self) -> Option<&'static str> {
        if self.heights.clearance.from_selection() {
            return Some("the clearance cannot be measured from the selection");
        }
        if self.heights.retract.from_selection() {
            return Some("the retract height cannot be measured from the selection");
        }
        if !self.heights.measured_from_selection() {
            return None;
        }
        if self.strategy.uses().holes {
            return Some(
                "a drill's bottom cannot be measured from the selection: each hole goes \
                 to its own",
            );
        }
        if self.selection.is_empty() {
            return Some("a height is measured from the selection, but nothing is picked");
        }
        None
    }
}
