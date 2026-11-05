use super::*;
use crate::agents::{AgentKind, OpenSession};
use std::{collections::BTreeMap, path::Path};
use tokio::time::timeout;

/// An ACP adapter in shell: it logs what it reads to `$1`, answers each
/// prompt at once, except one that starts with `hang`, which ends when the
/// turn is cancelled. Each run names its session after its process.
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
    }
}

async fn until(session: &Session, done: impl FnMut(&Live) -> bool) {
    let mut live = session.live.clone();
    timeout(Duration::from_secs(20), live.wait_for(done))
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
        .expect("the session starts");
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
    let link = agents.link_for(parent, &who, false).expect("within limits");
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
        let agents = Agents::new(vec![top.clone(), kid.clone()]);
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
async fn a_chain_may_go_three_deep_and_no_deeper() {
    let rig = Rig::new();
    let parent = root(&rig.agents, &rig.top, rig.dir.path()).await;
    let first = child_of(&rig.agents, &parent, "kid", "one", rig.dir.path()).await;
    let second = child_of(&rig.agents, &first, "kid", "two", rig.dir.path()).await;
    let third = child_of(&rig.agents, &second, "kid", "three", rig.dir.path()).await;
    assert_eq!(third.info().parent.expect("a link").depth, MAX_DEPTH);
    let who = rig.agents.who_of(&third);
    let refused = rig.agents.link_for(&third, &who, false).unwrap_err();
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
    let link = |others| ParentLink {
        machine: Some(far),
        session: "p1".into(),
        key: "p1".into(),
        root_machine: Some(far),
        root: "p0".into(),
        depth: 1,
        others,
        agent: "kid".into(),
        mode: None,
    };
    // Seven were counted on the machines before this one: one more fits.
    let first = rig
        .agents
        .start_child(link(7), start("kid", "go", rig.dir.path()))
        .await
        .expect("the eighth starts");
    let refused = rig
        .agents
        .start_child(link(7), start("kid", "go", rig.dir.path()))
        .await
        .unwrap_err();
    assert!(refused.contains("at most 8 agents"), "{refused}");
    // Once it ends there is room again.
    first.close().await;
    rig.agents
        .start_child(link(7), start("kid", "go", rig.dir.path()))
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
            id: format!("k{at}"),
            ended: false,
        };
        crate::lock(&rig.agents.family.remote).push(kid);
    }
    let refused = rig.agents.link_for(&parent, &who, false).unwrap_err();
    assert!(refused.contains("8 children"), "{refused}");
    // One that ended frees its place but stays there to read.
    let first = rig.agents.remote_kids(&who.session)[0].clone();
    rig.agents.end_remote(&first);
    assert!(rig.agents.link_for(&parent, &who, false).is_ok());
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
        .child_of(&other, "nothing")
        .expect_err("no such agent");
    assert_eq!(refused, nothing.replace("nothing", &id));
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
    let after = Agents::new(vec![rig.top.clone(), rig.kid.clone()]);
    after.settings_arrived();
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
    };
    assert_eq!(child_mode(&link("claude", None), "codex"), Ok(None));
    assert_eq!(
        child_mode(&link("claude", Some("plan")), "claude"),
        Ok(Some("plan".to_owned()))
    );
    let refused = child_mode(&link("claude", Some("plan")), "codex").unwrap_err();
    assert!(refused.contains("below full access"), "{refused}");
    let info = |mode: &str| SessionInfo {
        agent: "claude".into(),
        mode: Some(mode.into()),
        ..SessionInfo::default()
    };
    assert_eq!(held_to(&info("bypassPermissions")), None);
    assert_eq!(held_to(&info("plan")), Some("plan".to_owned()));
}

/// Endpoint ids for tests.
struct SecretKeyFor;

impl SecretKeyFor {
    fn id() -> EndpointId {
        iroh::SecretKey::generate().public()
    }
}
