use super::*;
use std::fs;

/// A folder holding `files`, each made with its parent folders; a name
/// ending `/` is an empty folder.
fn project(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp dir");

    for file in files {
        let path = dir.path().join(file);

        if file.ends_with('/') {
            fs::create_dir_all(&path).expect("a folder");
            continue;
        }

        fs::create_dir_all(path.parent().expect("a parent")).expect("a folder");
        fs::write(&path, "").expect("a file");
    }

    dir
}

fn found(files: &[&str]) -> Vec<Platform> {
    platforms(project(files).path())
}

#[test]
fn an_android_studio_project_is_android() {
    let files = [
        "settings.gradle.kts",
        "app/build.gradle.kts",
        "app/src/main/AndroidManifest.xml",
    ];
    assert_eq!(found(&files), [Platform::Android]);
}

#[test]
fn an_xcode_project_is_ios() {
    let files = ["Demo.xcodeproj/project.pbxproj", "Demo/App.swift"];
    assert_eq!(found(&files), [Platform::Ios]);
    assert_eq!(found(&["Demo.xcworkspace/"]), [Platform::Ios]);
}

#[test]
fn a_flutter_app_is_both() {
    let files = [
        "pubspec.yaml",
        "lib/main.dart",
        "android/build.gradle",
        "android/app/src/main/AndroidManifest.xml",
        "ios/Podfile",
        "ios/Runner.xcodeproj/project.pbxproj",
    ];
    assert_eq!(found(&files), [Platform::Android, Platform::Ios]);
}

#[test]
fn tauri_mobile_conf_files_count() {
    let files = [
        "src-tauri/tauri.conf.json",
        "src-tauri/tauri.android.conf.json",
        "src-tauri/tauri.ios.conf.json",
    ];
    assert_eq!(found(&files), [Platform::Android, Platform::Ios]);
    assert_eq!(found(&["src-tauri/tauri.ios.conf.json"]), [Platform::Ios]);
}

#[test]
fn a_react_native_layout_counts_by_its_folders() {
    let files = ["android/build.gradle.kts", "ios/Podfile"];
    assert_eq!(found(&files), [Platform::Android, Platform::Ios]);
    // Gradle or a Podfile elsewhere says nothing.
    assert!(found(&["build.gradle", "tools/Podfile"]).is_empty());
}

#[test]
fn a_plain_rust_repository_is_no_mobile_app() {
    let files = ["Cargo.toml", "src/main.rs", "README.md"];
    assert!(found(&files).is_empty());
}

#[test]
fn an_ignored_android_folder_does_not_count() {
    let dir = project(&[
        "Cargo.toml",
        "android/build.gradle",
        "android/app/src/main/AndroidManifest.xml",
    ]);
    fs::write(dir.path().join(".gitignore"), "android/\n").expect("a .gitignore");
    assert!(platforms(dir.path()).is_empty());
}

#[test]
fn an_xcode_project_is_not_entered() {
    // A manifest inside the bundle is not seen; the bundle alone is iOS.
    let files = ["Demo.xcodeproj/AndroidManifest.xml"];
    assert_eq!(found(&files), [Platform::Ios]);
}

#[test]
fn markers_deeper_than_the_walk_go_unseen() {
    let files = ["a/b/c/d/e/f/g/AndroidManifest.xml"];
    assert!(found(&files).is_empty());
}
