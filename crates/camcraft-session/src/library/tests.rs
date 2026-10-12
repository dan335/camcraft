use super::*;
use camcraft_core::job::{Job, ToolKind};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("camcraft-library-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("nested").join("tools.toml")
}

#[test]
fn a_missing_library_is_empty_not_an_error() {
    let lib = Library::load(&temp("missing")).unwrap();
    assert_eq!(lib.root.count(), 0);
}

/// Saved and read back, the library is the same, and its folder is made for it.
#[test]
fn the_library_round_trips() {
    let path = temp("round-trip");
    let mut lib = Library::default();
    lib.root.tools.insert(
        "6-ball".into(),
        ToolSpec {
            kind: ToolKind::Ball,
            diameter: 6.0,
            flutes: Some(2),
            stickout: Some(25.0),
            vendor: Some("Amana".into()),
            ..ToolSpec::default()
        },
    );
    lib.root.tools.insert("default".into(), ToolSpec::default());
    let m1 = lib.root.create(&[], "machine_01").unwrap();
    let shank = lib.root.create(&m1, "1-4 shank").unwrap();
    lib.root
        .at_mut(&shank)
        .unwrap()
        .tools
        .insert("default".into(), ToolSpec::default());
    lib.root.create(&[], "empty").unwrap();
    lib.save(&path).unwrap();
    assert_eq!(Library::load(&path).unwrap(), lib);
}

/// A library tool drops into a job's tools unchanged: one schema.
#[test]
fn a_library_tool_is_a_job_tool() {
    let path = temp("schema");
    let mut lib = Library::default();
    lib.root.tools.insert(
        "fl".into(),
        ToolSpec {
            flutes: Some(3),
            ..ToolSpec::default()
        },
    );
    lib.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let job: Job = toml::from_str(&format!("version = 1\n{text}")).unwrap();
    job.check_common().unwrap();
    assert_eq!(job.tools["fl"], lib.root.tools["fl"]);
}

#[test]
fn a_bad_tool_is_not_saved() {
    let path = temp("bad");
    let mut lib = Library::default();
    let deep = lib.root.create(&[], "m").unwrap();
    lib.root.at_mut(&deep).unwrap().tools.insert(
        "bad".into(),
        ToolSpec {
            flutes: Some(0),
            ..ToolSpec::default()
        },
    );
    assert!(
        matches!(lib.save(&path), Err(LibraryError::BadTool { ref name, .. }) if name == "m/bad")
    );
    assert!(!path.exists());
}

/// The System library is shown, never saved: what comes back is only the
/// person's own.
#[test]
fn the_system_library_is_not_saved() {
    let path = temp("system");
    let mut lib = Library::default();
    lib.root.tools.insert("mine".into(), ToolSpec::default());
    let own = lib.clone();
    system::add_system(&mut lib.root);
    lib.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains(system::SYSTEM), "{text}");
    assert_eq!(Library::load(&path).unwrap(), own);
}

#[test]
fn free_names_count_up() {
    let mut taken = BTreeMap::new();
    assert_eq!(free_name("ball", &taken), "ball");
    taken.insert("ball".into(), ToolSpec::default());
    taken.insert("ball-2".into(), ToolSpec::default());
    assert_eq!(free_name("ball", &taken), "ball-3");
    assert_eq!(free_name("  ", &taken), "tool");
}

/// A library written before folders reads as its top folder.
#[test]
fn a_library_from_before_folders_reads() {
    let old = "# camcraft tool library.\n\n[tools.flat-6]\nkind = \"flat\"\ndiameter = 6.0\n";
    let lib: Library = toml::from_str(old).unwrap();
    assert_eq!(lib.root.tools.len(), 1);
    assert!(lib.root.folders.is_empty());
}

#[test]
fn folders_are_made_renamed_and_removed() {
    let mut root = Folder::default();
    let a = root.create(&[], "machine_01").unwrap();
    assert_eq!(
        root.create(&[], " machine_01 "),
        Err(FolderError::Taken("machine_01".into()))
    );
    assert_eq!(root.create(&[], "a/b"), Err(FolderError::Slash));
    assert_eq!(root.create(&[], "  "), Err(FolderError::NoName));
    let b = root.create(&a, "ball").unwrap();
    assert_eq!(b, ["machine_01", "ball"]);
    root.at_mut(&b)
        .unwrap()
        .tools
        .insert("t".into(), ToolSpec::default());
    let renamed = root.rename(&a, "machine_02").unwrap();
    assert!(root.at(&["machine_02".into(), "ball".into()]).is_some());
    assert_eq!(root.count(), 1);
    assert_eq!(root.rename(&[], "x"), Err(FolderError::Root));
    let gone = root.remove(&renamed).unwrap();
    assert_eq!(gone.count(), 1);
    assert_eq!(root.count(), 0);
    assert_eq!(root.folder_paths(), vec![Vec::<String>::new()]);
}

/// A tool moved onto a name that is taken gets a free one.
#[test]
fn tools_move_between_folders() {
    let mut root = Folder::default();
    let a = root.create(&[], "a").unwrap();
    root.tools.insert("t".into(), ToolSpec::default());
    root.at_mut(&a)
        .unwrap()
        .tools
        .insert("t".into(), ToolSpec::default());
    assert_eq!(root.move_tool(&[], "t", &a).unwrap(), "t-2");
    assert!(root.tools.is_empty());
    assert_eq!(root.at(&a).unwrap().tools.len(), 2);
    assert!(root.move_tool(&[], "nope", &a).is_err());
    assert!(root.move_tool(&a, "t", &["nowhere".into()]).is_err());
    assert_eq!(
        root.at(&a).unwrap().tools.len(),
        2,
        "a failed move keeps the tool"
    );
    let walked: Vec<String> = root
        .walk()
        .iter()
        .map(|(p, n, _)| format!("{}/{n}", folder_label(p)))
        .collect();
    assert_eq!(walked, ["a/t", "a/t-2"]);
}

/// Tools are found by their name or description, ignoring case, here alone or
/// here and below.
#[test]
fn tools_are_found_by_name_or_description() {
    let mut root = Folder::default();
    let a = root.create(&[], "a").unwrap();
    root.tools.insert("Flat 6".into(), ToolSpec::default());
    let ball = ToolSpec {
        comment: "for FINISHING".into(),
        ..ToolSpec::default()
    };
    root.at_mut(&a).unwrap().tools.insert("b".into(), ball);
    let names = |found: Vec<(FolderPath, &str, &ToolSpec)>| -> Vec<String> {
        found
            .iter()
            .map(|(p, n, _)| format!("{}/{n}", folder_label(p)))
            .collect()
    };
    assert_eq!(names(root.find(false, "")), ["Library/Flat 6"]);
    assert_eq!(names(root.find(true, "")), ["Library/Flat 6", "a/b"]);
    assert_eq!(names(root.find(true, "flat")), ["Library/Flat 6"]);
    assert_eq!(names(root.find(true, "finish")), ["a/b"]);
    assert!(root.find(false, "finish").is_empty());
}
