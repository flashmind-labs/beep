//! Finding a project's checkouts on a machine: what the server's look at a node
//! gets from `found_folders`, against real git repositories under a search
//! folder. One test in this file: it sets `HOME` for the whole process, so the
//! agents' own logs on the machine running it stay out.
#![cfg(unix)]

use beep_node::agents::Agents;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

const ORIGIN: &str = "git@example.com:acme/widget.git";
/// More than the most a look used to be able to get back.
const MANY: usize = 300;
const ASK: u32 = 800;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?}");
}

fn repo(path: &Path, origin: &str) {
    fs::create_dir_all(path).expect("a folder");
    git(path, &["init", "-q"]);
    git(path, &["remote", "add", "origin", origin]);
}

async fn found(agents: &Agents) -> Vec<(PathBuf, String)> {
    agents
        .found_folders(ASK)
        .await
        .expect("the node can say")
        .into_iter()
        .map(|folder| (PathBuf::from(folder.path), folder.origin))
        .collect()
}

#[test]
fn a_checkout_under_the_search_folder_is_found_whatever_was_looked_at_before() {
    let temp = TempDir::new().expect("a temp folder");
    let home = temp.path().join("home").join("user");
    fs::create_dir_all(&home).expect("a home");
    let home = home.canonicalize().expect("a real path");
    let dir = temp.path().join("node");
    fs::create_dir_all(&dir).expect("the node's folder");
    // SAFETY: the only test in this binary, and no other thread started yet.
    unsafe {
        env::set_var("HOME", &home);
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
    let widget = home.join("Projects/acme/github/widget");
    repo(&widget, ORIGIN);

    tokio::runtime::Runtime::new()
        .expect("a runtime")
        .block_on(async {
            // The node just started: the walk has not ended, and an answer
            // without the checkout would be taken to mean it is not there.
            let starting = Agents::new(Vec::new());
            assert!(
                starting.found_folders(ASK).await.is_err(),
                "a node that has not looked yet must not answer"
            );

            starting.warm(Some(home.clone()), dir.clone()).await;
            let first = found(&starting).await;
            assert!(
                first.contains(&(widget.clone(), ORIGIN.to_owned())),
                "{first:?}"
            );

            // A checkout cloned later is found by the next start, on the list
            // the first walk saved.
            let later = home.join("Projects/acme/github/later");
            repo(&later, "https://example.com/acme/later.git");
            let restarted = Agents::new(Vec::new());
            restarted.warm(Some(home.clone()), dir.clone()).await;
            let second = found(&restarted).await;
            assert!(second.iter().any(|(path, _)| *path == later), "{second:?}");

            // A machine with many checkouts gets every one back.
            for one in 0..MANY {
                fs::create_dir_all(home.join(format!("many/{one}/.git"))).expect("a checkout");
            }
            let full = Agents::new(Vec::new());
            full.warm(Some(home.clone()), dir.clone()).await;
            let all = found(&full).await;
            assert!(all.len() > MANY, "only {} came back", all.len());
            assert!(all.iter().any(|(path, _)| *path == widget));
        });
}
