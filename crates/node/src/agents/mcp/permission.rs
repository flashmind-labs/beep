//! The ask the node raises itself for a machine tool the server cannot ask about: a call to a
//! machine only the mesh knows, or one made while the server's link is down. It is a question in
//! the calling agent's session, so it travels like any question of an agent: every follower of
//! the node's alerts hears it (each desktop directly, the server for the phones) and the first
//! answer settles it.

use crate::{
    agents::{Answered, Asking},
    link::Offers,
};
use beep_proto::node::{
    agent::{AnswerOption, OptionKind},
    tools::{Reply, Request, Tool},
};
use serde_json::Value;
use std::time::Duration;
use tokio::time::timeout;

/// How long the call waits for the user.
const ASK_LIMIT: Duration = Duration::from_secs(5 * 60);
const ALLOW: &str = "allow";
const DENY: &str = "deny";

/// Asks the user in the calling session to allow `request`. `None` when they did; otherwise the
/// failed reply that says why the call did not run.
pub(super) async fn ask_first(offers: &Offers, request: &Request, args: &Value) -> Option<Reply> {
    let answered = timeout(
        ASK_LIMIT,
        offers
            .agents
            .ask(&request.session, card(request.tool, args)),
    )
    .await;
    let text = match answered {
        Ok(Ok(Answered::Picked(option))) if option.kind == OptionKind::AllowOnce => return None,
        Ok(Ok(_)) => format!(
            "The user did not allow {}. Do not try it again unless they ask.",
            request.tool
        ),
        Ok(Err(e)) => format!("Not run: {e:#}"),
        Err(_) => format!(
            "Not run: nobody answered the question within {} minutes.",
            ASK_LIMIT.as_secs() / 60
        ),
    };
    Some(Reply::Done { text, failed: true })
}

/// The question that asks to allow `tool` with `args`.
fn card(tool: Tool, args: &Value) -> Asking {
    let action = match tool {
        Tool::SendFiles => "send files",
        Tool::RunOn => "run on a machine",
        _ => "read a machine's log",
    };
    let say = |label: &str, key: &str| {
        let value = match args.get(key)? {
            Value::String(text) => text.clone(),
            Value::Array(items) => items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            _ => return None,
        };
        Some(format!("{label}: {value}"))
    };
    let detail = [
        say("Machine", "machine"),
        say("Command", "command"),
        say("Prompt", "prompt"),
        say("Files", "paths"),
        say("Into", "dest"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n");
    let option = |id: &str, name: &str, kind| AnswerOption {
        id: id.into(),
        name: name.into(),
        kind,
    };
    Asking {
        text: format!("Allow {action}? (Your machines)"),
        detail: (!detail.is_empty()).then_some(detail),
        options: vec![
            option(ALLOW, "Allow", OptionKind::AllowOnce),
            option(DENY, "Deny", OptionKind::RejectOnce),
        ],
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{
        agents::{AgentKind, Begin, Command, Session},
        link::Offers,
        machine::provision::Provision,
    };
    use beep_proto::node::agent::{Alert, AlertKind, Status};
    use iroh::SecretKey;
    use serde_json::json;
    use std::{collections::BTreeMap, sync::Arc};

    /// An ACP adapter in shell that takes a session and ends every turn at once.
    const ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
while read -r line; do
  case "$line" in
    *'"initialize"'*) reply '{"protocolVersion":1,"agentCapabilities":{}}' ;;
    *'"session/new"'*) reply '{"sessionId":"s"}' ;;
    *'"session/prompt"'*) reply '{"stopReason":"end_turn"}' ;;
    *'"id"'*) reply '{}' ;;
  esac
done
"#;

    /// Offers with one idle session, and the folder that keeps them alive.
    async fn idle_session() -> (Offers, Arc<Session>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("a temp folder");
        let offers = Offers::local(&[], Provision::new(dir.path()), false);
        let kind = AgentKind {
            name: "fake".into(),
            program: "sh".into(),
            args: vec!["-c".into(), ADAPTER.into(), "fake-adapter".into()],
            env: BTreeMap::new(),
        };
        let session = offers
            .agents
            .start(
                &kind,
                Some(&dir.path().to_string_lossy()),
                Begin {
                    prompt: Some("hello".into()),
                    ..Begin::default()
                },
            )
            .expect("the session starts");
        let mut live = session.live.clone();
        timeout(
            Duration::from_secs(20),
            live.wait_for(|live| live.info.turns >= 1 && live.status() != Status::Working),
        )
        .await
        .expect("the session settles")
        .expect("the session is kept");
        (offers, session, dir)
    }

    fn send_files() -> Request {
        Request {
            tool: Tool::SendFiles,
            args: json!({ "machine": "box-b", "paths": ["a.txt"] }).to_string(),
            ..request("")
        }
    }

    fn request(session: &str) -> Request {
        Request {
            session: session.to_owned(),
            tool: Tool::SendFiles,
            args: String::new(),
            approved: beep_proto::node::tools::Approved::No,
        }
    }

    /// The next question a follower has not had, taken as a stream does. The turn that ended
    /// before it is skipped.
    async fn heard(follow: &crate::agents::alerts::Follow<'_>) -> Alert {
        loop {
            let alert = timeout(Duration::from_secs(20), follow.next())
                .await
                .expect("an alert is there");
            follow.sent();
            if matches!(alert.kind, AlertKind::Asks { .. }) {
                return alert;
            }
        }
    }

    fn asked(alert: &Alert) -> u32 {
        match &alert.kind {
            AlertKind::Asks { question } => question.id,
            other => panic!("expected a question, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_ask_from_a_node_reaches_two_desktops_and_the_first_answer_wins() {
        let (offers, session, _dir) = idle_session().await;
        let id = || SecretKey::generate().public();
        let (first_desktop, second_desktop, server) = (id(), id(), id());
        let hub = &offers.agents.alerts;
        let (first, second, upstream) = (
            hub.follow(first_desktop),
            hub.follow(second_desktop),
            hub.follow(server),
        );
        let request = Request {
            session: session.id.clone(),
            ..send_files()
        };
        let args: Value = serde_json::from_str(&request.args).expect("json");
        let call = tokio::spawn({
            let offers = offers.clone();
            async move { ask_first(&offers, &request, &args).await }
        });
        // Every follower hears the same question, as it hears an agent's: the desktops directly,
        // the server (which pushes it to the phones) on its own stream.
        let mut ids = Vec::new();
        for follow in [&first, &second, &upstream] {
            let alert = heard(follow).await;
            assert_eq!(alert.session, session.id);
            let AlertKind::Asks { question } = &alert.kind else {
                panic!("expected a question, got {alert:?}");
            };
            assert_eq!(question.text, "Allow send files? (Your machines)");
            assert_eq!(
                question.detail.as_deref(),
                Some("Machine: box-b\nFiles: a.txt")
            );
            ids.push(asked(&alert));
        }
        assert!(ids.iter().all(|id| *id == ids[0]), "{ids:?}");
        // The first answer wins and the call runs.
        let answer = |reply: &str| Command::Answer {
            question: ids[0],
            reply: reply.to_owned(),
        };
        session.order(answer("allow")).await.expect("taken");
        assert_eq!(call.await.expect("the call ends"), None);
        // The later answer is refused: the session no longer asks, which is what the desktop
        // reports as a conflict and what clears the card on the other devices.
        assert!(session.order(answer("deny")).await.is_err());
        assert!(session.info().question.is_none());
    }

    #[tokio::test]
    async fn a_denied_ask_fails_the_call_and_says_so() {
        let (offers, session, _dir) = idle_session().await;
        let follow = offers.agents.alerts.follow(SecretKey::generate().public());
        let request = Request {
            session: session.id.clone(),
            ..send_files()
        };
        let args: Value = serde_json::from_str(&request.args).expect("json");
        let call = tokio::spawn({
            let offers = offers.clone();
            async move { ask_first(&offers, &request, &args).await }
        });
        let question = asked(&heard(&follow).await);
        session
            .order(Command::Answer {
                question,
                reply: "deny".into(),
            })
            .await
            .expect("taken");
        let Some(Reply::Done { text, failed }) = call.await.expect("the call ends") else {
            panic!("a denied call must fail");
        };
        assert!(failed);
        assert!(
            text.starts_with("The user did not allow send_files."),
            "{text}"
        );
    }

    #[tokio::test]
    async fn a_send_files_ask_with_nobody_answering_fails_after_the_limit() {
        let (offers, session, _dir) = idle_session().await;
        let follow = offers.agents.alerts.follow(SecretKey::generate().public());
        // The clock is controlled from here on: the question is in-process, so nothing waits on
        // the adapter while it is stopped.
        tokio::time::pause();
        let request = Request {
            session: session.id.clone(),
            ..send_files()
        };
        let args: Value = serde_json::from_str(&request.args).expect("json");
        let mut call = tokio::spawn({
            let offers = offers.clone();
            async move { ask_first(&offers, &request, &args).await }
        });
        asked(&heard(&follow).await);
        tokio::time::advance(ASK_LIMIT - Duration::from_secs(1)).await;
        assert!(!call.is_finished(), "still waiting just before the limit");
        tokio::time::advance(Duration::from_secs(1)).await;
        let Some(Reply::Done { text, failed }) = (&mut call).await.expect("the call ends") else {
            panic!("an unanswered call must fail");
        };
        assert!(failed);
        assert_eq!(
            text,
            "Not run: nobody answered the question within 5 minutes."
        );
    }
}
