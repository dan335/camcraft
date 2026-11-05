//! Checking a job before anything is computed.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    Depths, FixedBox, JOB_VERSION, Job, JobError, Operation, Origin, Retraction, StockSpec,
    StockToLeave, StrategySpec, ToolSpec,
};
use crate::is_positive;
use crate::must::{AT_LEAST_ZERO, FINITE, POSITIVE};

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
            // Its cutter builds, and turns, among the rest.
            if let Some(source) = spec.check().into_iter().next() {
                return Err(JobError::BadToolField {
                    name: name.clone(),
                    source,
                });
            }
        }

        unique("model", self.models.iter().map(|m| m.name.as_str()))?;
        for model in &self.models {
            let t = &model.transform;
            let field = |f: &str| format!("model '{}'.transform.{f}", model.name);
            let values = [
                ("position", &t.position),
                ("rotation", &t.rotation),
                ("scale", &t.scale),
            ];
            for (f, vs) in values {
                for &v in vs {
                    // A scale of nothing flattens the model to a plane, and one
                    // below it mirrors the part: a left hand cut for a right.
                    if f == "scale" && !(v.is_finite() && v >= 1e-6) {
                        return Err(bad(field(f), v, "finite and at least 0.000001"));
                    }
                    finite(&field(f), v)?;
                }
            }
        }
        unique("stock", self.stocks.iter().map(|s| s.name.as_str()))?;
        unique("setup", self.setups.iter().map(|s| s.name.as_str()))?;

        for stock in &self.stocks {
            let field = |f: &str| format!("stock '{}'.{f}", stock.name);
            self.known_models(&format!("stock '{}'", stock.name), stock.spec.models())?;
            match &stock.spec {
                StockSpec::AroundModels { margin, below, .. } => {
                    for (i, v) in margin.iter().chain(std::iter::once(below)).enumerate() {
                        at_least_zero(&field(if i < 3 { "margin" } else { "below" }), *v)?;
                    }
                }
                StockSpec::FixedBox(FixedBox { size, place, .. }) => {
                    for v in place.iter().filter_map(|p| p.value()) {
                        finite(&field("place"), v)?;
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
        if let Some(machine) = &setup.machine {
            let Some(m) = self.machines.get(machine) else {
                return Err(JobError::UnknownMachine {
                    setup: name.clone(),
                    machine: machine.clone(),
                });
            };
            if let Some(i) = m.nose.iter().position(|s| {
                !is_positive(s.lower_diameter)
                    || !is_positive(s.upper_diameter)
                    || !is_positive(s.length)
            }) {
                return Err(JobError::BadNose {
                    machine: machine.clone(),
                    segment: i + 1,
                });
            }
        }
        if setup.models.is_empty() {
            return Err(JobError::NoModels(name.clone()));
        }
        self.known_models(&format!("setup '{name}'"), &setup.models)?;
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
        // Only picks something reads: ones left over from a boundary or an
        // outline since changed are kept, and not found, so cannot stop it.
        for op in running().filter(|op| op.cuts_selection() || op.heights.measured_from_selection())
        {
            for r in op.selection.iter() {
                pick(format!("operation '{name}/{}'", op.name), &r.model, &r.at)?;
            }
        }

        running().try_for_each(|op| op.check(name, &self.tools))?;
        // Each setting as its expression has it: one that cannot be worked
        // out holds what it last came to, which may be stale.
        for op in running().filter(|op| !op.expressions.is_empty()) {
            let tool = op.tool.as_ref().and_then(|t| self.tools.get(t));
            if let Some((setting, message)) = op
                .clone()
                .work_out_expressions(tool, self.tolerance)
                .faults
                .into_iter()
                .next()
            {
                return Err(JobError::BadExpression {
                    operation: format!("{name}/{}", op.name),
                    setting,
                    message,
                });
            }
        }
        Ok(())
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

impl Job {
    /// Check every one of `models`, named by `owner`, is a model of the job.
    fn known_models(&self, owner: &str, models: &[String]) -> Result<(), JobError> {
        match models.iter().find(|m| self.model(m).is_none()) {
            Some(m) => Err(JobError::UnknownModel {
                owner: owner.to_string(),
                model: m.clone(),
            }),
            None => Ok(()),
        }
    }
}

/// `field`'s `value`, which is not `range`.
fn bad(field: impl Into<String>, value: f64, range: &'static str) -> JobError {
    JobError::BadValue {
        field: field.into(),
        value,
        range,
    }
}

/// Check `field`'s `value` is finite and greater than zero.
fn positive(field: &str, value: f64) -> Result<(), JobError> {
    is_positive(value)
        .then_some(())
        .ok_or_else(|| bad(field, value, POSITIVE))
}

/// Check `field`'s `value` is finite and not below zero.
fn at_least_zero(field: &str, value: f64) -> Result<(), JobError> {
    (value.is_finite() && value >= 0.0)
        .then_some(())
        .ok_or_else(|| bad(field, value, AT_LEAST_ZERO))
}

/// Check `field`'s `value` is finite.
fn finite(field: &str, value: f64) -> Result<(), JobError> {
    value
        .is_finite()
        .then_some(())
        .ok_or_else(|| bad(field, value, FINITE))
}

impl StockToLeave {
    /// Check both are numbers, naming `field` if not.
    fn check(&self, field: &str) -> Result<(), JobError> {
        match self.unusable() {
            Some(value) => Err(bad(field, value, FINITE)),
            None => Ok(()),
        }
    }
}

impl Operation {
    /// Check this operation of setup `setup` can run with the job's `tools`: it
    /// names one the job has, the tool can do what it asks, and every value its
    /// strategy uses ([`StrategySpec::uses`]) is usable. One it does not use is
    /// not read, so is not checked. Its picks are checked with the setup's.
    pub(super) fn check(
        &self,
        setup: &str,
        tools: &BTreeMap<String, ToolSpec>,
    ) -> Result<(), JobError> {
        let at = format!("{setup}/{}", self.name);
        let field = |f: &str| format!("{at}.{f}");
        if let Some(why) = self.cannot_measure_from_picks() {
            return Err(JobError::BadHeight { op: at, why });
        }
        let Some(tool) = &self.tool else {
            return Err(JobError::NoTool { op: at });
        };
        let Some(spec) = tools.get(tool) else {
            return Err(JobError::UnknownTool {
                op: at,
                tool: tool.clone(),
            });
        };
        if let Some(why) = self.cannot_use(spec) {
            return Err(JobError::WrongTool {
                op: at,
                tool: tool.clone(),
                why: why.into(),
            });
        }
        let uses = self.strategy.uses();
        self.check_heights(&field)?;
        self.check_passes(&field)?;
        self.check_linking(&field)?;
        if uses.boundary {
            finite(&field("boundary.margin"), self.boundary.margin)?;
        }
        if let Some(stepdown) = self.stepdown() {
            positive(&field("passes.stepdown"), stepdown)?;
            // A fine stepdown no finer than the stepdown would add no level:
            // said, rather than left to do nothing.
            if let Some(fine) = self.passes.fine_stepdown.filter(|_| uses.fine_stepdown) {
                positive(&field("passes.fine_stepdown"), fine)?;
                if fine >= stepdown {
                    return Err(bad(
                        field("passes.fine_stepdown"),
                        fine,
                        "less than the stepdown",
                    ));
                }
            }
        } else if uses.depths == Depths::Required {
            return Err(JobError::NoStepdown { op: at });
        }
        self.check_strategy(&field)
    }

    /// Check the heights its strategy uses are numbers, `field` naming each.
    fn check_heights(&self, field: &dyn Fn(&str) -> String) -> Result<(), JobError> {
        let (uses, h) = (self.strategy.uses(), &self.heights);
        // The retract height is where a full retraction goes, and where a
        // strategy that stays down goes no higher than.
        let retract = uses.stay_down || self.linking.retraction == Retraction::Full;
        let used = [
            ("clearance", Some(h.clearance)),
            ("retract", retract.then_some(h.retract)),
            ("top", h.top.filter(|_| uses.top.is_some())),
            ("bottom", h.bottom.filter(|_| uses.bottom.is_some())),
        ];
        for (name, height) in used {
            if let Some(height) = height {
                finite(&field(&format!("heights.{name}.offset")), height.offset)?;
            }
        }
        Ok(())
    }

    /// Check the passes' values its strategy uses, `field` naming each.
    fn check_passes(&self, field: &dyn Fn(&str) -> String) -> Result<(), JobError> {
        let uses = self.strategy.uses();
        if uses.stepover {
            positive(&field("passes.stepover"), self.passes.stepover)?;
        }
        if uses.feed {
            positive(&field("passes.feed"), self.passes.feed)?;
        }
        if let Some(t) = self.passes.smoothing_tolerance.filter(|_| uses.smoothing) {
            positive(&field("passes.smoothing_tolerance"), t)?;
        }
        if let Some(f) = self
            .passes
            .feed_optimization
            .filter(|_| uses.feed_optimization)
        {
            let turn = f.max_directional_change;
            if !(turn > 0.0 && turn < 180.0) {
                return Err(bad(
                    field("passes.feed_optimization.max_directional_change"),
                    turn,
                    "between 0 and 180 degrees",
                ));
            }
            if let Some(r) = f.reduced_feed_radius {
                at_least_zero(&field("passes.feed_optimization.reduced_feed_radius"), r)?;
            }
            if let Some(d) = f.reduced_feed_distance {
                at_least_zero(&field("passes.feed_optimization.reduced_feed_distance"), d)?;
            }
            if let Some(feed) = f.reduced_feed {
                positive(&field("passes.feed_optimization.reduced_feed"), feed)?;
            }
        }
        if self.passes.rpm == Some(0) {
            return Err(bad(field("passes.rpm"), 0.0, "greater than zero"));
        }
        if uses.cut_step {
            positive(&field("passes.max_cut_step"), self.passes.max_cut_step)?;
        }
        self.stock_to_leave()
            .check(&field("passes.stock_to_leave"))?;
        if let Some(t) = self.passes.tabs.filter(|_| uses.tabs) {
            positive(&field("passes.tabs.width"), t.width)?;
            positive(&field("passes.tabs.height"), t.height)?;
            positive(&field("passes.tabs.spacing"), t.spacing)?;
        }
        if let Some(f) = self.passes.finishing.filter(|_| uses.finishing) {
            if f.passes == Some(0) {
                return Err(bad(field("passes.finishing.passes"), 0.0, "at least one"));
            }
            if let Some(s) = f.stepover {
                positive(&field("passes.finishing.stepover"), s)?;
            }
            if let Some(v) = f.feed {
                positive(&field("passes.finishing.feed"), v)?;
            }
        }
        Ok(())
    }

    /// Check the linking values its strategy uses, `field` naming each.
    fn check_linking(&self, field: &dyn Fn(&str) -> String) -> Result<(), JobError> {
        let (uses, l) = (self.strategy.uses(), &self.linking);
        positive(&field("linking.retract_feed"), l.retract_feed)?;
        positive(&field("linking.plunge_feed"), l.plunge_feed)?;
        // Every strategy's links are made above the material.
        at_least_zero(&field("linking.safe_distance"), l.safe_distance)?;
        if uses.stay_down
            && let Some(d) = l.stay_down
        {
            at_least_zero(&field("linking.stay_down"), d)?;
        }
        if uses.lead_radius
            && let Some(r) = l.lead_radius
        {
            at_least_zero(&field("linking.lead_radius"), r)?;
        }
        if uses.ramp {
            if !(l.ramp_angle > 0.0 && l.ramp_angle < 90.0) {
                return Err(bad(
                    field("linking.ramp_angle"),
                    l.ramp_angle,
                    "greater than 0 and less than 90 degrees",
                ));
            }
            if let Some(f) = l.ramp_feed {
                positive(&field("linking.ramp_feed"), f)?;
            }
            if let Some(d) = l.ramp_stepdown {
                positive(&field("linking.ramp_stepdown"), d)?;
            }
        }
        if let Some(leads) = l.leads.filter(|_| uses.leads) {
            if let Some(r) = leads.radius {
                positive(&field("linking.leads.radius"), r)?;
            }
            if !(leads.sweep > 0.0 && leads.sweep <= 180.0) {
                return Err(bad(
                    field("linking.leads.sweep"),
                    leads.sweep,
                    "greater than 0 and at most 180 degrees",
                ));
            }
            for (name, v) in [("straight", leads.straight), ("vertical", leads.vertical)] {
                if let Some(v) = v {
                    at_least_zero(&field(&format!("linking.leads.{name}")), v)?;
                }
            }
        }
        if let Some(e) = l.entry
            && !uses.entries.is_empty()
            && !uses.entries.contains(&e)
        {
            return Err(JobError::NotOffered {
                field: field("linking.entry"),
                value: e.name(),
                strategy: self.strategy.name(),
            });
        }
        if !uses.entries.is_empty()
            && let Some(d) = l.helix_diameter
        {
            positive(&field("linking.helix_diameter"), d)?;
        }
        Ok(())
    }

    /// Check its strategy's own values, `field` naming each.
    fn check_strategy(&self, field: &dyn Fn(&str) -> String) -> Result<(), JobError> {
        if let StrategySpec::Face {
            angle,
            pass_extension,
            stock_offset,
            ..
        } = self.strategy
        {
            finite(&field("angle"), angle)?;
            at_least_zero(&field("pass_extension"), pass_extension)?;
            finite(&field("stock_offset"), stock_offset)?;
        }
        if let Some((slope_from, slope_to)) = self.strategy.slopes() {
            if !(slope_from.is_finite() && (0.0..90.0).contains(&slope_from)) {
                return Err(bad(
                    field("slope_from"),
                    slope_from,
                    "at least 0 and less than 90 degrees",
                ));
            }
            if !(slope_to.is_finite() && slope_from < slope_to && slope_to <= 90.0) {
                return Err(bad(
                    field("slope_to"),
                    slope_to,
                    "greater than slope_from and at most 90 degrees",
                ));
            }
        }
        if let StrategySpec::Spiral {
            centre,
            inner_radius,
            outer_radius,
            ..
        }
        | StrategySpec::Radial {
            centre,
            inner_radius,
            outer_radius,
            ..
        } = self.strategy
        {
            check_centred(field, centre, inner_radius, outer_radius)?;
        }
        if let StrategySpec::MorphedSpiral { centre, .. } = self.strategy {
            // Its centre alone: it has no radii.
            check_centred(field, centre, 0.0, 0.0)?;
        }
        if let StrategySpec::Radial {
            angle_from,
            angle_to,
            ..
        } = self.strategy
        {
            finite(&field("angle_from"), angle_from)?;
            if !(angle_to.is_finite() && angle_from < angle_to && angle_to <= angle_from + 360.0) {
                return Err(bad(
                    field("angle_to"),
                    angle_to,
                    "greater than angle_from and at most a turn past it",
                ));
            }
        }
        if let StrategySpec::Adaptive {
            min_cutting_radius,
            rest_machining,
            ignore_stock_less_than,
            min_flat_stepdown,
            min_axial_engagement,
            shallow_areas,
            min_shallow_stepdown,
            max_shallow_stepover,
            slot_clearing,
            slot_width,
            stay_down_level,
            stay_down_clearance,
            lift_height,
            ..
        } = self.strategy
        {
            at_least_zero(&field("min_cutting_radius"), min_cutting_radius)?;
            // Read only with rest machining.
            if rest_machining {
                at_least_zero(&field("ignore_stock_less_than"), ignore_stock_less_than)?;
            }
            at_least_zero(&field("min_flat_stepdown"), min_flat_stepdown)?;
            at_least_zero(&field("min_axial_engagement"), min_axial_engagement)?;
            // Read only with shallow areas.
            if shallow_areas {
                positive(&field("min_shallow_stepdown"), min_shallow_stepdown)?;
                positive(&field("max_shallow_stepover"), max_shallow_stepover)?;
            }
            // Read only with slot clearing.
            if slot_clearing {
                at_least_zero(&field("slot_width"), slot_width)?;
            }
            if !(0.0..=100.0).contains(&stay_down_level) {
                return Err(bad(
                    field("stay_down_level"),
                    stay_down_level,
                    "a percentage from 0 to 100",
                ));
            }
            at_least_zero(&field("stay_down_clearance"), stay_down_clearance)?;
            at_least_zero(&field("lift_height"), lift_height)?;
        }
        if let StrategySpec::Drill { peck, dwell } = self.strategy {
            if let Some(p) = peck {
                positive(&field("peck"), p)?;
            }
            if let Some(d) = dwell {
                at_least_zero(&field("dwell"), d)?;
            }
        }
        Ok(())
    }
}

/// Check a pattern about a centre's centre and radii, `field` naming each: a
/// finite centre, and radii from zero with the inner inside the outer where the
/// outer is given.
fn check_centred(
    field: &dyn Fn(&str) -> String,
    centre: Option<[f64; 2]>,
    inner: f64,
    outer: f64,
) -> Result<(), JobError> {
    for v in centre.into_iter().flatten() {
        finite(&field("centre"), v)?;
    }
    at_least_zero(&field("inner_radius"), inner)?;
    if !(outer.is_finite() && outer >= 0.0 && (outer == 0.0 || inner < outer)) {
        return Err(bad(
            field("outer_radius"),
            outer,
            "zero, or finite and greater than inner_radius",
        ));
    }
    Ok(())
}
