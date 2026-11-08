use super::*;
use std::{path::Path, process::Command};

#[test]
fn a_checkout_names_its_branch_and_a_plain_folder_none() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("skipped: no git");
        return;
    }
    let root = tempfile::tempdir().expect("a temp folder");
    let home = root.path();
    let repo = home.join("code/app");
    std::fs::create_dir_all(&repo).expect("folders");
    let plain = FolderPath::Home("code".into());
    assert_eq!(read_branch(&run(&branch_script(&plain), home)), None);
    let ok = Command::new("git")
        .args(["init", "-q", "-b", "work"])
        .current_dir(&repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .expect("git runs");
    assert!(ok.success());
    let app = FolderPath::Home("code/app".into());
    assert_eq!(
        read_branch(&run(&branch_script(&app), home)).as_deref(),
        Some("work")
    );
}

fn parse(input: &str) -> Result<Option<FolderPath>, ImportError> {
    FolderPath::parse(input)
}

#[test]
fn takes_absolute_and_home_paths_only() {
    assert_eq!(parse("  "), Ok(None));
    assert_eq!(
        parse("/srv//code/./app/"),
        Ok(Some(FolderPath::Absolute("/srv/code/app".into())))
    );
    assert_eq!(parse("/"), Ok(Some(FolderPath::Absolute("/".into()))));
    assert_eq!(
        parse("~/Documents/my code"),
        Ok(Some(FolderPath::Home("Documents/my code".into())))
    );
    assert_eq!(parse("~"), Ok(Some(FolderPath::Home(String::new()))));
    for bad in [
        "code/app",
        "~user/code",
        "/srv/../etc",
        "/srv/a\nb",
        r"C:\code",
    ] {
        assert_eq!(parse(bad), Err(ImportError::BadFolder), "{bad:?}");
    }
    assert_eq!(
        parse(&format!("/{}", "a".repeat(MAX_PATH))),
        Err(ImportError::BadFolder)
    );
}

#[test]
fn the_path_stays_one_shell_word() {
    let path = FolderPath::Absolute("/srv/it's $(rm -rf ~)".into());
    // The shell reads it back as the one path, running nothing.
    let read = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("printf %s {}", path.shell_word()))
        .output()
        .expect("sh runs");
    assert_eq!(read.stdout, b"/srv/it's $(rm -rf ~)");
    assert_eq!(FolderPath::Home(String::new()).shell_word(), "\"$HOME\"");
    assert_eq!(
        FolderPath::Home("a b".into()).shell_word(),
        "\"$HOME\"/'a b'"
    );
}

fn run(script: &str, home: &Path) -> String {
    let out = Command::new("bash")
        .arg("-c")
        .arg("exec 2>&1; eval \"$BEEP_SCRIPT\" < /dev/null")
        .env("BEEP_SCRIPT", script)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("bash runs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn lists_a_real_folder() {
    let root = tempfile::tempdir().expect("a temp folder");
    let home = root.path();
    std::fs::create_dir_all(home.join("Documents/b dir")).expect("folders");
    std::fs::create_dir_all(home.join("Documents/a")).expect("folders");
    std::fs::create_dir_all(home.join("Documents/.hidden")).expect("folders");
    std::fs::write(home.join("Documents/notes.txt"), "x").expect("a file");
    let Some(Seen::Folder(seen)) = read(&run(&script(&FolderPath::Home("Documents".into())), home))
    else {
        panic!("a folder");
    };
    assert_eq!(seen.path, home.join("Documents").to_string_lossy());
    assert_eq!(seen.folders, ["a", "b dir"]);
    assert!(!seen.empty && seen.origin.is_none());

    let Some(Seen::Folder(empty)) =
        read(&run(&script(&FolderPath::Home("Documents/a".into())), home))
    else {
        panic!("a folder");
    };
    assert!(empty.empty && empty.folders.is_empty());

    let gone = script(&FolderPath::Home("nope".into()));
    let Some(Seen::Folder(gone)) = read(&run(&gone, home)) else {
        panic!("a missing folder");
    };
    assert!(!gone.exists);
    assert_eq!(gone.path, home.join("nope").to_string_lossy());

    let file = script(&FolderPath::Home("Documents/notes.txt".into()));
    assert!(matches!(read(&run(&file, home)), Some(Seen::File(_))));

    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("skipped the checkout: no git");
        return;
    }
    let repo = home.join("Documents/b dir");
    for args in [
        &["init", "-q"][..],
        &["remote", "add", "origin", "https://example.com/o/r.git"][..],
    ] {
        let ok = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", home)
            .status()
            .expect("git runs")
            .success();
        assert!(ok, "git {args:?}");
    }
    let Some(Seen::Folder(checkout)) = read(&run(
        &script(&FolderPath::Home("Documents/b dir".into())),
        home,
    )) else {
        panic!("a checkout");
    };
    assert_eq!(
        checkout.origin.as_deref(),
        Some("https://example.com/o/r.git")
    );
}
