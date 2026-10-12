//! The manager's tests: what its edits do to the library, and that the library
//! saves itself.

use std::path::Path;

use camcraft_core::job::ToolSpec;
use camcraft_session::library::system::SYSTEM;
use camcraft_session::library::{Folder, FolderPath, Library};

use super::ToolManager;
use super::library::{Set, fill_for_snapshot};
use super::tree::TreeAction;

fn tools(names: &[&str]) -> Folder {
    Folder {
        tools: names
            .iter()
            .map(|n| ((*n).to_string(), ToolSpec::default()))
            .collect(),
        ..Folder::default()
    }
}

#[test]
fn a_rename_onto_a_taken_name_or_nothing_is_refused() {
    let mut set = Set::new(tools(&["a", "b"]));
    assert!(set.rename(&[], "a", "b").is_err());
    assert!(set.rename(&[], "a", "  ").is_err());
    assert_eq!(set.edited.tools.len(), 2);
    set.rename(&[], "a", "c").unwrap();
    assert!(set.edited.tools.contains_key("c"));
}

fn manager(name: &str) -> ToolManager {
    let dir = std::env::temp_dir().join(format!("camcraft-tm-{name}-{}", std::process::id()));
    ToolManager::with_library(dir.join("never-written.toml"))
}

/// A maker's Fusion library lands in the folder showing, selected, and said
/// so; a file that is not one is said to be not one.
#[test]
fn a_fusion_library_is_imported_into_the_folder_showing() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/tools");
    let mut m = manager("import");
    let folder = m.library.edit().create(&[], "onefinity").unwrap();
    m.folder = folder.clone();
    m.import_fusion(&fixtures.join("srf4-250up.csv"));
    let tool = &m.library.edited.at(&folder).unwrap().tools["SRF4-250UP"];
    assert_eq!(tool.presets.len(), 4);
    assert_eq!(m.selected, Some((folder, "SRF4-250UP".to_string())));
    assert!(matches!(&m.message, Some((true, t)) if t.contains("added SRF4-250UP")));

    m.import_fusion(&fixtures.join("../step/sled_base.step"));
    assert!(matches!(&m.message, Some((false, _))), "{:?}", m.message);
}

/// Moving the tool being edited keeps it selected where it went, and a folder
/// renamed under the selection carries it along.
#[test]
fn the_selection_follows_moves_and_renames() {
    let mut m = manager("follow");
    fill_for_snapshot(m.library.edit());
    let flat = vec!["machine_01".to_string(), "flat".to_string()];
    m.select(Some((flat.clone(), "flat-6".into())));
    let ball = vec!["machine_01".to_string(), "ball".to_string()];
    m.move_tool(&flat, "flat-6", &ball);
    assert_eq!(m.selected, Some((ball.clone(), "flat-6".into())));
    m.folder = ball.clone();
    m.apply(TreeAction::Rename(
        vec!["machine_01".into()],
        "router".into(),
    ));
    let moved = vec!["router".to_string(), "ball".to_string()];
    assert_eq!(m.folder, moved);
    assert_eq!(m.selected, Some((moved.clone(), "flat-6".into())));
    assert!(m.selected_tool().is_some());
}

/// A folder with tools in it is deleted only once confirmed; an empty one at
/// once.
#[test]
fn deleting_a_full_folder_asks_first() {
    let mut m = manager("delete");
    fill_for_snapshot(m.library.edit());
    m.apply(TreeAction::Delete(vec!["machine_02".into()]));
    assert!(m.confirm_delete.is_some());
    assert!(m.library.edited.at(&["machine_02".into()]).is_some());
    let empty = m.library.edit().create(&[], "empty").unwrap();
    m.apply(TreeAction::Delete(empty.clone()));
    assert!(m.library.edited.at(&empty).is_none());
}

/// A library edit is written as soon as it is kept, with nothing to press.
#[test]
fn the_library_saves_itself() {
    let dir = std::env::temp_dir().join(format!("camcraft-tm-lib-{}", std::process::id()));
    let path = dir.join("library.toml");
    let _ = std::fs::remove_file(&path);
    let mut m = ToolManager::with_library(path.clone());
    let folder = m.library.edit().create(&[], "machine_01").unwrap();
    m.library
        .edit()
        .at_mut(&folder)
        .unwrap()
        .tools
        .insert("flat-6".into(), ToolSpec::default());
    m.save_library();
    assert!(!m.library.dirty());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        !text.contains(SYSTEM),
        "the System library is not saved: {text}"
    );
    let again = ToolManager::with_library(path);
    assert!(
        again
            .library
            .edited
            .at(&folder)
            .unwrap()
            .tools
            .contains_key("flat-6")
    );
}

/// A tool that does not check clean holds the save back, and says why, until
/// it is fixed.
#[test]
fn a_bad_library_tool_is_not_saved() {
    let dir = std::env::temp_dir().join(format!("camcraft-tm-bad-{}", std::process::id()));
    let path = dir.join("library.toml");
    let _ = std::fs::remove_file(&path);
    let mut m = ToolManager::with_library(path.clone());
    let bad = ToolSpec {
        diameter: -1.0,
        ..ToolSpec::default()
    };
    m.library.edit().tools.insert("bad".into(), bad);
    m.save_library();
    assert!(m.library.dirty());
    assert!(m.library_unsaved.is_some());
    assert!(!path.exists());
    m.library.edit().tools.get_mut("bad").unwrap().diameter = 6.0;
    m.save_library();
    assert!(!m.library.dirty());
    assert!(m.library_unsaved.is_none());
}

/// A library file that could not be read is never written over.
#[test]
fn an_unreadable_library_is_not_written_over() {
    let dir = std::env::temp_dir().join(format!("camcraft-tm-unread-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("library.toml");
    std::fs::write(&path, "this is not toml [").unwrap();
    let mut m = ToolManager::with_library(path.clone());
    assert!(m.library_error.is_some());
    assert_eq!(
        m.library.edited.at(&[SYSTEM.into()]).map(Folder::count),
        Some(20),
        "the System library is there all the same"
    );
    m.library
        .edit()
        .tools
        .insert("flat-6".into(), ToolSpec::default());
    m.save_library();
    assert!(m.library.dirty());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "this is not toml ["
    );
}

fn system(sub: &str) -> FolderPath {
    vec![SYSTEM.to_string(), sub.to_string()]
}

/// A library with nothing in it yet has the System library, and nothing to
/// save.
#[test]
fn a_new_library_has_the_system_bits() {
    let m = manager("fresh");
    assert_eq!(m.library.edited.count(), 20);
    assert!(!m.dirty());
    assert_eq!(
        m.library.next_number(),
        1,
        "its numbers are not the person's"
    );
}

/// Nothing in the System library can be deleted, renamed, moved or added to;
/// a bit moved out of it is copied.
#[test]
fn the_system_library_cannot_be_changed() {
    let mut m = manager("locked");
    let mine = m.library.edit().create(&[], "machine_01").unwrap();
    let ends = system("End mills");
    let before = m.library.edited.at(&[SYSTEM.into()]).unwrap().clone();

    m.delete(&ends, "1/4 upcut");
    m.apply(TreeAction::Delete(ends.clone()));
    m.apply(TreeAction::Rename(ends.clone(), "mine".into()));
    m.apply(TreeAction::NewIn(ends.clone()));
    m.folder = ends.clone();
    m.new_tool();
    m.move_tool(&mine, "nothing", &ends);
    m.move_tool(&ends, "1/4 upcut", &mine);
    assert!(
        matches!(&m.message, Some((true, t)) if t.contains("copied")),
        "{:?}",
        m.message
    );

    assert_eq!(m.library.edited.at(&[SYSTEM.into()]).unwrap(), &before);
    assert!(
        m.library
            .edited
            .at(&mine)
            .unwrap()
            .tools
            .contains_key("1/4 upcut")
    );
}

/// Copying a bit out puts it, numbered as the person's next, in the folder of
/// theirs last shown, and shows it there to be changed; and it is saved.
#[test]
fn a_system_bit_is_copied_to_the_folder_last_shown() {
    let dir = std::env::temp_dir().join(format!("camcraft-tm-copy-{}", std::process::id()));
    let path = dir.join("library.toml");
    let _ = std::fs::remove_file(&path);
    let mut m = ToolManager::with_library(path.clone());
    m.library.edit().tools.insert(
        "mine".into(),
        ToolSpec {
            number: 4,
            ..ToolSpec::default()
        },
    );
    let folder = m.library.edit().create(&[], "machine_01").unwrap();
    m.apply(TreeAction::Select(folder.clone()));
    let balls = system("Ball");
    m.apply(TreeAction::Select(balls.clone()));
    m.duplicate(&balls, "1/8 ball");
    m.duplicate(&balls, "1/8 ball");

    let here = &m.library.edited.at(&folder).unwrap().tools;
    assert_eq!(here["1/8 ball"].number, 5);
    assert_eq!(here["1/8 ball-2"].number, 6);
    assert_eq!(here["1/8 ball"].presets.len(), 4);
    assert_eq!(m.folder, folder);
    assert_eq!(m.selected, Some((folder.clone(), "1/8 ball-2".into())));

    m.save_library();
    assert!(!m.dirty());
    let saved = Library::load(&path).unwrap();
    assert_eq!(saved.root.at(&folder).unwrap().tools.len(), 2);
    assert!(saved.root.at(&[SYSTEM.into()]).is_none());
}

/// Only an edit that lands is saved: the library is not looked through for
/// changes every frame, and reverted, nothing is left to save.
#[test]
fn only_an_edit_is_saved() {
    let dir = std::env::temp_dir().join(format!("camcraft-tm-edit-{}", std::process::id()));
    let path = dir.join("library.toml");
    let _ = std::fs::remove_file(&path);
    let mut m = ToolManager::with_library(path.clone());
    m.save_library();
    assert!(!path.exists(), "nothing edited, nothing written");
    m.library
        .edit()
        .tools
        .insert("flat-6".into(), ToolSpec::default());
    m.save_library();
    assert!(path.exists() && !m.dirty());
    m.library.edit().tools.clear();
    assert!(m.dirty());
    m.library.revert();
    m.save_library();
    assert!(!m.dirty() && m.library_unsaved.is_none());
}
