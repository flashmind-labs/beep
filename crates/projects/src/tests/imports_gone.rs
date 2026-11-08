use super::*;
use std::{fs, path::Path, process::Command};

fn place(folder: &Path) -> Place {
    Place {
        project: "acme/widget".into(),
        folder: folder.to_string_lossy().into_owned(),
    }
}

fn run(script: &str) -> String {
    let out = Command::new("sh")
        .arg("-c")
        .arg(script)
        .output()
        .expect("sh runs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn only_a_deleted_folder_beside_others_counts_as_missing() {
    let root = tempfile::tempdir().expect("a temp folder");
    let code = root.path().join("code");
    fs::create_dir_all(code.join("kept")).expect("folders");
    fs::create_dir_all(code.join("other")).expect("folders");
    // A mount point with nothing mounted on it.
    let mount = root.path().join("disk");
    fs::create_dir_all(&mount).expect("folders");
    let places = [
        place(&code.join("kept")),
        place(&code.join("deleted")),
        place(&mount.join("project")),
        place(&root.path().join("nowhere/project")),
        place(Path::new("/mnt/project")),
        place(Path::new("/media/user/project")),
        place(&code.join("it's here")),
    ];
    fs::create_dir_all(code.join("it's here")).expect("folders");
    let output = run(&script(&places));
    assert_eq!(missing(&output, true, places.len()), [1], "{output}");
    // A look that did not finish says nothing.
    assert!(missing(&output, false, places.len()).is_empty());
}

#[test]
fn stray_or_out_of_range_lines_do_not_count() {
    let output = "BEEP_GONE: missing 7\nmissing 0\nBEEP_GONE: missing x\n\
                      BEEP_GONE: unsure 1\nBEEP_GONE: missing 2\r\n";
    assert_eq!(missing(output, true, 3), [2]);
}
