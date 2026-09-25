//! The app's one file dialog.
//!
//! `egui-file-dialog`, as in nodemangler: drawn inside egui, so it follows the
//! theme and never blocks the frame loop. The cost is that a dialog is not a
//! function that returns a path. A click can only *start* one; the request is
//! parked in the dialog as user data, and a later frame's [`AppFileDialog::update`]
//! hands the path back with the request it answers.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui_file_dialog::{FileDialog, FileDialogStorage, Filter};

use crate::worker::DocId;

const JOBS: &str = "Jobs (.camcraft)";
const MODELS: &str = "Models (STEP, STL)";
const PROFILES: &str = "Machine profiles (TOML)";
const TOOLS: &str = "Fusion tool libraries (CSV)";

/// What the dialog is opened for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileRequest {
    /// Job files to open, each in its own tab.
    OpenJobs,
    /// Where to save job `doc`.
    SaveJob(DocId),
    /// Model files to add to job `doc`, and to setup `setup` when one is named.
    AddModels(DocId, Option<String>),
    /// Where model `name` of job `doc` is now.
    Locate(DocId, String),
    /// A machine profile file for setup `setup` of job `doc`.
    Profile(DocId, String),
    /// The folder to post job `doc`'s programs into: one setup's, or every setup's.
    PostTo(DocId, Option<String>),
    /// Fusion tool library exports to bring into the tool library.
    ImportTools,
}

/// The dialog and whatever it is currently answering.
pub struct AppFileDialog {
    dialog: FileDialog,
}

/// Whether `path` has one of `extensions`, ignoring case: Fusion writes `.STEP`,
/// and the dialog's own extension filter is case-sensitive.
fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

impl AppFileDialog {
    /// A dialog, remembering where the last one was.
    pub fn new(storage: Option<FileDialogStorage>) -> Self {
        let dialog = FileDialog::new()
            .add_file_filter(
                JOBS,
                Filter::new(|p: &Path| has_extension(p, &["camcraft"])),
            )
            .add_file_filter(
                MODELS,
                Filter::new(|p: &Path| has_extension(p, &["step", "stp", "stl"])),
            )
            .add_file_filter(
                PROFILES,
                Filter::new(|p: &Path| has_extension(p, &["toml"])),
            )
            .add_file_filter(TOOLS, Filter::new(|p: &Path| has_extension(p, &["csv"])))
            .add_save_extension("Job", "camcraft")
            .default_save_extension("Job");
        let dialog = match storage {
            Some(s) => dialog.storage(s),
            None => dialog,
        };
        Self { dialog }
    }

    /// Open the dialog for `request`. `name` is the file name a save starts with.
    pub fn open(&mut self, request: FileRequest, name: &str) {
        let config = self.dialog.config_mut();
        let (title, filter) = match &request {
            FileRequest::OpenJobs => ("Open jobs", JOBS),
            FileRequest::SaveJob(_) => ("Save job as", JOBS),
            FileRequest::AddModels(..) => ("Add models", MODELS),
            FileRequest::Locate(_, _) => ("Find the model", MODELS),
            FileRequest::Profile(_, _) => ("Choose a machine profile", PROFILES),
            FileRequest::PostTo(_, _) => ("Post programs into", PROFILES),
            FileRequest::ImportTools => ("Import Fusion tool libraries", TOOLS),
        };
        config.title = Some(title.to_owned());
        config.default_file_filter = Some(filter.to_owned());
        config.default_file_name = name.to_owned();
        let many = matches!(
            request,
            FileRequest::OpenJobs | FileRequest::AddModels(..) | FileRequest::ImportTools
        );
        let save = matches!(request, FileRequest::SaveJob(_));
        let folder = matches!(request, FileRequest::PostTo(_, _));
        self.dialog.set_user_data(request);
        if folder {
            self.dialog.pick_directory();
        } else if save {
            self.dialog.save_file();
        } else if many {
            self.dialog.pick_multiple();
        } else {
            self.dialog.pick_file();
        }
    }

    /// Draw the dialog, and hand back the paths chosen, once they are, with the
    /// request they answer.
    pub fn update(&mut self, ctx: &egui::Context) -> Option<(FileRequest, Vec<PathBuf>)> {
        self.dialog.update(ctx);
        let request = self.dialog.user_data::<FileRequest>()?.clone();
        let paths = match request {
            FileRequest::OpenJobs | FileRequest::AddModels(..) | FileRequest::ImportTools => {
                self.dialog.take_picked_multiple()?
            }
            _ => vec![self.dialog.take_picked()?],
        };
        Some((request, paths))
    }

    /// What to remember between runs of the app.
    pub fn storage(&mut self) -> &FileDialogStorage {
        self.dialog.storage_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_match_regardless_of_case() {
        assert!(has_extension(
            Path::new("jig.STEP"),
            &["step", "stp", "stl"]
        ));
        assert!(has_extension(Path::new("jig.stp"), &["step", "stp", "stl"]));
        assert!(has_extension(Path::new("rail-jig.camcraft"), &["camcraft"]));
        assert!(!has_extension(Path::new("jig.step.bak"), &["step"]));
        assert!(!has_extension(Path::new("README"), &["step"]));
    }
}
