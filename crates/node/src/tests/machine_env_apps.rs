use super::*;

#[test]
fn windows_finds_apps_in_install_folders_and_epics_list() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let (files, data) = (dir.path().join("files"), dir.path().join("data"));
    let touch = |path: PathBuf| {
        fs::create_dir_all(path.parent().expect("a parent")).expect("folders");
        fs::write(path, "").expect("a file");
    };
    touch(files.join("Blender Foundation/Blender 5.2/blender.exe"));
    touch(files.join("Android/Android Studio/bin/studio64.exe"));
    touch(files.join("Epic Games/UE_5.8/Engine.txt"));
    fs::create_dir_all(files.join("Blender Foundation/Blender 4.0")).expect("a folder left behind");
    let list = data.join(LAUNCHER_LIST);
    fs::create_dir_all(list.parent().expect("a parent")).expect("folders");
    let entries = r#"{"InstallationList": [
            {"InstallLocation": "D:\\Epic\\UE_5.4", "AppName": "UE_5.4"},
            {"InstallLocation": "x", "AppName": "UE_5.8"},
            {"InstallLocation": "x", "AppName": "FabPlugin_5.8"}]}"#;
    fs::write(list, entries).expect("the list");

    let found = windows_apps(&files, &data);

    let seen: Vec<(AppName, Option<&str>)> = found
        .iter()
        .map(|a| (a.name, a.version.as_deref()))
        .collect();
    assert_eq!(
        seen,
        [
            (AppName::Blender, Some("5.2")),
            (AppName::AndroidStudio, None),
            (AppName::UnrealEngine, Some("5.4")),
            (AppName::UnrealEngine, Some("5.8")),
        ]
    );
}
