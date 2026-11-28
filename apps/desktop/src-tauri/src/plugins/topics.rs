//! Follows the mesh topics plugins want and hands each update to the host, so the page
//! does not carry them back.

use crate::store::lock;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use beep_plugins::{Host, Publication, Topic};
use beep_proto::node::events::Topic as NodeTopic;
use beep_proto::watched;
use tauri::{AppHandle, Manager};
use tokio::sync::{Notify, watch};
use uuid::Uuid;

use crate::fleet::{
    Fleet,
    events::Events,
    outbox::{Sink, WindowOutbox},
    view_signal::{Interest, Update},
};

/// The label this reader has in the outbox. No window has it, so no visibility pauses it.
const LABEL: &str = "plugins";

/// The topics plugins follow, as the host last reported them.
pub(super) struct Wanted(pub(super) watch::Receiver<Vec<Topic>>);

/// Follows the wanted mesh topics until the host is gone.
pub(crate) async fn follow(app: AppHandle) {
    let wanted = app.state::<Wanted>().0.clone();
    let events = app.state::<WindowOutbox>();
    let fleet = app.state::<Arc<Fleet>>().inner().clone();

    let heard = Arc::new(Heard::default());
    let mut reader = Reader::new(heard.clone().sink());
    let mut topics = wanted;

    loop {
        let interests = watched::read_and_update(&mut topics, |wanted| interests(wanted));

        if reader.follow(&events, interests) {
            app.state::<Events>().start(app.clone(), fleet.clone());
        }

        tokio::select! {
            changed = topics.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            () = heard.wake.notified() => {
                watched::read(&topics, |wanted| heard.take(wanted, |publication| {
                    if let Some(host) = app.try_state::<Host>()
                        && let Err(error) = host.publish(&publication)
                    {
                        log::debug!("a plugin update was refused: {error}");
                    }
                }));
            }
        }
    }

    reader.end(&events);
}

/// An update's machine, topic and query.
type UpdateKey = (String, NodeTopic, Option<u64>);

/// What the outbox sent and the host has not yet been given: the newest update of each
/// machine, topic and query, so a burst reaches plugins at the pace they are served.
#[derive(Default)]
struct Heard {
    latest: Mutex<BTreeMap<UpdateKey, Update>>,
    wake: Notify,
}

impl Heard {
    fn sink(self: Arc<Self>) -> Sink {
        Arc::new(move |update: Update| {
            // A query's first answer is one window's filtered view, not the topic's state.
            if update.initial && update.query.is_some() {
                return true;
            }

            let key = (
                update.machine_id.clone(),
                update.topic.clone(),
                update.query,
            );
            let mut latest = lock(&self.latest);

            // Concurrent dispatches can arrive out of order; the higher seq is the newer.
            if latest.get(&key).is_none_or(|held| held.seq <= update.seq) {
                latest.insert(key, update);
            }
            drop(latest);
            self.wake.notify_one();

            true
        })
    }

    /// Hands `deliver` one publication for each wanted topic each held update answers.
    fn take(&self, wanted: &[Topic], deliver: impl Fn(Publication)) {
        let held = std::mem::take(&mut *lock(&self.latest));

        for update in held.values() {
            publications(wanted, update).into_iter().for_each(&deliver);
        }
    }
}

/// The registration in the outbox, with its id: held while any mesh topic is wanted.
struct Reader {
    send: Sink,
    held: Option<(Uuid, Vec<Interest>)>,
}

impl Reader {
    fn new(send: Sink) -> Self {
        Self { send, held: None }
    }

    /// Makes `interests` what the reader follows. Returns whether the node streams need
    /// starting.
    fn follow(&mut self, events: &WindowOutbox, interests: Vec<Interest>) -> bool {
        if self.held.as_ref().map_or(&[][..], |(_, held)| held) == interests {
            return false;
        }

        if interests.is_empty() {
            self.end(events);
            return false;
        }

        let result = match &self.held {
            Some((id, _)) => events
                .retopic(LABEL, *id, interests.clone())
                .map(|()| Some(*id)),
            // A fresh id: the outbox remembers the ended one as removed.
            None => {
                let id = Uuid::new_v4();
                events
                    .register(
                        LABEL,
                        id,
                        interests.clone(),
                        self.send.clone(),
                        || true,
                        None,
                    )
                    .map(|registered| registered.map(|_| id))
            }
        };

        match result {
            Ok(Some(id)) => {
                self.held = Some((id, interests));
                true
            }
            Ok(None) => false,
            Err(error) => {
                log::warn!("plugins cannot follow the mesh topics they want: {error}");
                false
            }
        }
    }

    fn end(&mut self, events: &WindowOutbox) {
        if let Some((id, _)) = self.held.take() {
            events.remove(LABEL, id);
        }
    }
}

/// What the outbox is asked to follow for `wanted`: its mesh topics, on their machines.
fn interests(wanted: &[Topic]) -> Vec<Interest> {
    let mut interests: Vec<_> = wanted
        .iter()
        .filter(|topic| topic.on_mesh())
        .map(|topic| Interest {
            machine: topic.machine.clone(),
            topic: node_topic(topic),
        })
        .collect();
    interests.sort();
    interests.dedup();

    interests
}

fn node_topic(topic: &Topic) -> NodeTopic {
    match &topic.key {
        Some(key) => NodeTopic::keyed(&topic.name, key),
        None => NodeTopic::new(&topic.name),
    }
}

fn publications(wanted: &[Topic], update: &Update) -> Vec<Publication> {
    wanted
        .iter()
        .filter(|topic| topic.on_mesh())
        .filter(|topic| {
            topic
                .machine
                .as_ref()
                .is_none_or(|machine| machine == &update.machine_id)
                && node_topic(topic).matches(&update.topic)
        })
        .map(|topic| Publication {
            topic: topic.clone(),
            machine: Some(update.machine_id.clone()),
            data: update.data.clone().unwrap_or(serde_json::Value::Null),
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/plugins_topics.rs"]
mod tests;
