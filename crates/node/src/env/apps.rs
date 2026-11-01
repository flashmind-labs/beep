//! Creative and developer apps: app bundles on macOS, desktop entries on Linux.

use super::{PROBE_TIMEOUT, first_line};
use beep_proto::node::{AppInfo, AppName};
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

pub(crate) async fn apps() -> Vec<AppInfo> {
    let scan = match env::consts::OS {
        "macos" => spawn_blocking(|| mac_apps(Path::new(APPLICATIONS), Path::new(UNREAL_ROOT))),
        "linux" => spawn_blocking(linux_apps),
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
    let mut unreal: Vec<String> = fs::read_dir(unreal_root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let folder = entry.file_name().to_string_lossy().into_owned();
            folder.strip_prefix("UE_").map(str::to_owned)
        })
        .collect();
    unreal.sort();
    found.extend(unreal.into_iter().map(|version| AppInfo {
        name: AppName::UnrealEngine,
        version: Some(version),
    }));
    found
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
    use beep_proto::node::{AppInfo, AppName};

    #[test]
    fn desktop_entry_finds_blender() {
        let text = "[Desktop Entry]\nName=Blender\nExec=blender %f\nX-KDE-Example=1\n";
        assert_eq!(
            parse_desktop_entry(text),
            Some(AppInfo {
                name: AppName::Blender,
                version: None
            })
        );
        assert_eq!(
            parse_desktop_entry("[Desktop Entry]\nName=Calculator\n"),
            None
        );
    }

    #[test]
    fn plist_version_reads_short_version() {
        let xml = "<plist><dict>\n<key>CFBundleName</key>\n<string>Blender</string>\n\
            <key>CFBundleShortVersionString</key>\n\t<string>4.2.1</string>\n</dict></plist>";
        assert_eq!(parse_info_plist_version(xml).as_deref(), Some("4.2.1"));
        assert_eq!(parse_info_plist_version("<plist/>"), None);
    }
}
