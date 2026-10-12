//! What the manager's edits do to the library, drawn or not: saving it, bringing
//! a Fusion library in, and making, copying, moving and removing tools and
//! folders, the System library's kept as they are.

use std::path::Path;

use camcraft_core::job::ToolSpec;
use camcraft_session::library::system::is_system;
use camcraft_session::library::{Folder, FolderPath, Library, folder_label, free_name, fusion_csv};

use super::{LOCKED, ToolManager};

/// The library: as saved, and as edited.
#[derive(Debug, Clone, Default)]
pub(super) struct Set {
    pub(super) original: Folder,
    pub(super) edited: Folder,
    /// An edit has landed since saving was last tried: only then is the library
    /// looked through for what changed, not every frame.
    touched: bool,
}

impl Set {
    pub(super) fn new(root: Folder) -> Self {
        Self {
            original: root.clone(),
            edited: root,
            touched: false,
        }
    }

    /// The library as edited, to change: it is saved once the change is done.
    pub(super) fn edit(&mut self) -> &mut Folder {
        self.touched = true;
        &mut self.edited
    }

    pub(super) fn dirty(&self) -> bool {
        self.edited != self.original
    }

    pub(super) fn revert(&mut self) {
        self.edited = self.original.clone();
        // Looked at again, so why it was not saved goes with the edits.
        self.touched = true;
    }

    fn saved(&mut self) {
        self.original = self.edited.clone();
    }

    /// Rename a tool within its folder.
    pub(super) fn rename(&mut self, folder: &[String], from: &str, to: &str) -> Result<(), String> {
        let to = to.trim();
        if to.is_empty() {
            return Err("a tool needs a name".into());
        }
        if to == from {
            return Ok(());
        }
        let tools = &mut self
            .edit()
            .at_mut(folder)
            .ok_or_else(|| format!("there is no folder {}", folder_label(folder)))?
            .tools;
        if tools.contains_key(to) {
            return Err(format!("there is already a tool called '{to}' here"));
        }
        let tool = tools
            .remove(from)
            .ok_or_else(|| format!("no tool called '{from}'"))?;
        tools.insert(to.to_string(), tool);
        Ok(())
    }

    /// A tool number none of the person's own tools uses yet. The System
    /// library's numbers are its own, and do not push theirs up.
    pub(super) fn next_number(&self) -> u32 {
        self.edited
            .walk()
            .iter()
            .filter(|(f, _, _)| !is_system(f))
            .map(|(_, _, t)| t.number)
            .max()
            .unwrap_or(0)
            + 1
    }
}

impl ToolManager {
    /// Save the library if an edit has landed since it was last saved, or tried,
    /// and the edits can be kept. The manager does this itself as edits finish;
    /// the app calls it too, on the way out.
    pub fn save_library(&mut self) {
        if !std::mem::take(&mut self.library.touched) {
            return;
        }
        if !self.library.dirty() {
            self.library_unsaved = None;
            return;
        }
        let why = if self.library_error.is_some() {
            // Writing now would replace the file that failed to load with only
            // what has been added since.
            Some("the library file could not be read, so it is not written over".into())
        } else if let Some(path) = self.library_path.clone() {
            let lib = Library {
                root: self.library.edited.clone(),
            };
            lib.save(&path).err().map(|e| e.to_string())
        } else {
            Some("there is nowhere to keep the library".into())
        };
        if why.is_none() {
            self.library.saved();
        }
        self.library_unsaved = why;
    }

    /// Whether anything is unsaved.
    pub fn dirty(&self) -> bool {
        self.library.dirty()
    }

    /// Bring the tools in the Fusion tool library CSV at `path` into the folder
    /// showing. A tool already there gets the file's presets beside its own.
    pub fn import_fusion(&mut self, path: &Path) {
        if is_system(&self.folder) {
            self.message = Some((false, LOCKED.into()));
            return;
        }
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.message = Some((false, format!("{}: {e}", path.display())));
                return;
            }
        };
        let imported = match fusion_csv::import(&text) {
            Ok(i) => i,
            Err(e) => {
                self.message = Some((false, format!("{}: {e}", path.display())));
                return;
            }
        };
        let Some(folder) = self.library.edit().at_mut(&self.folder) else {
            return;
        };
        let merged = fusion_csv::merge(folder, imported.tools);
        let mut said = Vec::new();
        if !merged.added.is_empty() {
            said.push(format!("added {}", merged.added.join(", ")));
        }
        for (name, n) in &merged.extended {
            said.push(match n {
                0 => format!("'{name}' already has these presets"),
                1 => format!("added 1 preset to '{name}'"),
                n => format!("added {n} presets to '{name}'"),
            });
        }
        if !imported.skipped.is_empty() {
            said.push(format!(
                "skipped {}: {}",
                imported.skipped.len(),
                imported.skipped.join("; ")
            ));
        }
        if said.is_empty() {
            said.push("no tools in it".into());
        }
        let file = path.file_name().map_or_else(
            || path.display().to_string(),
            |f| f.to_string_lossy().into_owned(),
        );
        self.message = Some((
            imported.skipped.is_empty(),
            format!("{file}: {}", said.join("; ")),
        ));
        let first = merged
            .added
            .first()
            .or_else(|| merged.extended.first().map(|(n, _)| n))
            .cloned();
        if let Some(name) = first {
            self.select(Some((self.folder.clone(), name)));
        }
    }

    pub(super) fn new_folder(&mut self, parent: &[String]) {
        let taken: Vec<String> = self
            .library
            .edited
            .at(parent)
            .map(|f| f.folders.keys().cloned().collect())
            .unwrap_or_default();
        let name = (1..)
            .map(|n| {
                if n == 1 {
                    "New folder".to_string()
                } else {
                    format!("New folder {n}")
                }
            })
            .find(|n| !taken.contains(n))
            .unwrap_or_default();
        match self.library.edit().create(parent, &name) {
            Ok(path) => {
                self.folder = path.clone();
                self.own_folder = path.clone();
                self.select(None);
                self.renaming = Some((path, name));
            }
            Err(e) => self.message = Some((false, e.to_string())),
        }
    }

    pub(super) fn delete_folder(&mut self, path: &[String]) {
        match self.library.edit().remove(path) {
            Ok(_) => {
                if self.folder.starts_with(path) {
                    self.folder = path
                        .split_last()
                        .map(|(_, p)| p.to_vec())
                        .unwrap_or_default();
                }
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|(f, _)| f.starts_with(path))
                {
                    self.select(None);
                }
            }
            Err(e) => self.message = Some((false, e.to_string())),
        }
    }

    /// Move a tool to folder `to`; a System library bit is copied there instead.
    pub(super) fn move_tool(&mut self, from: &[String], name: &str, to: &[String]) {
        if from == to {
            return;
        }
        if is_system(to) {
            self.message = Some((false, LOCKED.into()));
            return;
        }
        if is_system(from) {
            self.copy_tool(from, name, to);
            return;
        }
        match self.library.edit().move_tool(from, name, to) {
            Ok(as_name) => {
                let was = self
                    .selected
                    .as_ref()
                    .is_some_and(|(f, n)| f.as_slice() == from && n == name);
                if was {
                    self.select(Some((to.to_vec(), as_name.clone())));
                }
                self.message = Some((
                    true,
                    format!("moved '{name}' to {} as '{as_name}'", folder_label(to)),
                ));
            }
            Err(e) => self.message = Some((false, e.to_string())),
        }
    }

    pub(super) fn new_tool(&mut self) {
        if is_system(&self.folder) {
            self.message = Some((false, LOCKED.into()));
            return;
        }
        let number = self.library.next_number();
        let Some(here) = self.library.edit().at_mut(&self.folder) else {
            return;
        };
        let name = free_name("tool", &here.tools);
        here.tools.insert(
            name.clone(),
            ToolSpec {
                number,
                ..ToolSpec::default()
            },
        );
        self.select(Some((self.folder.clone(), name)));
    }

    /// A copy of a tool beside it; a System library bit's copy goes to the
    /// person's own folder instead.
    pub(super) fn duplicate(&mut self, folder: &[String], name: &str) {
        if is_system(folder) {
            let to = self.own_folder();
            self.copy_tool(folder, name, &to);
            return;
        }
        let number = self.library.next_number();
        let Some(here) = self.library.edit().at_mut(folder) else {
            return;
        };
        let Some(tool) = here.tools.get(name).cloned() else {
            return;
        };
        let copy = free_name(&format!("{name}-copy"), &here.tools);
        here.tools.insert(copy.clone(), ToolSpec { number, ..tool });
        self.select(Some((folder.to_vec(), copy)));
    }

    /// The folder a System library bit is copied to: the last of the person's own
    /// shown, while it is still there, or else the top of the library.
    pub(super) fn own_folder(&self) -> FolderPath {
        if !is_system(&self.own_folder) && self.library.edited.at(&self.own_folder).is_some() {
            self.own_folder.clone()
        } else {
            Vec::new()
        }
    }

    /// Copy tool `name` of `from` into folder `to` under a free name and the next
    /// tool number, and show it there.
    fn copy_tool(&mut self, from: &[String], name: &str, to: &[String]) {
        let Some(tool) = self
            .library
            .edited
            .at(from)
            .and_then(|f| f.tools.get(name))
            .cloned()
        else {
            return;
        };
        let number = self.library.next_number();
        let Some(there) = self.library.edit().at_mut(to) else {
            return;
        };
        let as_name = free_name(name, &there.tools);
        there
            .tools
            .insert(as_name.clone(), ToolSpec { number, ..tool });
        let said = if as_name == name {
            format!("copied '{name}' to {} as T{number}", folder_label(to))
        } else {
            format!(
                "copied '{name}' to {} as '{as_name}', T{number}",
                folder_label(to)
            )
        };
        self.message = Some((true, said));
        self.folder = to.to_vec();
        self.own_folder = to.to_vec();
        self.select(Some((to.to_vec(), as_name)));
    }

    pub(super) fn delete(&mut self, folder: &[String], name: &str) {
        if is_system(folder) {
            self.message = Some((false, LOCKED.into()));
            return;
        }
        if let Some(here) = self.library.edit().at_mut(folder) {
            here.tools.remove(name);
        }
        self.select(None);
    }
}

/// Two machines' folders of tools, for pictures and tests.
#[cfg(test)]
pub(super) fn fill_for_snapshot(root: &mut Folder) {
    use camcraft_core::job::ToolKind;
    let tool = |kind, diameter: f64, number, comment: &str| ToolSpec {
        kind,
        diameter,
        number,
        comment: comment.into(),
        flutes: Some(2),
        stickout: Some(diameter * 4.0),
        ..ToolSpec::default()
    };
    for (machine, first) in [("machine_01", 1), ("machine_02", 11)] {
        let m = root.create(&[], machine).unwrap_or_default();
        let flat = root.create(&m, "flat").unwrap_or_default();
        let ball = root.create(&m, "ball").unwrap_or_default();
        if let Some(f) = root.at_mut(&flat) {
            f.tools.insert(
                "flat-6".into(),
                tool(ToolKind::Flat, 6.0, first, "6mm flat"),
            );
            f.tools.insert(
                "flat-3".into(),
                tool(ToolKind::Flat, 3.0, first + 1, "3mm flat"),
            );
        }
        if let Some(f) = root.at_mut(&ball) {
            f.tools.insert(
                "ball-6".into(),
                tool(ToolKind::Ball, 6.0, first + 2, "6mm ball"),
            );
        }
    }
}
