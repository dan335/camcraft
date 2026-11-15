//! A job's sheets arranged, live: worked out off the window's thread whenever
//! what a sheet is arranged from changes, the newest asked for stopping the
//! one in flight, and kept by what each was made from, so an edit undone or
//! redone finds its arrangement at once.
//!
//! A sheet is laid from its own settings and its parts' bodies alone, so each
//! is found by its own key ([`SheetKey`]) among those kept: arranging one
//! sheet again leaves the others standing as they were, and what was run on
//! them current. Where a model on a sheet stands comes only from here; its
//! body is placed again only when its file or its own rotation or scale
//! changes.

use std::collections::BTreeMap;
use std::sync::Arc;

use camcraft_core::arrange::{Arrangement, Laid, Outlines, SheetLayout, Stand, WHERE_IT_IS};
use camcraft_core::control::{Control, Stopped};
use camcraft_core::job::{Job, StockDef, StockSpec};
use camcraft_session::Model;

use super::{Document, ModelKey, ModelSlot, next_key};

/// How many arrangements a job keeps: enough for an edit or two to be undone
/// and redone at once, and never without bound.
const KEPT_ARRANGEMENTS: usize = 8;

/// What one sheet is laid from: the stock as the job has it, and each of its
/// parts as loaded, or `None` for one not loaded. Not where the parts are
/// placed, which a sheet's parts never are.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetKey {
    stock: StockDef,
    parts: Vec<Option<ModelKey>>,
}

/// What a job's sheets are arranged from: each of its arranged stocks, in its
/// order.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrangeKey(Vec<SheetKey>);

/// An arrangement to work out off the window's thread.
pub struct Arranging {
    /// Which it is, to tell it from one since asked for.
    pub seq: u64,
    /// What it is made from, to keep it by.
    pub key: ArrangeKey,
    /// The job as it is now.
    pub job: Job,
    /// The bodies of the sheets' parts, as loaded, by model.
    pub models: BTreeMap<String, Arc<Model>>,
    /// The outlines kept so far, for it to cast none again.
    pub outlines: Outlines,
    /// What stops it.
    pub control: Control,
}

/// Where one of a job's stocks stands as a sheet, now.
#[derive(Debug, Clone, Copy)]
pub enum SheetNow<'a> {
    /// It is not an arranged stock of the job.
    NotASheet,
    /// It is being arranged, or is about to be: nothing kept is laid as it is
    /// now.
    Arranging,
    /// Laid as it is now, in this arrangement.
    Laid(&'a Arc<Arrangement>, &'a SheetLayout),
    /// Its arrangement ended without an answer, and why.
    Lost(&'a str),
}

impl Document {
    /// What sheet `stock` is laid from now, if it is an arranged stock.
    fn sheet_key(&self, def: &StockDef) -> Option<SheetKey> {
        let StockSpec::Arranged(a) = &def.spec else {
            return None;
        };
        let parts = a
            .parts
            .iter()
            .map(|p| match self.models.get(&p.model) {
                Some(slot @ ModelSlot::Ready(..)) => Some(slot.key().clone()),
                _ => None,
            })
            .collect();
        Some(SheetKey {
            stock: def.clone(),
            parts,
        })
    }

    /// What the job's sheets are arranged from now; `None` for a job with no
    /// sheets.
    pub fn arrange_key(&self) -> Option<ArrangeKey> {
        let sheets: Vec<SheetKey> = self
            .job
            .stocks
            .iter()
            .filter_map(|d| self.sheet_key(d))
            .collect();
        (!sheets.is_empty()).then_some(ArrangeKey(sheets))
    }

    /// Whether `key` is what sheet `def` is laid from now: compared in place,
    /// as it is asked for every model every frame.
    fn laid_from(&self, key: &SheetKey, def: &StockDef) -> bool {
        let StockSpec::Arranged(a) = &def.spec else {
            return false;
        };
        key.stock == *def
            && key.parts.len() == a.parts.len()
            && key.parts.iter().zip(&a.parts).all(|(k, p)| {
                let now = match self.models.get(&p.model) {
                    Some(slot @ ModelSlot::Ready(..)) => Some(slot.key()),
                    _ => None,
                };
                k.as_ref() == now
            })
    }

    /// Sheet `stock` as it is now: laid in the latest arrangement kept that
    /// laid it as it is, if one does.
    pub fn sheet_now(&self, stock: &str) -> SheetNow<'_> {
        let Some(def) = self
            .job
            .stock(stock)
            .filter(|d| matches!(d.spec, StockSpec::Arranged(_)))
        else {
            return SheetNow::NotASheet;
        };
        for (kept, result) in self.arrangements.iter().rev() {
            if !kept.0.iter().any(|k| self.laid_from(k, def)) {
                continue;
            }
            return match result {
                Ok(arrangement) => match arrangement.sheet(stock) {
                    Some(sheet) => SheetNow::Laid(arrangement, sheet),
                    None => SheetNow::Arranging,
                },
                Err(why) => SheetNow::Lost(why),
            };
        }
        SheetNow::Arranging
    }

    /// The latest arrangement kept that laid sheet `stock` out, whatever it
    /// was laid from: what the sheet is drawn as while it is being arranged
    /// again, so it is not lost from view meanwhile.
    pub fn sheet_last_laid(&self, stock: &str) -> Option<Arc<Arrangement>> {
        self.arrangements
            .iter()
            .rev()
            .filter_map(|(_, r)| r.as_ref().ok())
            .find(|a| {
                a.sheet(stock)
                    .is_some_and(|s| matches!(s.laid, Laid::Done { .. }))
            })
            .cloned()
    }

    /// Where each copy of model `name` stands now: once where it is, for one
    /// no sheet holds; as its sheet is laid now, for one on a sheet; `None`
    /// while that sheet is being arranged.
    pub fn stands_now(&self, name: &str) -> Option<&[Stand]> {
        let Some((sheet, _)) = self.job.sheet_of(name) else {
            return Some(WHERE_IT_IS);
        };
        match self.sheet_now(&sheet.name) {
            SheetNow::Laid(arrangement, _) => arrangement.stands(name),
            SheetNow::NotASheet | SheetNow::Arranging | SheetNow::Lost(_) => None,
        }
    }

    /// Where each copy of model `name` stands, to draw it: as now, or while
    /// its sheet is being arranged, as it was last laid, so it is not lost
    /// from view meanwhile.
    pub(super) fn stands_shown(&self, name: &str) -> Option<&[Stand]> {
        if let Some(now) = self.stands_now(name) {
            return Some(now);
        }
        // As the latest arrangement that had it on a sheet laid it.
        self.arrangements
            .iter()
            .rev()
            .filter_map(|(_, r)| r.as_ref().ok())
            .filter(|a| {
                a.sheets()
                    .iter()
                    .any(|s| s.models.iter().any(|m| m == name))
            })
            .find_map(|a| a.stands(name))
    }

    /// What each sheet setup `index` uses is laid from now.
    fn setup_sheets(&self, index: usize) -> Vec<SheetKey> {
        self.job.setups.get(index).map_or_else(Vec::new, |setup| {
            setup
                .stocks
                .iter()
                .filter_map(|s| self.sheet_key(self.job.stock(s)?))
                .collect()
        })
    }

    /// The arrangement a run of setup `index` stands its models by: the
    /// latest kept that lays every sheet the setup uses as it is now. For a
    /// setup on no sheet, none at all, as every model of it stands where it
    /// is. `None` while one of its sheets is being arranged.
    pub fn arrangement_for(&self, index: usize) -> Option<Arc<Arrangement>> {
        let keys = self.setup_sheets(index);
        if keys.is_empty() {
            return Some(Arc::new(Arrangement::default()));
        }
        self.laying(&keys).cloned()
    }

    /// The latest arrangement kept that lays every one of `keys` as it is.
    fn laying(&self, keys: &[SheetKey]) -> Option<&Arc<Arrangement>> {
        self.arrangements
            .iter()
            .rev()
            .filter(|(kept, _)| keys.iter().all(|k| kept.0.contains(k)))
            .find_map(|(_, r)| r.as_ref().ok())
    }

    /// Whether every sheet setup `index` uses is laid as it is now, all in
    /// one arrangement: where its parts stand is known. So for a setup on no
    /// sheet.
    pub(super) fn sheets_laid(&self, index: usize) -> bool {
        let keys = self.setup_sheets(index);
        keys.is_empty() || self.laying(&keys).is_some()
    }

    /// Why setup `index` waits for its sheets, if it does: one being
    /// arranged, or one that cannot be.
    pub(super) fn sheets_hold_back(&self, index: usize) -> Option<String> {
        let setup = self.job.setups.get(index)?;
        let keys = self.setup_sheets(index);
        if keys.is_empty() {
            return None;
        }
        for stock in &setup.stocks {
            match self.sheet_now(stock) {
                SheetNow::NotASheet => {}
                SheetNow::Arranging => return Some(format!("'{stock}' is being arranged")),
                SheetNow::Lost(why) => {
                    return Some(format!("sheet '{stock}' could not be arranged: {why}"));
                }
                SheetNow::Laid(_, sheet) => match &sheet.laid {
                    Laid::Done { .. } => {}
                    Laid::Failed(e) => {
                        return Some(format!("sheet '{stock}' cannot be arranged: {e}"));
                    }
                    Laid::Waiting(_) => {
                        return Some(format!("'{stock}' waits for its parts to load"));
                    }
                },
            }
        }
        // Each is laid as it is, but not yet all in one arrangement.
        if self.laying(&keys).is_none() {
            return Some(format!("'{}' is being arranged", setup.stocks.join("', '")));
        }
        None
    }

    /// The arrangement the job wants worked out, if it wants one: its sheets
    /// as they are now are neither kept, together, nor being worked out. One
    /// in flight for anything else is stopped, as the newest asked for is the
    /// one wanted.
    pub fn arrange_wanted(&mut self) -> Option<Arranging> {
        let Some(key) = self.arrange_key() else {
            self.stop_arranging();
            return None;
        };
        if self.arrangements.iter().any(|(kept, _)| *kept == key) {
            self.stop_arranging();
            return None;
        }
        // A part being placed again is laid once it is, not as it was.
        let placing = self.job.stocks.iter().any(|s| {
            matches!(s.spec, StockSpec::Arranged(_))
                && s.spec
                    .models()
                    .iter()
                    .any(|m| self.placing.contains_key(*m))
        });
        if placing {
            return None;
        }
        if self.arranging.as_ref().is_some_and(|(_, k, _)| *k == key) {
            return None;
        }
        self.stop_arranging();
        let control = Control::new(1);
        let seq = next_key();
        self.arranging = Some((seq, key.clone(), control.clone()));
        let models = self
            .job
            .stocks
            .iter()
            .filter(|s| matches!(s.spec, StockSpec::Arranged(_)))
            .flat_map(|s| s.spec.models())
            .filter_map(|name| Some((name.to_string(), self.models.get(name)?.model()?.clone())))
            .collect();
        Some(Arranging {
            seq,
            key,
            job: self.job.clone(),
            models,
            outlines: self.outlines.clone(),
            control,
        })
    }

    /// Stop the arrangement in flight, if any.
    fn stop_arranging(&mut self) {
        if let Some((.., control)) = self.arranging.take() {
            control.stop();
        }
    }

    /// Arrangement `seq`, of the sheets made from `key`, is `result`, and
    /// the outlines cast for it are `outlines`: kept, whether or not it is
    /// still the one wanted, as an undo may want it again. One stopped is
    /// dropped.
    pub fn arranged(
        &mut self,
        seq: u64,
        key: ArrangeKey,
        result: Result<Arrangement, Stopped>,
        outlines: Outlines,
    ) {
        self.outlines.merge(outlines);
        if self.arranging.as_ref().is_some_and(|(s, ..)| *s == seq) {
            self.arranging = None;
        }
        if let Ok(arrangement) = result {
            self.keep_arrangement(key, Ok(Arc::new(arrangement)));
        }
    }

    /// Arrangement `seq` ended without an answer, for `why`: kept as that,
    /// where it is the one wanted, so it is not asked for again until
    /// something changes.
    pub fn arrangement_lost(&mut self, seq: u64, why: String) {
        if self.arranging.as_ref().is_some_and(|(s, ..)| *s == seq)
            && let Some((_, key, _)) = self.arranging.take()
        {
            self.keep_arrangement(key, Err(why));
        }
    }

    fn keep_arrangement(&mut self, key: ArrangeKey, result: Result<Arc<Arrangement>, String>) {
        self.arrangements.retain(|(k, _)| *k != key);
        self.arrangements.push((key, result));
        if self.arrangements.len() > KEPT_ARRANGEMENTS {
            self.arrangements.drain(..1);
        }
    }
}
