//! What coding agents on the machines of the mesh want the user for, sent
//! by the machines themselves: one `FollowAlerts` stream per machine, kept
//! open while the machine is a member and reachable. The alerts become the
//! same notices the server pushes, so they route the same way, and an
//! answer goes back to the machine. No server is involved.

use super::{Fleet, Opened};
use crate::error::{DesktopError, Result};
use beep_mesh::{MeshError, PeerClient};
use beep_node::link::reconnect::Backoff;
use beep_proto::{
    AgentNoticeKey, ApiError, Notice,
    node::agent::{Event, Request, SESSIONS_PROTOCOL, SessionsReply, SessionsRequest, protocol},
};
use iroh::EndpointId;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    select,
    sync::mpsc,
    task::JoinHandle,
    time::{sleep, timeout},
};

/// Notices and connections waiting for the app to route them.
const QUEUE: usize = 64;
/// How long a notice that came one way waits for the same one to come the
/// other way.
const TWICE: Duration = Duration::from_secs(60);
/// How long a machine gets to answer an answer.
const ANSWERING: Duration = Duration::from_secs(10);

/// What the streams tell the app.
pub(crate) enum Direct {
    /// A stream to a machine opened: what the app shows from machines may
    /// have changed while it was closed.
    Connected,
    Notice(Box<Notice>),
}

/// What follows every machine of the mesh, now and as it changes, and the
/// receiver for what it hears. The future runs until the receiver is
/// dropped; the caller spawns it.
pub(crate) fn start(fleet: Arc<Fleet>) -> (impl Future<Output = ()>, mpsc::Receiver<Direct>) {
    let (out, heard) = mpsc::channel(QUEUE);
    (follow(fleet, out), heard)
}

/// One task per machine, ended when the machine leaves the mesh or this
/// is dropped.
#[derive(Default)]
struct Running(HashMap<EndpointId, JoinHandle<()>>);

impl Running {
    /// Runs exactly the tasks for `machines`, starting `start` for new ones.
    fn keep(
        &mut self,
        machines: &HashSet<EndpointId>,
        mut start: impl FnMut(EndpointId) -> JoinHandle<()>,
    ) {
        self.0.retain(|id, task| {
            let stays = machines.contains(id);
            if !stays {
                task.abort();
            }
            stays
        });
        for id in machines {
            self.0.entry(*id).or_insert_with(|| start(*id));
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        for task in self.0.values() {
            task.abort();
        }
    }
}

async fn follow(fleet: Arc<Fleet>, out: mpsc::Sender<Direct>) {
    let mut opened = fleet.mesh.subscribe();
    let mut running = Running::default();
    loop {
        let current = opened.borrow_and_update().clone();
        let Opened::Open(mesh) = current else {
            if opened.changed().await.is_err() {
                return;
            }
            continue;
        };
        let mut updates = mesh.updates();
        loop {
            updates.borrow_and_update();
            let machines = mesh.devices().into_iter().map(|d| d.device.id).collect();
            running.keep(&machines, |id| {
                tokio::spawn(machine(fleet.clone(), id, out.clone()))
            });
            select! {
                _ = out.closed() => return,
                // Joined another mesh: look again.
                changed = opened.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    break;
                }
                changed = updates.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

/// Follows `id`'s alerts, again after each break, until the receiver goes.
async fn machine(fleet: Arc<Fleet>, id: EndpointId, out: mpsc::Sender<Direct>) {
    let mut backoff = Backoff::eager();
    loop {
        match stream(&fleet, id, &out, &mut backoff).await {
            Ok(()) => return,
            Err(e) => log::debug!("following alerts from {}: {e}", fleet.name_of(id)),
        }
        sleep(backoff.pause()).await;
    }
}

/// One stream's life. `Ok` when the receiver is gone.
async fn stream(
    fleet: &Fleet,
    id: EndpointId,
    out: &mpsc::Sender<Direct>,
    backoff: &mut Backoff,
) -> Result<()> {
    let client = fleet.client(id).await?;
    let (mut tx, mut rx) = client.open(SESSIONS_PROTOCOL).await?;
    tx.send(&SessionsRequest::FollowAlerts {})
        .await
        .map_err(MeshError::Link)?;
    backoff.reset();
    if out.send(Direct::Connected).await.is_err() {
        return Ok(());
    }
    // `tx` stays open: finishing it ends the stream.
    loop {
        let reply = rx.recv::<SessionsReply>().await.map_err(MeshError::Link)?;
        let Some(reply) = reply else {
            return Err(MeshError::Link(anyhow::anyhow!("the machine ended the stream")).into());
        };
        let SessionsReply::Alert { alert } = reply else {
            continue;
        };
        let key = alert.key(&id.to_string(), &fleet.name_of(id));
        let notice = Notice {
            title: alert.notice_title(&key.machine_name),
            body: alert.notice_body(),
            session_id: key.to_string(),
            app: None,
            approval: None,
            changed: false,
            group: false,
        };
        if out.send(Direct::Notice(Box::new(notice))).await.is_err() {
            return Ok(());
        }
    }
}

/// Answers the question `key` names, on the machine that asked it. Refused
/// as a conflict when the session no longer asks it.
pub(crate) async fn answer(fleet: &Fleet, key: &AgentNoticeKey, reply: &str) -> Result<()> {
    let (Some(question), Ok(machine)) = (key.question, EndpointId::from_str(&key.machine_id))
    else {
        return Err(DesktopError::BadRequest("not a question".into()));
    };
    let client = fleet.client(machine).await?;
    timeout(ANSWERING, attached(&client, key, question, reply))
        .await
        .map_err(|_| DesktopError::BadRequest("the machine did not answer in time".into()))?
}

async fn attached(
    client: &PeerClient,
    key: &AgentNoticeKey,
    question: u32,
    reply: &str,
) -> Result<()> {
    let (mut tx, mut rx) = client.open(&protocol(&key.agent)).await?;
    tx.send(&Request::Attach {
        session: key.session.clone(),
    })
    .await
    .map_err(MeshError::Link)?;
    loop {
        let event = rx.recv::<Event>().await.map_err(MeshError::Link)?;
        match event {
            Some(Event::Attached { session }) => {
                if session.question.as_ref().map(|q| q.id) != Some(question) {
                    return Err(ApiError::Conflict {
                        message: "The session no longer asks that question.".into(),
                    }
                    .into());
                }
                tx.send(&Request::Answer {
                    question,
                    reply: reply.to_owned(),
                })
                .await
                .map_err(MeshError::Link)?;
            }
            Some(Event::Refused { .. }) => return Err(ApiError::NotFound {}.into()),
            Some(Event::Ack { error: None }) => return Ok(()),
            Some(Event::Ack { error: Some(why) }) => return Err(DesktopError::BadRequest(why)),
            // The session's own news while attached.
            Some(_) => {}
            None => {
                return Err(
                    MeshError::Link(anyhow::anyhow!("the machine ended the stream")).into(),
                );
            }
        }
    }
}

/// Whether `machine`, as text, is this Mac or a member of its mesh: the
/// machines `answer` reaches.
pub(crate) fn reaches(fleet: &Fleet, machine: &str) -> bool {
    EndpointId::from_str(machine)
        .is_ok_and(|id| id == fleet.id() || fleet.mesh().is_some_and(|mesh| mesh.is_member(id)))
}

/// Which way a notice came.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Via {
    Server,
    Machine,
}

/// Coding-agent notices heard lately, so one that comes both from the
/// server and from the machine shows once.
#[derive(Default)]
pub(crate) struct Heard(HashMap<AgentNoticeKey, (Via, Instant)>);

impl Heard {
    /// Whether `key`, heard `via` at `now`, is news. The same notice from
    /// the other way, lately, is not, and is forgotten once matched. From
    /// the same way it is news: another turn of the session ended.
    pub(crate) fn first(&mut self, key: &AgentNoticeKey, via: Via, now: Instant) -> bool {
        self.0
            .retain(|_, (_, at)| now.saturating_duration_since(*at) < TWICE);
        // The server and the mesh may call the machine differently.
        let key = AgentNoticeKey {
            machine_name: String::new(),
            ..key.clone()
        };
        match self.0.get(&key) {
            Some((was, _)) if *was != via => {
                self.0.remove(&key);
                false
            }
            _ => {
                self.0.insert(key, (via, now));
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_mesh::transport_config;
    use beep_node::{agents::AgentKind, link::Offers, machine::provision::Provision};
    use iroh::{Endpoint, SecretKey, endpoint::presets};
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    const LIMIT: Duration = Duration::from_secs(60);

    /// An adapter that asks permission on its first prompt, ends the turn
    /// once it is answered, and does nothing more.
    const ASKING: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line; reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; reply '{"sessionId":"s"}'
read -r line
prompt=$(id)
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t1","title":"Run cargo test"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"},{"optionId":"reject","name":"Reject","kind":"reject_once"}]}}'
read -r line
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
cat > /dev/null
"#;

    fn key(question: Option<u32>, machine_name: &str) -> AgentNoticeKey {
        AgentNoticeKey {
            machine_id: "ab12".into(),
            agent: "claude".into(),
            session: "a3f9c1".into(),
            question,
            machine_name: machine_name.into(),
        }
    }

    #[test]
    fn a_notice_seen_on_both_paths_notifies_once() {
        let mut heard = Heard::default();
        let start = Instant::now();
        let after = |secs| start + Duration::from_secs(secs);
        // The server's and the mesh's names for the machine differ.
        assert!(heard.first(&key(Some(3), "Studio"), Via::Server, start));
        assert!(!heard.first(&key(Some(3), "studio.box"), Via::Machine, after(1)));
        // Matched once; the next turn's finish is news again, and a second
        // one the same way is too: two turns ended.
        assert!(heard.first(&key(None, "Studio"), Via::Machine, after(2)));
        assert!(heard.first(&key(None, "Studio"), Via::Machine, after(3)));
        assert!(!heard.first(&key(None, "Studio"), Via::Server, after(4)));
        // Heard one way only, it stays news; a late twin is a new notice.
        assert!(heard.first(&key(Some(4), "Studio"), Via::Server, after(5)));
        assert!(heard.first(&key(Some(4), "Studio"), Via::Machine, after(5) + TWICE));
    }

    async fn bind() -> Endpoint {
        Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::generate())
            .transport_config(transport_config())
            .bind()
            .await
            .expect("the endpoint binds")
    }

    async fn heard(heard: &mut mpsc::Receiver<Direct>) -> Notice {
        timeout(LIMIT, async {
            loop {
                match heard.recv().await.expect("the stream follows") {
                    Direct::Notice(notice) => return *notice,
                    Direct::Connected => {}
                }
            }
        })
        .await
        .expect("a notice came in time")
    }

    #[tokio::test]
    async fn an_agents_question_reaches_the_desktop_and_its_answer_returns() {
        let dir = TempDir::new().expect("a temp dir");
        let work = TempDir::new().expect("a temp dir");
        let agent = AgentKind {
            name: "pi".into(),
            program: "sh".into(),
            args: vec!["-c".into(), ASKING.into(), "fake-adapter".into()],
            env: BTreeMap::new(),
        };
        let offers = Offers::local(&[agent], Provision::new(dir.path()), false);
        let fleet = Arc::new(Fleet::new(bind().await, dir.path(), "Studio", offers));
        fleet.run(None).await;
        let (following, mut heard_from) = start(fleet.clone());
        let following = tokio::spawn(following);

        // A session starts on this Mac and asks, with no server anywhere.
        let client = fleet.client(fleet.id()).await.expect("this Mac is local");
        let (mut tx, rx) = client.open(&protocol("pi")).await.expect("pi opens");
        tx.send(&Request::Start {
            cwd: Some(work.path().display().to_string()),
            prompt: "go".into(),
            model: None,
            effort: None,
            checkout: None,
        })
        .await
        .expect("the start is sent");
        let asked = heard(&mut heard_from).await;
        assert_eq!(asked.title, "pi on Studio asks");
        assert!(asked.body.contains("Run cargo test"), "{}", asked.body);
        let key = asked.session_id.parse::<AgentNoticeKey>().expect("a key");
        assert_eq!(key.machine_id, fleet.id().to_string());
        assert!(key.question.is_some());

        answer(&fleet, &key, "allow")
            .await
            .expect("the answer is taken");
        let done = heard(&mut heard_from).await;
        assert_eq!(done.title, "pi on Studio finished");
        // Answering it again: the session no longer asks.
        let again = answer(&fleet, &key, "allow").await;
        assert!(
            matches!(again, Err(DesktopError::Api(ApiError::Conflict { .. }))),
            "{again:?}"
        );
        drop((tx, rx, heard_from));
        timeout(LIMIT, following)
            .await
            .expect("following ends once nobody listens")
            .expect("the task ends cleanly");
        fleet.stop().await;
    }
}
