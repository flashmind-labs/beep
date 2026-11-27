use super::*;
use crate::agents::Parts;
use crate::agents::AgentKind;
use beep_proto::node::agent::{CodeFinding, CodeVerdict, FindingSeverity};
use beep_proto::watched;
use std::{collections::BTreeMap, path::Path, time::Duration};
use tokio::time::{sleep, timeout};

/// An ACP adapter in shell: it logs what it reads to `$1`, names its session
/// `$2` when given and answers each prompt at once, except one that holds `HOLD`, which ends when the turn is
/// cancelled.
const ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line; printf '%s\n' "$line" >> "$1"
reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}'
while read -r line; do
  printf '%s\n' "$line" >> "$1"
  case "$line" in
    *'"session/new"'*) reply '{"sessionId":"'"${2:-s-$$}"'"}' ;;
    *'"session/prompt"'*HOLD*) prompt=$(id) ;;
    *'"session/cancel"'*) [ -n "$prompt" ] && printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$prompt" ;;
    *'"session/prompt"'*) reply '{"stopReason":"end_turn"}' ;;
    *'"method"'*'"id"'*|*'"id"'*'"method"'*) reply '{}' ;;
  esac
done
"#;

fn kind(name: &str, log: &Path) -> AgentKind {
    named_kind(name, log, "")
}

/// `kind`, its sessions named `session`.
fn named_kind(name: &str, log: &Path, session: &str) -> AgentKind {
    AgentKind {
        name: name.into(),
        program: "sh".into(),
        args: vec![
            "-c".into(),
            ADAPTER.into(),
            "fake-adapter".into(),
            log.display().to_string(),
            session.into(),
        ],
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    }
}

struct Rig {
    dir: tempfile::TempDir,
    agents: Arc<Agents>,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let top = kind("top", &dir.path().join("top.log"));
        let kid = kind("kid", &dir.path().join("kid.log"));
        Self {
            agents: Arc::new(Agents::new(Parts { kinds: vec![top, kid], ..Parts::default() })),
            dir,
        }
    }

    fn cwd(&self) -> String {
        self.dir.path().display().to_string()
    }

    fn order(&self, focus: &str, parent: Option<SessionId>) -> ReviewOrder {
        ReviewOrder {
            cwd: self.cwd(),
            agent: "kid".into(),
            base: "main".into(),
            focus: focus.into(),
            parent,
            ..ReviewOrder::default()
        }
    }

    /// A session the user began, once its first turn ended.
    async fn root(&self) -> Arc<Session> {
        let top = self.agents.named("top").expect("the top agent");
        let cwd = self.cwd();
        let begin = Begin {
            prompt: Some("go".into()),
            ..Begin::default()
        };
        let session = self
            .agents
            .start(&top, Some(&cwd), begin)
            .await.expect("the session starts");
        let mut live = session.live.clone();
        timeout(
            Duration::from_secs(20),
            watched::wait_for(&mut live, |live| {
                live.agent_session.is_some() && live.info.turns == 1
            }),
        )
        .await
        .expect("the session got there in time")
        .expect("the session is kept");
        session
    }

    /// Waits until the review `id` is over and its reviewer is gone.
    async fn finished(&self, id: &SessionId) -> ReviewRun {
        let over = async {
            loop {
                if let Some(run) = self.agents.review(id)
                    && !run.running()
                    && self.agents.get(id).is_none()
                {
                    return run;
                }

                sleep(Duration::from_millis(20)).await;
            }
        };
        timeout(Duration::from_secs(20), over)
            .await
            .expect("the review ended in time")
    }

    fn nudges(&self) -> usize {
        std::fs::read_to_string(self.dir.path().join("kid.log"))
            .unwrap_or_default()
            .matches(NUDGE)
            .count()
    }
}

fn found() -> SubmittedReview {
    SubmittedReview {
        verdict: CodeVerdict::Changes,
        summary: "One bug.".into(),
        findings: vec![CodeFinding {
            file: "src/limit.rs".into(),
            line: Some(42),
            severity: FindingSeverity::High,
            summary: "Off by one".into(),
            scenario: "A limit of 0 lets one request through.".into(),
        }],
    }
}

#[tokio::test]
async fn a_submitted_review_stays_with_the_parent_and_the_reviewer_goes() {
    let rig = Rig::new();
    let parent = rig.root().await;
    let run = rig
        .agents
        .start_review(rig.order("HOLD", Some(parent.id.clone())))
        .await
        .expect("the review starts");
    let reviewer = rig.agents.get(&run.id).expect("the reviewer runs");
    let link = reviewer.info().parent.expect("the reviewer is a child");
    assert_eq!(link.session, Agents::name_of(&parent));
    assert!(rig.agents.is_reviewer(&run.id));
    assert!(!rig.agents.is_reviewer(&parent.id));
    let listed: Vec<SessionId> = rig.agents.sessions.list().into_iter().map(|info| info.id).collect();
    assert_eq!(listed, [parent.info().id], "the reviewer is not listed");
    let mut all: Vec<(SessionId, bool)> = rig
        .agents
        .sessions.listed(true)
        .into_iter()
        .map(|info| (info.id, info.reviewer))
        .collect();
    all.sort_by_key(|(_, reviewer)| *reviewer);
    assert_eq!(
        all,
        [(parent.info().id, false), (reviewer.info().id, true)],
        "ListAll lists the reviewer under its parent"
    );
    assert!(
        rig.agents
            .open_sessions()
            .iter()
            .all(|open| open.depth() == 0),
        "the reviewer is not kept over a restart"
    );
    assert!(
        rig.agents.submit_review(&parent.id, found()).is_err(),
        "only the reviewer hands in"
    );
    rig.agents
        .submit_review(&run.id, found())
        .expect("the reviewer hands in");
    // Its turn ends once the tool call is answered.
    reviewer
        .order(Command::Cancel { by_user: false })
        .await
        .expect("cancelled");
    let done = rig.finished(&run.id).await;
    assert_eq!(done.review, Some(found()));
    assert_eq!(done.error, None);
    assert_eq!(rig.nudges(), 0);
    assert_eq!(
        rig.agents.reviews_of(&parent.id),
        std::slice::from_ref(&done)
    );
    assert_eq!(
        rig.agents.reviews_of(&Agents::name_of(&parent)),
        std::slice::from_ref(&done)
    );
    let who = rig.agents.who_of(&parent);
    assert_eq!(rig.agents.review_for(&who, &run.id), Some(done));
    rig.agents.shutdown().await;
}

#[tokio::test]
async fn a_reviewer_that_ends_without_a_review_is_nudged_once() {
    let rig = Rig::new();
    let run = rig
        .agents
        .start_review(rig.order("", None))
        .await
        .expect("the review starts");
    assert_eq!(run.parent, None);
    let done = rig.finished(&run.id).await;
    assert_eq!(rig.nudges(), 1);
    assert_eq!(done.review, None);
    assert_eq!(
        done.error.as_deref(),
        Some("The reviewer ended without calling submit_review.")
    );
    assert!(rig.agents.sessions.list().is_empty());
    assert!(
        rig.agents.sessions.listed(true).is_empty(),
        "a reviewer without a parent is never listed"
    );
}

#[tokio::test]
async fn a_finished_reviewer_leaves_no_session_on_disk() {
    const ID: &str = "0f3c2a9e-1111-4222-8333-944455556666";
    let dir = tempfile::tempdir().expect("a temporary folder");
    let transcript = dir
        .path()
        .join(format!("claude/projects/-srv-project/{ID}.jsonl"));
    std::fs::create_dir_all(transcript.parent().expect("a parent")).expect("folders");
    std::fs::write(
        &transcript,
        r#"{"type":"user","timestamp":"2026-10-29T08:00:00.000Z","message":{"role":"user","content":"Review these changes."}}"#,
    )
    .expect("a transcript");
    let claude = named_kind("claude", &dir.path().join("claude.log"), ID);
    let rig = Rig {
        agents: Arc::new(Agents {
            occupancy: crate::occupancy::Occupancy::over(crate::logs::claude::Store::at(&dir.path().join("claude")), Default::default()),
            ..Agents::new(Parts { kinds: vec![claude], ..Parts::default() })
        }),
        dir,
    };
    let run = rig
        .agents
        .start_review(ReviewOrder {
            agent: "claude".into(),
            ..rig.order("HOLD", None)
        })
        .await
        .expect("the review starts");
    rig.agents
        .submit_review(&run.id, found())
        .expect("the reviewer hands in");
    let reviewer = rig.agents.get(&run.id).expect("the reviewer runs");
    reviewer
        .order(Command::Cancel { by_user: false })
        .await
        .expect("cancelled");
    let done = rig.finished(&run.id).await;
    assert_eq!(done.review, Some(found()));
    assert!(!transcript.exists(), "the reviewer's transcript is deleted");
    rig.agents.shutdown().await;
}

#[test]
fn the_prompt_names_the_base_and_the_tool() {
    let uncommitted = prompt("HEAD", "", None);
    assert!(uncommitted.contains("`git diff HEAD`"), "{uncommitted}");
    assert!(uncommitted.contains("submit_review"));
    let branch = prompt("origin/main; rm -rf /", "the cache", None);
    assert!(
        branch.contains("merge-base HEAD origin/mainrm-rf"),
        "{branch}"
    );
    assert!(branch.contains("Look first at what the author asks: the cache"));
    let house = house::HouseRules {
        path: ".codex/agents/reviewer.toml".into(),
        prompt: "Flag every unbounded channel.".into(),
    };
    let with = prompt("HEAD", "", Some(&house));
    assert!(
        with.ends_with("\n\nFlag every unbounded channel."),
        "{with}"
    );
    assert!(with.contains("defined in .codex/agents/reviewer.toml"));
}

#[tokio::test]
async fn a_wait_on_a_review_ends_when_the_review_does() {
    let rig = Rig::new();
    let parent = rig.root().await;
    let who = rig.agents.who_of(&parent);
    let run = rig
        .agents
        .start_review(rig.order("HOLD", Some(parent.id.clone())))
        .await
        .expect("the review starts");
    let reviewer = rig.agents.get(&run.id).expect("the reviewer runs");
    let wait = tokio::spawn({
        let (agents, who, run) = (rig.agents.clone(), who.clone(), run.clone());
        async move { crate::tools::review::waited(&agents, &who, run).await }
    });

    // The wait is parked on the running review before it ends.
    tokio::task::yield_now().await;
    assert!(!wait.is_finished());
    rig.agents
        .submit_review(&run.id, found())
        .expect("the reviewer hands in");
    reviewer
        .order(Command::Cancel { by_user: false })
        .await
        .expect("cancelled");

    // Well inside the one second a polling wait would add, and the cap.
    let ended = timeout(Duration::from_millis(500), wait)
        .await
        .expect("the wait ended when the review did")
        .expect("the wait task ran");
    let ended = serde_json::to_value(&ended).expect("a view is JSON");
    assert_eq!(ended["state"], "done");
    assert_eq!(ended["note"], serde_json::Value::Null);
    rig.agents.shutdown().await;
}
