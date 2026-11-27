use super::folder::Folders;
use super::harness::Stored;
use super::*;
use crate::logs::found::Agent;
use std::path::Path;

const ID: &str = "0b1c2d3e-0000-4000-8000-0123456789ab";

fn move_with(harness: Harness, transcript: &str, file: &str) -> Stored {
    store::rewrite(
        harness,
        transcript.as_bytes(),
        &SessionId::from(ID),
        file,
        &Folders {
            old: PathBuf::from("/srv/laptop/app"),
            new: PathBuf::from("/work/app"),
        },
    )
}

fn text(stored: &Stored) -> &str {
    std::str::from_utf8(&stored.bytes).expect("utf8")
}

#[test]
fn claude_changes_cwd_on_each_line_and_files_under_the_new_slug() {
    let input = concat!(
        "{\"type\":\"user\",\"cwd\":\"/srv/laptop/app\",\"message\":{\"content\":\"cwd is \\\"/srv/laptop/app\\\"\"}}\n",
        "{\"cwd\":\"/srv/laptop/app/sub\",\"type\":\"assistant\", \"n\": 1.50}\n",
        "{\"type\":\"summary\"}\n",
        "{\"cwd\":\"/elsewhere\"}\n",
        "{\"cwd\":\"/srv/laptop/app\",\"type\":\"user\"}",
    );
    let out = move_with(Harness::Claude, input, "x.jsonl");
    let want = concat!(
        "{\"type\":\"user\",\"cwd\":\"/work/app\",\"message\":{\"content\":\"cwd is \\\"/srv/laptop/app\\\"\"}}\n",
        "{\"cwd\":\"/work/app/sub\",\"type\":\"assistant\", \"n\": 1.50}\n",
        "{\"type\":\"summary\"}\n",
        "{\"cwd\":\"/elsewhere\"}\n",
        "{\"cwd\":\"/work/app\",\"type\":\"user\"}",
    );
    assert_eq!(text(&out), want);
    assert_eq!(out.path, PathBuf::from(format!("-work-app/{ID}.jsonl")));
}

#[test]
fn claude_keeps_a_line_it_cannot_read() {
    let input = "not json \"cwd\":\"/srv/laptop/app\"\n{\"cwd\":\n";
    let out = move_with(Harness::Claude, input, "x.jsonl");
    assert_eq!(text(&out), input);
}

#[test]
fn codex_changes_meta_roots_and_turn_contexts_only() {
    let file = format!("rollout-2026-11-04T09-30-00-{ID}.jsonl");
    let input = concat!(
        "{\"timestamp\":\"t\",\"type\":\"session_meta\",\"payload\":{\"id\":\"x\",\"cwd\":\"/srv/laptop/app\",\"runtime_workspace_roots\":[\"/srv/laptop/app\",\"/other\"],\"note\":\"/srv/laptop/app\"}}\n",
        "{\"type\":\"turn_context\",\"payload\":{\"cwd\":\"file:///srv/laptop/app/my%20dir\",\"model\":\"m\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"cwd\":\"/srv/laptop/app\",\"text\":\"/srv/laptop/app\"}}\n",
    );
    let out = move_with(Harness::Codex, input, &file);
    let want = concat!(
        "{\"timestamp\":\"t\",\"type\":\"session_meta\",\"payload\":{\"id\":\"x\",\"cwd\":\"/work/app\",\"runtime_workspace_roots\":[\"/work/app\",\"/other\"],\"note\":\"/srv/laptop/app\"}}\n",
        "{\"type\":\"turn_context\",\"payload\":{\"cwd\":\"file:///work/app/my%20dir\",\"model\":\"m\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"cwd\":\"/srv/laptop/app\",\"text\":\"/srv/laptop/app\"}}\n",
    );
    assert_eq!(text(&out), want);
    assert_eq!(out.path, PathBuf::from("sessions/2026/11/04").join(&file));
}

#[test]
fn pi_changes_the_header_only_and_files_under_the_folder_name() {
    let file = format!("2026-11-04T09-30-00-000Z_{ID}.jsonl");
    let input = concat!(
        "{\"type\":\"session\",\"version\":3,\"id\":\"x\",\"cwd\":\"/srv/laptop/app\"}\n",
        "{\"type\":\"message\",\"cwd\":\"/srv/laptop/app\"}\n",
    );
    let out = move_with(Harness::Pi, input, &file);
    let want = concat!(
        "{\"type\":\"session\",\"version\":3,\"id\":\"x\",\"cwd\":\"/work/app\"}\n",
        "{\"type\":\"message\",\"cwd\":\"/srv/laptop/app\"}\n",
    );
    assert_eq!(text(&out), want);
    assert_eq!(out.path, PathBuf::from("--work-app--").join(&file));
}

#[test]
fn a_folder_that_only_shares_a_prefix_is_not_inside() {
    let input = "{\"cwd\":\"/srv/laptop/app2\"}\n";
    let out = move_with(Harness::Claude, input, "x.jsonl");
    assert_eq!(text(&out), input);
}

#[test]
fn only_stores_in_files_have_a_harness() {
    assert_eq!(Harness::try_from(Agent::Pi).ok(), Some(Harness::Pi));
    assert_eq!(Harness::try_from(Agent::Flsh).ok(), Some(Harness::Flsh));
    assert_eq!(Harness::try_from(Agent::OpenCode).ok(), None);
}

#[test]
fn flsh_changes_the_header_folder_only_and_files_under_flshs_names() {
    let input = include_str!("../logs/found/fixtures/flsh.jsonl");
    let out = move_with(Harness::Flsh, input, "ignored.jsonl");
    let (header, rest) = input.split_once('\n').expect("a header");
    let want = header.replace(
        r#""working_dir":"/srv/laptop/app""#,
        r#""working_dir":"/work/app""#,
    );
    assert_ne!(want, header);
    assert_eq!(text(&out), format!("{want}\n{rest}"));
    // flsh 0.2.10's folder for sessions run in `/work/app`, and its name
    // for session `ID`'s file.
    let name = "6d1024705c6059e7586a8d74fd797093cef21babfe58070e31ad35f6b58a86d7.jsonl";
    assert_eq!(out.path, PathBuf::from("455e16743bf22d19").join(name));
}

#[test]
fn paths_in_the_project_follow_the_destination_for_every_harness() {
    let (from, to) = (
        Path::new("/srv/source/project"),
        Path::new("/srv/target/project"),
    );
    let old = from.join(".worktrees/fix/src");
    let new = to.join(".worktrees/fix/src");
    let file = format!("rollout-2026-11-04T09-30-00-{ID}.jsonl");
    for (harness, input) in [
        (
            Harness::Claude,
            format!(r#"{{"type":"user","cwd":"{}"}}"#, old.display()),
        ),
        (
            Harness::Codex,
            format!(
                r#"{{"type":"session_meta","payload":{{"id":"{ID}","cwd":"{}","runtime_workspace_roots":["{}"]}}}}"#,
                old.display(),
                old.display()
            ),
        ),
        (
            Harness::Pi,
            format!(
                r#"{{"type":"session","version":3,"id":"{ID}","cwd":"{}"}}"#,
                old.display()
            ),
        ),
        (
            Harness::Flsh,
            format!(
                r#"{{"type":"session_metadata","metadata":{{"id":"{ID}","working_dir":"{}"}}}}"#,
                old.display()
            ),
        ),
    ] {
        let out = store::rewrite(
            harness,
            input.as_bytes(),
            &SessionId::from(ID),
            &file,
            &Folders {
                old: from.to_owned(),
                new: to.to_owned(),
            },
        );
        assert!(
            text(&out).contains(new.to_str().unwrap()),
            "{harness:?}: {}",
            text(&out)
        );
        assert!(
            !text(&out).contains(old.to_str().unwrap()),
            "{harness:?}: {}",
            text(&out)
        );
    }
}

#[test]
fn a_moved_session_begins_as_its_source_saved_it() {
    use super::resume::Arrival;
    use crate::agents::SavedSession;
    use beep_proto::{
        ModelId,
        node::agent::{Effort, SessionInfo},
    };

    let from = iroh::SecretKey::generate().public();
    let saved = SavedSession {
        info: SessionInfo {
            id: "held-1".into(),
            title: Some("Plan the week".into()),
            model: Some(ModelId::new("deep")),
            assistant: true,
            ..SessionInfo::default()
        },
        effort: Some(Effort::High),
        fast: Some(true),
        account: None,
        named: false,
    };
    let arrival = Arrival {
        id: &SessionId::from("held-1"),
        mode: Some("plan".into()),
        model: Some(ModelId::new("fast")),
        worktrees: Vec::new(),
        from,
        saved: Some(saved),
        chat: false,
    };
    let me = iroh::SecretKey::generate().public();
    let begin = arrival.begin("moved".into(), "brief".into(), me);
    assert_eq!(begin.title.as_deref(), Some("Plan the week"));
    assert_eq!(begin.model, Some(ModelId::new("deep")));
    assert_eq!(begin.mode.as_deref(), Some("plan"));
    assert_eq!(begin.effort, Some(Effort::High));
    assert_eq!(begin.fast, Some(true));
    assert!(begin.assistant);

    // A source from before saved sessions sends only the move record's.
    let older = Arrival {
        saved: None,
        ..arrival
    };
    let begin = older.begin("moved".into(), "brief".into(), me);
    assert_eq!(begin.model, Some(ModelId::new("fast")));
    assert!(!begin.assistant);
    assert_eq!(begin.title, None);
}

#[test]
fn a_thread_that_moves_with_its_chat_stays_its_chats() {
    use super::resume::{Arrival, kept_here};
    use crate::agents::SavedSession;
    use beep_proto::node::agent::{ParentLink, SessionInfo};

    let (from, me, elsewhere) = (
        iroh::SecretKey::generate().public(),
        iroh::SecretKey::generate().public(),
        iroh::SecretKey::generate().public(),
    );
    let link = ParentLink {
        machine: Some(from),
        session: "chat-1".into(),
        key: "a3f9c1".into(),
        root_machine: Some(from),
        root: "chat-1".into(),
        depth: 1,
        others: 0,
        agent: "claude".into(),
        mode: None,
        background: false,
    };
    let saved = |parent: ParentLink| SavedSession {
        info: SessionInfo {
            id: "held-1".into(),
            title: Some("Pricing".into()),
            cwd: "/srv/laptop/.beep/assistant/ab12/pricing".into(),
            parent: Some(parent),
            ..SessionInfo::default()
        },
        effort: None,
        fast: None,
        account: None,
        named: true,
    };
    let thread = Arrival {
        id: &SessionId::from("held-1"),
        mode: None,
        model: None,
        worktrees: Vec::new(),
        from,
        saved: Some(saved(link.clone())),
        chat: true,
    };

    // Its chat runs here now, under its own id, and its name is its parent's.
    let begin = thread.begin("moved".into(), "brief".into(), me);
    let parent = begin.parent.expect("it keeps its parent");
    assert_eq!((parent.machine, parent.root_machine), (Some(me), Some(me)));
    assert_eq!(
        (parent.session.as_ref(), parent.key.as_ref()),
        ("chat-1", "chat-1")
    );
    assert!(begin.named);

    // One that came ended is kept in its folder here, under its chat.
    let kept = kept_here(&thread, Path::new("/work/.beep/assistant/ab12/pricing"), me)
        .expect("what the source saved stays");
    assert_eq!(kept.info.cwd, "/work/.beep/assistant/ab12/pricing");
    assert_eq!(kept.info.parent.and_then(|link| link.machine), Some(me));

    // A parent on a third machine stays where it is.
    let remote = Arrival {
        saved: Some(saved(ParentLink {
            machine: Some(elsewhere),
            ..link.clone()
        })),
        ..thread
    };
    let parent = remote.begin("moved".into(), "brief".into(), me).parent;
    assert_eq!(parent.and_then(|link| link.machine), Some(elsewhere));

    // A project's session leaves the one that started it behind.
    let project = Arrival {
        saved: Some(saved(link)),
        chat: false,
        ..remote
    };
    assert_eq!(
        project.begin("moved".into(), "brief".into(), me).parent,
        None
    );
}
