//! Notices for what a node's agent sessions did, sent to the device that started them.
//! Sessions no device started go to computers only; chat or project sessions are left out.

use super::Online;
use crate::{
    account::user::User,
    app::{Reach, UserNotice},
    protocol::{
        Notice,
        node::agent::{Alert, SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
    },
    store::SessionOn,
};
use anyhow::{Result, bail};
use iroh::EndpointId;

impl Online {
    /// Tells the user what the node's sessions did until the stream or the connection ends.
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

/// `alert` as a notice for `user`'s devices in `to`.
fn user_notice(user: &str, host: &Host, alert: &Alert, to: Reach) -> UserNotice {
    let notice = Notice {
        title: alert.notice_title(&host.name),
        body: alert.notice_body(),
        session_id: String::new(),
        app: None,
        approval: None,
        changed: false,
        group: false,
    };
    let desktop = Notice {
        session_id: alert.key(&host.id, &host.name).to_string(),
        ..notice.clone()
    };
    UserNotice {
        user: user.to_owned(),
        notice,
        desktop: Some(desktop),
        to,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        AgentNoticeKey,
        node::agent::{AlertKind, Question},
    };

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
}
