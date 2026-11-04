//! Following agent sessions started outside Beep, posting what they do to the chat that follows.
//! Follows live in memory: a server restart forgets them.

use super::{Online, Post, deliver::agent_name};
use crate::{
    account::user::User,
    app::App,
    protocol::node::agent::{
        Activity, ActivityKind, SESSIONS_PROTOCOL, SessionsReply, SessionsRequest,
    },
    tools::machines::agent_call,
};
use anyhow::{Result, bail};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
};
use tokio::time::timeout;
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

/// The most sessions followed on one node.
pub const MAX_FOLLOWED: usize = 16;
/// Who follows a session: the chat that asked.
#[derive(Clone)]
pub struct Follower {
    pub user: Arc<User>,
    pub chat: Uuid,
    /// The node as commands name it, such as `build-box`.
    pub node: String,
    /// The agent, such as `claude`.
    pub agent: String,
}

/// The followed sessions on every node, by node key.
#[derive(Default)]
pub struct Follows(Mutex<HashMap<String, NodeFollows>>);

#[derive(Default)]
struct NodeFollows {
    sessions: HashMap<String, Follower>,
    /// Reads the node's `Watch` stream; replaced whenever the followed sessions change.
    watch: Option<AbortOnDropHandle<()>>,
}

impl Follows {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, NodeFollows>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Follows `session` on `node` for `follower`, moving it from any earlier follower.
    pub fn follow(
        &self,
        app: &Arc<App>,
        node: &str,
        session: &str,
        follower: Follower,
    ) -> Result<()> {
        let mut all = self.lock();
        let follows = all.entry(node.to_owned()).or_default();
        if !follows.sessions.contains_key(session) && follows.sessions.len() >= MAX_FOLLOWED {
            bail!("{MAX_FOLLOWED} sessions are followed there already; unfollow one first");
        }
        follows.sessions.insert(session.to_owned(), follower);
        restart(app, node, follows);
        Ok(())
    }

    /// Stops following `session`; `false` when it was not followed.
    pub fn unfollow(&self, app: &Arc<App>, node: &str, session: &str) -> bool {
        let mut all = self.lock();
        let Some(follows) = all.get_mut(node) else {
            return false;
        };
        if follows.sessions.remove(session).is_none() {
            return false;
        }
        if follows.sessions.is_empty() {
            all.remove(node);
        } else {
            restart(app, node, follows);
        }
        true
    }

    pub fn followed(&self, node: &str) -> Vec<String> {
        let mut sessions: Vec<String> = self
            .lock()
            .get(node)
            .map(|follows| follows.sessions.keys().cloned().collect())
            .unwrap_or_default();
        sessions.sort();
        sessions
    }

    /// `node` connected again: its stream starts over on the new connection.
    pub fn reconnected(&self, app: &Arc<App>, node: &str) {
        if let Some(follows) = self.lock().get_mut(node) {
            restart(app, node, follows);
        }
    }

    fn follower(&self, node: &str, session: &str) -> Option<Follower> {
        self.lock().get(node)?.sessions.get(session).cloned()
    }
}

/// Replaces the node's task with one for its sessions now; it holds the app weakly.
fn restart(app: &Arc<App>, node: &str, follows: &mut NodeFollows) {
    let sessions: Vec<String> = follows.sessions.keys().cloned().collect();
    let watch = tokio::spawn(watch(Arc::downgrade(app), node.to_owned(), sessions));
    follows.watch = Some(AbortOnDropHandle::new(watch));
}

/// Reads the node's `Watch` stream until it ends.
async fn watch(app: Weak<App>, node: String, sessions: Vec<String>) {
    let Some(online) = app.upgrade().and_then(|app| app.nodes.online(&node)) else {
        return;
    };
    if let Err(e) = read_watch(&app, &node, &online, sessions).await {
        log::info!("follow: stopped watching node {}: {e:#}", online.name);
    }
}

async fn read_watch(
    app: &Weak<App>,
    node: &str,
    online: &Online,
    sessions: Vec<String>,
) -> Result<()> {
    let (mut tx, mut rx) = online.open_stream(SESSIONS_PROTOCOL).await?;
    tx.send(&SessionsRequest::Watch { sessions }).await?;
    match timeout(crate::api::transport::READ_TIMEOUT, rx.recv()).await?? {
        Some(SessionsReply::Watching { unknown }) if !unknown.is_empty() => {
            log::info!(
                "follow: {} sessions not found on {}",
                unknown.len(),
                online.name
            );
        }
        Some(SessionsReply::Watching { .. }) => {}
        Some(SessionsReply::Error { message }) => bail!("{message}"),
        Some(other) => bail!("the node answered {other:?}"),
        None => bail!("the node does not follow sessions; it may need an update"),
    }
    // `tx` stays open: closing it ends the watch.
    loop {
        match rx.recv::<SessionsReply>().await? {
            Some(SessionsReply::Activity { activity }) if worth_telling(&activity) => {
                let Some(app) = app.upgrade() else {
                    return Ok(());
                };
                if let Some(follower) = app.nodes.follows.follower(node, &activity.session) {
                    let text = report(&follower.node, &follower.agent, &activity);
                    let label = label(&follower.agent, activity.kind);
                    let post = Post {
                        user: follower.user,
                        chat: follower.chat,
                        label,
                        text,
                    };
                    app.nodes.deliveries.post(&app, post);
                }
            }
            Some(_) => {}
            None => return Ok(()),
        }
    }
}

/// Only a finished turn or a session waiting for the user wakes a chat.
fn worth_telling(activity: &Activity) -> bool {
    matches!(
        activity.kind,
        ActivityKind::TurnDone | ActivityKind::NeedsInput
    )
}

/// The label of a turn that reports a followed session.
fn label(agent: &str, kind: ActivityKind) -> String {
    let name = agent_name(agent);
    match kind {
        ActivityKind::NeedsInput => format!("{name} is waiting for you"),
        _ => format!("{name} finished"),
    }
}

/// The turn's message: what the session did, and what Beep can do.
fn report(node: &str, agent: &str, activity: &Activity) -> String {
    let id = &activity.session;
    let mut text = format!(
        "A Claude Code session you follow on {node} {}: {id}.",
        activity.kind.as_str()
    );
    if !activity.message.is_empty() {
        let what = match activity.kind {
            ActivityKind::NeedsInput => "It shows",
            _ => "Its last message",
        };
        text.push_str(&format!("\n{what}:\n{}", activity.message));
    }
    text.push_str(&format!(
        "\n\nTell the user in a sentence or two what it did or what it waits for. It runs in \
         their terminal, so only they can answer it there. {} reads more, and {} stops \
         these updates.",
        agent_call(node, agent, "transcript", id),
        agent_call(node, agent, "unfollow", id)
    ));
    text
}
