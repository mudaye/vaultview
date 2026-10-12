#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::error::Error;
use std::fs;
use std::path::PathBuf;

#[test]
fn adding_folders_rejects_duplicates_files_and_missing_paths() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos");
    fs::create_dir(&folder).unwrap();
    let file = dir.path().join("file.txt");
    fs::write(&file, b"x").unwrap();

    let mut fav = Favorites::new();
    assert!(fav.add(&folder).unwrap());
    assert!(!fav.add(&folder).unwrap());
    assert!(fav.contains(&folder));
    assert_eq!(fav.folders(), &[folder.clone()]);

    assert!(matches!(fav.add(&file), Err(Error::NotADirectory(_))));
    assert!(matches!(fav.add(&dir.path().join("missing")), Err(Error::Io(_))));
    assert_eq!(fav.folders().len(), 1);
}

#[test]
fn removing_a_favorite_only_forgets_the_reference() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("keep.jpg"), b"x").unwrap();

    let mut fav = Favorites::new();
    fav.add(&folder).unwrap();
    assert!(fav.remove(&folder));
    assert!(!fav.remove(&folder));
    assert!(!fav.contains(&folder));
    assert!(folder.join("keep.jpg").exists());
}

#[test]
fn insertion_order_is_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let mut fav = Favorites::new();
    let mut expected = Vec::new();
    for name in ["zeta", "alpha", "mid"] {
        let p = dir.path().join(name);
        fs::create_dir(&p).unwrap();
        fav.add(&p).unwrap();
        expected.push(p);
    }
    assert_eq!(fav.folders(), expected.as_slice());
}

#[test]
fn json_round_trip_preserves_folders() {
    let mut fav = Favorites::new();
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    fav.add(&a).unwrap();
    fav.add(&b).unwrap();

    let json = fav.to_json().unwrap();
    let parsed = Favorites::from_json(&json).unwrap();
    assert_eq!(parsed.folders(), fav.folders());
}

#[test]
fn stored_metadata_contains_only_the_version_and_folder_paths() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("a");
    fs::create_dir(&folder).unwrap();
    let mut fav = Favorites::new();
    fav.add(&folder).unwrap();

    let value: serde_json::Value = serde_json::from_str(&fav.to_json().unwrap()).unwrap();
    let object = value.as_object().unwrap();
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, vec!["folders", "version"]);
    assert_eq!(object["version"], serde_json::json!(FAVORITES_FORMAT_VERSION));
    assert_eq!(object["folders"].as_array().unwrap().len(), 1);
}

#[test]
fn invalid_or_unsupported_json_is_rejected() {
    assert!(matches!(Favorites::from_json("not json"), Err(Error::Serialization(_))));
    assert!(matches!(Favorites::from_json("{}"), Err(Error::Serialization(_))));
    assert!(matches!(
        Favorites::from_json(r#"{"version":99,"folders":[]}"#),
        Err(Error::Serialization(_))
    ));
}

#[test]
fn loading_a_missing_store_yields_an_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let fav = Favorites::load(&dir.path().join("never_written.json")).unwrap();
    assert!(fav.folders().is_empty());
}

#[test]
fn loading_a_corrupt_store_is_an_error_not_a_silent_reset() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join(FAVORITES_FILE_NAME);
    fs::write(&store, b"{ definitely not valid").unwrap();
    assert!(matches!(Favorites::load(&store), Err(Error::Serialization(_))));
    assert_eq!(fs::read(&store).unwrap(), b"{ definitely not valid");
}

#[test]
fn save_creates_parent_directories_and_leaves_no_temporary_files() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos");
    fs::create_dir(&folder).unwrap();
    let store = dir.path().join("nested").join("config").join(FAVORITES_FILE_NAME);

    let mut fav = Favorites::new();
    fav.add(&folder).unwrap();
    fav.save(&store).unwrap();

    let names: Vec<String> = fs::read_dir(store.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![FAVORITES_FILE_NAME.to_string()]);

    let loaded = Favorites::load(&store).unwrap();
    assert_eq!(loaded.folders(), fav.folders());
}

#[test]
fn saving_again_replaces_the_previous_contents() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    let store = dir.path().join(FAVORITES_FILE_NAME);

    let mut fav = Favorites::new();
    fav.add(&a).unwrap();
    fav.save(&store).unwrap();
    fav.remove(&a);
    fav.add(&b).unwrap();
    fav.save(&store).unwrap();

    let loaded = Favorites::load(&store).unwrap();
    assert_eq!(loaded.folders(), &[b]);
}

#[test]
fn unavailable_favorites_are_identified() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("here");
    fs::create_dir(&folder).unwrap();
    let file = dir.path().join("file.txt");
    fs::write(&file, b"x").unwrap();

    assert_eq!(status(&folder), FavoriteStatus::Available);
    assert_eq!(status(&dir.path().join("moved_away")), FavoriteStatus::Missing);
    assert_eq!(status(&file), FavoriteStatus::NotADirectory);
}

#[test]
fn a_favorite_that_disappears_after_being_added_is_reported_missing() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("temporary");
    fs::create_dir(&folder).unwrap();
    let mut fav = Favorites::new();
    fav.add(&folder).unwrap();
    fs::remove_dir(&folder).unwrap();
    assert!(fav.contains(&folder));
    assert_eq!(status(&folder), FavoriteStatus::Missing);
}

#[test]
fn default_store_path_is_namespaced_by_application() {
    if let Some(path) = default_store_path() {
        assert!(path.ends_with(FAVORITES_FILE_NAME));
        let parent: PathBuf = path.parent().unwrap().to_path_buf();
        assert_eq!(parent.file_name().unwrap().to_string_lossy(), APP_NAME);
    }
}