use super::*;
use crate::agents::Parts;
use crate::agents::{AgentKind, OpenSession, SavedSession, family::links::{Links, Waits}};
use crate::team::{asks::Delivery, link::TeamLink};
use beep_proto::node::agent::{Persona, Request};
use beep_proto::watched;
use std::{collections::BTreeMap, path::Path};
use tokio::time::timeout;

/// An ACP adapter in shell: it logs what it reads to `$1`, answers each
/// prompt at once, except one that starts with `hang`, which ends when the
/// turn is cancelled, says "It is done." to one that holds `say it` and
/// names its session on one that holds `name it`.
/// Each run names its session after its process.
const ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line; printf '%s\n' "$line" >> "$1"
reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}'
while read -r line; do
  printf '%s\n' "$line" >> "$1"
  case "$line" in
    *'"session/new"'*) reply '{"sessionId":"s-'$$'"}' ;;
    *'"session/load"'*) reply '{}' ;;
    *'"text":"hang'*) prompt=$(id) ;;
    *'"session/cancel"'*) [ -n "$prompt" ] && printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$prompt" ;;
    *'name it'*) printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-%s","update":{"sessionUpdate":"session_info_update","title":"Named by the agent"}}}\n' "$$"; reply '{"stopReason":"end_turn"}' ;;
    *'say it'*) printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"It is done."}}}}\n' "$$"; reply '{"stopReason":"end_turn"}' ;;
    *'"session/prompt"'*) reply '{"stopReason":"end_turn"}' ;;
    *'"method"'*'"id"'*|*'"id"'*'"method"'*) reply '{}' ;;
  esac
done
"#;

fn kind(name: &str, log: &Path) -> AgentKind {
    AgentKind {
        name: name.into(),
        program: "sh".into(),
        args: vec![
            "-c".into(),
            ADAPTER.into(),
            "fake-adapter".into(),
            log.display().to_string(),
        ],
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    }
}

async fn until(session: &Session, done: impl FnMut(&Live) -> bool) {
    let mut live = session.live.clone();
    timeout(Duration::from_secs(20), watched::wait_for(&mut live, done))
        .await
        .expect("the session got there in time")
        .expect("the session is kept");
}

/// A session the user began, once it took its first prompt.
async fn root(agents: &Agents, harness: &AgentKind, dir: &Path) -> Arc<Session> {
    let session = agents
        .start(
            harness,
            Some(&dir.to_string_lossy()),
            Begin {
                prompt: Some("go".into()),
                ..Begin::default()
            },
        )
        .await.expect("the session starts");
    until(&session, |live| {
        live.agent_session.is_some() && live.info.turns == 1
    })
    .await;
    session
}

fn start(harness: &str, prompt: &str, dir: &Path) -> ChildStart {
    ChildStart {
        harness: harness.into(),
        prompt: prompt.into(),
        cwd: Some(dir.to_string_lossy().into_owned()),
        model: None,
        review: None,
        account: None,
        title: None,
        persona: None,
    }
}

/// A child of `parent`, started the way `start_agent` starts one here.
async fn child_of(
    agents: &Agents,
    parent: &Arc<Session>,
    harness: &str,
    prompt: &str,
    dir: &Path,
) -> Arc<Session> {
    let who = agents.who_of(parent);
    let link = agents.link_for(parent, &who).expect("within limits");
    agents
        .start_child(link, start(harness, prompt, dir))
        .await
        .expect("the child starts")
}

struct Rig {
    dir: tempfile::TempDir,
    agents: Agents,
    top: AgentKind,
    kid: AgentKind,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let top = kind("top", &dir.path().join("top.log"));
        let kid = kind("kid", &dir.path().join("kid.log"));
        let agents = Agents::new(Parts { kinds: vec![top.clone(), kid.clone()], ..Parts::default() });
        Self {
            dir,
            agents,
            top,
            kid,
        }
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(name)).unwrap_or_default()
    }
}

#[tokio::test]
async fn a_child_is_a_session_linked_to_its_parent() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    let info = child.info();
    let link = info.parent.expect("the child names its parent");
    assert_eq!(link.session, Agents::name_of(&parent));
    assert_eq!(link.root, link.session);
    assert_eq!(link.depth, 1);
    assert_eq!(info.agent, "kid");
    let who = rig.agents.who_of(&parent);
    assert_eq!(rig.agents.children_of(&who).len(), 1);
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn child_completions_reach_the_parent_without_user_alerts() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let alerts = rig.agents.alerts.drain();
    assert_eq!(alerts.len(), 1, "the root turn notifies the user");
    let child = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    until(&child, |live| live.info.turns == 1).await;
    assert!(rig.agents.alerts.drain().is_empty());
    let who = rig.agents.who_of(&parent);
    let children = rig.agents.children_of(&who);
    assert_eq!(children.len(), 1);
    assert!(Arc::ptr_eq(&children[0], &child));
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_chain_may_go_three_deep_and_no_deeper() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let first = child_of(&rig.agents, &parent, "kid", "one", rig.dir.path()).await;
    let second = child_of(&rig.agents, &first, "kid", "two", rig.dir.path()).await;
    let third = child_of(&rig.agents, &second, "kid", "three", rig.dir.path()).await;
    assert_eq!(third.info().parent.expect("a link").depth, MAX_DEPTH);
    let who = rig.agents.who_of(&third);
    let refused = rig.agents.link_for(&third, &who).unwrap_err();
    assert!(refused.contains("3 levels deep"), "{refused}");
    // Every child of the chain still counts towards the root.
    let root_id = Agents::name_of(&parent);
    assert_eq!(rig.agents.live_descendants(None, &root_id), 3);
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn the_depth_a_caller_on_another_machine_passes_is_checked_here() {
    let rig = Rig::new();
    let far = SecretKeyFor::id();
    let link = |depth| ParentLink {
        machine: Some(far),
        session: "p1".into(),
        key: "p1".into(),
        root_machine: Some(far),
        root: "p0".into(),
        depth,
        others: 0,
        agent: "kid".into(),
        mode: None,
        background: false,
    };
    let refused = rig
        .agents
        .start_child(link(MAX_DEPTH + 1), start("kid", "go", rig.dir.path()))
        .await
        .unwrap_err();
    assert!(refused.contains("3 levels deep"), "{refused}");
    // A link that claims depth 0 is no way round it either.
    let refused = rig
        .agents
        .start_child(link(0), start("kid", "go", rig.dir.path()))
        .await
        .unwrap_err();
    assert!(refused.contains("levels deep"), "{refused}");
    let child = rig
        .agents
        .start_child(link(MAX_DEPTH), start("kid", "go", rig.dir.path()))
        .await
        .expect("the last level starts");
    assert_eq!(child.info().parent.expect("a link").root, "p0");
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_root_may_not_have_more_than_eight_agents_across_machines() {
    let rig = Rig::new();
    let far = SecretKeyFor::id();
    let link = ParentLink {
        machine: Some(far),
        session: "p1".into(),
        key: "p1".into(),
        root_machine: Some(far),
        root: "p0".into(),
        depth: 1,
        others: 0,
        agent: "kid".into(),
        mode: None,
        background: false,
    };
    // Starts under way count too: each holds a place until its child is listed.
    let root = (Some(far), SessionId::from("p0"));
    let places: Vec<_> = (0..MAX_CHILDREN)
        .map(|_| rig.agents.take_place(&root, 0).expect("within the limit"))
        .collect();
    let refused = rig
        .agents
        .start_child(link.clone(), start("kid", "go", rig.dir.path()))
        .await
        .unwrap_err();
    assert!(refused.contains("at most 8 agents"), "{refused}");
    drop(places);
    let mut started = Vec::new();

    for _ in 0..MAX_CHILDREN {
        let child = rig
            .agents
            .start_child(link.clone(), start("kid", "go", rig.dir.path()))
            .await
            .expect("within the limit");
        started.push(child);
    }

    let refused = rig
        .agents
        .start_child(link.clone(), start("kid", "go", rig.dir.path()))
        .await
        .unwrap_err();
    assert!(refused.contains("at most 8 agents"), "{refused}");
    // Once one ends there is room again.
    started[0].close().await;
    rig.agents
        .start_child(link, start("kid", "go", rig.dir.path()))
        .await
        .expect("a slot is free");
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_session_may_not_have_more_than_eight_children() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let who = rig.agents.who_of(&parent);
    // The node itself runs eight sessions at most, so the children are
    // kept on another machine for this count.
    for at in 0..MAX_CHILDREN {
        let kid = RemoteKid {
            parent: who.session.clone(),
            machine: SecretKeyFor::id(),
            name: format!("box-{at}"),
            id: SessionId::from(format!("k{at}")),
            ended: false,
            since: 0,
        };
        crate::lock(&rig.agents.links.remote).push(kid);
    }

    let refused = rig.agents.link_for(&parent, &who).unwrap_err();
    assert!(refused.contains("8 children"), "{refused}");
    // One that ended frees its place but stays there to read.
    let first = rig.agents.remote_kids(&who.session)[0].clone();
    rig.agents.end_remote(&first);
    assert!(rig.agents.link_for(&parent, &who).is_ok());
    assert_eq!(rig.agents.remote_kids(&who.session).len(), MAX_CHILDREN);
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn an_agent_reaches_only_its_own_children() {
    let rig = Rig::new();
    let mine = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let theirs = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &mine, "kid", "work", rig.dir.path()).await;
    let id = Agents::name_of(&child);
    let me = rig.agents.who_of(&mine);
    let other = rig.agents.who_of(&theirs);
    assert!(rig.agents.child_of(&me, &id).is_ok());
    let refused = rig.agents.child_of(&other, &id).expect_err("not theirs");
    // It says no more than for an agent that does not exist.
    let nothing = rig
        .agents
        .child_of(&other, &SessionId::from("nothing"))
        .expect_err("no such agent");
    assert_eq!(refused, nothing.replace("nothing", id.as_ref()));
    // Another machine's session with the same id is another caller.
    let abroad = Who {
        machine: Some(SecretKeyFor::id()),
        session: me.session.clone(),
    };
    assert!(rig.agents.child_of(&abroad, &id).is_err());
    // A session reaches neither its parent nor itself.
    assert!(rig.agents.child_of(&me, &Agents::name_of(&mine)).is_err());
    let child_who = Who {
        machine: None,
        session: id.clone(),
    };
    assert!(
        rig.agents
            .child_of(&child_who, &Agents::name_of(&mine))
            .is_err()
    );
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn closing_a_parent_closes_its_children_and_theirs() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "one", rig.dir.path()).await;
    let grandchild = child_of(&rig.agents, &child, "kid", "two", rig.dir.path()).await;
    until(&grandchild, |live| live.info.turns == 1).await;
    parent.close().await;
    until(&child, |live| live.status() == Status::Ended).await;
    until(&grandchild, |live| live.status() == Status::Ended).await;
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_parent_is_told_once_each_time_its_child_finishes() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    // The idle parent takes the note as a prompt, which runs a turn.
    until(&parent, |live| live.info.turns == 2).await;
    let id = Agents::name_of(&child);
    let told = format!("[child {id} finished]");
    let notes = |log: &str| log.matches("<beep-child ").count();
    let log = rig.read("top.log");
    assert_eq!(notes(&log), 1, "{log}");
    assert!(log.contains(&told), "{log}");
    assert!(log.contains("stop it with stop_agent"), "{log}");
    assert!(log.contains("state=\\\"finished\\\""), "{log}");
    // The next turn of the child is the next note, and the first is not
    // sent again.
    child
        .order(Command::Prompt("more".to_owned().into()))
        .await
        .expect("the child takes a prompt");
    until(&parent, |live| live.info.turns == 3).await;
    assert_eq!(notes(&rig.read("top.log")), 2);
    rig.agents.shutdown().await;
}

/// What the user writes to a session, as an app sends it.
fn said(text: &str) -> Request {
    Request::Prompt {
        text: text.to_owned(),
    }
}

const CHANGES: &str = "Since your last turn in this chat";

#[tokio::test]
async fn a_users_stop_of_a_thread_waits_for_the_main_chats_next_turn() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "hang on", rig.dir.path()).await;
    until(&child, |live| live.status() == Status::Working).await;
    child
        .order(Command::Cancel { by_user: true })
        .await
        .expect("the child takes the stop");
    until(&child, |live| live.status() == Status::Idle).await;
    let (entries, _) = watched::read(&child.live, |live| live.transcript(usize::MAX));
    assert!(
        entries.iter().any(|en| en.text == "Stopped by you"),
        "{entries:?}"
    );
    // The idle parent runs no turn for it: its next one says so.
    rig.agents
        .send_session("top", &parent.id, said("where are we"))
        .await
        .expect("the parent takes the user's message");
    until(&parent, |live| live.info.turns == 2).await;
    let id = Agents::name_of(&child);
    let log = rig.read("top.log");
    assert!(!log.contains("was stopped by the user"), "{log}");
    assert!(log.contains(&format!("{id}, stopped")), "{log}");
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_users_message_to_the_main_chat_says_what_its_threads_came_to() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    // The turn the parent started tells it, as a prompt.
    until(&parent, |live| live.info.turns == 2).await;
    // The turn the user starts in the thread does not.
    rig.agents
        .send_session("kid", &child.id, said("say it"))
        .await
        .expect("the thread takes the user's message");
    until(&child, |live| {
        live.info.turns == 2 && live.status() == Status::Idle
    })
    .await;
    rig.agents
        .send_session("top", &parent.id, said("where are we"))
        .await
        .expect("the parent takes the user's message");
    until(&parent, |live| live.info.turns == 3).await;
    let id = Agents::name_of(&child);
    let log = rig.read("top.log");
    assert_eq!(log.matches("<beep-child ").count(), 1, "{log}");
    assert_eq!(log.matches(CHANGES).count(), 1, "{log}");
    assert!(log.contains("- agent \\\""), "{log}");
    assert!(log.contains(&format!("{id}, idle: It is done.")), "{log}");
    // The user's words stay as typed: the context goes beside them.
    let (entries, _) = watched::read(&parent.live, |live| live.transcript(usize::MAX));
    assert!(
        entries.iter().any(|en| en.text == "where are we"),
        "{entries:?}"
    );
    // What it was told is not told again.
    rig.agents
        .send_session("top", &parent.id, said("and now"))
        .await
        .expect("the parent takes the user's message");
    until(&parent, |live| live.info.turns == 4).await;
    assert_eq!(rig.read("top.log").matches(CHANGES).count(), 1);
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_users_message_to_a_thread_says_what_its_siblings_and_main_came_to() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let first = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    until(&parent, |live| live.info.turns == 2).await;
    let second = child_of(&rig.agents, &parent, "kid", "say it", rig.dir.path()).await;
    until(&parent, |live| live.info.turns == 3).await;
    rig.agents
        .send_session("top", &parent.id, said("say it"))
        .await
        .expect("the parent takes the user's message");
    until(&parent, |live| live.info.turns == 4).await;

    rig.agents
        .send_session("kid", &first.id, said("next"))
        .await
        .expect("the thread takes the user's message");
    until(&first, |live| live.info.turns == 2).await;
    let (own, other) = (Agents::name_of(&first), Agents::name_of(&second));
    let log = rig.read("kid.log");
    assert_eq!(log.matches(CHANGES).count(), 1, "{log}");
    assert!(log.contains("- parent session: It is done."), "{log}");
    assert!(log.contains(&format!("{other}, idle: It is done.")), "{log}");
    assert!(!log.contains(&format!("{own}, idle")), "{log}");
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_slash_command_takes_no_context() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    until(&parent, |live| live.info.turns == 2).await;
    rig.agents
        .send_session("kid", &child.id, said("say it"))
        .await
        .expect("the thread takes the user's message");
    until(&child, |live| {
        live.info.turns == 2 && live.status() == Status::Idle
    })
    .await;
    // A command must stay the whole message, so the news waits for the next.
    rig.agents
        .send_session("top", &parent.id, said("/status"))
        .await
        .expect("the parent takes the command");
    until(&parent, |live| live.info.turns == 3).await;
    assert_eq!(rig.read("top.log").matches(CHANGES).count(), 0);
    rig.agents
        .send_session("top", &parent.id, said("go"))
        .await
        .expect("the parent takes the user's message");
    until(&parent, |live| live.info.turns == 4).await;
    assert_eq!(rig.read("top.log").matches(CHANGES).count(), 1);
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn news_for_a_message_a_session_did_not_take_comes_with_the_next() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    until(&parent, |live| live.info.turns == 2).await;
    rig.agents
        .send_session("kid", &child.id, said("say it"))
        .await
        .expect("the thread takes the user's message");
    until(&child, |live| {
        live.info.turns == 2 && live.status() == Status::Idle
    })
    .await;
    // Worked out for a message the session then refuses: not seen yet.
    let (refused, _) = rig.agents.heard(&parent, String::from("go").into()).await;
    assert!(refused.context.is_some());
    rig.agents
        .send_session("top", &parent.id, said("go"))
        .await
        .expect("the parent takes the user's message");
    until(&parent, |live| live.info.turns == 3).await;
    assert_eq!(rig.read("top.log").matches(CHANGES).count(), 1);
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn the_words_that_resume_an_ended_thread_are_the_users() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    until(&parent, |live| live.info.turns == 2).await;
    rig.agents
        .send_session("top", &parent.id, said("say it"))
        .await
        .expect("the parent takes the user's message");
    until(&parent, |live| live.info.turns == 3).await;
    child.close().await;

    let resumed = rig
        .agents
        .resume(&rig.kid, &child.id, Some("say it".into()), None)
        .await
        .expect("the thread resumes");
    until(&resumed, |live| {
        live.info.last_message == "It is done." && live.status() == Status::Idle
    })
    .await;
    // It heard what its parent last said, with the user's words.
    let log = rig.read("kid.log");
    assert_eq!(log.matches(CHANGES).count(), 1, "{log}");
    assert!(log.contains("- parent session: It is done."), "{log}");

    // And its turn woke no idle parent: the parent's next turn is the user's.
    rig.agents
        .send_session("top", &parent.id, said("and now"))
        .await
        .expect("the parent takes the user's message");
    until(&parent, |live| live.info.turns == 4).await;
    let log = rig.read("top.log");
    assert_eq!(log.matches("<beep-child ").count(), 1, "{log}");
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_parents_message_makes_the_turn_its_own() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    until(&child, |live| {
        live.info.turns == 1 && live.status() == Status::Idle
    })
    .await;
    rig.agents
        .send_session("kid", &child.id, said("hang on"))
        .await
        .expect("the thread takes the user's message");
    assert!(child.user_turn());
    // What the parent now asks of it is the parent's to hear the end of.
    Agents::tell_child(&child, "more".to_owned())
        .await
        .expect("the thread takes its parent's message");
    assert!(!child.user_turn());
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_thread_that_ended_still_tells_whether_the_user_began_its_last_turn() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    until(&child, |live| {
        live.info.turns == 1 && live.status() == Status::Idle
    })
    .await;
    child.set_user_turn(true).await;
    child.close().await;
    // The session's task is gone, so the order is refused: the follower
    // that asks after the turn still finds the turn the user's.
    assert!(child.take_user_turn().await);
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_reviewer_is_no_thread_of_its_parents_chat() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let who = rig.agents.who_of(&parent);
    let link = rig.agents.link_for(&parent, &who).expect("within limits");
    let reviewer = rig
        .agents
        .start_child(
            link,
            ChildStart {
                review: Some(Review::default()),
                ..start("kid", "say it", rig.dir.path())
            },
        )
        .await
        .expect("the reviewer starts");
    until(&reviewer, |live| {
        live.info.turns >= 1 && live.status() == Status::Idle
    })
    .await;
    rig.agents
        .send_session("top", &parent.id, said("where are we"))
        .await
        .expect("the parent takes the user's message");
    until(&parent, |live| live.info.turns == 2).await;
    let log = rig.read("top.log");
    assert!(!log.contains(CHANGES), "{log}");
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_thread_keeps_the_name_its_parent_gave_when_its_agent_names_it() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let named = |title: Option<&str>| ChildStart {
        title: title.map(str::to_owned),
        ..start("kid", "name it", rig.dir.path())
    };
    let who = rig.agents.who_of(&parent);
    let link = rig.agents.link_for(&parent, &who).expect("within limits");
    let given = rig
        .agents
        .start_child(link, named(Some("Solid-state timelines")))
        .await
        .expect("the child starts");
    let link = rig.agents.link_for(&parent, &who).expect("within limits");
    let unnamed = rig
        .agents
        .start_child(link, named(None))
        .await
        .expect("the child starts");
    until(&given, |live| live.info.turns == 1).await;
    until(&unnamed, |live| live.info.turns == 1).await;
    assert_eq!(given.info().title.as_deref(), Some("Solid-state timelines"));
    assert_eq!(unnamed.info().title.as_deref(), Some("Named by the agent"));

    // Resumed, the name its parent gave still holds.
    given.close().await;
    let again = rig
        .agents
        .resume(&rig.kid, &given.id, Some("name it".into()), None)
        .await
        .expect("the thread resumes");
    until(&again, |live| live.info.turns >= 1 && live.status() == Status::Idle).await;
    assert_eq!(again.info().title.as_deref(), Some("Solid-state timelines"));
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_users_steer_into_a_working_thread_still_tells_the_main_chat() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "hang on", rig.dir.path()).await;
    until(&child, |live| live.status() == Status::Working).await;
    rig.agents
        .send_session(
            "kid",
            &child.id,
            Request::Steer {
                text: "note".to_owned(),
                images: Vec::new(),
            },
        )
        .await
        .expect("the thread takes the user's steer");
    // The parent's turn goes on: it stops it and sends the next prompt itself.
    child
        .order(Command::Cancel { by_user: false })
        .await
        .expect("the child takes the stop");
    child
        .order(Command::Prompt("go".to_owned().into()))
        .await
        .expect("the child takes a prompt");
    until(&parent, |live| live.info.turns >= 2).await;
    let id = Agents::name_of(&child);
    let log = rig.read("top.log");
    assert!(log.contains(&format!("[child {id} finished]")), "{log}");
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_thread_takes_the_title_its_parent_gives() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let who = rig.agents.who_of(&parent);
    let link = rig.agents.link_for(&parent, &who).expect("within limits");
    let child = rig
        .agents
        .start_child(
            link,
            ChildStart {
                title: Some("Solid-state timelines".into()),
                ..start("kid", "work", rig.dir.path())
            },
        )
        .await
        .expect("the child starts");
    until(&child, |live| live.info.turns == 1).await;
    assert_eq!(
        child.info().title.as_deref(),
        Some("Solid-state timelines")
    );
    rig.agents.shutdown().await;
}

#[test]
fn reviewers_and_background_agents_are_no_threads() {
    let link = |background| ParentLink {
        machine: None,
        session: "p1".into(),
        key: "p1".into(),
        root_machine: None,
        root: "p1".into(),
        depth: 1,
        others: 0,
        agent: "top".into(),
        mode: None,
        background,
    };
    let info = |parent: Option<ParentLink>, reviewer| SessionInfo {
        parent,
        reviewer,
        ..SessionInfo::default()
    };

    assert!(changes::is_thread(&info(Some(link(false)), false)));
    assert!(!changes::is_thread(&info(Some(link(false)), true)));
    assert!(!changes::is_thread(&info(Some(link(true)), false)));
    assert!(!changes::is_thread(&info(None, false)));
}

#[tokio::test]
async fn a_thread_with_its_parent_elsewhere_takes_the_message_unchanged() {
    let rig = Rig::new();
    let far = SecretKeyFor::id();
    let link = ParentLink {
        machine: Some(far),
        session: "p1".into(),
        key: "p1".into(),
        root_machine: Some(far),
        root: "p0".into(),
        depth: 1,
        others: 0,
        agent: "kid".into(),
        mode: None,
        background: false,
    };
    let child = rig
        .agents
        .start_child(link, start("kid", "work", rig.dir.path()))
        .await
        .expect("the child starts");
    until(&child, |live| live.info.turns == 1).await;
    rig.agents
        .send_session("kid", &child.id, said("go on"))
        .await
        .expect("the thread takes the user's message");
    until(&child, |live| live.info.turns == 2).await;
    let log = rig.read("kid.log");
    assert!(log.contains("go on"), "{log}");
    assert!(!log.contains(CHANGES), "{log}");
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_parent_that_ends_with_the_node_leaves_its_children_to_resume() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    until(&child, |live| live.info.turns == 1).await;
    // The node stops: both end with it, and both are to come back.
    rig.agents.shutdown().await;
    let open = rig.agents.open_sessions();
    assert_eq!(open.len(), 2, "{open:?}");
}

/// Stop ends the parent's own turn and leaves the agents it started
/// working; Stop all stops them too (#565).
#[tokio::test]
async fn stop_leaves_a_parent_s_children_working_and_stop_all_stops_them() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "hang on", rig.dir.path()).await;
    until(&child, |live| live.status() == Status::Working).await;
    let id = parent.info().id;

    rig.agents
        .send_session("top", &id, Request::Cancel {})
        .await
        .expect("the parent takes the stop");
    assert!(!rig.read("kid.log").contains("session/cancel"));
    assert_eq!(child.info().state.status, Status::Working);

    rig.agents
        .send_session("top", &id, Request::StopAll {})
        .await
        .expect("the parent takes the stop");
    until(&child, |live| live.status() == Status::Idle).await;
    assert!(rig.read("kid.log").contains("session/cancel"));
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn stopping_a_parent_stops_its_children_s_turns() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "hang on", rig.dir.path()).await;
    let grandchild = child_of(&rig.agents, &child, "kid", "hang too", rig.dir.path()).await;
    until(&child, |live| live.status() == Status::Working).await;
    until(&grandchild, |live| live.status() == Status::Working).await;
    rig.agents.cancel_family(&parent).await;
    until(&child, |live| live.status() == Status::Idle).await;
    until(&grandchild, |live| live.status() == Status::Idle).await;
    assert!(rig.read("kid.log").contains("session/cancel"));
    // The parent stopped them, so neither counts as stopped by the user.
    assert!(!watched::read(&child.live, |live| live.stopped_by_user));
    assert!(!watched::read(&grandchild.live, |live| live.stopped_by_user));
    // Stop only ends turns: the sessions stay.
    assert!(rig.agents.runs(&Agents::name_of(&child)));
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn the_link_survives_a_restart_and_the_family_still_stops_together() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    until(&child, |live| live.info.turns == 1).await;
    let before = child.info().parent.expect("a link");
    let noted = serde_json::to_string(&rig.agents.open_sessions()).expect("the note writes");
    rig.agents.shutdown().await;

    let noted: Vec<OpenSession> = serde_json::from_str(&noted).expect("the note reads");
    let after = Agents::new(Parts { kinds: vec![rig.top.clone(), rig.kid.clone()], ..Parts::default() });
    after.catalog.settings_arrived();
    // The child is noted first on purpose: parents resume before children.
    let mut noted = noted;
    noted.sort_by_key(|open| std::cmp::Reverse(open.depth()));
    after.reopen(noted).await;
    let resumed = after.get(&before.session).expect("the parent is back");
    let who = after.who_of(&resumed);
    let kids = after.children_of(&who);
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0].info().parent, Some(before));
    // The resumed parent stops its resumed child.
    resumed.close().await;
    until(&kids[0], |live| live.status() == Status::Ended).await;
    after.shutdown().await;
}

#[tokio::test]
async fn a_thread_started_as_a_persona_hears_its_instructions_and_keeps_its_name() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let who = rig.agents.who_of(&parent);
    let link = rig.agents.link_for(&parent, &who).expect("within limits");
    let researcher = beep_proto::CustomAgent {
        name: "researcher".into(),
        harness: beep_proto::CustomAgentHarness::Claude {},
        scope: beep_proto::CustomAgentScope::Project {},
        description: "Digs".into(),
        model: None,
        tools: Vec::new(),
        color: Some("blue".into()),
        path: ".claude/agents/researcher.md".into(),
        prompt: "Cite every source you read.".into(),
        prompt_cut: false,
        error: None,
    };
    let child = rig
        .agents
        .start_child(
            link,
            ChildStart {
                persona: Some(researcher),
                ..start("kid", "work", rig.dir.path())
            },
        )
        .await
        .expect("the child starts");
    until(&child, |live| live.info.turns == 1).await;
    let persona = Persona {
        name: "researcher".into(),
        color: Some("blue".into()),
    };
    assert_eq!(child.info().persona.as_ref(), Some(&persona));
    // Its instructions go with Beep's word, not as the user's prompt.
    let log = rig.read("kid.log");
    let prompt = log
        .lines()
        .find(|line| line.contains("session/prompt"))
        .expect("the child was prompted");
    assert!(prompt.contains("Cite every source you read."), "{prompt}");
    assert!(prompt.contains("beep-context"), "{prompt}");

    // Kept over a restart.
    let noted = serde_json::to_string(&rig.agents.open_sessions()).expect("the note writes");
    let saved = rig.agents.saved_sessions();
    assert!(saved.iter().any(|kept| kept.info.persona.as_ref() == Some(&persona)));
    rig.agents.shutdown().await;
    let noted: Vec<OpenSession> = serde_json::from_str(&noted).expect("the note reads");
    let after = Agents::new(Parts { kinds: vec![rig.top.clone(), rig.kid.clone()], ..Parts::default() });
    after.catalog.settings_arrived();
    after.reopen(noted).await;
    let id = Agents::name_of(&child);
    let resumed = after.get(&id).expect("the thread is back");
    assert_eq!(resumed.info().persona, Some(persona));
    after.shutdown().await;
}

#[test]
fn a_note_from_before_the_link_reads_with_none() {
    let old = r#"{"agent":"claude","session":"s1","cwd":"/srv/project","held":[]}"#;
    let open: OpenSession = serde_json::from_str(old).expect("an old note reads");
    assert_eq!(open.depth(), 0);
    assert!(open.kids.is_empty());
}

#[test]
fn a_child_never_runs_above_its_parents_mode() {
    let link = |agent: &str, mode: Option<&str>| ParentLink {
        machine: None,
        session: "p1".into(),
        key: "p1".into(),
        root_machine: None,
        root: "p1".into(),
        depth: 1,
        others: 0,
        agent: agent.into(),
        mode: mode.map(str::to_owned),
        background: false,
    };
    assert_eq!(child_mode(&link("claude", None), "codex"), Ok(None));
    assert_eq!(
        child_mode(&link("claude", Some("plan")), "claude"),
        Ok(Some("plan".to_owned()))
    );
    let refused = child_mode(&link("claude", Some("plan")), "codex").unwrap_err();
    assert!(refused.contains("below full access"), "{refused}");
}

fn info_in(agent: &str, mode: Option<&str>, offers: &[&str]) -> SessionInfo {
    SessionInfo {
        agent: agent.into(),
        mode: mode.map(str::to_owned),
        modes: offers
            .iter()
            .map(|id| beep_proto::node::agent::ModeOption {
                id: (*id).into(),
                name: (*id).into(),
            })
            .collect(),
        ..SessionInfo::default()
    }
}

#[test]
fn every_harness_is_held_to_any_mode_but_its_own_full_access() {
    // Each known harness with the id of its full access mode: free there,
    // held in any other mode, which a child of that harness then keeps.
    for (agent, full) in [
        ("claude", "bypassPermissions"),
        ("codex", "agent-full-access"),
        ("agy", "default"),
        ("hermes", "dont_ask"),
    ] {
        let offers = [full, "plan", "read-only"];
        assert_eq!(
            held_to(&info_in(agent, Some(full), &offers)),
            Ok(None),
            "{agent}"
        );

        for mode in ["plan", "read-only", "default", "accept-edits"]
            .into_iter()
            .filter(|mode| *mode != full)
        {
            assert_eq!(
                held_to(&info_in(agent, Some(mode), &offers)),
                Ok(Some(mode.to_owned())),
                "{agent} in {mode}"
            );
        }
        // Offering modes without saying which one it is in: unknown.
        assert!(held_to(&info_in(agent, None, &offers)).is_err(), "{agent}");
    }
    // The full access mode of one harness frees none of the others.
    assert_eq!(
        held_to(&info_in(
            "codex",
            Some("bypassPermissions"),
            &["bypassPermissions"]
        )),
        Ok(Some("bypassPermissions".to_owned()))
    );
    // A harness with no modes asks nothing: nothing to hold a child to. One
    // the node does not know that offers modes is held to whichever it is in.
    for agent in ["pi", "flsh", "opencode", "custom"] {
        assert_eq!(held_to(&info_in(agent, None, &[])), Ok(None), "{agent}");
    }

    assert_eq!(
        held_to(&info_in("custom", Some("safe"), &["safe", "free"])),
        Ok(Some("safe".to_owned()))
    );
}

#[tokio::test]
async fn a_resumed_session_goes_on_under_its_parent_in_its_mode() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let who = rig.agents.who_of(&parent);
    let mut link = rig.agents.link_for(&parent, &who).expect("within limits");
    link.agent = "kid".into();
    link.mode = Some("plan".into());
    let child = rig
        .agents
        .start_child(link, start("kid", "work", rig.dir.path()))
        .await
        .expect("the child starts in plan mode");
    until(&child, |live| live.info.mode.as_deref() == Some("plan")).await;
    let before = child.info();
    child.close().await;
    let held = SavedSession {
        info: before.clone(),
        effort: None,
        fast: None,
        account: None,
        named: false,
    };
    let begin = rig.agents.resumed_begin(&SessionId::from("agent-id"), held);
    assert_eq!(begin.parent, before.parent);
    assert_eq!(begin.mode.as_deref(), Some("plan"));
    assert_eq!(begin.resume, Some(SessionId::from("agent-id")));
    // The parent runs here: the resumed child ends with it again.
    assert!(begin.follows.is_some());
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_resumed_child_with_nothing_to_do_reads_idle() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let child = child_of(&rig.agents, &parent, "kid", "work", rig.dir.path()).await;
    until(&child, |live| live.info.turns == 1).await;
    let noted = rig.agents.open_sessions();
    rig.agents.shutdown().await;

    let after = Agents::new(Parts { kinds: vec![rig.top.clone(), rig.kid.clone()], ..Parts::default() });
    after.catalog.settings_arrived();
    after.reopen(noted).await;
    let resumed = after
        .get(&Agents::name_of(&child))
        .expect("the child is back");
    until(&resumed, |live| live.status() == Status::Idle).await;
    // Its turn count started over, which no longer says it never ran.
    assert_eq!(resumed.info().turns, 0);
    let view = crate::agents::child_view::ChildView::of(&resumed.info(), &SessionId::from("k1"));
    assert_eq!(view.state, "idle");
    after.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn a_pushed_list_says_whether_the_parent_is_there_and_a_burst_of_changes_is_one_ask() {
    use links::Parent;
    use orphans::{ORPHAN_CHECK, Word, asks, word};
    use std::sync::{Arc, atomic::{AtomicU32, Ordering::SeqCst}};
    use tokio::{sync::{Notify, mpsc, watch}, task::yield_now, time::advance};

    let link = ParentLink {
        machine: None,
        session: "p1".into(),
        key: "k1".into(),
        root_machine: None,
        root: "p0".into(),
        depth: 1,
        others: 0,
        agent: "claude".into(),
        mode: None,
        background: false,
    };
    let row = |id: &str, status: Status, error: Option<&str>, summary: &str| SessionInfo {
        id: id.into(),
        state: beep_proto::node::agent::State {
            status,
            error: error.map(str::to_owned),
            summary: summary.into(),
            ..Default::default()
        },
        ..SessionInfo::default()
    };
    let mut last = None;

    assert_eq!(word(&[], &mut last, &link), Word::Gone);
    assert_eq!(word(&[row("other", Status::Idle, None, "")], &mut last, &link), Word::Gone);
    assert_eq!(word(&[row("p1", Status::Idle, None, "")], &mut last, &link), Word::Moved);
    assert_eq!(word(&[row("p1", Status::Idle, None, "")], &mut last, &link), Word::Same);
    // By the id it was listed under when the child began, too.
    assert_eq!(word(&[row("k1", Status::Idle, None, "")], &mut None, &link), Word::Moved);
    // Ended for good is gone; an end with the node is not.
    assert_eq!(word(&[row("p1", Status::Ended, None, "")], &mut last, &link), Word::Gone);
    let stopped = row("p1", Status::Ended, Some(crate::agents::session::NODE_STOPPED), "");
    assert_eq!(word(&[stopped], &mut last, &link), Word::Moved);

    // Activity-only changes of the row are one ask; the answer that the
    // parent no longer keeps the child orphans it.
    let moved = Arc::new(Notify::new());
    let asked = Arc::new(AtomicU32::new(0));
    let (tx, mut heard) = mpsc::channel(4);
    let task = {
        let (moved, asked) = (moved.clone(), asked.clone());
        tokio::spawn(async move {
            let ask = || async {
                asked.fetch_add(1, SeqCst);
                Some(Parent::Gone)
            };
            asks(&moved, watch::channel(true).1, ask, &tx).await;
        })
    };

    for turn in 0..20 {
        let doing = format!("Edit file {turn}");
        let changed = word(&[row("p1", Status::Working, None, &doing)], &mut last, &link);
        assert_eq!(changed, Word::Moved);
        moved.notify_one();
        yield_now().await;
    }

    assert_eq!(asked.load(SeqCst), 1, "a burst of changes is one ask");
    assert_eq!(heard.recv().await, Some(Parent::Gone));
    // The next one waits for the pause.
    advance(ORPHAN_CHECK / 2).await;
    yield_now().await;
    assert_eq!(asked.load(SeqCst), 1);
    advance(ORPHAN_CHECK).await;
    yield_now().await;
    assert_eq!(asked.load(SeqCst), 2, "the changes during the pause are asked once after it");
    task.abort();
}

// An ask nobody answers is a failed call: it goes out again until it is
// answered, though the parent's row never changes.
#[tokio::test(start_paused = true)]
async fn an_unanswered_ask_is_asked_again_until_it_is_answered() {
    use links::Parent;
    use orphans::{ORPHAN_CHECK, asks};
    use std::sync::{Arc, Mutex, atomic::{AtomicU32, Ordering::SeqCst}};
    use tokio::{sync::{Notify, mpsc, watch}, task::yield_now, time::advance};

    let moved = Arc::new(Notify::new());
    let asked = Arc::new(AtomicU32::new(0));
    let answers = Arc::new(Mutex::new(vec![Parent::Keeps, Parent::Silent]));
    let (there, there_rx) = watch::channel(true);
    let (tx, mut heard) = mpsc::channel(8);
    let task = {
        let (moved, asked, answers) = (moved.clone(), asked.clone(), answers.clone());
        tokio::spawn(async move {
            let ask = || async {
                asked.fetch_add(1, SeqCst);
                Some(answers.lock().unwrap().pop().unwrap_or(Parent::Keeps))
            };
            asks(&moved, there_rx, ask, &tx).await;
        })
    };

    moved.notify_one();
    yield_now().await;
    assert_eq!(tokio::time::timeout(Duration::from_millis(1), heard.recv()).await.ok().flatten(), Some(Parent::Silent));
    assert_eq!(asked.load(SeqCst), 1);
    // No push follows: the retry alone gets the answer.
    advance(Duration::from_secs(6)).await;
    yield_now().await;
    assert_eq!(tokio::time::timeout(Duration::from_millis(1), heard.recv()).await.ok().flatten(), Some(Parent::Keeps));
    assert_eq!(asked.load(SeqCst), 2);
    advance(Duration::from_secs(30)).await;
    yield_now().await;
    assert_eq!(asked.load(SeqCst), 2, "an answered ask is not asked again");

    // Retries stop when the parent is gone.
    answers.lock().unwrap().extend([Parent::Silent, Parent::Silent]);
    advance(ORPHAN_CHECK).await;
    moved.notify_one();
    yield_now().await;
    assert_eq!(tokio::time::timeout(Duration::from_millis(1), heard.recv()).await.ok().flatten(), Some(Parent::Silent));
    there.send_replace(false);
    yield_now().await;
    let before = asked.load(SeqCst);
    advance(Duration::from_secs(30)).await;
    yield_now().await;
    assert_eq!(asked.load(SeqCst), before);
    task.abort();
}

// Only a machine that turns the stream away at the handshake takes the
// compat path; an error it sends mid-stream is a drop that is dialed again.
#[test]
fn a_transient_error_on_a_stream_is_not_a_refusal() {
    use crate::peer_stream::{Refused, heard};
    use beep_proto::node::{events::{HeldSessions, Topic}, subscriptions::Reply};

    let mut held = HeldSessions::default();
    let errors = [
        Reply::Error { id: 1, reason: "too many pending watches".into() },
        Reply::SnapshotError { query: None, topic: Topic::new("agents"), reason: "failed to list".into() },
    ];

    for reply in errors {
        let failed = heard(reply, &mut held).unwrap_err();
        assert!(!failed.is::<Refused>(), "{failed:#}");
    }

    assert!(heard(Reply::Ready, &mut held).unwrap().is_none());
}

// A redial opens a new list, whose first push is a change again. The asker
// outlives it, so that push is paced by the last ask instead of asking at
// once.
#[tokio::test(start_paused = true)]
async fn a_redial_does_not_reset_the_pacing_of_the_asks() {
    use beep_proto::node::{agent::SessionsReply, events::Value};
    use links::Parent;
    use orphans::{ORPHAN_CHECK, Rings, asks, hear_list};
    use std::sync::{Arc, atomic::{AtomicU32, Ordering::SeqCst}};
    use tokio::{sync::{Notify, mpsc, watch}, task::yield_now, time::advance};
    use tokio_util::task::AbortOnDropHandle;

    let link = ParentLink {
        machine: None,
        session: "p1".into(),
        key: "k1".into(),
        root_machine: None,
        root: "p0".into(),
        depth: 1,
        others: 0,
        agent: "claude".into(),
        mode: None,
        background: false,
    };
    let list = || -> Value {
        let parent = SessionInfo { id: "p1".into(), ..SessionInfo::default() };

        Value::Sessions(Box::new(SessionsReply::Sessions { sessions: vec![parent] }))
    };
    let (moved, asked) = (Arc::new(Notify::new()), Arc::new(AtomicU32::new(0)));
    let (there, there_rx) = watch::channel(true);
    let (tx, _heard) = mpsc::channel(8);
    let asker = {
        let (moved, asked, tx) = (moved.clone(), asked.clone(), tx.clone());
        AbortOnDropHandle::new(tokio::spawn(async move {
            let ask = || async {
                asked.fetch_add(1, SeqCst);
                Some(Parent::Keeps)
            };
            asks(&moved, there_rx, ask, &tx).await;
        }))
    };
    let rings = Rings { moved: &moved, there: &there, asker: &asker };
    // A dialed list starts with nothing seen: the parent's row shows as a change.
    let follow = async || {
        let mut last = None;
        assert!(!hear_list(list(), &mut last, &link, &tx, &rings).await);
    };

    follow().await;
    yield_now().await;
    assert_eq!(asked.load(SeqCst), 1);
    assert!(!asker.is_finished(), "a list ending does not end the asker");

    // Dialed again 31 s later, its first push waits out the pause.
    advance(Duration::from_secs(31)).await;
    follow().await;
    yield_now().await;
    assert_eq!(asked.load(SeqCst), 1);
    advance(ORPHAN_CHECK - Duration::from_secs(31 + 1)).await;
    yield_now().await;
    assert_eq!(asked.load(SeqCst), 1);
    advance(Duration::from_secs(2)).await;
    yield_now().await;
    assert_eq!(asked.load(SeqCst), 2);
}

// A node outside any mesh reads as a parent that is silent, at once: the
// membership is a value the check holds, not one it waits for.
#[tokio::test(start_paused = true)]
async fn the_orphan_check_reads_a_node_outside_any_mesh_as_a_silent_parent() {
    use std::sync::Weak;
    use tokio::sync::mpsc;

    let link = ParentLink {
        machine: Some(SecretKeyFor::id()),
        session: "p1".into(),
        key: "k1".into(),
        root_machine: None,
        root: "p0".into(),
        depth: 1,
        others: 0,
        agent: "claude".into(),
        mode: None,
        background: false,
    };
    let (tx, mut heard) = mpsc::channel(4);
    let peer = SecretKeyFor::id();
    let task = tokio::spawn(orphans::feed(crate::membership::Membership::default(), Weak::new(), peer, link, tx));

    assert_eq!(heard.recv().await, Some(links::Parent::Silent));
    task.abort();
}

// The child closes once its parent has been gone or silent for
// `ORPHAN_AFTER`, on the pushed word and the clock only.
#[tokio::test(start_paused = true)]
async fn a_child_closes_once_its_parent_has_been_gone_or_silent_for_long_enough() {
    use links::Parent;
    use orphans::{ORPHAN_AFTER, orphaned};
    use tokio::{sync::mpsc, task::yield_now, time::advance};

    let (tx, heard) = mpsc::channel(4);
    let mut orphan = tokio::spawn(orphaned(heard));
    // A parent that keeps it never orphans it.
    tx.send(Parent::Keeps).await.unwrap();
    // Gone or silent starts the clock; only the time passing closes it.
    tx.send(Parent::Gone).await.unwrap();
    yield_now().await;
    advance(ORPHAN_AFTER / 2).await;
    tx.send(Parent::Silent).await.unwrap();
    yield_now().await;
    assert!(!orphan.is_finished());
    // Hearing from the parent in between starts it over.
    tx.send(Parent::Keeps).await.unwrap();
    tx.send(Parent::Gone).await.unwrap();
    yield_now().await;
    advance(ORPHAN_AFTER - Duration::from_secs(1)).await;
    yield_now().await;
    assert!(!orphan.is_finished());
    advance(Duration::from_secs(2)).await;
    assert!((&mut orphan).await.unwrap());

    // A follow that ends orphans nothing.
    let (tx, heard) = mpsc::channel(4);
    tx.send(Parent::Gone).await.unwrap();
    drop(tx);
    assert!(!orphaned(heard).await);
}

#[tokio::test]
async fn a_parents_node_says_which_remote_children_it_keeps() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let name = Agents::name_of(&parent);
    let (there, other) = (SecretKeyFor::id(), SecretKeyFor::id());
    let mut kid = RemoteKid::expected(&name, there, "box");
    kid.id = "k1".into();
    rig.agents.adopt_remote(&parent, kid.clone());
    assert!(rig.agents.keeps_child(there, &name, &SessionId::from("k1"), &SessionId::default()));
    // By the id it started under, too.
    assert!(rig.agents.keeps_child(there, &name, &SessionId::from("agent-id"), &SessionId::from("k1")));
    // Not another machine's, not another child.
    assert!(!rig.agents.keeps_child(other, &name, &SessionId::from("k1"), &SessionId::default()));
    assert!(!rig.agents.keeps_child(there, &name, &SessionId::from("k2"), &SessionId::default()));
    assert!(!rig.agents.keeps_child(there, &SessionId::from("no-such-parent"), &SessionId::from("k1"), &SessionId::default()));
    // One that ended is no longer kept.
    rig.agents.end_remote(&kid);
    assert!(!rig.agents.keeps_child(there, &name, &SessionId::from("k1"), &SessionId::default()));
    // A start that got no answer is kept for a while, whichever id the
    // machine gave it.
    rig.agents
        .adopt_remote(&parent, RemoteKid::expected(&name, other, "far"));
    assert!(rig.agents.keeps_child(other, &name, &SessionId::from("anything"), &SessionId::default()));
    // A parent that ended for good keeps nothing.
    parent.close().await;
    assert!(!rig.agents.keeps_child(other, &name, &SessionId::from("anything"), &SessionId::default()));
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_settle_note_wakes_a_wait_and_goes_in_no_prompt() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let name = Agents::name_of(&parent);
    let there = SecretKeyFor::id();
    let mut kid = RemoteKid::expected(&name, there, "box");
    kid.id = "k1".into();
    rig.agents.adopt_remote(&parent, kid);
    let note = ChildNote {
        id: "k1".into(),
        key: SessionId::default(),
        name: "kid".to_owned(),
        state: crate::agents::family::links::NoteState::Settled,
        text: String::new(),
    };
    let turns = parent.info().turns;
    // With no wait it is told to nobody.
    rig.agents
        .note_from(there, &name, &note)
        .await
        .expect("the note is taken");
    assert_eq!(parent.info().turns, turns);

    let waiting = rig.agents.links.waits.hold(&name, "k1@box");
    rig.agents
        .note_from(there, &name, &note)
        .await
        .expect("the note is taken");
    timeout(Duration::from_secs(5), waiting.woken())
        .await
        .expect("the wait is woken");
    assert_eq!(parent.info().turns, turns);
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_start_with_no_answer_holds_its_place_until_it_expires_and_a_refusal_frees_it() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let name = Agents::name_of(&parent);
    let who = rig.agents.who_of(&parent);
    let there = SecretKeyFor::id();

    for _ in 0..MAX_CHILDREN {
        rig.agents
            .adopt_remote(&parent, RemoteKid::expected(&name, there, "box"));
    }
    // A retry cannot start a ninth behind the eight that may be running.
    let refused = rig.agents.link_for(&parent, &who).unwrap_err();
    assert!(refused.contains("8 children"), "{refused}");
    // One the machine turned down never started.
    rig.agents.forget_expected(&name, there);
    assert!(rig.agents.link_for(&parent, &who).is_ok());
    // One that answered takes the id it was given, once.
    rig.agents.settle_remote(&name, there, &SessionId::from("k1"));
    rig.agents.settle_remote(&name, there, &SessionId::from("k1"));
    let ids: Vec<SessionId> = rig
        .agents
        .remote_kids(&name)
        .into_iter()
        .map(|kid| kid.id)
        .collect();
    assert_eq!(
        ids.iter().filter(|id| **id == "k1").count(),
        1,
        "{ids:?}"
    );
    // And the ones that never answered stop counting after a while.
    let mut old = RemoteKid::expected(&name, there, "box");
    old.since = 0;
    assert!(!old.counts());
    rig.agents.shutdown().await;
}

/// Endpoint ids for tests.
struct SecretKeyFor;

impl SecretKeyFor {
    fn id() -> EndpointId {
        iroh::SecretKey::generate().public()
    }
}

/// Whether the thread at `task` (`<pid>/task/<tid>` under `/proc`) sleeps,
/// as one blocked on a lock does.
#[cfg(target_os = "linux")]
fn sleeps(task: &Path) -> bool {
    std::fs::read_to_string(Path::new("/proc").join(task).join("stat"))
        .ok()
        .and_then(|stat| {
            let (_, rest) = stat.rsplit_once(')')?;
            Some(rest.trim_start().starts_with('S'))
        })
        .unwrap_or(false)
}

/// The note of what is open must not hold the session list, or a session's
/// state, while it reads a session's children. A node held both: every
/// request that lists sessions waited behind it, and the node stopped
/// answering anything (#329).
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn sessions_are_listed_while_the_open_note_waits_on_the_children() {
    use std::{sync::mpsc, time::Instant};
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let name = Agents::name_of(&parent);
    let mut kid = RemoteKid::expected(&name, SecretKeyFor::id(), "box");
    kid.id = "k1".into();
    rig.agents.adopt_remote(&parent, kid.clone());
    let agents = &rig.agents;
    let noted = std::thread::scope(|scope| {
        // The children's list is busy, so the note stops on it.
        let busy = crate::lock(&agents.links.remote);
        let (tx, rx) = mpsc::channel();
        let noting = scope.spawn(move || {
            let me = std::fs::read_link("/proc/thread-self").expect("the thread's /proc entry");
            tx.send(me).expect("the test waits for it");
            agents.open_sessions()
        });
        let task = rx.recv().expect("the noting thread starts");
        let deadline = Instant::now() + Duration::from_secs(10);

        while !sleeps(&task) {
            assert!(Instant::now() < deadline, "the note never waited");
            std::thread::yield_now();
        }

        let (tx, rx) = mpsc::channel();
        scope.spawn(move || {
            // Sending fails only once the test gave up.
            let _ = tx.send(agents.sessions.list().len());
        });
        let listed = rx.recv_timeout(Duration::from_secs(10));
        drop(busy);
        assert_eq!(listed, Ok(1), "listing the sessions waited on the note");
        noting.join().expect("the note is made")
    });
    assert_eq!(noted.len(), 1);
    assert_eq!(noted[0].kids, vec![kid]);
    parent.close().await;
}

/// A parent's Stop all on another machine reaches its child as Stop all; a
/// parent's node from 0.5.0 and before, which asks without `stop_all`, still
/// stops the child's turn (#565).
#[tokio::test]
async fn a_far_parent_s_stop_all_stops_its_child_here() {
    use beep_proto::node::tools::{Reply, Request as ToolRequest, Tool};

    let rig = Rig::new();
    let far = SecretKeyFor::id();
    let link = ParentLink {
        machine: Some(far),
        session: "p1".into(),
        key: "p1".into(),
        root_machine: Some(far),
        root: "p0".into(),
        depth: 1,
        others: 0,
        agent: "kid".into(),
        mode: None,
        background: false,
    };
    let stop = |id: SessionId, args: serde_json::Value| {
        let mut args = args;
        args["id"] = serde_json::json!(id);
        ToolRequest {
            session: "p1".into(),
            tool: Tool::StopAgent,
            args: args.to_string(),
            approved: Default::default(),
        }
    };

    for args in [
        serde_json::json!({ "turn_only": true }),
        serde_json::json!({ "turn_only": true, "stop_all": true }),
    ] {
        let child = rig
            .agents
            .start_child(link.clone(), start("kid", "hang on", rig.dir.path()))
            .await
            .expect("the child starts");
        until(&child, |live| live.status() == Status::Working).await;

        let reply = crate::tools::children::answer_peer(
            &rig.agents,
            far,
            stop(Agents::name_of(&child), args),
        )
        .await;
        assert!(
            matches!(&reply, Reply::Done { failed: false, .. }),
            "{reply:?}"
        );
        until(&child, |live| live.status() == Status::Idle).await;
        assert!(!watched::read(&child.live, |live| live.stopped_by_user));
        child.close().await;
    }

    rig.agents.shutdown().await;
}

/// A node with no mesh cannot note a settle to its parent, so it must not tell
/// the parent's wait to skip its recheck (a desktop's own node that serves no
/// MCP socket is one).
#[tokio::test]
async fn a_node_with_no_mesh_does_not_say_it_notes_every_settle() {
    use beep_proto::node::tools::{Reply, Request as ToolRequest, Tool};

    let rig = Rig::new();
    let far = SecretKeyFor::id();
    let link = ParentLink {
        machine: Some(far),
        session: "p1".into(),
        key: "p1".into(),
        root_machine: Some(far),
        root: "p0".into(),
        depth: 1,
        others: 0,
        agent: "kid".into(),
        mode: None,
        background: false,
    };
    let child = rig
        .agents
        .start_child(link, start("kid", "hello", rig.dir.path()))
        .await
        .expect("the child starts");
    let reply = crate::tools::children::answer_peer(
        &rig.agents,
        far,
        ToolRequest {
            session: "p1".into(),
            tool: Tool::AgentStatus,
            args: serde_json::json!({ "id": Agents::name_of(&child) }).to_string(),
            approved: Default::default(),
        },
    )
    .await;
    let Reply::Done { text, failed: false } = reply else {
        panic!("the status is served: {reply:?}");
    };
    let view: crate::agents::child_view::ChildView =
        serde_json::from_str(&text).expect("a child view");

    assert!(!view.notes_settles, "{text}");
    child.close().await;
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn another_machine_s_agent_lists_and_messages_any_agent_until_the_user_speaks() {
    use beep_proto::node::tools::{Reply, Request as ToolRequest, Tool};

    let rig = Rig::new();
    let session = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let id = Agents::name_of(&session);
    let far = SecretKeyFor::id();
    let call = |tool: Tool, args: serde_json::Value| {
        crate::tools::children::answer_peer(
            &rig.agents,
            far,
            ToolRequest {
                session: "far-agent".into(),
                tool,
                args: args.to_string(),
                approved: Default::default(),
            },
        )
    };

    let Reply::Done {
        text,
        failed: false,
    } = call(Tool::ListAgents, serde_json::json!({})).await
    else {
        panic!("the list is served");
    };
    assert!(text.contains(&format!("\"id\":\"{id}\"")), "{text}");

    let send = || {
        call(
            Tool::SendToAgent,
            serde_json::json!({ "id": id, "text": "hello" }),
        )
    };
    for _ in 0..crate::agents::session::PEER_MESSAGES {
        let reply = send().await;
        assert!(
            matches!(&reply, Reply::Done { failed: false, .. }),
            "{reply:?}"
        );
    }
    let refused = send().await;
    assert!(
        matches!(&refused, Reply::Done { failed: true, .. }),
        "{refused:?}"
    );

    rig.agents
        .send_session(
            "top",
            &session.id,
            Request::Prompt {
                text: "carry on".into(),
            },
        )
        .await
        .expect("the user's prompt goes");
    let reply = send().await;
    assert!(
        matches!(&reply, Reply::Done { failed: false, .. }),
        "{reply:?}"
    );
    // Messages go as steers, merged into as few turns as the agent allows.
    until(&session, |live| {
        live.info.turns >= 3 && live.info.state.queued == 0 && live.status() == Status::Idle
    })
    .await;
    let delivered = rig.read("top.log").matches("hello").count();
    assert_eq!(
        delivered,
        usize::try_from(crate::agents::session::PEER_MESSAGES).expect("a small count") + 1
    );

    rig.agents.shutdown().await;
}

/// A prompt to a session that ended opens it again and goes (#883).
#[tokio::test]
async fn a_prompt_to_a_session_that_ended_resumes_it() {
    let rig = Rig::new();
    let session = root(&rig.agents, &rig.top, rig.dir.path()).await;
    until(&session, |live| live.info.turns == 1).await;
    let id = session.info().id;
    session.close().await;

    let info = rig
        .agents
        .send_session(
            "top",
            &id,
            Request::Prompt {
                text: "carry on".into(),
            },
        )
        .await
        .expect("the prompt resumes the session");
    let resumed = rig.agents.get(&info.id).expect("the session runs again");
    until(&resumed, |live| live.info.turns >= 1).await;
    assert!(rig.read("top.log").contains("carry on"));

    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_teammate_reaches_no_session_the_user_stopped() {
    let rig = Rig::new();
    let session = root(&rig.agents, &rig.top, rig.dir.path()).await;
    session
        .order(Command::Prompt("hang on".to_owned().into()))
        .await
        .expect("the session takes the prompt");
    until(&session, |live| live.status() == Status::Working).await;
    session
        .order(Command::Cancel { by_user: true })
        .await
        .expect("the session takes the stop");
    until(&session, |live| live.status() == Status::Idle).await;
    let id = Agents::name_of(&session);
    let card = format!("{id}@build-box");

    for delivery in [Delivery::Ask, Delivery::Reply] {
        let given = rig
            .agents
            .deliver_from_teammate(&id, &card, delivery, "Can I take #52?".to_owned())
            .await;
        assert_eq!(given, Err(format!("{card} was stopped by its user")));
    }

    let log = rig.read("top.log");
    assert!(!log.contains("Can I take #52?"), "{log}");
    rig.agents.shutdown().await;
}

/// What this machine answers when `from` hands `text` to the session on `card`.
async fn handed(
    link: &TeamLink,
    agents: &Agents,
    from: Option<EndpointId>,
    card: &str,
    delivery: Delivery,
) -> (String, bool) {
    let args = serde_json::json!({ "card": card, "delivery": delivery, "text": "Can I take #52?" });
    let request = beep_proto::node::tools::Request {
        session: SessionId::default(),
        tool: beep_proto::node::tools::Tool::TeamDeliver,
        args: args.to_string(),
        approved: Default::default(),
    };

    match crate::tools::team::answer(link, agents, from, &request).await {
        beep_proto::node::tools::Reply::Done { text, failed } => (text, failed),
        other => panic!("not done: {other:?}"),
    }
}

#[tokio::test]
async fn a_teammate_reaches_only_a_team_session_through_its_gateway() {
    use crate::team::home::{Checkout, ForgeCli, Project};
    use beep_proto::team::{TeamRecord, TeamRepo};

    let rig = Rig::new();
    let session = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let card = format!("{}@build-box", Agents::name_of(&session));
    let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
        .bind()
        .await
        .expect("an endpoint");
    // Handing a text on asks the forge nothing.
    let link = TeamLink::new(endpoint, Arc::new(ForgeCli));
    let repo = || "github.com/org/repo".parse::<TeamRepo>().expect("a repo");
    let elected = iroh::SecretKey::generate().public();
    let gateway = Some(elected);
    let agents = &rig.agents;

    let (text, failed) = handed(&link, agents, gateway, &card, Delivery::Ask).await;
    assert!(failed && text.contains("works on no team's project"), "{text}");

    let mut record = TeamRecord::new(repo(), Vec::new());
    record.asks = false;
    let folder = rig.dir.path().to_owned();
    link.with(|teams| teams.rosters.refresh([(repo(), record)].into()));
    link.publish(|served| {
        served.gateways.insert(repo(), elected);
    });
    link.edit_home(|home| {
        let checkout = Some(Checkout {
            folder,
            remote: String::new(),
        });
        let project = Project {
            name: "org/repo".to_owned(),
            checkout,
            devices: Vec::new(),
        };
        home.projects.insert(repo(), project);
    });

    let elsewhere = Some(iroh::SecretKey::generate().public());
    let (text, failed) = handed(&link, agents, elsewhere, &card, Delivery::Reply).await;
    assert!(failed && text.contains("does not act for the team"), "{text}");

    let (text, failed) = handed(&link, agents, gateway, &card, Delivery::Ask).await;
    assert!(failed && text.contains("takes no messages"), "{text}");
    assert!(!rig.read("top.log").contains("Can I take #52?"));

    let (text, failed) = handed(&link, agents, gateway, &card, Delivery::Reply).await;
    assert!(!failed, "{text}");
    rig.agents.shutdown().await;
}

#[test]
fn a_teammate_s_words_never_answer_what_the_user_is_asked() {
    let mut live = Live::default();
    live.info.state.status = Status::Asking;
    assert_eq!(
        notes::open_to_teammates(&live, "s1@build-box"),
        Err("s1@build-box is waiting on its user".to_owned())
    );

    live.info.state.status = Status::Working;
    assert_eq!(notes::open_to_teammates(&live, "s1@build-box"), Ok(false));
    live.info.state.status = Status::Idle;
    assert_eq!(notes::open_to_teammates(&live, "s1@build-box"), Ok(true));
}

#[tokio::test]
async fn a_note_about_a_child_ends_its_parents_wait_and_goes_in_no_prompt() {
    let waits = Waits::default();
    let parent = SessionId::from("p0");

    assert!(!waits.wake(&parent, "k0@build-box"), "no wait holds it yet");

    let waiting = waits.hold(&parent, "k0@build-box");
    assert!(!waits.wake(&parent, "k1@build-box"), "another child");
    assert!(waits.wake(&parent, "k0@build-box"));
    // The note came before the wait looked: it still counts.
    timeout(Duration::from_secs(20), waiting.woken())
        .await
        .expect("the note woke the wait");

    drop(waiting);
    assert!(!waits.wake(&parent, "k0@build-box"), "the wait ended");
}

#[tokio::test]
async fn parallel_waits_on_one_child_wake_together_and_end_apart() {
    let waits = Waits::default();
    let parent = SessionId::from("p0");
    let first = waits.hold(&parent, "k0@build-box");
    let second = waits.hold(&parent, "k0@build-box");

    assert!(waits.wake(&parent, "k0@build-box"));
    for waiting in [&first, &second] {
        timeout(Duration::from_secs(20), waiting.woken())
            .await
            .expect("the note woke both waits");
    }

    drop(first);
    assert!(waits.wake(&parent, "k0@build-box"), "the second still waits");
    drop(second);
    assert!(!waits.wake(&parent, "k0@build-box"), "both ended");
}

#[tokio::test(start_paused = true)]
async fn a_note_the_parents_node_did_not_hear_is_sent_again_until_it_does() {
    use crate::agents::family::notes::tell_until_heard;
    use crate::agents::family::peer::NoteFailed;

    let tries = std::cell::Cell::new(0);
    let started = tokio::time::Instant::now();
    let sent = tell_until_heard(|| {
        tries.set(tries.get() + 1);
        std::future::ready(if tries.get() < 3 {
            Err(NoteFailed::Silent("no link".to_owned()))
        } else {
            Ok(())
        })
    })
    .await;

    assert_eq!(sent, Ok(()));
    assert_eq!(tries.get(), 3);
    assert_eq!(started.elapsed(), std::time::Duration::from_secs(10));
}

#[tokio::test(start_paused = true)]
async fn a_note_the_parents_node_refuses_is_not_sent_again() {
    use crate::agents::family::notes::tell_until_heard;
    use crate::agents::family::peer::NoteFailed;

    let tries = std::cell::Cell::new(0);
    let sent = tell_until_heard(|| {
        tries.set(tries.get() + 1);
        std::future::ready(Err(NoteFailed::Refused("no such child".to_owned())))
    })
    .await;

    assert_eq!(sent, Err("no such child".to_owned()));
    assert_eq!(tries.get(), 1);
}

#[tokio::test]
async fn a_background_child_that_closes_is_handed_to_the_deleter_at_once() {
    let rig = Rig::new();
    let mut closed = rig.agents.closed.take().expect("the deleter's end");
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let who = rig.agents.who_of(&parent);
    let mut link = rig.agents.link_for(&parent, &who).expect("within limits");
    link.background = true;
    let child = rig
        .agents
        .start_child(link, start("kid", "one", rig.dir.path()))
        .await
        .expect("the child starts");
    let other = child_of(&rig.agents, &parent, "kid", "two", rig.dir.path()).await;

    until(&child, |live| live.info.turns == 1).await;
    child.close().await;
    other.close().await;

    let told = timeout(Duration::from_secs(20), closed.recv())
        .await
        .expect("told in time")
        .expect("the channel is open");

    assert_eq!(told.id, child.id);
    // The one that is no background child tells nobody.
    assert!(closed.try_recv().is_err());
    rig.agents.shutdown().await;
}

#[test]
fn links_keep_the_remote_children_without_an_agents() {
    let links = Links::default();
    let machine = SecretKeyFor::id();
    let parent = SessionId::from("p0");
    let mut settled = RemoteKid::expected(&parent, machine, "box");
    settled.id = SessionId::from("k0");
    let waiting = RemoteKid::expected(&parent, machine, "box");
    crate::lock(&links.remote).extend([settled.clone(), waiting.clone()]);

    assert_eq!(links.kids_to_keep(&parent), [settled]);
    assert!(links.kids_to_keep(&SessionId::from("other")).is_empty());
    assert!(links.keeps(&parent, machine, &SessionId::from("k0"), &SessionId::default()));
    assert!(links.keeps(&parent, machine, &SessionId::from("x"), &SessionId::from("y")));
    assert!(!links.keeps(&SessionId::from("other"), machine, &SessionId::from("k0"), &SessionId::default()));
}

// The links are made from the node's membership and answer without an
// `Agents` around them.
#[test]
fn links_are_built_from_a_membership_and_name_the_kids_a_restart_keeps() {
    let links = Links::new(crate::membership::Membership::default());
    let (parent, other) = (SessionId::from("p1"), SessionId::from("p2"));
    let kid = |parent: &SessionId, id: &str| RemoteKid {
        parent: parent.clone(),
        machine: SecretKeyFor::id(),
        name: format!("box-{id}"),
        id: SessionId::from(id),
        ended: false,
        since: 0,
    };
    crate::lock(&links.remote).extend([kid(&parent, "k1"), kid(&other, "k2"), kid(&parent, "")]);

    assert!(links.membership.get().is_none());
    // The pending start is not kept, and the other parent's kid is not its.
    let kept = links.kids_to_keep(&parent);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].id, SessionId::from("k1"));
}
