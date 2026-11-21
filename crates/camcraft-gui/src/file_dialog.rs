//! The app's file dialogs: the operating system's own, through `rfd`.
//!
//! Explorer's dialog on Windows, the desktop's file chooser through the XDG
//! portal on Linux, and the open and save panels on macOS. They sort, search
//! and list recent files, which a dialog drawn inside egui did not (decision
//! 0032, `docs/decisions/0032-native-file-dialogs.md`).
//!
//! A dialog never holds up the window. A click only records what it wants
//! ([`AppFileDialog::open`]). The next frame's [`AppFileDialog::update`] builds
//! the dialog on the window's thread, as macOS needs, with the window as its
//! parent so it is modal to it, and leaves a thread waiting for its answer,
//! which wakes the window when it comes. A later frame's `update` hands the
//! paths back with the request they answer. There is one dialog at a time.
//!
//! Each kind of file starts where that kind was last picked from: jobs,
//! models, machine profiles, tool libraries, and the folder programs were
//! posted into, remembered between runs ([`Folders`]). Save As starts in the
//! job's own folder with its name, and finding a missing model starts where it
//! was. A folder that is no longer there is not passed, and the dialog starts
//! where the system would. Windows is given the folder to open in, not a
//! default it may overrule (rfd sets it with `IFileDialog::SetFolder`).
//!
//! What it cannot do:
//!
//! - **Tell a dialog that could not be shown from one cancelled.** rfd 0.18
//!   answers both with nothing, on every platform. On Windows every COM error
//!   becomes no answer. On Linux, with no file chooser portal it falls back to
//!   `zenity`, and with neither it logs an error and answers nothing. So a
//!   dialog that never appears looks like Cancel. Linux needs
//!   `xdg-desktop-portal` with a backend for the desktop
//!   (`xdg-desktop-portal-gtk`, `-gnome` or `-kde`), or `zenity`. What fails on
//!   this side is reported: the waiting thread not starting, or stopping
//!   without an answer.
//! - **Rely on the dialog to add `.camcraft`.** Windows and macOS add the
//!   filter's extension to a name typed without one, and ask before replacing
//!   the file that names. A portal's file chooser need not add it, so a job
//!   saved under a name without it gets it here ([`save_target`]). The dialog
//!   asked about the name it returned, not the completed one, so if a file of
//!   the completed name is already there the app asks before replacing it.
//! - **Be drawn in the app's theme, or pictured by the snapshot harness.** No
//!   dialog is ever shown under test: there each is cancelled as soon as it is
//!   asked for (`show`).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use camcraft_session::fs::with_job_extension;
use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::worker::DocId;

/// What a dialog is opened for.
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
    /// Tool library CSV files to bring into the tool library.
    ImportTools,
}

impl FileRequest {
    /// The job it is for, if it is for one.
    pub fn doc(&self) -> Option<DocId> {
        match self {
            Self::SaveJob(id)
            | Self::AddModels(id, _)
            | Self::Locate(id, _)
            | Self::Profile(id, _)
            | Self::PostTo(id, _) => Some(*id),
            Self::OpenJobs | Self::ImportTools => None,
        }
    }

    fn kind(&self) -> Kind {
        match self {
            Self::OpenJobs | Self::SaveJob(_) => Kind::Jobs,
            Self::AddModels(..) | Self::Locate(..) => Kind::Models,
            Self::Profile(..) => Kind::Profiles,
            Self::PostTo(..) => Kind::Post,
            Self::ImportTools => Kind::Tools,
        }
    }

    fn how(&self) -> How {
        match self {
            Self::OpenJobs | Self::AddModels(..) | Self::ImportTools => How::Many,
            Self::Locate(..) | Self::Profile(..) => How::One,
            Self::SaveJob(_) => How::Save,
            Self::PostTo(..) => How::Folder,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            Self::OpenJobs => "Open jobs",
            Self::SaveJob(_) => "Save job as",
            Self::AddModels(..) => "Add models",
            Self::Locate(..) => "Find the model",
            Self::Profile(..) => "Choose a machine profile",
            Self::PostTo(..) => "Post programs into",
            Self::ImportTools => "Import tools from CSV files",
        }
    }

    /// The filters its dialog offers, the first chosen to start with.
    fn filters(&self) -> &'static [Filter] {
        match self {
            Self::OpenJobs => &[JOBS, ALL],
            // A job is saved as a job. "All files" would have macOS add `.*`
            // to the name, and Windows nothing it does not add already.
            Self::SaveJob(_) => &[JOBS],
            Self::AddModels(..) | Self::Locate(..) => &[MODELS, ALL],
            Self::Profile(..) => &[PROFILES, ALL],
            Self::ImportTools => &[TOOLS, ALL],
            Self::PostTo(..) => &[],
        }
    }
}

/// A kind of file, each kind remembering its own folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Jobs,
    Models,
    Profiles,
    Tools,
    /// The folder programs are posted into.
    Post,
}

/// What a dialog asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum How {
    One,
    Many,
    Save,
    Folder,
}

/// A filter a dialog offers: its name, and the extensions it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Filter {
    name: &'static str,
    extensions: &'static [&'static str],
}

// Each extension in lower and upper case: some CAD programs write `.STEP`, and a portal's
// file chooser matches a pattern's case, where Windows and macOS do not. The
// lower-case one first: it is what Windows adds to a name saved without one.
const JOBS: Filter = Filter {
    name: "Jobs (.camcraft)",
    extensions: &["camcraft", "CAMCRAFT"],
};
const MODELS: Filter = Filter {
    name: "Models (STEP, STL)",
    extensions: &["step", "stp", "stl", "STEP", "STP", "STL"],
};
const PROFILES: Filter = Filter {
    name: "Machine profiles (TOML)",
    extensions: &["toml", "TOML"],
};
const TOOLS: Filter = Filter {
    name: "Tool libraries (CSV)",
    extensions: &["csv", "CSV"],
};
/// Every file, whatever it is named: `*` is every name to each of rfd's
/// backends, a name with no extension included.
const ALL: Filter = Filter {
    name: "All files",
    extensions: &["*"],
};

/// The folder each kind of file was last picked from, kept between runs.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Folders {
    jobs: Option<PathBuf>,
    models: Option<PathBuf>,
    profiles: Option<PathBuf>,
    tools: Option<PathBuf>,
    post: Option<PathBuf>,
}

impl Folders {
    fn slot(&mut self, kind: Kind) -> &mut Option<PathBuf> {
        match kind {
            Kind::Jobs => &mut self.jobs,
            Kind::Models => &mut self.models,
            Kind::Profiles => &mut self.profiles,
            Kind::Tools => &mut self.tools,
            Kind::Post => &mut self.post,
        }
    }

    fn last(&self, kind: Kind) -> Option<&Path> {
        match kind {
            Kind::Jobs => self.jobs.as_deref(),
            Kind::Models => self.models.as_deref(),
            Kind::Profiles => self.profiles.as_deref(),
            Kind::Tools => self.tools.as_deref(),
            Kind::Post => self.post.as_deref(),
        }
    }

    /// Remember where `paths`, answering `request`, were picked: the folder
    /// itself for a folder, else the one the first file is in.
    fn remember(&mut self, request: &FileRequest, paths: &[PathBuf]) {
        let Some(first) = paths.first() else {
            return;
        };
        let folder = match request.how() {
            How::Folder => Some(first.as_path()),
            How::One | How::Many | How::Save => first.parent(),
        };
        if let Some(folder) = folder.filter(|f| !f.as_os_str().is_empty()) {
            *self.slot(request.kind()) = Some(folder.to_path_buf());
        }
    }

    /// The folder a dialog for `request` starts in: that of `about`, the file
    /// it is about, if that folder is there; else where its kind was last
    /// picked from, if that is still there; else none, for the system's own.
    fn start(&self, request: &FileRequest, about: Option<&Path>) -> Option<PathBuf> {
        let own = about
            .and_then(Path::parent)
            .filter(|d| !d.as_os_str().is_empty());
        own.into_iter()
            .chain(self.last(request.kind()))
            .find(|d| d.is_dir())
            .map(Path::to_path_buf)
    }
}

/// A dialog to show: everything the operating system's is given.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Spec {
    title: &'static str,
    how: How,
    filters: &'static [Filter],
    /// The folder it opens in; the system's choice when `None`.
    folder: Option<PathBuf>,
    /// The name a save starts with.
    name: Option<String>,
}

/// What a dialog answered once it closed.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Chosen(Vec<PathBuf>),
    /// Cancelled, or, as rfd cannot tell the two apart, never shown.
    Cancelled,
}

/// A dialog done with, and the request it answers. A cancelled one is not
/// handed back: there is nothing to do.
#[derive(Debug, PartialEq, Eq)]
pub enum Finished {
    /// The paths chosen.
    Chosen(FileRequest, Vec<PathBuf>),
    /// Why nothing could be chosen.
    Failed(FileRequest, String),
}

/// Where the dialog stands.
#[derive(Debug, Default)]
enum State {
    #[default]
    Idle,
    /// Asked for, and shown at the next update.
    Wanted(FileRequest),
    /// Showing, its answer to come on the channel.
    Showing(FileRequest, Receiver<Answer>),
}

/// The app's one file dialog, and what it is answering.
#[derive(Debug, Default)]
pub struct AppFileDialog {
    folders: Folders,
    state: State,
}

impl AppFileDialog {
    /// A dialog that starts each kind of file in `folders`.
    pub fn new(folders: Folders) -> Self {
        Self {
            folders,
            state: State::Idle,
        }
    }

    /// Ask for a dialog for `request`, shown at the next [`Self::update`].
    ///
    /// If a dialog is already asked for or showing, this request is dropped:
    /// the system shows one at a time, modal to the window, so a second can
    /// only come from the same frame as the first, or from a shortcut pressed
    /// while the first was showing.
    pub fn open(&mut self, request: FileRequest) {
        if matches!(self.state, State::Idle) {
            self.state = State::Wanted(request);
        }
    }

    /// Whether a dialog asking where to save job `id` is asked for or showing.
    pub fn saving(&self, id: DocId) -> bool {
        matches!(
            &self.state,
            State::Wanted(FileRequest::SaveJob(d)) | State::Showing(FileRequest::SaveJob(d), _)
                if *d == id
        )
    }

    /// Show the dialog asked for, parented on `window`, and hand back the one
    /// that has finished, if one has. `about` names the file a request is
    /// about, if there is one: a job's own file, or a missing model's old
    /// place. The dialog starts in its folder, and a save with its name.
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        window: &eframe::Frame,
        about: impl FnOnce(&FileRequest) -> Option<PathBuf>,
    ) -> Option<Finished> {
        self.step(about, |spec| show(spec, window, ctx))
    }

    /// [`Self::update`], with `show` showing the dialog.
    fn step(
        &mut self,
        about: impl FnOnce(&FileRequest) -> Option<PathBuf>,
        show: impl FnOnce(Spec) -> std::io::Result<Receiver<Answer>>,
    ) -> Option<Finished> {
        match std::mem::take(&mut self.state) {
            State::Idle => None,
            State::Wanted(request) => {
                let spec = self.spec(&request, about(&request).as_deref());
                match show(spec) {
                    Ok(answer) => {
                        self.state = State::Showing(request, answer);
                        None
                    }
                    Err(e) => Some(Finished::Failed(
                        request,
                        format!("the file dialog could not be started: {e}"),
                    )),
                }
            }
            State::Showing(request, answer) => match answer.try_recv() {
                Ok(Answer::Chosen(paths)) => {
                    self.folders.remember(&request, &paths);
                    Some(Finished::Chosen(request, paths))
                }
                Ok(Answer::Cancelled) => None,
                Err(TryRecvError::Empty) => {
                    self.state = State::Showing(request, answer);
                    None
                }
                Err(TryRecvError::Disconnected) => Some(Finished::Failed(
                    request,
                    "the file dialog stopped without an answer".into(),
                )),
            },
        }
    }

    /// The dialog for `request`, about the file `about`.
    fn spec(&self, request: &FileRequest, about: Option<&Path>) -> Spec {
        let how = request.how();
        let name = match how {
            How::Save => about
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy().into_owned()),
            How::One | How::Many | How::Folder => None,
        };
        Spec {
            title: request.title(),
            how,
            filters: request.filters(),
            folder: self.folders.start(request, about),
            name,
        }
    }

    /// Where each kind of file was last picked from, to keep between runs.
    pub fn folders(&self) -> &Folders {
        &self.folders
    }
}

/// Where a job is written that the Save dialog answered with `chosen`.
#[derive(Debug, PartialEq, Eq)]
pub enum SaveTo {
    /// Here, as it is: the name the dialog gave, which it asked about if a file
    /// had it; or that name with `.camcraft` added, which no file has.
    Write(PathBuf),
    /// Here, once the person agrees: `.camcraft` was added to the name the
    /// dialog gave, and a file already has the name that makes, which the
    /// dialog never asked about.
    AskFirst(PathBuf),
}

/// Where a job the Save dialog answered with `chosen` is written, `exists`
/// saying whether a file is there: `chosen`, with `.camcraft` added if it
/// has no such extension.
pub fn save_target(chosen: &Path, exists: impl FnOnce(&Path) -> bool) -> SaveTo {
    let path = with_job_extension(chosen);
    if path.as_path() != chosen && exists(&path) {
        SaveTo::AskFirst(path)
    } else {
        SaveTo::Write(path)
    }
}

/// Show the dialog `spec` describes, parented on `window`. Its answer comes on
/// the channel, and wakes the window through `ctx`.
#[cfg(not(test))]
fn show(
    spec: Spec,
    window: &eframe::Frame,
    ctx: &egui::Context,
) -> std::io::Result<Receiver<Answer>> {
    use std::future::Future;
    use std::pin::Pin;

    type Paths = Pin<Box<dyn Future<Output = Option<Vec<PathBuf>>> + Send>>;

    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title(spec.title)
        .set_parent(window);
    for f in spec.filters {
        dialog = dialog.add_filter(f.name, f.extensions);
    }
    if let Some(folder) = &spec.folder {
        dialog = dialog.set_directory(folder);
    }
    if let Some(name) = spec.name {
        dialog = dialog.set_file_name(name);
    }
    // Each is started here, on the window's thread, and only waited for on
    // the next: macOS shows its panel as it is started, and only from this
    // thread. Windows and Linux show theirs from a thread of rfd's.
    let paths: Paths = match spec.how {
        How::One => {
            let file = dialog.pick_file();
            Box::pin(async move { file.await.map(|f| vec![PathBuf::from(f)]) })
        }
        How::Many => {
            let files = dialog.pick_files();
            Box::pin(async move {
                files
                    .await
                    .map(|f| f.into_iter().map(PathBuf::from).collect())
            })
        }
        How::Save => {
            let file = dialog.save_file();
            Box::pin(async move { file.await.map(|f| vec![PathBuf::from(f)]) })
        }
        How::Folder => {
            let folder = dialog.pick_folder();
            Box::pin(async move { folder.await.map(|f| vec![PathBuf::from(f)]) })
        }
    };
    let (send, answer) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::Builder::new()
        .name("file dialog".into())
        .spawn(move || {
            let said = match pollster::block_on(paths) {
                Some(paths) if !paths.is_empty() => Answer::Chosen(paths),
                _ => Answer::Cancelled,
            };
            // No one takes it only once the app has closed: there is no one
            // left to tell.
            if send.send(said).is_ok() {
                ctx.request_repaint();
            }
        })?;
    Ok(answer)
}

/// Under test no dialog is ever shown, since one would wait for a person: the
/// snapshot harness drives the real app, through this. Each is cancelled as
/// soon as it is asked for.
#[cfg(test)]
fn show(
    _spec: Spec,
    _window: &eframe::Frame,
    ctx: &egui::Context,
) -> std::io::Result<Receiver<Answer>> {
    let (send, answer) = mpsc::channel();
    send.send(Answer::Cancelled)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    ctx.request_repaint();
    Ok(answer)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A folder of its own under the system's temporary folder, empty.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "camcraft-file-dialog-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const EVERY: [FileRequest; 7] = [
        FileRequest::OpenJobs,
        FileRequest::SaveJob(1),
        FileRequest::AddModels(1, None),
        FileRequest::Locate(1, String::new()),
        FileRequest::Profile(1, String::new()),
        FileRequest::PostTo(1, None),
        FileRequest::ImportTools,
    ];

    /// Each kind of file is offered by its extensions, in both cases, with
    /// "All files" last; a save offers jobs only, and a folder nothing.
    #[test]
    fn each_dialog_offers_its_kind_then_all_files() {
        let names = |r: &FileRequest| r.filters().iter().map(|f| f.name).collect::<Vec<_>>();
        assert_eq!(names(&FileRequest::OpenJobs), [JOBS.name, ALL.name]);
        assert_eq!(
            names(&FileRequest::AddModels(1, None)),
            [MODELS.name, ALL.name]
        );
        assert_eq!(
            names(&FileRequest::Locate(1, "m".into())),
            [MODELS.name, ALL.name]
        );
        assert_eq!(
            names(&FileRequest::Profile(1, "s".into())),
            [PROFILES.name, ALL.name]
        );
        assert_eq!(names(&FileRequest::ImportTools), [TOOLS.name, ALL.name]);
        assert_eq!(names(&FileRequest::SaveJob(1)), [JOBS.name]);
        assert!(FileRequest::PostTo(1, None).filters().is_empty());

        for filter in [JOBS, MODELS, PROFILES, TOOLS] {
            for ext in filter.extensions {
                let upper = ext.to_ascii_uppercase();
                let lower = ext.to_ascii_lowercase();
                assert!(filter.extensions.contains(&upper.as_str()), "{ext}");
                assert!(filter.extensions.contains(&lower.as_str()), "{ext}");
            }
            // What Windows adds to a name saved without an extension.
            assert_eq!(
                filter.extensions[0],
                filter.extensions[0].to_ascii_lowercase()
            );
        }
        assert!(
            MODELS.extensions.contains(&"STEP"),
            "as some CAD programs write it"
        );
        assert_eq!(ALL.extensions, ["*"]);
    }

    /// A file picked is remembered by the folder it is in, for its kind only;
    /// a folder picked to post into, by itself.
    #[test]
    fn each_kind_starts_where_it_was_last_picked() {
        let dir = temp("kinds");
        let (downloads, jobs, out) = (dir.join("Downloads"), dir.join("jobs"), dir.join("out"));
        for d in [&downloads, &jobs, &out] {
            std::fs::create_dir_all(d).unwrap();
        }
        let mut folders = Folders::default();
        assert_eq!(folders.start(&FileRequest::AddModels(1, None), None), None);

        let models = FileRequest::AddModels(1, None);
        folders.remember(
            &models,
            &[downloads.join("jig.STEP"), downloads.join("b.stl")],
        );
        folders.remember(&FileRequest::OpenJobs, &[jobs.join("rail-jig.camcraft")]);
        folders.remember(&FileRequest::PostTo(1, None), std::slice::from_ref(&out));

        assert_eq!(folders.start(&models, None), Some(downloads.clone()));
        assert_eq!(
            folders.start(&FileRequest::Locate(1, "m".into()), None),
            Some(downloads)
        );
        assert_eq!(
            folders.start(&FileRequest::OpenJobs, None),
            Some(jobs.clone())
        );
        assert_eq!(folders.start(&FileRequest::SaveJob(1), None), Some(jobs));
        assert_eq!(
            folders.start(&FileRequest::PostTo(1, None), None),
            Some(out)
        );
        assert_eq!(folders.start(&FileRequest::ImportTools, None), None);
        assert_eq!(
            folders.start(&FileRequest::Profile(1, "s".into()), None),
            None
        );

        // Nothing picked is nothing remembered.
        let before = folders.clone();
        folders.remember(&FileRequest::ImportTools, &[]);
        assert_eq!(folders, before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A folder that has gone is not passed to the dialog.
    #[test]
    fn a_folder_that_has_gone_is_not_passed() {
        let dir = temp("gone");
        let models = dir.join("models");
        std::fs::create_dir_all(&models).unwrap();
        let mut folders = Folders::default();
        let request = FileRequest::AddModels(1, None);
        folders.remember(&request, &[models.join("part.step")]);
        assert_eq!(folders.start(&request, None), Some(models.clone()));
        std::fs::remove_dir_all(&models).unwrap();
        assert_eq!(folders.start(&request, None), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Save As starts in the job's own folder with its name, and finding a model
    /// where it was; either, gone, falls back to where its kind was last picked.
    #[test]
    fn a_dialog_about_a_file_starts_beside_it() {
        let dir = temp("about");
        let (jobs, elsewhere, old) = (dir.join("jobs"), dir.join("elsewhere"), dir.join("old"));
        for d in [&jobs, &elsewhere, &old] {
            std::fs::create_dir_all(d).unwrap();
        }
        let mut dialog = AppFileDialog::default();
        dialog
            .folders
            .remember(&FileRequest::OpenJobs, &[elsewhere.join("other.camcraft")]);

        let save = FileRequest::SaveJob(1);
        let spec = dialog.spec(&save, Some(&jobs.join("rail-jig.camcraft")));
        assert_eq!(spec.folder, Some(jobs.clone()));
        assert_eq!(spec.name.as_deref(), Some("rail-jig.camcraft"));
        assert_eq!(spec.how, How::Save);
        assert_eq!(spec.title, "Save job as");

        // A job never saved: its name, in the folder jobs were last in.
        let spec = dialog.spec(&save, Some(Path::new("Untitled.camcraft")));
        assert_eq!(spec.folder, Some(elsewhere.clone()));
        assert_eq!(spec.name.as_deref(), Some("Untitled.camcraft"));

        // A job whose folder has gone.
        let spec = dialog.spec(&save, Some(&dir.join("gone/rail-jig.camcraft")));
        assert_eq!(spec.folder, Some(elsewhere));

        // A missing model, where it was; an open dialog is given no name.
        let locate = FileRequest::Locate(1, "jig".into());
        let spec = dialog.spec(&locate, Some(&old.join("jig.step")));
        assert_eq!(spec.folder, Some(old));
        assert_eq!(spec.name, None);
        let spec = dialog.spec(&locate, Some(&dir.join("gone/jig.step")));
        assert_eq!(spec.folder, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The folders come back from the app's storage as they went in, by the
    /// same calls the app makes; a store with fewer kinds reads the rest as
    /// none.
    #[test]
    fn folders_round_trip_through_the_apps_storage() {
        #[derive(Default)]
        struct Memory(std::collections::HashMap<String, String>);
        impl eframe::Storage for Memory {
            fn get_string(&self, key: &str) -> Option<String> {
                self.0.get(key).cloned()
            }
            fn set_string(&mut self, key: &str, value: String) {
                self.0.insert(key.into(), value);
            }
            fn remove_string(&mut self, key: &str) {
                self.0.remove(key);
            }
            fn flush(&mut self) {}
        }

        let mut folders = Folders::default();
        folders.remember(
            &FileRequest::AddModels(1, None),
            &[PathBuf::from("/home/me/Downloads/jig.STEP")],
        );
        folders.remember(
            &FileRequest::PostTo(1, None),
            &[PathBuf::from("/mnt/router/jobs")],
        );
        let mut store = Memory::default();
        eframe::set_value(&mut store, "file_folders", &folders);
        let back: Folders = eframe::get_value(&store, "file_folders").unwrap();
        assert_eq!(back, folders);
        assert_eq!(
            back.models.as_deref(),
            Some(Path::new("/home/me/Downloads"))
        );

        eframe::Storage::set_string(
            &mut store,
            "file_folders",
            r#"(jobs: Some("/jobs"))"#.into(),
        );
        let back: Folders = eframe::get_value(&store, "file_folders").unwrap();
        assert_eq!(back.jobs.as_deref(), Some(Path::new("/jobs")));
        assert_eq!(back.models, None);
    }

    /// A name with no `.camcraft` gets it; if that names a file already there,
    /// which the dialog never asked about, the app asks first. A name the
    /// dialog gave whole was asked about by the dialog, if it had to be.
    #[test]
    fn a_job_is_saved_with_its_extension_and_never_over_a_file_unasked() {
        let there = |_: &Path| true;
        let not_there = |_: &Path| false;
        let dir = Path::new("jobs");
        assert_eq!(
            save_target(&dir.join("jig"), not_there),
            SaveTo::Write(dir.join("jig.camcraft"))
        );
        assert_eq!(
            save_target(&dir.join("jig"), there),
            SaveTo::AskFirst(dir.join("jig.camcraft"))
        );
        assert_eq!(
            save_target(&dir.join("jig.camcraft"), there),
            SaveTo::Write(dir.join("jig.camcraft"))
        );
        assert_eq!(
            save_target(&dir.join("jig.CAMCRAFT"), there),
            SaveTo::Write(dir.join("jig.CAMCRAFT"))
        );
        // Another extension is kept, as Windows keeps it.
        assert_eq!(
            save_target(&dir.join("jig.txt"), not_there),
            SaveTo::Write(dir.join("jig.txt.camcraft"))
        );
        // The completed name is the one asked about.
        let mut asked = None;
        let _ = save_target(&dir.join("jig"), |p| {
            asked = Some(p.to_path_buf());
            false
        });
        assert_eq!(asked, Some(dir.join("jig.camcraft")));
    }

    /// One dialog at a time: a request made while one is asked for or showing
    /// is dropped; once it has answered, the next is shown.
    #[test]
    fn one_dialog_at_a_time() {
        let mut dialog = AppFileDialog::default();
        dialog.open(FileRequest::SaveJob(7));
        dialog.open(FileRequest::OpenJobs);
        assert!(dialog.saving(7));
        assert!(!dialog.saving(8));

        let (send, answer) = mpsc::channel();
        let mut shown = Vec::new();
        let about = |_: &FileRequest| Some(PathBuf::from("Untitled.camcraft"));
        assert_eq!(
            dialog.step(about, |spec| {
                shown.push(spec);
                Ok(answer)
            }),
            None
        );
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].title, "Save job as");
        assert!(dialog.saving(7), "showing");

        // Asked for while it shows: dropped, and nothing more is shown.
        dialog.open(FileRequest::ImportTools);
        let again = std::cell::Cell::new(false);
        let never = |_: Spec| -> std::io::Result<Receiver<Answer>> {
            again.set(true);
            Err(std::io::Error::other("not wanted"))
        };
        assert_eq!(dialog.step(|_| None, never), None, "still showing");
        assert!(!again.get());

        let dir = temp("one");
        send.send(Answer::Chosen(vec![dir.join("jig")])).unwrap();
        assert_eq!(
            dialog.step(|_| None, never),
            Some(Finished::Chosen(
                FileRequest::SaveJob(7),
                vec![dir.join("jig")]
            ))
        );
        assert!(!dialog.saving(7));
        assert_eq!(
            dialog.folders.start(&FileRequest::OpenJobs, None),
            Some(dir.clone())
        );
        // The import asked for while it showed was dropped.
        assert_eq!(dialog.step(|_| None, never), None);
        assert!(!again.get());

        // The next is shown.
        dialog.open(FileRequest::ImportTools);
        let (send, answer) = mpsc::channel();
        assert_eq!(dialog.step(|_| None, |_| Ok(answer)), None);
        send.send(Answer::Cancelled).unwrap();
        assert_eq!(
            dialog.step(|_| None, never),
            None,
            "cancelled: nothing to do"
        );
        assert!(matches!(dialog.state, State::Idle));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A dialog that could not be started, or whose thread stopped without an
    /// answer, says so, and lets the next be shown.
    #[test]
    fn a_dialog_that_fails_says_so() {
        let mut dialog = AppFileDialog::default();
        dialog.open(FileRequest::OpenJobs);
        let failed = dialog.step(|_| None, |_| Err(std::io::Error::other("no threads")));
        assert_eq!(
            failed,
            Some(Finished::Failed(
                FileRequest::OpenJobs,
                "the file dialog could not be started: no threads".into()
            ))
        );

        dialog.open(FileRequest::AddModels(3, None));
        let (send, answer) = mpsc::channel::<Answer>();
        assert_eq!(dialog.step(|_| None, |_| Ok(answer)), None);
        drop(send);
        let failed = dialog.step(|_| None, |_| Ok(mpsc::channel().1));
        assert_eq!(
            failed,
            Some(Finished::Failed(
                FileRequest::AddModels(3, None),
                "the file dialog stopped without an answer".into()
            ))
        );
        dialog.open(FileRequest::OpenJobs);
        assert!(matches!(dialog.state, State::Wanted(FileRequest::OpenJobs)));
    }

    /// Every request names a kind, a title and a way to ask; and every one is
    /// for a job but opening jobs and importing tools.
    #[test]
    fn every_request_is_described() {
        for r in &EVERY {
            assert!(!r.title().is_empty());
            assert_eq!(
                r.doc().is_none(),
                matches!(r, FileRequest::OpenJobs | FileRequest::ImportTools)
            );
            assert_eq!(r.how() == How::Folder, r.kind() == Kind::Post);
        }
    }
}
