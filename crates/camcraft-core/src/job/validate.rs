//! Checking a job before anything is computed.

use std::collections::{BTreeMap, BTreeSet};

use super::{FixedBox, JOB_VERSION, Job, JobError, Origin, StockSpec};
use crate::is_positive;

impl Job {
    /// Check the whole job is runnable before anything is computed: everything
    /// [`check_common`](Self::check_common) checks, and every setup.
    ///
    /// Everything catchable is caught here, so a job fails on its own terms rather
    /// than part-way through an operation with a half-built path in hand.
    pub fn validate(&self) -> Result<(), JobError> {
        self.check_common()?;
        if self.setups.is_empty() {
            return Err(JobError::NoSetups);
        }
        (0..self.setups.len()).try_for_each(|i| self.check_setup(i))
    }

    /// Check what every setup depends on: the format, the tolerance, the tools,
    /// the names, and the stocks. A setup still being written does not fail this,
    /// so another setup can run.
    pub fn check_common(&self) -> Result<(), JobError> {
        if self.version != JOB_VERSION {
            return Err(JobError::UnsupportedVersion(self.version));
        }
        positive("tolerance", self.tolerance)?;

        for (name, spec) in &self.tools {
            spec.build().map_err(|source| JobError::BadTool {
                name: name.clone(),
                source,
            })?;
            if spec.rpm == 0 {
                return Err(JobError::BadValue {
                    field: format!("tools.{name}.rpm"),
                    value: 0.0,
                });
            }
            if let Some(source) = spec.check().into_iter().next() {
                return Err(JobError::BadToolField {
                    name: name.clone(),
                    source,
                });
            }
        }

        unique("model", self.models.iter().map(|m| m.name.as_str()))?;
        unique("stock", self.stocks.iter().map(|s| s.name.as_str()))?;
        unique("setup", self.setups.iter().map(|s| s.name.as_str()))?;

        for stock in &self.stocks {
            let field = |f: &str| format!("stock '{}'.{f}", stock.name);
            match &stock.spec {
                StockSpec::AroundModels {
                    models,
                    margin,
                    below,
                } => {
                    for m in models {
                        if self.model(m).is_none() {
                            return Err(JobError::UnknownModel {
                                owner: format!("stock '{}'", stock.name),
                                model: m.clone(),
                            });
                        }
                    }
                    for (i, v) in margin.iter().chain(std::iter::once(below)).enumerate() {
                        if !v.is_finite() || *v < 0.0 {
                            return Err(JobError::BadValue {
                                field: field(if i < 3 { "margin" } else { "below" }),
                                value: *v,
                            });
                        }
                    }
                }
                StockSpec::FixedBox(FixedBox {
                    size,
                    models,
                    place,
                }) => {
                    for m in models {
                        if self.model(m).is_none() {
                            return Err(JobError::UnknownModel {
                                owner: format!("stock '{}'", stock.name),
                                model: m.clone(),
                            });
                        }
                    }
                    for v in place.iter().filter_map(|p| p.value()) {
                        if !v.is_finite() {
                            return Err(JobError::BadValue {
                                field: field("place"),
                                value: v,
                            });
                        }
                    }
                    for v in size {
                        positive(&field("size"), *v)?;
                    }
                }
            }
        }

        // Only tools the operations use reach the machine, so only they must not
        // share a number. A table may keep a spare with any number it likes. The
        // machine's tool table is one, so this is across every setup.
        let mut used: BTreeMap<u32, &str> = BTreeMap::new();
        for op in self.setups.iter().flat_map(|s| &s.operations) {
            let Some((name, spec)) = op.tool.as_deref().and_then(|t| self.tools.get_key_value(t))
            else {
                continue;
            };
            match used.get(&spec.number) {
                Some(first) if *first != name.as_str() => {
                    return Err(JobError::DuplicateToolNumber {
                        number: spec.number,
                        first: (*first).to_string(),
                        second: name.clone(),
                    });
                }
                _ => {
                    used.insert(spec.number, name);
                }
            }
        }
        Ok(())
    }

    /// Check setup `index` can run, given [`check_common`](Self::check_common)
    /// passes. An index past the end is [`JobError::NoSetups`].
    pub fn check_setup(&self, index: usize) -> Result<(), JobError> {
        self.check_setup_where(index, &|_| true)
    }

    /// As [`check_setup`](Self::check_setup), checking only the operations that
    /// will run: those turned on for which `runs` holds, by their place. The
    /// rest are left out of the run, and may be unfinished.
    pub fn check_setup_where(
        &self,
        index: usize,
        runs: &dyn Fn(usize) -> bool,
    ) -> Result<(), JobError> {
        let setup = self.setups.get(index).ok_or(JobError::NoSetups)?;
        let running = || {
            setup
                .operations
                .iter()
                .enumerate()
                .filter(|(i, op)| op.enabled && runs(*i))
                .map(|(_, op)| op)
        };
        let name = &setup.name;
        setup
            .orientation
            .check()
            .map_err(|source| JobError::BadOrientation {
                setup: name.clone(),
                source,
            })?;
        if !(54..=59).contains(&setup.wcs) {
            return Err(JobError::BadWorkOffset {
                setup: name.clone(),
                wcs: setup.wcs,
            });
        }
        if setup.models.is_empty() {
            return Err(JobError::NoModels(name.clone()));
        }
        for m in &setup.models {
            if self.model(m).is_none() {
                return Err(JobError::UnknownModel {
                    owner: format!("setup '{name}'"),
                    model: m.clone(),
                });
            }
        }
        if setup.stocks.is_empty() {
            return Err(JobError::NoStock(name.clone()));
        }
        for s in &setup.stocks {
            if self.stock(s).is_none() {
                return Err(JobError::UnknownStock {
                    setup: name.clone(),
                    stock: s.clone(),
                });
            }
        }
        if setup.operations.is_empty() {
            return Err(JobError::NoOperations(name.clone()));
        }
        let pick = |owner: String, model: &str, at: &[f64; 3]| {
            if !setup.models.iter().any(|m| m == model) {
                return Err(JobError::PickedFromOtherModel {
                    setup: name.clone(),
                    owner,
                    model: model.to_string(),
                });
            }
            if !at.iter().all(|v| v.is_finite()) {
                return Err(JobError::BadPick { owner, at: *at });
            }
            Ok(())
        };
        if let Origin::Point { point, .. } = &setup.origin {
            pick(format!("setup '{name}' work zero"), &point.model, &point.at)?;
        }
        for op in running() {
            for r in op.selection.iter() {
                pick(format!("operation '{name}/{}'", op.name), &r.model, &r.at)?;
            }
        }

        running().try_for_each(|op| op.check(name, &self.tools))
    }
}

fn unique<'a>(what: &'static str, names: impl Iterator<Item = &'a str>) -> Result<(), JobError> {
    let mut seen = BTreeSet::new();
    for n in names {
        if n.trim().is_empty() {
            return Err(JobError::NoName { what });
        }
        if !seen.insert(n) {
            return Err(JobError::DuplicateName {
                what,
                name: n.to_string(),
            });
        }
    }
    Ok(())
}

pub(super) fn positive(field: &str, value: f64) -> Result<(), JobError> {
    is_positive(value)
        .then_some(())
        .ok_or_else(|| JobError::BadValue {
            field: field.to_string(),
            value,
        })
}
