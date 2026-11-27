//! A node lists the checkouts sessions worked in without running git: it
//! reads them in process. A wrapper on `PATH` notes any run. One test in this file: it sets `HOME`
//! and `PATH` for the whole process.
#![cfg(unix)]

use beep_node::agents::{Agents, Parts};
use std::{env, fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
use tempfile::TempDir;

const ASK: u32 = 100;
const ASKS_AT_ONCE: usize = 8;

fn git(dir: &Path, args: &[&str], date: Option<u64>) {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com");

    if let Some(seconds) = date {
        let date = format!("@{seconds} +0000");
        command
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date);
    }

    let status = command.status().expect("git runs");
    assert!(status.success(), "git {args:?}");
}

/// The git runs the wrapper noted since the last call.
fn runs(log: &Path) -> Vec<String> {
    let text = fs::read_to_string(log).unwrap_or_default();
    fs::write(log, "").expect("the log emptied");
    text.lines().map(str::to_owned).collect()
}

#[test]
fn listing_checkouts_runs_no_git_and_sees_changes() {
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
    let Some(real_git) = real_git else {
        eprintln!("skipped: no git");
        return;
    };
    let temp = TempDir::new().expect("a temp folder");
    let home = temp.path().join("home").join("user");
    fs::create_dir_all(&home).expect("a home");
    let home = home.canonicalize().expect("a real path");
    let dir = temp.path().join("node");
    fs::create_dir_all(&dir).expect("the node's folder");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).expect("a bin folder");
    let log = temp.path().join("git.log");
    let wrapper = bin.join("git");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec '{real_git}' \"$@\"\n",
            log.display()
        ),
    )
    .expect("the wrapper");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).expect("executable");

    let widget = home.join("Projects/widget");
    let gadget = home.join("Projects/gadget");
    for repo in [&widget, &gadget] {
        fs::create_dir_all(repo).expect("a folder");
        git(repo, &["init", "-q", "-b", "main"], None);
        git(
            repo,
            &["commit", "-q", "--allow-empty", "-m", "a"],
            Some(1_000_000_000),
        );
    }

    // SAFETY: the only test in this binary, and no other thread started yet.
    unsafe {
        env::set_var("HOME", &home);
        env::set_var(
            "PATH",
            format!("{}:{}", bin.display(), env::var("PATH").unwrap_or_default()),
        );
        for store in [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "FLASHMIND_HOME",
            "OPENCODE_DB",
            "PI_CODING_AGENT_DIR",
            "PI_CODING_AGENT_SESSION_DIR",
            "XDG_DATA_HOME",
        ] {
            env::remove_var(store);
        }
    }

    tokio::runtime::Runtime::new()
        .expect("a runtime")
        .block_on(async {
            let agents = Agents::new(Parts::default());
            agents.listings.warm(Some(home.clone()), dir.clone()).await;
            runs(&log);

            // Asks that come together share one listing.
            let asks = (0..ASKS_AT_ONCE).map(|_| agents.listings.found_folders(ASK));
            let answers = futures::future::join_all(asks).await;
            let first = answers[0].as_ref().expect("the node can say").clone();
            for answer in &answers {
                assert_eq!(answer.as_ref().expect("the node can say"), &first);
            }
            assert_eq!(runs(&log), Vec::<String>::new());

            agents
                .listings
                .found_folders(ASK)
                .await
                .expect("the node can say");
            assert_eq!(runs(&log), Vec::<String>::new());

            // A commit shows, and only its checkout is asked again.
            git(
                &widget,
                &["commit", "-q", "--allow-empty", "-m", "b"],
                Some(1_100_000_000),
            );
            runs(&log);
            let after = agents
                .listings
                .found_folders(ASK)
                .await
                .expect("the node can say");
            let changed = after
                .iter()
                .find(|folder| Path::new(&folder.path) == widget)
                .expect("the checkout is listed");
            assert_eq!(changed.last_activity, 1_100_000_000);
            assert_eq!(runs(&log), Vec::<String>::new());

            // So does a branch switch.
            git(&widget, &["switch", "-q", "-c", "old", "HEAD~1"], None);
            runs(&log);
            let switched = agents
                .listings
                .found_folders(ASK)
                .await
                .expect("the node can say");
            let back = switched
                .iter()
                .find(|folder| Path::new(&folder.path) == widget)
                .expect("the checkout is listed");
            assert_eq!(back.last_activity, 1_000_000_000);
        });
}
