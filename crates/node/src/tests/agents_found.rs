use super::*;

fn seen(agent: Agent, id: &str, cwd: &Path, at: u64) -> Seen {
    Seen {
        agent,
        id: id.into(),
        cwd: cwd.to_owned(),
        started_at: at,
        last_activity: at,
        title: None,
        file: None,
    }
}

#[test]
fn sessions_fold_into_their_checkout() {
    let root = tempfile::tempdir().expect("a temp folder");
    let home = root.path().join("home/user");
    let repo = home.join("proj");
    let sub = repo.join("crates/core");
    let tree = repo.join(".worktrees/fix");
    let loose = home.join("notes");
    for dir in [&sub, &tree, &loose] {
        std::fs::create_dir_all(dir).expect("folders");
    }
    let gone = home.join("gone");
    let all = vec![
        seen(Agent::Claude, "a", &repo, 100),
        seen(Agent::Claude, "b", &sub, 300),
        seen(Agent::Codex, "c", &tree, 200),
        seen(Agent::Pi, "d", &loose, 400),
        seen(Agent::Claude, "e", &home, 500),
        seen(Agent::Flsh, "f", Path::new("/"), 600),
        seen(Agent::Codex, "g", &gone, 700),
    ];
    let lookup = |dir: &Path| {
        dir.starts_with(&repo).then(|| Checkout {
            root: repo.clone(),
            origin: "git@example.com:org/proj.git".into(),
        })
    };
    let found = folders(&all, Some(&home), 10, lookup);
    assert_eq!(found.len(), 1, "{found:?}");
    let one = &found[0];
    assert_eq!(one.path, repo.to_string_lossy());
    assert_eq!(one.origin, "git@example.com:org/proj.git");
    assert_eq!(one.last_activity, 300);
    assert_eq!(
        one.agents,
        [
            AgentSessions {
                agent: "claude".into(),
                sessions: 2
            },
            AgentSessions {
                agent: "codex".into(),
                sessions: 1
            },
        ]
    );
}

#[test]
fn a_checkout_at_home_is_left_out() {
    let root = tempfile::tempdir().expect("a temp folder");
    let home = root.path().to_owned();
    let all = vec![seen(Agent::Claude, "a", &home, 1)];
    let lookup = |_: &Path| {
        Some(Checkout {
            root: home.clone(),
            origin: String::new(),
        })
    };
    assert!(folders(&all, Some(&home), 10, lookup).is_empty());
}

/// Writes `text` over `path` and gives it back its modification time,
/// so only a read of its content can tell it changed.
fn rewrite_in_place(path: &Path, text: &str) {
    let modified = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .expect("a modification time");
    std::fs::write(path, text).expect("the rewrite");
    File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(modified))
        .expect("the old time back");
}

#[test]
fn an_unchanged_session_file_is_not_read_again_and_a_changed_one_is() {
    let root = tempfile::tempdir().expect("a temp folder");
    let dir = root.path().join("--home-user-proj--");
    std::fs::create_dir_all(&dir).expect("folders");
    let path = dir.join("2026-08-27T23-01-39-077Z_p1.jsonl");
    let session = |cwd: &str, prompt: &str| {
        format!(
            "{{\"type\":\"session\",\"version\":3,\"id\":\"p1\",\"timestamp\":\"2026-08-27T23:01:39.077Z\",\"cwd\":\"{cwd}\"}}\n\
             {{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"{prompt}\"}}}}\n"
        )
    };
    std::fs::write(&path, session("/home/user/proj", "Tidy up")).expect("a session");
    let stores = Stores::at(None, Some(root.path()), None);
    let found = stores.find(Agent::Pi, "p1").expect("the session");
    assert_eq!(found.title.as_deref(), Some("Tidy up"));

    rewrite_in_place(&path, &session("/home/user/proi", "Tidy on"));
    let found = stores.find(Agent::Pi, "p1").expect("the session");
    assert_eq!(found.cwd, "/home/user/proj", "kept from the first read");
    assert_eq!(found.title.as_deref(), Some("Tidy up"));

    std::fs::write(&path, session("/home/user/other", "Tidy it all up")).expect("a session");
    let found = stores.find(Agent::Pi, "p1").expect("the session");
    assert_eq!(found.cwd, "/home/user/other");
    assert_eq!(found.title.as_deref(), Some("Tidy it all up"));
}

#[test]
fn tagged_prompts_are_not_titles() {
    assert_eq!(prompt_title("  <environment_context>"), None);
    assert_eq!(prompt_title(" Fix it "), Some("Fix it".into()));
    assert_eq!(prompt_title(""), None);
}
