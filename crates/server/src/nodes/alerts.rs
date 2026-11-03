//! Notices for what a node's coding-agent sessions did: a question they
//! ask, a turn that ended, a session that failed. The node sends them on
//! one `FollowAlerts` stream per connection (see `beep_proto::node::agent`)
//! and each goes to the user's devices as a `Notice`.
//!
//! It goes only to the device that started the session through Beep. A
//! session no device started (one run in a terminal, or by a routine or a
//! project agent) tells the user's computers, never a phone.
//!
//! A phone gets it with no chat, so it opens Beep. A computer gets the same
//! text with an `AgentNoticeKey` as its `session_id`, so a click opens the
//! session and a question has Allow and Deny. A session that reports to a
//! chat or works for a project is left out: the chat or project already
//! tells the user.

use super::Online;
use crate::{
    app::{Reach, UserNotice},
    nodes::deliver::agent_name,
    protocol::{
        AgentNoticeKey, Notice,
        node::agent::{
            Alert, AlertKind, SESSIONS_PROTOCOL, SessionsReply, SessionsRequest, StopReason,
        },
    },
    store::SessionOn,
    user::User,
};
use anyhow::{Result, bail};
use iroh::EndpointId;
use std::path::Path;

/// The longest body, in characters; the session has the rest.
const MAX_BODY: usize = 240;

impl Online {
    /// Tells the user what the node's sessions did until the stream or the
    /// connection ends. A node from before `FollowAlerts` closes the stream
    /// at once.
    pub(super) async fn keep_alerts(&self, user: &User) -> Result<()> {
        let (mut tx, mut rx) = self.open_stream(SESSIONS_PROTOCOL).await?;
        tx.send(&SessionsRequest::FollowAlerts {}).await?;
        let machine = self.connection.remote_id();
        // The name the user gave the machine, as the session list shows it.
        let name = user
            .nodes()
            .await?
            .into_iter()
            .find(|node| node.endpoint == machine.to_string())
            .map_or_else(|| self.name.clone(), |node| node.name);
        let host = Host {
            id: machine.to_string(),
            name,
        };
        while let Some(reply) = rx.recv::<SessionsReply>().await? {
            let SessionsReply::Alert { alert } = reply else {
                bail!("the node answered something other than an alert");
            };
            let Some(app) = self.app.upgrade() else {
                return Ok(());
            };
            let elsewhere = self.agents.owner(&alert.session).is_some()
                || app.projects.worker_of(&machine, &alert.session).is_some();
            if elsewhere {
                continue;
            }
            let to = reach(user, &machine, &alert).await;
            // Nobody listening is fine: the notice has nowhere to go.
            let _ = app
                .notifications
                .send(user_notice(user.id(), &host, &alert, to));
        }
        Ok(())
    }
}

/// The device that started the alert's session, or else the computers.
async fn reach(user: &User, machine: &EndpointId, alert: &Alert) -> Reach {
    let on = SessionOn {
        machine,
        agent: &alert.agent,
        session: &alert.session,
    };
    match user.agent_starter(on).await {
        Ok(Some(device)) => Reach::Device(device),
        Ok(None) => Reach::Desktops,
        // The computers still hear of it; a phone is never told by mistake.
        Err(e) => {
            log::warn!("agent alert starter not read; error={e:#}");
            Reach::Desktops
        }
    }
}

/// The machine a session runs on.
struct Host {
    id: String,
    name: String,
}

/// `alert` as a notice for `user`'s devices in `to`: without a chat for
/// phones, with the session's key for computers.
fn user_notice(user: &str, host: &Host, alert: &Alert, to: Reach) -> UserNotice {
    let notice = Notice {
        title: title(host, alert),
        body: body(alert),
        session_id: String::new(),
        app: None,
        approval: None,
        changed: false,
        group: false,
    };
    let question = match &alert.kind {
        AlertKind::Asks { question } => Some(question.id),
        _ => None,
    };
    let key = AgentNoticeKey {
        machine_id: host.id.clone(),
        agent: alert.agent.clone(),
        session: alert.session.clone(),
        question,
        machine_name: host.name.clone(),
    };
    let desktop = Notice {
        session_id: key.to_string(),
        ..notice.clone()
    };
    UserNotice {
        user: user.to_owned(),
        notice,
        desktop: Some(desktop),
        to,
    }
}

/// "Claude Code on build-box asks".
fn title(host: &Host, alert: &Alert) -> String {
    let what = match &alert.kind {
        AlertKind::Asks { .. } => "asks",
        AlertKind::TurnEnded { stop_reason, .. } => match stop_reason {
            StopReason::EndTurn => "finished",
            StopReason::Failed => "failed",
            _ => "stopped",
        },
        AlertKind::Failed { .. } => "failed",
        // A kind from a newer node still says something happened.
        _ => "needs a look",
    };
    format!("{} on {} {what}", agent_name(&alert.agent), host.name)
}

/// What the session is about, then what happened, on one short line.
fn body(alert: &Alert) -> String {
    let said = match &alert.kind {
        AlertKind::Asks { question } => match &question.detail {
            Some(detail) => format!("{} ({detail})", question.text),
            None => question.text.clone(),
        },
        AlertKind::TurnEnded {
            stop_reason,
            last_message,
            error,
        } => {
            let fallback = || stop_reason.to_string();
            match stop_reason {
                StopReason::EndTurn => Some(last_message.as_str())
                    .filter(|message| !message.trim().is_empty())
                    .map_or_else(fallback, str::to_owned),
                StopReason::Failed => error.clone().unwrap_or_else(fallback),
                _ => fallback(),
            }
        }
        AlertKind::Failed { reason } => reason.clone(),
        _ => String::new(),
    };
    clip(&format!("{}: {said}", about(alert)))
}

/// The session's title, or else its folder's name, or else its id.
fn about(alert: &Alert) -> String {
    if let Some(title) = alert.title.as_deref().filter(|t| !t.trim().is_empty()) {
        return title.trim().to_owned();
    }
    Path::new(&alert.cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(&alert.session)
        .to_owned()
}

/// `text` on one line without emphasis or code marks, which a
/// notification shows literally, cut to `MAX_BODY` characters.
fn clip(text: &str) -> String {
    let plain = text.replace("**", "").replace('`', "");
    let line = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(MAX_BODY) {
        Some((end, _)) => format!("{}...", line[..end].trim_end()),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::node::agent::Question;

    fn host() -> Host {
        Host {
            id: "ab12".into(),
            name: "build-box".into(),
        }
    }

    fn alert(kind: AlertKind) -> Alert {
        Alert {
            session: "a3f9c1".into(),
            agent: "claude".into(),
            title: Some("Fix the login test".into()),
            cwd: "/srv/project".into(),
            kind,
        }
    }

    fn ended(stop_reason: StopReason, last_message: &str, error: Option<&str>) -> Alert {
        alert(AlertKind::TurnEnded {
            stop_reason,
            last_message: last_message.into(),
            error: error.map(str::to_owned),
        })
    }

    #[test]
    fn a_question_tells_phones_and_lets_computers_answer() {
        let question = Question {
            id: 3,
            text: "Run cargo test".into(),
            detail: Some("cargo test -p server".into()),
            options: Vec::new(),
        };
        let sent = user_notice(
            "u1",
            &host(),
            &alert(AlertKind::Asks { question }),
            Reach::Desktops,
        );
        assert_eq!(sent.user, "u1");
        let phone = &sent.notice;
        assert_eq!(phone.title, "Claude Code on build-box asks");
        assert_eq!(
            phone.body,
            "Fix the login test: Run cargo test (cargo test -p server)"
        );
        assert_eq!(phone.session_id, "", "a phone opens Beep, not a chat");
        assert!(phone.approval.is_none() && !phone.changed && !phone.group);
        let desktop = sent.desktop.expect("computers get the session");
        assert_eq!((&desktop.title, &desktop.body), (&phone.title, &phone.body));
        assert_eq!(
            desktop.session_id.parse::<AgentNoticeKey>(),
            Ok(AgentNoticeKey {
                machine_id: "ab12".into(),
                agent: "claude".into(),
                session: "a3f9c1".into(),
                question: Some(3),
                machine_name: "build-box".into(),
            })
        );
    }

    #[test]
    fn a_turn_says_how_it_ended() {
        let said = |alert: &Alert| {
            let sent = user_notice("u1", &host(), alert, Reach::Desktops);
            (sent.notice.title, sent.notice.body)
        };
        assert_eq!(
            said(&ended(
                StopReason::EndTurn,
                "All **42** tests pass.\n",
                None
            )),
            (
                "Claude Code on build-box finished".into(),
                "Fix the login test: All 42 tests pass.".into()
            )
        );
        assert_eq!(
            said(&ended(StopReason::Failed, "", Some("rate limit"))),
            (
                "Claude Code on build-box failed".into(),
                "Fix the login test: rate limit".into()
            )
        );
        assert_eq!(
            said(&ended(StopReason::Cancelled, "Halfway", None)),
            (
                "Claude Code on build-box stopped".into(),
                "Fix the login test: cancelled".into()
            )
        );
        let mut untitled = ended(StopReason::MaxTokens, "", None);
        untitled.title = None;
        assert_eq!(said(&untitled).1, "project: hit the token limit");
        let failed = alert(AlertKind::Failed {
            reason: "the agent exited".into(),
        });
        assert_eq!(said(&failed).0, "Claude Code on build-box failed");
        let key: AgentNoticeKey = user_notice("u1", &host(), &failed, Reach::Desktops)
            .desktop
            .unwrap()
            .session_id
            .parse()
            .unwrap();
        assert_eq!(key.question, None, "only a question has Allow and Deny");
    }

    #[test]
    fn a_long_message_is_cut_to_one_line() {
        let long = format!("First line\n\n{}", "word ".repeat(200));
        let body = body(&ended(StopReason::EndTurn, &long, None));
        assert!(body.starts_with("Fix the login test: First line word"));
        assert!(body.ends_with("..."));
        assert_eq!(body.chars().count(), MAX_BODY + 3);
    }
}
