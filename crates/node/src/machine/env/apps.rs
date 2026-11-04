//! Creative and developer apps: app bundles on macOS, desktop entries on Linux,
//! install folders and Epic's launcher list on Windows.

use super::{PROBE_TIMEOUT, first_line};
use beep_proto::node::{AppInfo, AppName};
use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use tokio::{task::spawn_blocking, time::timeout};

const APPLICATIONS: &str = "/Applications";
const UNREAL_ROOT: &str = "/Users/Shared/Epic Games";
const DESKTOP_DIRS: [&str; 2] = [
    "/usr/share/applications",
    "/var/lib/flatpak/exports/share/applications",
];
const LAUNCHER_LIST: &str = "Epic/UnrealEngineLauncher/LauncherInstalled.dat";

pub(crate) async fn apps() -> Vec<AppInfo> {
    let scan = match env::consts::OS {
        "macos" => spawn_blocking(|| mac_apps(Path::new(APPLICATIONS), Path::new(UNREAL_ROOT))),
        "linux" => spawn_blocking(linux_apps),
        "windows" => spawn_blocking(|| {
            let folder = |var, default| {
                env::var_os(var).map_or_else(|| PathBuf::from(default), PathBuf::from)
            };
            windows_apps(
                &folder("ProgramFiles", r"C:\Program Files"),
                &folder("ProgramData", r"C:\ProgramData"),
            )
        }),
        _ => return Vec::new(),
    };
    // A slow disk or a panicking task means no apps; the report still goes out.
    match timeout(PROBE_TIMEOUT, scan).await {
        Ok(Ok(found)) => found,
        Ok(Err(_)) | Err(_) => Vec::new(),
    }
}

/// Bundles in `applications`, and one Unreal entry per `UE_*` folder in `unreal_root`.
fn mac_apps(applications: &Path, unreal_root: &Path) -> Vec<AppInfo> {
    const BUNDLES: [(&str, AppName); 4] = [
        ("Blender.app", AppName::Blender),
        ("Xcode.app", AppName::Xcode),
        ("Android Studio.app", AppName::AndroidStudio),
        ("Docker.app", AppName::DockerDesktop),
    ];
    let mut found = Vec::new();
    for (bundle, name) in BUNDLES {
        let path = applications.join(bundle);
        if path.is_dir() {
            let plist = fs::read_to_string(path.join("Contents/Info.plist")).ok();
            found.push(AppInfo {
                name,
                version: plist.as_deref().and_then(parse_info_plist_version),
            });
        }
    }
    let mut unreal = unreal_folders(unreal_root);
    unreal.sort();
    found.extend(unreal.into_iter().map(unreal_app));
    found
}

/// The versions of the `UE_*` folders in `root`.
fn unreal_folders(root: &Path) -> Vec<String> {
    folder_names(root)
        .filter_map(|folder| folder.strip_prefix("UE_").map(str::to_owned))
        .collect()
}

fn folder_names(root: &Path) -> impl Iterator<Item = String> {
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
}

fn unreal_app(version: String) -> AppInfo {
    AppInfo {
        name: AppName::UnrealEngine,
        version: Some(version),
    }
}

/// Default install folders under `program_files`, and Unreal also from Epic's
/// list under `program_data`, which knows engines installed on another drive.
fn windows_apps(program_files: &Path, program_data: &Path) -> Vec<AppInfo> {
    let blender_root = program_files.join("Blender Foundation");
    let mut blender: Vec<String> = folder_names(&blender_root)
        .filter(|folder| blender_root.join(folder).join("blender.exe").is_file())
        .filter_map(|folder| folder.strip_prefix("Blender ").map(str::to_owned))
        .collect();
    blender.sort();
    let mut found: Vec<AppInfo> = blender
        .into_iter()
        .map(|version| AppInfo {
            name: AppName::Blender,
            version: Some(version),
        })
        .collect();
    for (exe, name) in [
        (
            "Android/Android Studio/bin/studio64.exe",
            AppName::AndroidStudio,
        ),
        ("Docker/Docker/Docker Desktop.exe", AppName::DockerDesktop),
    ] {
        if program_files.join(exe).is_file() {
            found.push(AppInfo {
                name,
                version: None,
            });
        }
    }
    let mut unreal = unreal_folders(&program_files.join("Epic Games"));
    // Without the launcher's list, the folders still answer.
    if let Ok(text) = fs::read_to_string(program_data.join(LAUNCHER_LIST)) {
        unreal.extend(launcher_unreal_versions(&text));
    }
    unreal.sort();
    unreal.dedup();
    found.extend(unreal.into_iter().map(unreal_app));
    found
}

#[derive(Deserialize)]
struct LauncherList {
    #[serde(rename = "InstallationList")]
    installed: Vec<LauncherEntry>,
}

#[derive(Deserialize)]
struct LauncherEntry {
    #[serde(rename = "AppName")]
    app_name: String,
}

/// The Unreal versions in Epic's list, which also holds its games and plugins.
fn launcher_unreal_versions(text: &str) -> Vec<String> {
    let installed = serde_json::from_str::<LauncherList>(text)
        .map_or_else(|_| Vec::new(), |list| list.installed);
    installed
        .into_iter()
        .filter_map(|entry| entry.app_name.strip_prefix("UE_").map(str::to_owned))
        .collect()
}

fn linux_apps() -> Vec<AppInfo> {
    let mut dirs: Vec<PathBuf> = DESKTOP_DIRS.iter().map(PathBuf::from).collect();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/applications"));
    }
    let mut found: Vec<AppInfo> = Vec::new();
    let mut add = |app: AppInfo| {
        if !found.iter().any(|known| known.name == app.name) {
            found.push(app);
        }
    };
    for dir in dirs {
        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "desktop"))
            .collect();
        entries.sort();
        for path in entries {
            if let Some(app) = fs::read_to_string(path)
                .ok()
                .and_then(|text| parse_desktop_entry(&text))
            {
                add(app);
            }
        }
    }
    if Path::new("/snap/bin/blender").exists() {
        add(AppInfo {
            name: AppName::Blender,
            version: None,
        });
    }
    found
}

/// The app a `.desktop` file launches, when it is one we list.
fn parse_desktop_entry(text: &str) -> Option<AppInfo> {
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if let Some(name) = line.strip_prefix("Name=").filter(|_| in_entry) {
            let name = name.to_lowercase();
            let app = [
                ("blender", AppName::Blender),
                ("unreal engine", AppName::UnrealEngine),
                ("android studio", AppName::AndroidStudio),
                ("docker desktop", AppName::DockerDesktop),
            ]
            .into_iter()
            .find(|(prefix, _)| name.starts_with(prefix))?
            .1;
            return Some(AppInfo {
                name: app,
                version: None,
            });
        }
    }
    None
}

/// `CFBundleShortVersionString` in an `Info.plist` in XML form.
fn parse_info_plist_version(xml: &str) -> Option<String> {
    const KEY: &str = "<key>CFBundleShortVersionString</key>";
    let after = &xml[xml.find(KEY)? + KEY.len()..];
    let value = after.split_once("<string>")?.1.split_once("</string>")?.0;
    first_line(value)
}

#[cfg(test)]
mod tests {
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
        fs::create_dir_all(files.join("Blender Foundation/Blender 4.0"))
            .expect("a folder left behind");
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
}
