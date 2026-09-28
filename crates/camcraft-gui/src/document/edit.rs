//! Editing a job: its models, stocks, setups, operations and tools, each change
//! carrying every reference to what it renames, moves or removes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use camcraft_core::job::{
    Boundary, Containment, LengthUnit, ModelDef, Operation, Origin, Passes, Preset, Setup,
    StockDef, StockSpec, StrategySpec, ToolSpec,
};
use camcraft_geom::bvh::MeshBvh;
use camcraft_session::fs::relative_path;
use camcraft_session::{Model, SessionError};

use super::{
    Document, Level, ModelKey, ModelSlot, Selection, checked_name, free, moved_index, rename_in,
    shift,
};

/// How a wanted model is to be had.
#[derive(Debug, Clone)]
pub enum Wanted {
    /// Read from its file.
    Read,
    /// Placed again: this, loaded from the same file, placed elsewhere.
    Place(Arc<Model>),
}

impl Document {
    // ---- models ---------------------------------------------------------------

    /// Models that should be loaded and are not, or are loaded from something that
    /// has since changed: each with what to load it as, and how. One being placed
    /// waits for that to finish, so a model dragged about is placed as often as it
    /// can be and no more.
    pub fn wanted_loads(&self) -> Vec<(ModelDef, ModelKey, Wanted)> {
        let dir = self.dir();
        self.job
            .models
            .iter()
            .filter_map(|def| {
                let key = ModelKey::of(&dir, def, self.job.tolerance);
                let current = self.models.get(&def.name);
                // An STL waits for its units rather than failing without them.
                let stl = Path::new(&def.path)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("stl"));
                if stl && def.units.is_none() {
                    return None;
                }
                if self.placing.contains_key(&def.name) || current.map(ModelSlot::key) == Some(&key)
                {
                    return None;
                }
                let how = match current {
                    Some(ModelSlot::Ready(k, model, _)) if k.same_file(&key) => {
                        Wanted::Place(model.clone())
                    }
                    _ => Wanted::Read,
                };
                Some((def.clone(), key, how))
            })
            .collect()
    }

    /// A model has started loading.
    pub fn loading(&mut self, name: &str, key: ModelKey) {
        self.models
            .insert(name.to_string(), ModelSlot::Loading(key));
    }

    /// A loaded model has started being placed again, and is shown where it was
    /// until it is.
    pub fn placing(&mut self, name: &str, key: ModelKey) {
        self.placing.insert(name.to_string(), key);
    }

    /// A model has loaded, or failed to. A reply for something since asked for
    /// differently is dropped.
    pub fn loaded(
        &mut self,
        name: &str,
        key: &ModelKey,
        result: Result<(Arc<Model>, Arc<MeshBvh>), SessionError>,
    ) {
        let placed = self.placing.get(name) == Some(key);
        if placed {
            self.placing.remove(name);
        }
        match self.models.get(name) {
            Some(ModelSlot::Loading(k)) if k == key => {}
            Some(ModelSlot::Ready(..)) if placed => {}
            _ => return,
        }
        let slot = match result {
            Ok((model, bvh)) => {
                // Placed again, it says nothing new of its file.
                if !placed {
                    for n in &model.notes {
                        self.log.push((Level::Info, format!("{name}: {n}")));
                    }
                    for w in &model.warnings {
                        self.log.push((Level::Warn, format!("{name}: {w}")));
                    }
                }
                ModelSlot::Ready(key.clone(), model, bvh)
            }
            Err(e) => {
                let missing = matches!(e, SessionError::ModelMissing { .. });
                let why = crate::app::explain(&e);
                self.note(Level::Error, why.clone());
                ModelSlot::Failed {
                    key: key.clone(),
                    why,
                    missing,
                }
            }
        };
        self.models.insert(name.to_string(), slot);
    }

    /// The names of models whose files are not where the job says.
    pub fn missing_models(&self) -> Vec<String> {
        self.models
            .iter()
            .filter(|(_, s)| matches!(s, ModelSlot::Failed { missing: true, .. }))
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// How the job names `file`: relative to it when it has been saved, whole when
    /// it has not.
    fn job_path_for(&self, file: &Path) -> String {
        match &self.path {
            Some(_) => relative_path(&self.dir(), file),
            None => file.display().to_string(),
        }
    }

    /// Add model files, each under a name from its file name. A setup that has no
    /// models yet takes them. Returns the names given.
    pub fn add_models(&mut self, files: &[PathBuf]) -> Vec<String> {
        let mut names = Vec::new();
        for file in files {
            let base = file
                .file_stem()
                .map_or_else(|| "model".into(), |s| s.to_string_lossy().into_owned());
            let name = free(&base, self.job.models.iter().map(|m| m.name.as_str()));
            let stl = file
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("stl"));
            self.job.models.push(ModelDef {
                name: name.clone(),
                path: self.job_path_for(file),
                // An STL says nothing of its units; millimetres is a start the
                // person sees, and can change, before anything is cut.
                units: stl.then_some(LengthUnit::Mm),
                transform: Default::default(),
            });
            names.push(name);
        }
        for setup in &mut self.job.setups {
            if setup.models.is_empty() {
                setup.models.clone_from(&names);
            }
        }
        if let Some(last) = names.last() {
            self.selection = Selection::Model(last.clone());
        }
        names
    }

    /// Have setup `setup` machine `names` too, and show it: models added from its
    /// panel are added to it.
    pub fn include_models(&mut self, setup: &str, names: &[String]) {
        let Some(s) = self.setup_mut(setup) else {
            return;
        };
        for n in names {
            if !s.models.contains(n) {
                s.models.push(n.clone());
            }
        }
        self.selection = Selection::Setup(setup.to_string());
    }

    /// Point model `name` at `file`, as when it has been found elsewhere.
    pub fn locate_model(&mut self, name: &str, file: &Path) {
        let path = self.job_path_for(file);
        if let Some(m) = self.job.models.iter_mut().find(|m| m.name == name) {
            m.path = path;
        }
    }

    /// Rename a model, and every reference to it.
    pub fn rename_model(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = checked_name(to, "model", self.job.models.iter().map(|m| &*m.name), from)?;
        if to == from {
            return Ok(());
        }
        let Some(m) = self.job.models.iter_mut().find(|m| m.name == from) else {
            return Err(format!("there is no model '{from}'"));
        };
        m.name.clone_from(&to);
        for s in &mut self.job.setups {
            rename_in(&mut s.models, from, &to);
        }
        for s in &mut self.job.stocks {
            if let StockSpec::AroundModels { models, .. } = &mut s.spec {
                rename_in(models, from, &to);
            }
        }
        self.job.rename_model_refs(from, &to);
        if let Some(slot) = self.models.remove(from) {
            self.models.insert(to.clone(), slot);
        }
        if self.selection == Selection::Model(from.to_string()) {
            self.selection = Selection::Model(to);
        }
        Ok(())
    }

    /// Copy a model, as another of the same part: the same file, turned and scaled
    /// the same, beside it along X by its width and a gap, rounded up to a whole
    /// millimetre, so it is seen as
    /// another. Every setup and stock that names it names the copy too, so it is
    /// machined with it. Its picks stay the original's. Loaded, the copy starts
    /// from the one loaded rather than reading the file again.
    pub fn duplicate_model(&mut self, name: &str) -> Option<String> {
        /// Between the original and its copy, millimetres.
        const GAP: f64 = 10.0;
        let i = self.job.models.iter().position(|m| m.name == name)?;
        let mut copy = self.job.models.get(i)?.clone();
        copy.name = free(
            &format!("{name} copy"),
            self.job.models.iter().map(|m| m.name.as_str()),
        );
        let new = copy.name.clone();
        let slot = self.models.get(name).cloned();
        if let Some(ModelSlot::Ready(_, model, _)) = &slot {
            // In whole millimetres, so the copy's position reads as typed.
            copy.transform.position[0] += (model.bounds.extent().x + GAP).ceil();
        }
        self.job.models.insert(i + 1, copy);
        for s in &mut self.job.setups {
            if s.models.iter().any(|m| m == name) {
                s.models.push(new.clone());
            }
        }
        for s in &mut self.job.stocks {
            let models = match &mut s.spec {
                StockSpec::AroundModels { models, .. } => models,
                StockSpec::FixedBox(b) => &mut b.models,
            };
            if models.iter().any(|m| m == name) {
                models.push(new.clone());
            }
        }
        // Where the original is: the copy is placed again from it, not read.
        if let Some(slot @ ModelSlot::Ready(..)) = slot {
            self.models.insert(new.clone(), slot);
        }
        self.selection = Selection::Model(new.clone());
        Some(new)
    }

    /// Remove a model nothing uses.
    pub fn remove_model(&mut self, name: &str) -> Result<(), String> {
        let users: Vec<String> = self
            .job
            .setups
            .iter()
            .filter(|s| s.models.iter().any(|m| m == name))
            .map(|s| format!("setup '{}'", s.name))
            .chain(
                self.job
                    .stocks
                    .iter()
                    .filter(|s| {
                        matches!(&s.spec, StockSpec::AroundModels { models, .. }
                            if models.iter().any(|m| m == name))
                    })
                    .map(|s| format!("stock '{}'", s.name)),
            )
            .collect();
        if !users.is_empty() {
            return Err(format!("'{name}' is used by {}", users.join(", ")));
        }
        self.job.models.retain(|m| m.name != name);
        self.models.remove(name);
        self.selection = Selection::Models;
        Ok(())
    }

    // ---- stocks ---------------------------------------------------------------

    /// Add a stock around the models of whichever setup uses it.
    pub fn add_stock(&mut self) -> String {
        let name = free("Stock", self.job.stocks.iter().map(|s| s.name.as_str()));
        self.job.stocks.push(StockDef {
            name: name.clone(),
            spec: StockSpec::default(),
        });
        self.selection = Selection::Stock(name.clone());
        name
    }

    /// A copy of stock `name`, after it, and shown. No setup uses it yet.
    pub fn duplicate_stock(&mut self, name: &str) -> Option<String> {
        let i = self.job.stocks.iter().position(|s| s.name == name)?;
        let mut copy = self.job.stocks.get(i)?.clone();
        copy.name = free(
            &format!("{name} copy"),
            self.job.stocks.iter().map(|s| s.name.as_str()),
        );
        let new = copy.name.clone();
        self.job.stocks.insert(i + 1, copy);
        self.selection = Selection::Stock(new.clone());
        Some(new)
    }

    /// Rename a stock, and every setup's reference to it.
    pub fn rename_stock(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = checked_name(to, "stock", self.job.stocks.iter().map(|s| &*s.name), from)?;
        if to == from {
            return Ok(());
        }
        let Some(s) = self.job.stocks.iter_mut().find(|s| s.name == from) else {
            return Err(format!("there is no stock '{from}'"));
        };
        s.name.clone_from(&to);
        for s in &mut self.job.setups {
            rename_in(&mut s.stocks, from, &to);
        }
        if self.selection == Selection::Stock(from.to_string()) {
            self.selection = Selection::Stock(to);
        }
        Ok(())
    }

    /// Remove a stock no setup uses.
    pub fn remove_stock(&mut self, name: &str) -> Result<(), String> {
        let users: Vec<&str> = self
            .job
            .setups
            .iter()
            .filter(|s| s.stocks.iter().any(|m| m == name))
            .map(|s| s.name.as_str())
            .collect();
        if !users.is_empty() {
            return Err(format!(
                "'{name}' is used by setup '{}'",
                users.join("', '")
            ));
        }
        self.job.stocks.retain(|s| s.name != name);
        self.selection = Selection::Stocks;
        Ok(())
    }

    // ---- setups ---------------------------------------------------------------

    /// Where setup `name` is in the job.
    pub fn setup_index(&self, name: &str) -> Option<usize> {
        self.job.setups.iter().position(|s| s.name == name)
    }

    /// Setup `name`.
    pub fn setup(&self, name: &str) -> Option<&Setup> {
        self.job.setups.iter().find(|s| s.name == name)
    }

    /// Setup `name`, to change.
    pub fn setup_mut(&mut self, name: &str) -> Option<&mut Setup> {
        self.job.setups.iter_mut().find(|s| s.name == name)
    }

    /// Operation `index` of setup `setup`.
    pub fn operation(&self, setup: &str, index: usize) -> Option<&Operation> {
        self.setup(setup)?.operations.get(index)
    }

    /// Operation `index` of setup `setup`, to change.
    pub fn operation_mut(&mut self, setup: &str, index: usize) -> Option<&mut Operation> {
        self.setup_mut(setup)?.operations.get_mut(index)
    }

    /// Add a setup machining every model, from the first stock.
    pub fn add_setup(&mut self) -> String {
        let name = free("Setup", self.job.setups.iter().map(|s| s.name.as_str()));
        let mut setup = Setup::new(name.clone());
        setup.origin = Origin::stock_corner();
        setup.models = self.job.models.iter().map(|m| m.name.clone()).collect();
        setup.stocks = self
            .job
            .stocks
            .first()
            .map(|s| vec![s.name.clone()])
            .unwrap_or_default();
        self.job.setups.push(setup);
        self.show(&name);
        name
    }

    /// A copy of setup `name`, after it.
    pub fn duplicate_setup(&mut self, name: &str) -> Option<String> {
        let i = self.setup_index(name)?;
        let mut copy = self.job.setups.get(i)?.clone();
        copy.name = free(
            &format!("{name} copy"),
            self.job.setups.iter().map(|s| s.name.as_str()),
        );
        let new = copy.name.clone();
        self.job.setups.insert(i + 1, copy);
        self.show(&new);
        Some(new)
    }

    /// Rename a setup, carrying its state with it.
    pub fn rename_setup(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = checked_name(to, "setup", self.job.setups.iter().map(|s| &*s.name), from)?;
        if to == from {
            return Ok(());
        }
        let Some(s) = self.setup_mut(from) else {
            return Err(format!("there is no setup '{from}'"));
        };
        s.name.clone_from(&to);
        if let Some(state) = self.setups.remove(from) {
            self.setups.insert(to.clone(), state);
        }
        if self.active.as_deref() == Some(from) {
            self.active = Some(to.clone());
        }
        self.selection = match &self.selection {
            Selection::Setup(s) if s == from => Selection::Setup(to),
            Selection::Operation(s, i) if s == from => Selection::Operation(to, *i),
            other => other.clone(),
        };
        Ok(())
    }

    /// Remove a setup and what it ran.
    pub fn remove_setup(&mut self, name: &str) {
        self.job.setups.retain(|s| s.name != name);
        self.setups.remove(name);
        if self.active.as_deref() == Some(name) {
            self.active = self.job.setups.first().map(|s| s.name.clone());
        }
        self.selection = Selection::Setups;
    }

    /// Move a setup up (`-1`) or down (`1`).
    pub fn move_setup(&mut self, name: &str, by: isize) {
        if let Some(i) = self.setup_index(name) {
            shift(&mut self.job.setups, i, by);
        }
    }

    /// Show setup `name` in the view, and select it.
    pub fn show(&mut self, name: &str) {
        self.active = Some(name.to_string());
        self.selection = Selection::Setup(name.to_string());
    }

    // ---- operations -----------------------------------------------------------

    /// Add an operation of `strategy` at the end of setup `setup`, with the tool
    /// its last operation used.
    pub fn add_operation(&mut self, setup: &str, strategy: StrategySpec) -> Option<usize> {
        let s = self.job.setups.iter_mut().find(|s| s.name == setup)?;
        let base = match strategy {
            StrategySpec::Parallel { .. } => "finish",
            StrategySpec::ZlevelRough { .. } => "rough",
            StrategySpec::Profile { .. } => "profile",
            StrategySpec::Pocket { .. } => "pocket",
            StrategySpec::Face { .. } => "face",
            StrategySpec::Drill { .. } => "drill",
        };
        let name = free(base, s.operations.iter().map(|o| o.name.as_str()));
        let tool = s.operations.last().and_then(|o| o.tool.clone());
        let preset = tool
            .as_ref()
            .and_then(|t| self.job.tools.get(t))
            .and_then(|t| t.presets.first());
        let mut op = Operation {
            name,
            strategy,
            tool,
            preset: None,
            selection: Default::default(),
            boundary: Default::default(),
            outline: Default::default(),
            heights: Default::default(),
            passes: Passes {
                stepdown: strategy.default_stepdown(),
                stock_to_leave: strategy.default_stock_to_leave(),
                ..Default::default()
            },
            linking: Default::default(),
            enabled: true,
        };
        if let Some(p) = preset {
            op.apply_preset(p);
        }
        let spec = op.tool.as_ref().and_then(|t| self.job.tools.get(t));
        fit_stepover(&mut op, spec, preset, self.job.tolerance);
        start_on_silhouette(&mut op);
        fit_channel(&mut op, spec);
        let s = self.setup_mut(setup)?;
        s.operations.push(op);
        let i = s.operations.len() - 1;
        self.active = Some(setup.to_string());
        self.selection = Selection::Operation(setup.to_string(), i);
        Some(i)
    }

    /// A copy of an operation, after it.
    pub fn duplicate_operation(&mut self, setup: &str, index: usize) {
        let Some(s) = self.setup_mut(setup) else {
            return;
        };
        let Some(mut op) = s.operations.get(index).cloned() else {
            return;
        };
        op.name = free(
            &format!("{} copy", op.name),
            s.operations.iter().map(|o| o.name.as_str()),
        );
        s.operations.insert(index + 1, op);
        self.selection = Selection::Operation(setup.to_string(), index + 1);
    }

    /// Remove an operation.
    pub fn remove_operation(&mut self, setup: &str, index: usize) {
        if let Some(s) = self.setup_mut(setup)
            && index < s.operations.len()
        {
            s.operations.remove(index);
        }
        self.selection = Selection::Setup(setup.to_string());
    }

    /// Move operation `from` of `setup` so it is at `to`, the others closing up
    /// behind it and making room; the selection stays on the operation it was on.
    pub fn reorder_operation(&mut self, setup: &str, from: usize, to: usize) {
        let Some(s) = self.setup_mut(setup) else {
            return;
        };
        let n = s.operations.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let op = s.operations.remove(from);
        s.operations.insert(to, op);
        if let Selection::Operation(sel, k) = &mut self.selection
            && sel == setup
        {
            *k = moved_index(*k, from, to);
        }
    }

    /// Move operation `from` of setup `from_setup` to place `to` of setup
    /// `to_setup`: within one setup, as [`Document::reorder_operation`]; into
    /// another, under a name free there, and shown there. It keeps its tool, its
    /// heights and its picks, which find what they can among the new setup's
    /// models.
    pub fn move_operation_to(&mut self, from_setup: &str, from: usize, to_setup: &str, to: usize) {
        if from_setup == to_setup {
            let last = self
                .setup(from_setup)
                .map_or(0, |s| s.operations.len().saturating_sub(1));
            self.reorder_operation(from_setup, from, to.min(last));
            return;
        }
        let (Some(si), Some(ti)) = (self.setup_index(from_setup), self.setup_index(to_setup))
        else {
            return;
        };
        let Some(source) = self.job.setups.get_mut(si) else {
            return;
        };
        if from >= source.operations.len() {
            return;
        }
        let mut op = source.operations.remove(from);
        let Some(target) = self.job.setups.get_mut(ti) else {
            return;
        };
        op.name = free(&op.name, target.operations.iter().map(|o| o.name.as_str()));
        let to = to.min(target.operations.len());
        target.operations.insert(to, op);
        self.show(to_setup);
        self.selection = Selection::Operation(to_setup.to_string(), to);
    }

    /// Move an operation up (`-1`) or down (`1`) within its setup.
    pub fn move_operation(&mut self, setup: &str, index: usize, by: isize) {
        if let Some(s) = self.setup_mut(setup)
            && let Some(to) = shift(&mut s.operations, index, by)
        {
            self.selection = Selection::Operation(setup.to_string(), to);
        }
    }

    // ---- tools ----------------------------------------------------------------

    /// Use `spec`, from the library as `hint`, for an operation: the job's own copy
    /// if it already has an identical one, otherwise a new copy under a free name.
    /// Returns the name in the job.
    pub fn adopt_tool(&mut self, hint: &str, spec: &ToolSpec) -> String {
        if let Some((name, _)) = self.job.tools.iter().find(|(_, t)| *t == spec) {
            return name.clone();
        }
        let name = camcraft_session::library::free_name(hint, &self.job.tools);
        self.job.tools.insert(name.clone(), spec.clone());
        name
    }

    /// Give operation `op` of `setup` the job's tool `tool`, and copy in the values
    /// of its preset `preset`. Without one, the operation keeps its own values and
    /// no longer names a preset.
    pub fn use_tool(&mut self, setup: &str, op: usize, tool: String, preset: Option<&str>) {
        let spec = self.job.tools.get(&tool).cloned();
        let preset = preset.and_then(|p| spec.as_ref()?.preset(p).cloned());
        let tolerance = self.job.tolerance;
        let Some(o) = self.operation_mut(setup, op) else {
            return;
        };
        let first = o.tool.is_none();
        o.tool = Some(tool);
        match &preset {
            Some(p) => o.apply_preset(p),
            None => o.preset = None,
        }
        fit_stepover(o, spec.as_ref(), preset.as_ref(), tolerance);
        // The channel is made once, for the first tool: an offset of zero after
        // that is one chosen, and a new tool keeps it.
        if first {
            fit_channel(o, spec.as_ref());
        }
    }

    /// Another tool the job's operations use with the same number as tool `name`,
    /// when `name` is used too: the machine would load one and cut with the other.
    pub fn number_clash(&self, name: &str) -> Option<String> {
        let number = self.job.tools.get(name)?.number;
        let used = self.used_tools();
        if !used.iter().any(|n| n == name) {
            return None;
        }
        used.into_iter()
            .find(|n| n != name && self.job.tools.get(n).is_some_and(|t| t.number == number))
    }

    /// A tool number no tool in the job has.
    pub fn free_number(&self) -> u32 {
        self.job.tools.values().map(|t| t.number).max().unwrap_or(0) + 1
    }

    /// The names of the tools the job's operations use.
    pub fn used_tools(&self) -> Vec<String> {
        let mut used: Vec<String> = self
            .job
            .setups
            .iter()
            .flat_map(|s| &s.operations)
            .filter_map(|o| o.tool.clone())
            .collect();
        used.sort();
        used.dedup();
        used
    }
}

/// Start a new 3D operation on the part's silhouette, as Fusion does. Parallel
/// finishing keeps the tool's centre inside it. Z-level roughing reaches in from
/// outside: kept inside the part's outline, the tool never reaches its walls, as
/// its centre sits a radius outside them ([`fit_channel`] makes room round them).
/// A 2D operation's outline starts as the silhouette already. Facing and
/// drilling have no boundary.
fn start_on_silhouette(op: &mut Operation) {
    let containment = match op.strategy {
        StrategySpec::Parallel { .. } => Containment::Center,
        StrategySpec::ZlevelRough { .. } => Containment::Outside,
        StrategySpec::Profile { .. }
        | StrategySpec::Pocket { .. }
        | StrategySpec::Face { .. }
        | StrategySpec::Drill { .. } => {
            return;
        }
    };
    op.boundary.kind = Boundary::Silhouette;
    op.boundary.containment = containment;
}

/// Room for `tool` round the part, as the Geometry tab gives it: a boundary round
/// the part that the tool reaches in from outside, with no offset yet, is offset
/// by the tool's diameter, a channel two tools wide. Set once, as a number, so
/// changing the tool later does not move it.
fn fit_channel(op: &mut Operation, tool: Option<&ToolSpec>) {
    // Only a 3D boundary has an additional offset to make room with.
    if !op.strategy.uses().boundary {
        return;
    }
    let b = &mut op.boundary;
    let around_part = matches!(b.kind, Boundary::BoundingBox | Boundary::Silhouette);
    if around_part
        && b.containment == Containment::Outside
        && b.margin == 0.0
        && let Some(t) = tool
    {
        b.margin = t.diameter;
    }
}

/// Give `op` the stepover Fusion gives its strategy with `tool`, unless `preset`
/// set one: a fixed millimetre suits a 1/32" cutter and starves a 1/4" one.
fn fit_stepover(
    op: &mut Operation,
    tool: Option<&ToolSpec>,
    preset: Option<&Preset>,
    tolerance: f64,
) {
    if preset.is_some_and(|p| p.stepover.is_some()) {
        return;
    }
    if let Some(over) = tool.and_then(|t| op.default_stepover(t, tolerance)) {
        op.passes.stepover = over;
    }
}
