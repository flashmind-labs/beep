//! Bakes the server's node id in, from `BEEP_NODE_ID` or the profile
//! `just bake-profile` writes for the Android build.

use std::{env, fs, path::Path};

fn main() {
    let profile = Path::new("../../../.dev/android-assets/testing-profile.json");
    println!("cargo:rerun-if-env-changed=BEEP_NODE_ID");
    println!("cargo:rerun-if-changed={}", profile.display());
    let node_id = env::var("BEEP_NODE_ID")
        .ok()
        .filter(|id| !id.is_empty())
        .or_else(|| fs::read_to_string(profile).ok().and_then(|text| node_id(&text)))
        .unwrap_or_default();
    println!("cargo:rustc-env=BEEP_SERVER_NODE_ID={node_id}");
    tauri_build::build();
}

/// The 64 hex characters after `"node_id"` in the profile.
fn node_id(profile: &str) -> Option<String> {
    let after = &profile[profile.find("\"node_id\"")? + 9..];
    let start = after.find(|c: char| c.is_ascii_hexdigit())?;
    let id: String = after[start..]
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();
    (id.len() == 64).then_some(id)
}
