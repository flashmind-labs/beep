//! What coding agents on the machines of the mesh want the user for, sent by the machines
//! themselves: one `FollowAlerts` and one `FollowRuns` stream per machine, kept open while the
//! machine is a member and reachable. The alerts become the same notices the server pushes, and an
//! answer goes back to the machine. No server is involved.

use crate::{
    error::{FleetError, Result},
    peer::Peers,
};
use beep_mesh::{Mesh, MeshError, PeerClient};
use beep_proto::{
    AgentNoticeKey, ApiError, Notice,
    node::agent::{
        AnswerOption, Event, Request, RunNews, SESSIONS_PROTOCOL, SessionsReply, SessionsRequest,
        protocol,
    },
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

/// Notices and connections waiting for the app to take them.
const QUEUE: usize = 64;
/// How long a question that came once waits before the same one counts as news again.
const TWICE: Duration = Duration::from_secs(60);
/// How long a stream stays open before it counts as working.
const HELD: Duration = Duration::from_secs(30);
/// How long a machine gets to answer an answer.
const ANSWERING: Duration = Duration::from_secs(10);
const FIRST_RETRY: Duration = Duration::from_secs(1);
/// The longest wait between tries, for an app the user is looking at.
const LAST_RETRY: Duration = Duration::from_secs(2);

/// What the streams tell the app.
pub enum Heard {
    /// A stream to a machine opened: what the app shows from machines may have changed while it
    /// was closed.
    Connected,
    /// A question or a turn that ended, with the answers the machine offers to the question.
    Notice {
        key: AgentNoticeKey,
        notice: Box<Notice>,
        options: Vec<AnswerOption>,
    },
    /// A CI run the user started ended, told by the machine that watches its project.
    Run(Box<RunNews>),
}

/// Which stream a machine is followed on.
#[derive(Clone, Copy)]
enum Follows {
    /// Coding agents' questions and turns: `FollowAlerts`.
    Alerts,
    /// CI runs: `FollowRuns`, which a machine from before it closes.
    Runs,
}

impl AsRef<str> for Follows {
    fn as_ref(&self) -> &str {
        match self {
            Follows::Alerts => "alerts",
            Follows::Runs => "runs",
        }
    }
}

/// What follows every machine of `mesh`, now and as it changes, and the receiver for what it
/// hears. The future runs until the receiver is dropped or the mesh closes; the caller spawns it.
pub fn follow(
    peers: Arc<dyn Peers>,
    mesh: Mesh,
) -> (impl Future<Output = ()>, mpsc::Receiver<Heard>) {
    let (out, heard) = mpsc::channel(QUEUE);
    (run(peers, mesh, out), heard)
}

async fn run(peers: Arc<dyn Peers>, mesh: Mesh, out: mpsc::Sender<Heard>) {
    let (inner, mut raw) = mpsc::channel(QUEUE);
    let mut seen = Seen::default();
    let watching = each_machine(&mesh, |id| {
        tokio::spawn(machine(peers.clone(), mesh.clone(), id, inner.clone()))
    });
    tokio::pin!(watching);

    loop {
        select! {
            _ = &mut watching => return,
            _ = out.closed() => return,
            Some(heard) = raw.recv() => {
                let fresh = match &heard {
                    Heard::Notice { key, .. } => seen.fresh(key, Instant::now()),
                    Heard::Connected | Heard::Run(_) => true,
                };

                if fresh && out.send(heard).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// One task per machine, ended when the machine leaves the mesh or this is dropped.
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

/// Runs one task per code host of `mesh`, started by `start`, as hosts come and go. Ends when the
/// mesh closes; the tasks end with it.
pub async fn each_machine(mesh: &Mesh, start: impl FnMut(EndpointId) -> JoinHandle<()>) {
    let mut start = start;
    let mut running = Running::default();
    let mut updates = mesh.updates();

    loop {
        updates.borrow_and_update();
        let machines = mesh.code_hosts().into_iter().map(|d| d.device.id).collect();
        running.keep(&machines, &mut start);

        if updates.changed().await.is_err() {
            return;
        }
    }
}

/// Follows `id`'s alerts and runs until the receiver goes.
async fn machine(peers: Arc<dyn Peers>, mesh: Mesh, id: EndpointId, out: mpsc::Sender<Heard>) {
    tokio::join!(
        follow_one(&*peers, &mesh, id, &out, Follows::Alerts),
        follow_one(&*peers, &mesh, id, &out, Follows::Runs),
    );
}

/// What the mesh calls `machine`, or its short id.
fn name_of(mesh: &Mesh, machine: EndpointId) -> String {
    mesh.machines()
        .into_iter()
        .find(|status| status.device.id == machine)
        .map_or_else(
            || machine.fmt_short().to_string(),
            |status| status.device.name,
        )
}

/// Follows one of `id`'s streams, again after each break, until the receiver goes or the machine
/// is not one this side can reach at all.
async fn follow_one(
    peers: &dyn Peers,
    mesh: &Mesh,
    id: EndpointId,
    out: &mpsc::Sender<Heard>,
    what: Follows,
) {
    let mut backoff = Backoff::new();
    // An offline machine fails the same way on every try: logged once, and again when the error
    // changes or after a stream that worked.
    let mut logged = None;

    loop {
        let started = Instant::now();
        let mut replied = false;

        match stream(peers, mesh, id, out, what, &mut replied).await {
            Ok(()) | Err(FleetError::NotFound) => return,
            Err(e) => {
                let error = e.to_string();

                if logged.as_ref() != Some(&error) {
                    log::debug!(
                        "following {} from {}: {error}",
                        what.as_ref(),
                        name_of(mesh, id)
                    );
                    logged = Some(error);
                }
            }
        }

        if held(replied, started.elapsed()) {
            backoff.reset();
            logged = None;
        }

        sleep(backoff.pause()).await;
    }
}

/// The pause between tries: from `FIRST_RETRY`, doubling up to `LAST_RETRY`, each shifted by up to
/// a quarter so machines cut off together do not all come back at once.
struct Backoff(Duration);

impl Backoff {
    fn new() -> Self {
        Self(FIRST_RETRY)
    }

    fn reset(&mut self) {
        self.0 = FIRST_RETRY;
    }

    fn pause(&mut self) -> Duration {
        let wait = self.0.mul_f64(0.75 + fastrand::f64() / 2.0);
        self.0 = (self.0 * 2).min(LAST_RETRY);
        wait
    }
}

/// Whether a stream that broke was a working one, so the next try comes at once: the machine sent
/// something, or kept it open for `HELD`. One that is accepted and closed again keeps backing
/// off.
fn held(replied: bool, lived: Duration) -> bool {
    replied || lived >= HELD
}

fn ended() -> FleetError {
    MeshError::Link(anyhow::anyhow!("the machine ended the stream")).into()
}

/// One stream's life. `Ok` when the receiver is gone; `replied` says whether the machine sent
/// anything.
async fn stream(
    peers: &dyn Peers,
    mesh: &Mesh,
    id: EndpointId,
    out: &mpsc::Sender<Heard>,
    what: Follows,
    replied: &mut bool,
) -> Result<()> {
    let client = peers.client(id).await?;
    let (mut tx, mut rx) = client
        .open(SESSIONS_PROTOCOL)
        .await
        .map_err(FleetError::from)?;
    let request = match what {
        Follows::Alerts => SessionsRequest::FollowAlerts {},
        Follows::Runs => SessionsRequest::FollowRuns {},
    };
    tx.send(&request).await.map_err(MeshError::Link)?;

    if matches!(what, Follows::Alerts) && out.send(Heard::Connected).await.is_err() {
        return Ok(());
    }
    // `tx` stays open: finishing it ends the stream.
    loop {
        let reply = rx.recv::<SessionsReply>().await.map_err(MeshError::Link)?;
        let Some(reply) = reply else {
            return Err(ended());
        };
        *replied = true;
        let heard = match reply {
            SessionsReply::Alert { alert } => {
                let key = alert.key(&id.to_string(), &name_of(mesh, id));
                let notice = Notice {
                    title: alert.notice_title(&key.machine_name),
                    body: alert.notice_body(),
                    session_id: key.to_string(),
                    app: None,
                    approval: None,
                    changed: false,
                    group: false,
                };
                let options = match alert.kind {
                    beep_proto::node::agent::AlertKind::Asks { question } => question.options,
                    _ => Vec::new(),
                };
                Heard::Notice {
                    key,
                    notice: Box::new(notice),
                    options,
                }
            }
            SessionsReply::Run { news } => Heard::Run(Box::new(news)),
            _ => continue,
        };

        if out.send(heard).await.is_err() {
            return Ok(());
        }
    }
}

/// Questions heard lately, so one a machine tells again (a reconnect it did not know the first
/// told) shows once. A turn that ended is news each time.
#[derive(Default)]
struct Seen(HashMap<AgentNoticeKey, Instant>);

impl Seen {
    /// Whether `key`, heard at `now`, is news.
    fn fresh(&mut self, key: &AgentNoticeKey, now: Instant) -> bool {
        if key.question.is_none() {
            return true;
        }

        self.0
            .retain(|_, at| now.saturating_duration_since(*at) < TWICE);
        self.0.insert(key.clone(), now).is_none()
    }
}

/// Answers the question `key` names, on the machine that asked it. Refused as a conflict when the
/// session no longer asks it.
pub async fn answer(peers: &dyn Peers, key: &AgentNoticeKey, reply: &str) -> Result<()> {
    let (Some(question), Ok(machine)) = (key.question, EndpointId::from_str(&key.machine_id))
    else {
        return Err(FleetError::Refused("not a question".into()));
    };
    let client = peers.client(machine).await?;
    timeout(ANSWERING, attached(&client, key, question, reply))
        .await
        .map_err(|_| FleetError::Refused("the machine did not answer in time".into()))?
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
                    return Err(FleetError::Conflict(
                        "The session no longer asks that question.".into(),
                    ));
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
            Some(Event::Ack { error: Some(why) }) => return Err(FleetError::Refused(why)),
            // The session's own news while attached.
            Some(_) => {}
            None => return Err(ended()),
        }
    }
}

#[cfg(test)]
#[path = "tests/alerts.rs"]
mod tests;
