//! Which of a form's settings differ from their defaults, and putting them back.
//!
//! A form is bound to what it edits and to that thing's default: an operation
//! and the one a new operation with its tool would be, a setup and a new setup,
//! and so on. Both are read as a job file has them, and each setting is named by
//! its place there (`passes.stepover`, `linking.ramp_angle`, a strategy's own
//! `order`), so one mechanism serves every form and every kind of thing, nested
//! settings and a strategy's own among them, with nothing written for any one
//! field. A setting that differs has its label in amber and a ↺ that puts the
//! default's back: copied in where the setting is, and the thing read again, as
//! a job file is read.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{LazyLock, Mutex, PoisonError};

use eframe::egui;
use serde::Serialize;
use serde::de::DeserializeOwned;
use toml::{Table, Value};

use crate::help::{Help, Topic};
use crate::ui::widgets;

/// A form's thing and its default, as a job file has them, and the settings
/// asked to go back to the default while the form was drawn.
pub struct Defaults<T> {
    now: Table,
    default: Table,
    /// The default with each optional group this thing has given as giving it
    /// starts it ([`Defaults::started`]): what the settings in a group are
    /// against. The group's being given at all is against `default`.
    started: Option<Table>,
    resets: Vec<(String, Layer)>,
    of: PhantomData<T>,
    /// Every setting the form asked about, for the tests to check each names
    /// one ([`ASKED`]).
    #[cfg(test)]
    asked: std::cell::RefCell<Vec<String>>,
}

#[cfg(test)]
thread_local! {
    /// Every setting a form asked about as it was drawn, by the kind of thing it
    /// is of, and whether it names a setting there: a path mistyped would show
    /// nothing as changed, and say nothing of it.
    pub static ASKED: std::cell::RefCell<Vec<(&'static str, String, bool)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl<T: Serialize + DeserializeOwned> Defaults<T> {
    /// `value`'s settings against `default`'s. Something that cannot be read
    /// as a job file has it shows nothing as changed.
    pub fn of(value: &T, default: &T) -> Self {
        let table = |v: &T| Table::try_from(v).unwrap_or_default();
        Self {
            now: table(value),
            default: table(default),
            started: None,
            resets: Vec::new(),
            of: PhantomData,
            #[cfg(test)]
            asked: std::cell::RefCell::default(),
        }
    }

    /// With `started`, the default with each optional group (tabs, a
    /// pocket's finishing passes, leads) this thing has given as ticking it
    /// starts it: the settings in a group are against what they start as,
    /// and the group's being given at all against the default
    /// ([`Defaults::reset_group`]).
    pub fn started(mut self, started: &T) -> Self {
        self.started = Some(Table::try_from(started).unwrap_or_default());
        self
    }

    /// What a setting is against: what its group started as, where it is in
    /// one, or the default.
    fn against(&self) -> &Table {
        self.started.as_ref().unwrap_or(&self.default)
    }

    /// Whether the setting at `path` differs from the default's: or any of
    /// them, for a row that edits several, named with commas between
    /// (`slope_from,slope_to`).
    pub fn changed(&self, path: &str) -> bool {
        #[cfg(test)]
        self.asked
            .borrow_mut()
            .extend(path.split(',').map(|p| p.trim().to_string()));
        path.split(',')
            .map(str::trim)
            .any(|p| at(&self.now, p) != at(self.against(), p) && !self.required(p))
    }

    /// Whether `path` names a setting of `T`: one this thing or its default
    /// writes, or, where neither does (a setting not given), one a plain value
    /// put there is read as and written back at.
    #[cfg(test)]
    fn names_a_setting(&self, path: &str) -> bool {
        if at(&self.now, path).is_some() || at(&self.default, path).is_some() {
            return true;
        }
        let tries = [
            Value::Float(1.0),
            Value::Integer(1),
            Value::Boolean(true),
            Value::String("1".into()),
            Value::Array(vec![Value::Float(1.0), Value::Float(1.0)]),
        ];
        let read = |v: Value| {
            let mut table = self.default.clone();
            put(&mut table, path, Some(v));
            Value::Table(table).try_into::<T>()
        };
        if tries.into_iter().any(|v| {
            read(v)
                .ok()
                .and_then(|back| Table::try_from(&back).ok())
                .is_some_and(|written| at(&written, path).is_some())
        }) {
            return true;
        }
        // A setting written as a table or a name, at its default not written
        // at all: something no setting could be is refused as the wrong kind
        // of value there, where a name that is no setting is refused as that.
        match read(Value::String("\u{1}".into())) {
            Err(e) => !e.to_string().contains("unknown field"),
            Ok(_) => false,
        }
    }

    /// Whether the setting at `path` is one a job file must give: it has no
    /// default to differ from, or go back to, as a tool's diameter has none.
    /// Found by reading the default without it, once for each kind of thing
    /// and setting.
    fn required(&self, path: &str) -> bool {
        // In a group ticked to give it, what ticking starts it as is its
        // default, though the group's own kind must have it.
        if let Some(started) = &self.started {
            let group = path.rsplit_once('.').map(|(parent, _)| parent);
            if group.is_some_and(|g| at(started, g).is_some() && at(&self.default, g).is_none()) {
                return false;
            }
        }
        static FOUND: LazyLock<Mutex<HashMap<(&'static str, String), bool>>> =
            LazyLock::new(Mutex::default);
        let key = (std::any::type_name::<T>(), path.to_string());
        if let Some(&known) = FOUND
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return known;
        }
        let mut without = self.against().clone();
        put(&mut without, path, None);
        let required = Value::Table(without).try_into::<T>().is_err();
        FOUND
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, required);
        required
    }

    /// A row of a form, as [`widgets::row`], for the setting at `path`.
    pub fn row<R>(
        &mut self,
        ui: &mut egui::Ui,
        topic: &Topic,
        path: &str,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        self.row_as(ui, topic.title, topic, path, add)
    }

    /// As [`Defaults::row`], labelled `text`.
    pub fn row_as<R>(
        &mut self,
        ui: &mut egui::Ui,
        text: &str,
        topic: &Topic,
        path: &str,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let (out, reset) = widgets::setting_row(ui, text, topic, self.changed(path), add);
        if reset {
            self.push(path, Layer::Started);
        }
        out
    }

    /// Ask for the settings at `path` to go back, from `layer`.
    fn push(&mut self, path: &str, layer: Layer) {
        self.resets
            .extend(path.split(',').map(|p| (p.trim().to_string(), layer)));
    }

    /// Whether the optional group at `path` is given where the default has
    /// none, or not where it has one.
    pub fn group_changed(&self, path: &str) -> bool {
        #[cfg(test)]
        self.asked.borrow_mut().push(path.to_string());
        at(&self.now, path).is_some() != at(&self.default, path).is_some()
    }

    /// A ↺ for the optional group at `path`, beside the box ticked to give
    /// it, where giving it or not differs from the default: back to that.
    pub fn reset_group(&mut self, ui: &mut egui::Ui, path: &str) {
        if self.group_changed(path) && widgets::reset_button(ui).clicked() {
            self.push(path, Layer::Default);
        }
    }

    /// A box ticked to give the optional group at `path`, labelled `label`,
    /// with its ↺ beside it: ticked, the group is given as it starts;
    /// unticked, it is taken away. Its rows, if it is given, follow a little
    /// below.
    pub fn optional_group<G: Default>(
        &mut self,
        ui: &mut egui::Ui,
        topic: &Topic,
        path: &str,
        label: &str,
        group: &mut Option<G>,
    ) {
        let mut on = group.is_some();
        ui.horizontal(|ui| {
            ui.checkbox(&mut on, label).help(topic);
            self.reset_group(ui, path);
        });
        match (on, group.is_some()) {
            (true, false) => *group = Some(G::default()),
            (false, true) => *group = None,
            _ => {}
        }
        if group.is_some() {
            ui.add_space(crate::ui::theme::SPACE_2);
        }
    }

    /// A row for the optional group at `path`, its label and ↺ as
    /// [`Defaults::reset_group`] has them.
    pub fn group_row<R>(
        &mut self,
        ui: &mut egui::Ui,
        topic: &Topic,
        path: &str,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let changed = self.group_changed(path);
        let (out, reset) = widgets::setting_row(ui, topic.title, topic, changed, add);
        if reset {
            self.push(path, Layer::Default);
        }
        out
    }

    /// `text`, amber where the setting at `path` differs: for a form laid out
    /// its own way, which draws its labels itself.
    pub fn tint(&self, text: egui::RichText, path: &str) -> egui::RichText {
        if self.changed(path) {
            text.color(crate::ui::theme::egui_color(crate::ui::theme::SELECTED))
        } else {
            text
        }
    }

    /// A ↺ for the setting at `path`, where it differs, after whatever else
    /// edits it, such as a box ticked to have it at all.
    pub fn reset(&mut self, ui: &mut egui::Ui, path: &str) {
        if self.changed(path) && widgets::reset_button(ui).clicked() {
            self.push(path, Layer::Started);
        }
    }

    /// Put back into `value` the default's settings ↺ was clicked for. Called
    /// once the form is drawn.
    pub fn apply(self, value: &mut T) {
        #[cfg(test)]
        {
            let asked: Vec<String> = self.asked.borrow().clone();
            let found: Vec<_> = asked
                .into_iter()
                .map(|p| {
                    let names = self.names_a_setting(&p);
                    (std::any::type_name::<T>(), p, names)
                })
                .collect();
            ASKED.with(|a| a.borrow_mut().extend(found));
        }
        if self.resets.is_empty() {
            return;
        }
        let Ok(mut now) = Table::try_from(&*value) else {
            return;
        };
        for (path, layer) in &self.resets {
            put(&mut now, path, at(self.layer(*layer), path).cloned());
        }
        if let Ok(back) = Value::Table(now).try_into::<T>() {
            *value = back;
            return;
        }
        // A setting that decides which others there are, such as a stock's
        // shape, cannot go back alone: what it belongs to goes back whole,
        // with everything it decides.
        let Ok(mut now) = Table::try_from(&*value) else {
            return;
        };
        for (path, layer) in &self.resets {
            let from = self.layer(*layer);
            match path.rsplit_once('.') {
                Some((parent, _)) => put(&mut now, parent, at(from, parent).cloned()),
                None => now = from.clone(),
            }
        }
        if let Ok(back) = Value::Table(now).try_into::<T>() {
            *value = back;
        }
    }
}

/// Which of a form's defaults a setting goes back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    /// The thing's default.
    Default,
    /// What the setting's group started as, where it is in one.
    Started,
}

impl<T> Defaults<T> {
    /// The default `layer` is.
    fn layer(&self, layer: Layer) -> &Table {
        match layer {
            Layer::Default => &self.default,
            Layer::Started => self.started.as_ref().unwrap_or(&self.default),
        }
    }
}

/// The setting at `path`, dotted, in `table`, a number in it the place in a
/// list (`size.0`): `None` where it is not written, as one at its default
/// often is not.
fn at<'t>(table: &'t Table, path: &str) -> Option<&'t Value> {
    let mut keys = path.split('.');
    let mut value = table.get(keys.next()?)?;
    for key in keys {
        value = match value {
            Value::Table(t) => t.get(key)?,
            Value::Array(a) => a.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(value)
}

/// Set the setting at `path` in `table` to `value`, or take it out for `None`,
/// making the tables on the way to it.
fn put(table: &mut Table, path: &str, value: Option<Value>) {
    let mut keys: Vec<&str> = path.split('.').collect();
    let Some(last) = keys.pop() else {
        return;
    };
    let mut here = table;
    for (k, key) in keys.iter().enumerate() {
        // A place in a list is set where it is: the list is there, whole.
        if let Some(Value::Array(list)) = here.get_mut(*key) {
            let rest: Vec<&str> = keys.iter().skip(k + 1).copied().chain([last]).collect();
            put_in_list(list, &rest, value);
            return;
        }
        let next = here
            .entry((*key).to_string())
            .or_insert_with(|| Value::Table(Table::new()));
        let Some(t) = next.as_table_mut() else {
            return;
        };
        here = t;
    }
    match value {
        Some(v) => {
            here.insert(last.to_string(), v);
        }
        None => {
            here.remove(last);
        }
    }
}

/// Set the place `path` leads to in `list`, its first key the place: a list's
/// places are only ever set, never taken out.
fn put_in_list(list: &mut [Value], path: &[&str], value: Option<Value>) {
    let Some((first, rest)) = path.split_first() else {
        return;
    };
    let Some(slot) = first.parse::<usize>().ok().and_then(|i| list.get_mut(i)) else {
        return;
    };
    match (rest.is_empty(), slot) {
        (true, slot) => {
            if let Some(v) = value {
                *slot = v;
            }
        }
        (false, Value::Table(t)) => put(t, &rest.join("."), value),
        (false, Value::Array(inner)) => put_in_list(inner, rest, value),
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use camcraft_core::job::{Operation, StrategySpec};

    fn adaptive() -> Operation {
        Operation::new("rough", StrategySpec::named("adaptive").unwrap())
    }

    /// A setting is changed where it differs from the default's, nested or a
    /// strategy's own, and not where it is the same; put back, it is the
    /// default's, and nothing else moves.
    #[test]
    fn a_setting_differs_and_goes_back() {
        let def = adaptive();
        let mut op = adaptive();
        op.passes.stepover = 3.5;
        op.passes.stepdown = Some(12.7);
        op.linking.ramp_angle = 4.0;
        let d = Defaults::of(&op, &def);
        assert!(d.changed("passes.stepover"));
        assert!(
            d.changed("passes.stepdown"),
            "given where the default has none"
        );
        assert!(d.changed("linking.ramp_angle"));
        assert!(!d.changed("passes.feed"));
        assert!(!d.changed("direction"), "the strategy's own, as it was");

        let mut d = d;
        d.push("passes.stepover", Layer::Started);
        d.push("passes.stepdown", Layer::Started);
        d.apply(&mut op);
        assert_eq!(op.passes.stepover, def.passes.stepover);
        assert_eq!(op.passes.stepdown, def.passes.stepdown, "taken out again");
        assert_eq!(op.linking.ramp_angle, 4.0, "left as it was");
    }

    /// An optional group given where the default has none: given is the
    /// change, against the default; its settings are against what ticking
    /// it starts them as, and go back to that, the group kept; the group
    /// goes back to none.
    #[test]
    fn a_groups_settings_are_against_what_it_starts_as() {
        use camcraft_core::job::Tabs;
        let def = Operation::new("cut", StrategySpec::named("profile").unwrap());
        let mut op = def.clone();
        op.passes.tabs = Some(Tabs::default());
        let mut started = def.clone();
        started.passes.tabs = Some(Tabs::default());

        let d = Defaults::of(&op, &def).started(&started);
        assert!(d.group_changed("passes.tabs"));
        assert!(!d.changed("passes.tabs.width"), "as ticking starts it");

        if let Some(t) = &mut op.passes.tabs {
            t.width = 9.0;
        }
        let mut d = Defaults::of(&op, &def).started(&started);
        assert!(d.changed("passes.tabs.width"));
        d.push("passes.tabs.width", Layer::Started);
        d.apply(&mut op);
        assert_eq!(
            op.passes.tabs,
            Some(Tabs::default()),
            "the width back, the tabs kept"
        );

        let mut d = Defaults::of(&op, &def).started(&started);
        d.push("passes.tabs", Layer::Default);
        d.apply(&mut op);
        assert_eq!(op.passes.tabs, None);
    }

    /// A place in a list is a setting of its own: changed on its own, and
    /// put back on its own.
    #[test]
    fn a_place_in_a_list_goes_back() {
        use camcraft_core::job::{FixedBox, StockSpec};
        let def = StockSpec::FixedBox(FixedBox::centred([100.0, 60.0, 20.0]));
        let mut spec = StockSpec::FixedBox(FixedBox::centred([120.0, 60.0, 25.0]));
        let mut d = Defaults::of(&spec, &def);
        assert!(d.changed("size.0"));
        assert!(!d.changed("size.1"));
        d.push("size.0", Layer::Started);
        d.apply(&mut spec);
        assert_eq!(
            spec,
            StockSpec::FixedBox(FixedBox::centred([100.0, 60.0, 25.0]))
        );
    }

    /// A strategy's own setting, written beside the strategy's name, is one
    /// like any other.
    #[test]
    fn a_strategys_own_setting_goes_back() {
        let def = Operation::new("finish", StrategySpec::named("scallop").unwrap());
        let mut op = def.clone();
        if let StrategySpec::Scallop { slope_to, .. } = &mut op.strategy {
            *slope_to = 60.0;
        }
        let mut d = Defaults::of(&op, &def);
        assert!(d.changed("slope_to"), "{:?}", d.now);
        d.push("slope_to", Layer::Started);
        d.apply(&mut op);
        assert_eq!(op.strategy, def.strategy);
    }
}
