use super::*;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn only_new_artifacts_outside_ignored_folders_are_found() {
    let dir = TempDir::new().expect("a temp folder");
    let cwd = dir.path();
    fs::write(cwd.join("Cargo.toml"), "").expect("write");
    fs::write(cwd.join("old.html"), "<p>old</p>").expect("write");
    let since = SystemTime::now() + Duration::from_secs(60);
    // Files stamped after `since`, so the test needs no sleep.
    let stamp = |path: &Path| {
        let file = fs::File::options().write(true).open(path).expect("open");
        file.set_modified(since + Duration::from_secs(1))
            .expect("stamp");
    };
    for path in [
        "out/coverage.pdf",
        "target/doc/index.html",
        "notes.txt",
        "written.md",
    ] {
        let path = cwd.join(path);
        fs::create_dir_all(path.parent().expect("a folder")).expect("mkdir");
        fs::write(&path, "x").expect("write");
        stamp(&path);
    }
    let found = find(cwd, since, &[PathBuf::from("written.md")]);
    assert_eq!(found, ["out/coverage.pdf"]);
    assert_eq!(note(&found), "(made out/coverage.pdf)");
}
