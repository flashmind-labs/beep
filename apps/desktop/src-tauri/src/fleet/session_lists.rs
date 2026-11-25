//! The session lists of project pages and Home, owned by one actor. It keeps
//! each machine's last answer, asks the machines again in the background and
//! sends a list to the windows only once every machine in it has answered and
//! the list changed, so a slow or failing machine never empties a list.

use super::{
    Fleet, SessionParent,
    files::{Place, in_any_folder},
    session_mirror::{Rows, SessionMirror},
};
use crate::{beep::epoch_ms, chat::markdown};
use beep_mesh::machine::listing::{self, MAX_PROJECT_SESSIONS};
use beep_proto::{
    AccountName, AgentQuestion, MachineSession, ModelId, SessionId,
    node::{
        agent::{LineChanges, Persona, SessionsReply, SessionsRequest, WorkPlace},
        events::{Read, Value},
    },
    watched,
};
use iroh::EndpointId;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hasher},
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::timeout,
};
use tokio_util::task::AbortOnDropHandle;

/// The event that carries a list to the windows.
pub(crate) const SESSION_LIST: &str = "session-list";
/// How long another machine's answer stays fresh when its node does not push the list; this
/// Mac is asked on every read.
const FRESH: Duration = Duration::from_secs(15);
/// The least time between two redraws for the mirror's changes.
const REDRAW_GAP: Duration = Duration::from_secs(1);
/// How long one machine gets to answer.
const ANSWER_LIMIT: Duration = Duration::from_secs(6);

/// A list the windows show: one project's sessions, or every project's for Home.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ListId {
    Project(String),
    Home,
}

/// A machine and the project folders asked of it.
pub(crate) type MachineFolders = (EndpointId, Vec<String>);

/// What the windows hear when a list changes.
#[derive(Clone, Serialize)]
struct Update {
    list: ListId,
    sessions: Vec<SessionRow>,
}

/// The handle the commands and session changes use.
pub(crate) struct SessionLists {
    asks: mpsc::UnboundedSender<Ask>,
}

enum Ask {
    Want {
        list: ListId,
        /// The label of the window that shows the list.
        holder: String,
        places: Vec<Place>,
        reply: oneshot::Sender<Option<Vec<SessionRow>>>,
    },
    Answered {
        asked: Asked,
        rows: Option<Vec<MachineSession>>,
    },
    Forget {
        machine: EndpointId,
        session: SessionId,
    },
    Stale {
        machine: EndpointId,
    },
    /// The page of `list` in `holder`'s window went away.
    Drop {
        list: ListId,
        holder: String,
    },
    /// A window closed: it shows no list.
    Closed(String),
    /// A move of `session` to `folder` went through.
    Relocated {
        machine: EndpointId,
        session: SessionId,
        folder: String,
    },
    /// The mirror's rows changed: the lists send again what differs.
    Mirrored,
    /// A machine pushed its history in a list's folders.
    Fed {
        key: MachineFolders,
        rows: Vec<MachineSession>,
    },
    /// The push of a machine's history in a list's folders ended or cannot serve the list: it
    /// is read again, in pages, when asked for.
    Unfed(MachineFolders),
}

impl SessionLists {
    pub(crate) fn start(app: AppHandle, fleet: Arc<Fleet>) -> Self {
        let (asks, queue) = mpsc::unbounded_channel();
        tauri::async_runtime::spawn(serve(app, fleet, asks.clone(), queue));
        Self { asks }
    }

    /// `list` as it stands, with `places` its checkouts now, and asks the machines whose answer
    /// is old. `None` until every machine in it answered once: its update follows.
    pub(crate) async fn want(
        &self,
        list: ListId,
        holder: &str,
        places: Vec<Place>,
    ) -> Option<Vec<SessionRow>> {
        let (reply, answer) = oneshot::channel();
        self.asks
            .send(Ask::Want {
                list,
                holder: holder.to_owned(),
                places,
                reply,
            })
            .ok()?;
        answer.await.ok().flatten()
    }

    /// `session` on `machine` is gone.
    pub(crate) fn forget(&self, machine: EndpointId, session: SessionId) {
        let _ = self.asks.send(Ask::Forget { machine, session });
    }

    /// Sends again the lists whose rows differ now that the mirror's rows changed, until the
    /// mirror goes. A busy agent changes the rows many times a second, so the lists wait
    /// `REDRAW_GAP` between draws.
    pub(crate) fn follow(&self, mut rows: watch::Receiver<Arc<Rows>>) {
        let asks = self.asks.clone();
        tauri::async_runtime::spawn(async move {
            while rows.changed().await.is_ok() {
                if asks.send(Ask::Mirrored).is_err() {
                    return;
                }

                tokio::time::sleep(REDRAW_GAP).await;
            }
        });
    }

    /// `machine`'s sessions changed: ask it again, keeping what it said until it answers.
    pub(crate) fn stale(&self, machine: EndpointId) {
        let _ = self.asks.send(Ask::Stale { machine });
    }

    /// `holder`'s window no longer shows `list`: once no window does, it is not rebuilt or sent
    /// again until asked for.
    pub(crate) fn drop_list(&self, list: ListId, holder: &str) {
        let _ = self.asks.send(Ask::Drop {
            list,
            holder: holder.to_owned(),
        });
    }

    /// The window `holder` closed.
    pub(crate) fn closed(&self, holder: &str) {
        let _ = self.asks.send(Ask::Closed(holder.to_owned()));
    }

    /// `session` on `machine` moved to `folder`: ask again only the answers that hold it or
    /// cover its new folder.
    pub(crate) fn relocated(&self, machine: EndpointId, session: SessionId, folder: String) {
        let _ = self.asks.send(Ask::Relocated {
            machine,
            session,
            folder,
        });
    }
}

async fn serve(
    app: AppHandle,
    fleet: Arc<Fleet>,
    asks: mpsc::UnboundedSender<Ask>,
    mut queue: mpsc::UnboundedReceiver<Ask>,
) {
    let mut lists = Lists::new(fleet.id());
    // The rows the lists were last drawn from, to tell which machines changed.
    let mut seen = Arc::new(Rows::default());
    // One held read of each listed key's history, ended with the key's last list.
    let mut feeds: HashMap<MachineFolders, AbortOnDropHandle<()>> = HashMap::new();

    while let Some(ask) = queue.recv().await {
        let now = Instant::now();
        let mirrored = app
            .try_state::<Arc<SessionMirror>>()
            .map(|mirror| mirror.current())
            .unwrap_or_default();
        let shown = |keys: &[MachineFolders], rows: Vec<MachineSession>| {
            finish(&fleet, &mirrored, keys, rows)
        };
        let mut started = false;
        let moved = match ask {
            Ask::Want {
                list,
                holder,
                places,
                reply,
            } => {
                let (rows, moved) = lists.want(list, &holder, keys(&places), now, shown);
                let _ = reply.send(rows.map(session_rows));
                started = true;
                moved
            }
            Ask::Fed { key, rows } => lists.fed(key, rows, shown),
            Ask::Unfed(key) => lists.unfed(&key),
            Ask::Answered { asked, rows } => lists.answered(asked, rows, now, shown),
            Ask::Forget { machine, session } => lists.forget(machine, &session, shown),
            Ask::Stale { machine } => lists.stale(machine),
            Ask::Drop { list, holder } => {
                lists.drop_list(&list, &holder);
                Moved::default()
            }
            Ask::Closed(holder) => {
                lists.closed(&holder);
                Moved::default()
            }
            Ask::Relocated {
                machine,
                session,
                folder,
            } => lists.relocated(machine, &session, &folder),
            Ask::Mirrored => {
                let changed = mirrored.changed_since(&seen);
                seen = mirrored.clone();
                lists.redraw(&changed, shown)
            }
        };

        let listed = lists.keys();
        feeds.retain(|key, _| listed.contains(key));

        for key in listed {
            if started && feeds.get(&key).is_none_or(|feed| feed.is_finished()) {
                let held = tokio::spawn(feed(fleet.clone(), asks.clone(), key.clone()));
                feeds.insert(key, AbortOnDropHandle::new(held));
            }
        }

        for asked in moved.fetch {
            tauri::async_runtime::spawn(ask_machine(fleet.clone(), asks.clone(), asked));
        }

        for (list, rows) in moved.sent {
            let sessions = session_rows(rows);
            if let Err(e) = app.emit(SESSION_LIST, Update { list, sessions }) {
                log::warn!("could not send a session list: {e}");
            }
        }
    }
}

/// Holds `key`'s history read, which the machine's node pushes as it changes, and sends each
/// value to the lists. Ends, telling them, when the machine does not push it or the history
/// is too long for one read.
async fn feed(fleet: Arc<Fleet>, asks: mpsc::UnboundedSender<Ask>, key: MachineFolders) {
    let (machine, folders) = key.clone();
    let limit = u32::try_from(MAX_PROJECT_SESSIONS).unwrap_or(u32::MAX);
    let read = Read::Sessions(Box::new(SessionsRequest::InFolders { folders, limit }));

    if let Some(Ok(mut current)) = fleet.snapshots_follow(machine, read) {
        let host = fleet.host_of(machine);

        loop {
            match watched::read(&current, Clone::clone) {
                Some(Ok(Value::Sessions(reply))) => match *reply {
                    SessionsReply::External { sessions }
                        if sessions.len() < MAX_PROJECT_SESSIONS =>
                    {
                        let rows = sessions
                            .into_iter()
                            .map(|session| listing::past_session(&host, session))
                            .collect();

                        if asks
                            .send(Ask::Fed {
                                key: key.clone(),
                                rows,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                    _ => break,
                },
                Some(_) => break,
                None => {}
            }

            if current.changed().await.is_err() {
                break;
            }
        }
    }

    let _ = asks.send(Ask::Unfed(key));
}

/// One machine's sessions in its folders; `None` when it failed or did not answer in time.
async fn ask_machine(fleet: Arc<Fleet>, asks: mpsc::UnboundedSender<Ask>, asked: Asked) {
    let (machine, folders) = asked.key.clone();
    let read = async {
        let client = fleet.client(machine).await.ok()?;
        listing::project_history_rows(&client, &fleet.host_of(machine), folders).await
    };
    let rows = timeout(ANSWER_LIMIT, read).await.ok().flatten();
    let _ = asks.send(Ask::Answered { asked, rows });
}

/// The rows as the windows read them.
/// One session as the views list it.
#[derive(Clone, Debug, Serialize)]
pub struct SessionRow {
    pub(crate) machine_id: String,
    pub(crate) machine_name: String,
    pub(crate) id: SessionId,
    pub(crate) agent: String,
    pub(crate) title: Option<String>,
    pub(crate) cwd: String,
    pub(crate) status: &'static str,
    pub(crate) doing: Option<String>,
    /// The first line of its last reply, shown while it is idle, as inline HTML.
    pub(crate) reply_html: Option<String>,
    /// The lines changed in its checkout when its last turn ended.
    pub(crate) changes: Option<LineChanges>,
    /// In milliseconds since the Unix epoch.
    pub(crate) started_at: u64,
    pub(crate) last_activity: u64,
    pub(crate) external: bool,
    pub(crate) question: Option<AgentQuestion>,
    /// The session whose agent started this one.
    pub(crate) parent: Option<SessionParent>,
    /// A temporary reviewer of its parent's changes.
    pub(crate) reviewer: bool,
    /// Started in the background by its parent, so no thread of its chat.
    pub(crate) background: bool,
    /// Its subagents that still run.
    pub(crate) subagents: u32,
    /// The model it runs on, when its node says.
    pub(crate) model: Option<ModelId>,
    /// The home assistant.
    pub(crate) assistant: bool,
    /// The named account it runs as; `None` for the agent's default one.
    pub(crate) account: Option<AccountName>,
    /// The custom agent it runs as.
    pub(crate) persona: Option<Persona>,
    /// Where its node says it works: the checkout and branch.
    pub(crate) place: Option<WorkPlace>,
}

impl SessionRow {
    pub(crate) fn id(&self) -> &SessionId {
        &self.id
    }
}

impl From<MachineSession> for SessionRow {
    fn from(session: MachineSession) -> Self {
        Self {
            status: session.status.into(),
            started_at: epoch_ms(session.started_at),
            last_activity: epoch_ms(session.last_activity),
            machine_id: session.machine_id,
            machine_name: session.machine_name,
            id: session.id,
            agent: session.agent,
            title: session.title,
            cwd: session.cwd,
            doing: session.doing,
            reply_html: session.reply.as_deref().map(markdown::inline_html),
            changes: session.changes,
            external: session.external,
            question: session.question,
            parent: None,
            reviewer: false,
            background: false,
            subagents: session.subagents,
            model: session.model,
            assistant: session.assistant,
            account: session.account,
            persona: session.persona,
            place: session.place,
        }
    }
}

/// Puts what this app knows on `session`: its new folder when moved and the
/// user's title. False when it is being deleted and shows no more.
pub(crate) fn overlay(fleet: &Fleet, session: &mut MachineSession) -> bool {
    let ops = fleet.node_ops();

    if ops.is_deleting_session(&session.machine_id, &session.id) {
        return false;
    }

    if let Some(folder) = ops.relocated_folder(&session.machine_id, &session.id) {
        session.cwd = folder;
    }

    if let Some(mesh) = fleet.mesh() {
        beep_fleet::session_titles::apply(&mesh, session);
    }

    true
}

fn session_rows(rows: Vec<MachineSession>) -> Vec<SessionRow> {
    rows.into_iter().map(SessionRow::from).collect()
}

/// Each machine's folders among `places`, sorted, once.
fn keys(places: &[Place]) -> Vec<MachineFolders> {
    let mut keys: Vec<MachineFolders> = Vec::new();

    for place in places {
        match keys
            .iter_mut()
            .find(|(machine, _)| *machine == place.machine)
        {
            Some((_, folders)) => folders.push(place.folder.clone()),
            None => keys.push((place.machine, vec![place.folder.clone()])),
        }
    }

    for (_, folders) in &mut keys {
        folders.sort();
        folders.dedup();
    }

    keys
}

/// The machines' rows as the windows list them: Beep's own sessions as the mirror holds them,
/// those in the lists' folders it has and the answers lack, without sessions being deleted,
/// in their new folder when moved, with their saved titles, newest first.
fn finish(
    fleet: &Fleet,
    mirrored: &Rows,
    keys: &[MachineFolders],
    rows: Vec<MachineSession>,
) -> Vec<MachineSession> {
    let rows = with_mirrored(rows, mirrored, keys)
        .into_iter()
        .filter_map(|mut session| overlay(fleet, &mut session).then_some(session));

    listing::project_history(rows)
}

/// `rows`, the machines' history, with Beep's own sessions as the mirror holds them: its row
/// for each the history lists, and the ones in the lists' folders the history lacks.
fn with_mirrored(
    rows: Vec<MachineSession>,
    mirrored: &Rows,
    keys: &[MachineFolders],
) -> Vec<MachineSession> {
    let mut rows = listing::merge_owned(rows, mirrored.linked());

    for (machine, folders) in keys {
        let machine_id = machine.to_string();
        let missing: Vec<MachineSession> = mirrored
            .beep()
            .filter(|row| row.machine_id == machine_id && in_any_folder(&row.cwd, folders))
            .filter(|row| {
                !rows.iter().any(|held| {
                    held.machine_id == machine_id && held.agent == row.agent && held.id == row.id
                })
            })
            .cloned()
            .collect();
        rows.extend(missing);
    }

    rows
}

/// The actor's state: each machine's last answer and each list's machines and last sent rows.
pub(crate) struct Lists {
    me: EndpointId,
    answers: HashMap<MachineFolders, Answer>,
    lists: HashMap<ListId, Listed>,
}

#[derive(Default)]
struct Answer {
    /// `None` until the machine answered or failed once.
    rows: Option<Vec<MachineSession>>,
    at: Option<Instant>,
    asking: bool,
    /// Its node pushes the history, so it is not read again when a window asks.
    fed: bool,
    /// Raised when its sessions change here, so an answer asked before is dropped.
    generation: u64,
}

struct Listed {
    keys: Vec<MachineFolders>,
    /// A fingerprint of the rows last sent, so a list is not kept twice.
    sent: Option<u64>,
    /// The windows showing it, by label.
    holders: HashSet<String>,
}

/// A machine asked for its sessions, and the generation of its answer then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Asked {
    pub(crate) key: MachineFolders,
    generation: u64,
}

/// What a change leads to: the lists to send and the machines to ask.
#[derive(Default)]
pub(crate) struct Moved {
    pub(crate) sent: Vec<(ListId, Vec<MachineSession>)>,
    pub(crate) fetch: Vec<Asked>,
}

/// Puts what the app knows on the rows a list merged, given the machines and folders it holds.
type Finish<'a> = &'a dyn Fn(&[MachineFolders], Vec<MachineSession>) -> Vec<MachineSession>;

impl Lists {
    pub(crate) fn new(me: EndpointId) -> Self {
        Self {
            me,
            answers: HashMap::new(),
            lists: HashMap::new(),
        }
    }

    /// `list` shown by `holder`, with `keys` its machines now: its rows once every machine
    /// answered, and the machines whose answer is old to ask. This Mac's is always old.
    pub(crate) fn want(
        &mut self,
        list: ListId,
        holder: &str,
        keys: Vec<MachineFolders>,
        now: Instant,
        finish: impl Fn(&[MachineFolders], Vec<MachineSession>) -> Vec<MachineSession>,
    ) -> (Option<Vec<MachineSession>>, Moved) {
        let mut moved = Moved::default();

        for key in &keys {
            let mine = key.0 == self.me;
            let answer = self.answers.entry(key.clone()).or_default();
            if !answer.fed && answer.at.is_none_or(|at| mine || now - at >= FRESH) {
                moved.fetch.extend(ask(key, answer));
            }
        }

        let rows = self.merged(&keys).map(|rows| finish(&keys, rows));
        let mut holders = self
            .lists
            .remove(&list)
            .map(|listed| listed.holders)
            .unwrap_or_default();
        holders.insert(holder.to_owned());
        self.lists.insert(
            list,
            Listed {
                sent: rows.as_deref().map(fingerprint),
                keys,
                holders,
            },
        );

        (rows, moved)
    }

    /// A machine answered, or failed with `None`, which keeps what it said before. An answer
    /// asked before its sessions changed here is dropped and the machine asked again.
    pub(crate) fn answered(
        &mut self,
        asked: Asked,
        rows: Option<Vec<MachineSession>>,
        now: Instant,
        finish: impl Fn(&[MachineFolders], Vec<MachineSession>) -> Vec<MachineSession>,
    ) -> Moved {
        if !self.listed(&asked.key) {
            // Its lists went away while it was asked.
            self.answers.remove(&asked.key);
            return Moved::default();
        }

        let answer = self.answers.entry(asked.key.clone()).or_default();
        answer.asking = false;

        // A pushed history is newer than the read that was out.
        if answer.fed && asked.generation != answer.generation {
            return Moved::default();
        }

        if asked.generation != answer.generation {
            return Moved {
                sent: Vec::new(),
                fetch: ask(&asked.key, answer).into_iter().collect(),
            };
        }

        answer.at = Some(now);
        match rows {
            Some(rows) => answer.rows = Some(rows),
            None => {
                answer.rows.get_or_insert_with(Vec::new);
            }
        }

        Moved {
            sent: self.changed(|keys| keys.contains(&asked.key), &finish),
            fetch: Vec::new(),
        }
    }

    /// The keys some list holds.
    pub(crate) fn keys(&self) -> HashSet<MachineFolders> {
        self.lists
            .values()
            .flat_map(|listed| listed.keys.iter().cloned())
            .collect()
    }

    /// The machine pushed its history for `key`: it is the answer, and a read that was out
    /// is dropped.
    pub(crate) fn fed(
        &mut self,
        key: MachineFolders,
        rows: Vec<MachineSession>,
        finish: impl Fn(&[MachineFolders], Vec<MachineSession>) -> Vec<MachineSession>,
    ) -> Moved {
        if !self.listed(&key) {
            return Moved::default();
        }

        let answer = self.answers.entry(key.clone()).or_default();
        answer.fed = true;
        answer.generation += 1;
        answer.at = Some(Instant::now());
        answer.rows = Some(rows);

        Moved {
            sent: self.changed(|keys| keys.contains(&key), &finish),
            fetch: Vec::new(),
        }
    }

    /// The push for `key` ended: the next ask reads it, and it is read now.
    pub(crate) fn unfed(&mut self, key: &MachineFolders) -> Moved {
        let listed = self.listed(key);
        let Some(answer) = self.answers.get_mut(key) else {
            return Moved::default();
        };

        if !answer.fed {
            return Moved::default();
        }

        answer.fed = false;
        answer.at = None;

        Moved {
            sent: Vec::new(),
            fetch: ask(key, answer).filter(|_| listed).into_iter().collect(),
        }
    }

    /// `session` on `machine` is gone from every list.
    pub(crate) fn forget(
        &mut self,
        machine: EndpointId,
        session: &SessionId,
        finish: impl Fn(&[MachineFolders], Vec<MachineSession>) -> Vec<MachineSession>,
    ) -> Moved {
        for (_, answer) in self.answers.iter_mut().filter(|(key, _)| key.0 == machine) {
            answer.generation += 1;
            if let Some(rows) = &mut answer.rows {
                rows.retain(|row| row.id != *session);
            }
        }

        Moved {
            sent: self.changed(|keys| keys.iter().any(|key| key.0 == machine), &finish),
            fetch: Vec::new(),
        }
    }

    /// The lists of `machines` whose rows differ from what was sent, now sent.
    pub(crate) fn redraw(
        &mut self,
        machines: &HashSet<EndpointId>,
        finish: impl Fn(&[MachineFolders], Vec<MachineSession>) -> Vec<MachineSession>,
    ) -> Moved {
        Moved {
            sent: self.changed(
                |keys| keys.iter().any(|key| machines.contains(&key.0)),
                &finish,
            ),
            fetch: Vec::new(),
        }
    }

    /// Asks `machine` again for its lists; what it said stays until it answers.
    pub(crate) fn stale(&mut self, machine: EndpointId) -> Moved {
        self.stale_where(machine, |_| true)
    }

    /// Asks `machine` again for the lists whose answer holds `session`, which has the old
    /// folder, or covers `folder`, which holds it now. The others cannot have changed.
    pub(crate) fn relocated(
        &mut self,
        machine: EndpointId,
        session: &SessionId,
        folder: &str,
    ) -> Moved {
        self.stale_where(machine, |(key, answer)| {
            in_any_folder(folder, &key.1)
                || answer
                    .rows
                    .as_ref()
                    .is_some_and(|rows| rows.iter().any(|row| row.id == *session))
        })
    }

    fn stale_where(
        &mut self,
        machine: EndpointId,
        stale: impl Fn((&MachineFolders, &Answer)) -> bool,
    ) -> Moved {
        let listed: Vec<MachineFolders> = self
            .lists
            .values()
            .flat_map(|listed| &listed.keys)
            .filter(|key| key.0 == machine)
            .cloned()
            .collect();
        let mut moved = Moved::default();

        for (key, answer) in self.answers.iter_mut().filter(|(key, _)| key.0 == machine) {
            if !stale((key, answer)) {
                continue;
            }

            answer.generation += 1;
            answer.at = None;
            if listed.contains(key) {
                moved.fetch.extend(ask(key, answer));
            }
        }

        moved
    }

    /// `holder` no longer shows `list`: with no window left on it, the list goes.
    pub(crate) fn drop_list(&mut self, list: &ListId, holder: &str) {
        if let Some(listed) = self.lists.get_mut(list) {
            listed.holders.remove(holder);
        }

        self.drop_unheld();
    }

    /// The window `holder` closed shows no list.
    pub(crate) fn closed(&mut self, holder: &str) {
        for listed in self.lists.values_mut() {
            listed.holders.remove(holder);
        }

        self.drop_unheld();
    }

    /// Drops the project lists no window shows, and the answers no list holds. Home stays.
    fn drop_unheld(&mut self) {
        self.lists
            .retain(|id, listed| *id == ListId::Home || !listed.holders.is_empty());
        self.keep_answers();
    }

    /// Drops the answers no list holds.
    fn keep_answers(&mut self) {
        self.answers
            .retain(|key, _| self.lists.values().any(|listed| listed.keys.contains(key)));
    }

    fn listed(&self, key: &MachineFolders) -> bool {
        self.lists.values().any(|listed| listed.keys.contains(key))
    }

    /// The lists `touched` names whose rows differ from what was sent, now sent.
    fn changed(
        &mut self,
        touched: impl Fn(&[MachineFolders]) -> bool,
        finish: Finish<'_>,
    ) -> Vec<(ListId, Vec<MachineSession>)> {
        let ids: Vec<ListId> = self
            .lists
            .iter()
            .filter(|(_, listed)| touched(&listed.keys))
            .map(|(id, _)| id.clone())
            .collect();
        let mut sent = Vec::new();

        for id in ids {
            let Some(listed) = self.lists.get(&id) else {
                continue;
            };
            let Some(rows) = self
                .merged(&listed.keys)
                .map(|rows| finish(&listed.keys, rows))
            else {
                continue;
            };
            let Some(listed) = self.lists.get_mut(&id) else {
                continue;
            };
            let print = fingerprint(&rows);
            if listed.sent != Some(print) {
                listed.sent = Some(print);
                sent.push((id, rows));
            }
        }

        sent
    }

    /// Every machine's rows among `keys`, once each has answered.
    fn merged(&self, keys: &[MachineFolders]) -> Option<Vec<MachineSession>> {
        let mut rows = Vec::new();

        for key in keys {
            rows.extend(self.answers.get(key)?.rows.clone()?);
        }

        Some(rows)
    }
}

/// A hash of the rows as they serialize, which is all a window sees of them.
fn fingerprint(rows: &[MachineSession]) -> u64 {
    struct Hashing(DefaultHasher);

    impl std::io::Write for Hashing {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.write(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut hashing = Hashing(DefaultHasher::new());
    // A row that fails to serialize leaves the print as it was: the rows would still differ
    // elsewhere, and a missed redraw heals at the next one.
    let _ = serde_json::to_writer(&mut hashing, rows);

    hashing.0.finish()
}

/// `key` to ask, unless it is being asked.
fn ask(key: &MachineFolders, answer: &mut Answer) -> Option<Asked> {
    if answer.asking {
        return None;
    }

    answer.asking = true;
    Some(Asked {
        key: key.clone(),
        generation: answer.generation,
    })
}

#[cfg(test)]
#[path = "../tests/fleet_session_lists.rs"]
mod tests;
