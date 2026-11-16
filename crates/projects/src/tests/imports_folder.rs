use super::*;
use std::{path::Path, process::Command};

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

fn first(output: &str) -> Option<Seen> {
    read(output).into_iter().next()
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
    std::fs::create_dir_all(home.join("Documents/clone/.git")).expect("folders");
    std::fs::create_dir_all(home.join("Documents/.hidden")).expect("folders");
    std::fs::write(home.join("Documents/notes.txt"), "x").expect("a file");
    let Some(Seen::Folder(seen)) = first(&run(
        &script(&FolderPath::Home("Documents".into()), 0),
        home,
    )) else {
        panic!("a folder");
    };
    assert_eq!(seen.path, home.join("Documents").to_string_lossy());
    assert_eq!(seen.folders, ["a", "b dir", "clone"]);
    assert_eq!(seen.repos, ["clone"]);
    assert!(!seen.empty && seen.origin.is_none());

    let Some(Seen::Folder(empty)) = first(&run(
        &script(&FolderPath::Home("Documents/a".into()), 0),
        home,
    )) else {
        panic!("a folder");
    };
    assert!(empty.empty && empty.folders.is_empty());

    let gone = script(&FolderPath::Home("nope".into()), 0);
    let Some(Seen::Folder(gone)) = first(&run(&gone, home)) else {
        panic!("a missing folder");
    };
    assert!(!gone.exists);
    assert_eq!(gone.path, home.join("nope").to_string_lossy());

    let file = script(&FolderPath::Home("Documents/notes.txt".into()), 0);
    assert!(matches!(first(&run(&file, home)), Some(Seen::File(_))));

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
    let Some(Seen::Folder(checkout)) = first(&run(
        &script(&FolderPath::Home("Documents/b dir".into()), 0),
        home,
    )) else {
        panic!("a checkout");
    };
    assert_eq!(
        checkout.origin.as_deref(),
        Some("https://example.com/o/r.git")
    );
}

#[test]
fn reads_three_levels_in_one_run() {
    let root = tempfile::tempdir().expect("a temp folder");
    let home = root.path();
    for dir in [
        "work/app/src",
        "work/clone/.git/objects",
        "work/clone/lib",
        "notes",
    ] {
        std::fs::create_dir_all(home.join(dir)).expect("folders");
    }
    let seen = read(&run(&script(&FolderPath::Home(String::new()), 2), home));
    let paths: Vec<_> = seen
        .iter()
        .map(|seen| match seen {
            Seen::Folder(folder) => folder
                .path
                .strip_prefix(&*home.to_string_lossy())
                .unwrap_or(&folder.path)
                .to_owned(),
            other => panic!("a folder, not {other:?}"),
        })
        .collect();

    // The folder, the folders in it, then theirs; nothing inside a checkout, nothing deeper.
    assert_eq!(paths, ["", "/notes", "/work", "/work/app", "/work/clone"]);
    let Seen::Folder(work) = &seen[2] else {
        panic!("a folder");
    };
    assert_eq!(work.folders, ["app", "clone"]);
    assert_eq!(work.repos, ["clone"]);
}
