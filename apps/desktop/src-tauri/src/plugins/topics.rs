//! Follows the mesh topics plugins want and hands each update to the host, so the page
//! does not carry them back.

use std::sync::Arc;

use beep_plugins::{Host, Publication, Topic};
use beep_proto::node::events::Topic as NodeTopic;
use tauri::{AppHandle, Manager};
use tokio::sync::watch;
use uuid::Uuid;

use crate::fleet::{
    Fleet,
    events::Events,
    outbox::{Sink, WindowOutbox},
    view_signal::{Interest, Update},
};

/// The label this reader has in the outbox. No window has it, so no visibility pauses it.
const LABEL: &str = "plugins";

/// The mesh topics plugins may follow. The others are `ui.*`, which the page publishes,
/// and `session.turn`, which it builds.
const MESH_NAMES: [&str; 2] = ["agents", "agent"];

/// The topics plugins follow, as the host last reported them.
pub(super) struct Wanted(pub(super) watch::Receiver<Vec<Topic>>);

/// Follows the wanted mesh topics until the host is gone.
pub(crate) async fn follow(app: AppHandle) {
    let wanted = app.state::<Wanted>().0.clone();
    let events = app.state::<WindowOutbox>();
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    events.start(fleet.id());

    let deliver = {
        let app = app.clone();
        move |publication: Publication| {
            if let Some(host) = app.try_state::<Host>()
                && let Err(error) = host.publish(&publication)
            {
                log::debug!("a plugin update was refused: {error}");
            }
        }
    };
    let mut reader = Reader::new(sink(wanted.clone(), deliver));
    let mut topics = wanted;

    loop {
        let interests = interests(&topics.borrow_and_update());

        if reader.follow(&events, interests) {
            app.state::<Events>().start(app.clone(), fleet.clone());
        }

        if topics.changed().await.is_err() {
            break;
        }
    }

    reader.end(&events);
}

/// The registration in the outbox: held while any mesh topic is wanted.
struct Reader {
    id: Uuid,
    send: Sink,
    held: Option<Vec<Interest>>,
}

impl Reader {
    fn new(send: Sink) -> Self {
        Self {
            id: Uuid::new_v4(),
            send,
            held: None,
        }
    }

    /// Makes `interests` what the reader follows. Returns whether the node streams need
    /// starting.
    fn follow(&mut self, events: &WindowOutbox, interests: Vec<Interest>) -> bool {
        if self.held.as_ref().unwrap_or(&Vec::new()) == &interests {
            return false;
        }

        if interests.is_empty() {
            self.end(events);
            return false;
        }

        let result = match self.held {
            Some(_) => events
                .retopic(LABEL, self.id, interests.clone())
                .map(|()| true),
            None => events
                .register(
                    LABEL,
                    self.id,
                    interests.clone(),
                    self.send.clone(),
                    || true,
                    None,
                )
                .map(|registered| registered.is_some()),
        };

        match result {
            Ok(started) => {
                self.held = Some(interests);
                started
            }
            Err(error) => {
                log::warn!("plugins cannot follow the mesh topics they want: {error}");
                false
            }
        }
    }

    fn end(&mut self, events: &WindowOutbox) {
        if self.held.take().is_some() {
            events.remove(LABEL, self.id);
        }
    }
}

/// What the outbox is asked to follow for `wanted`: its mesh topics, on their machines.
fn interests(wanted: &[Topic]) -> Vec<Interest> {
    let mut interests: Vec<_> = wanted
        .iter()
        .filter(|topic| MESH_NAMES.contains(&topic.name.as_str()))
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

/// Hands `deliver` one publication for each wanted topic an update answers, as the page did
/// for each topic it followed.
fn sink(
    wanted: watch::Receiver<Vec<Topic>>,
    deliver: impl Fn(Publication) + Send + Sync + 'static,
) -> Sink {
    Arc::new(move |update: Update| {
        for publication in publications(&wanted.borrow(), &update) {
            deliver(publication);
        }

        true
    })
}

fn publications(wanted: &[Topic], update: &Update) -> Vec<Publication> {
    wanted
        .iter()
        .filter(|topic| MESH_NAMES.contains(&topic.name.as_str()))
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
