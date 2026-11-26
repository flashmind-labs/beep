use super::*;
use std::os::unix::fs::PermissionsExt;

fn install(dir: &Path, program: &str) -> PathBuf {
    fs::create_dir_all(dir).expect("folders");
    let path = dir.join(program);
    fs::write(&path, "#!/bin/sh\n").expect("a program");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable");
    path
}

#[test]
fn a_program_is_found_in_the_install_folders_under_home() {
    let home = tempfile::tempdir().expect("a temp folder");
    let name = "beep-test-agent-cli";
    assert_eq!(fallback(name, home.path()), None);
    let local = install(&home.path().join(".local/bin"), name);
    assert_eq!(fallback(name, home.path()), Some(local));
    let bun = install(&home.path().join(".bun/bin"), "beep-test-bun-cli");
    assert_eq!(fallback("beep-test-bun-cli", home.path()), Some(bun));
}

#[test]
fn the_newest_node_version_of_nvm_wins() {
    let home = tempfile::tempdir().expect("a temp folder");
    let versions = home.path().join(".nvm/versions/node");
    install(&versions.join("v9.11.0/bin"), "beep-test-nvm-cli");
    let newest = install(&versions.join("v20.3.1/bin"), "beep-test-nvm-cli");
    assert_eq!(fallback("beep-test-nvm-cli", home.path()), Some(newest));
}

#[test]
fn a_found_folder_goes_first_on_the_path() {
    let path = path_with(&[Path::new("/home/user/.bun/bin/claude")]).expect("a path");
    let first = env::split_paths(&path).next().expect("a folder");
    assert_eq!(first, Path::new("/home/user/.bun/bin"));
}

#[test]
fn the_nodes_own_node_goes_before_the_cli() {
    let node = Path::new("/srv/node/agent-adapters/node/bin/node");
    let cli = Path::new("/home/user/.local/bin/claude");
    let path = path_with(&[node, cli]).expect("a path");
    let first: Vec<PathBuf> = env::split_paths(&path).take(2).collect();
    assert_eq!(
        first,
        [
            PathBuf::from("/srv/node/agent-adapters/node/bin"),
            PathBuf::from("/home/user/.local/bin")
        ]
    );
}
