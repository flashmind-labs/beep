//! The mobile platforms a checkout builds for, found by the names of its
//! files and folders, never their contents.

use beep_proto::node::device::Platform;
use ignore::{DirEntry, WalkBuilder};
use std::{
    ffi::OsStr,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// How deep the walk goes; Android Studio keeps its manifest four levels
/// down, Flutter five.
const MAX_DEPTH: usize = 6;
/// The most entries one walk visits, so a huge folder answers quickly.
const MAX_ENTRIES: usize = 20_000;

/// The platforms the app in `root` builds for, Android first. The walk
/// honors `.gitignore` and skips hidden folders. Blocking.
pub(super) fn platforms(root: &Path) -> Vec<Platform> {
    let mut android = false;
    // Set by the walk's filter, which sees the Xcode bundles it skips.
    let bundle = Arc::new(AtomicBool::new(false));
    let seen = bundle.clone();

    let walk = WalkBuilder::new(root)
        .max_depth(Some(MAX_DEPTH))
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            let skip = is_bundle(entry);
            if skip {
                seen.store(true, Ordering::Relaxed);
            }
            !skip
        })
        .build();
    let mut ios = false;

    for entry in walk.filter_map(Result::ok).take(MAX_ENTRIES) {
        match marks(&entry) {
            Some(Platform::Android) => android = true,
            Some(Platform::Ios) => ios = true,
            None => {}
        }

        ios |= bundle.load(Ordering::Relaxed);

        if android && ios {
            break;
        }
    }

    ios |= bundle.load(Ordering::Relaxed);

    [(android, Platform::Android), (ios, Platform::Ios)]
        .into_iter()
        .filter_map(|(found, platform)| found.then_some(platform))
        .collect()
}

/// The platform file `entry` marks a project for, if any; Xcode bundles
/// are seen by the walk's filter.
fn marks(entry: &DirEntry) -> Option<Platform> {
    if entry.file_type().is_some_and(|kind| kind.is_dir()) {
        return None;
    }

    let name = entry.file_name().to_str()?;
    let parent = entry
        .path()
        .parent()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str);

    match (name, parent) {
        ("AndroidManifest.xml" | "tauri.android.conf.json", _)
        | ("build.gradle" | "build.gradle.kts", Some("android")) => Some(Platform::Android),
        ("tauri.ios.conf.json", _) | ("Podfile", Some("ios")) => Some(Platform::Ios),
        _ => None,
    }
}

/// An Xcode project or workspace: an iOS marker the walk does not enter.
fn is_bundle(entry: &DirEntry) -> bool {
    entry.depth() > 0
        && entry.file_type().is_some_and(|kind| kind.is_dir())
        && Path::new(entry.file_name())
            .extension()
            .is_some_and(|ext| ext == "xcodeproj" || ext == "xcworkspace")
}

#[cfg(test)]
#[path = "../../tests/run_device_platforms.rs"]
mod tests;
