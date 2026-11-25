//! What a team page hears without asking again: the project's record and cards as `team-changed`,
//! and the invites waiting for this desktop as `team-invites`. A page reads once when it opens
//! (`team_record`, `team_cards`, `team_invites`), and that read makes its window follow: the
//! nodes push the cards under `team-cards` for the project, only the machine that acts for
//! the team answers with them, and the invites come from this desktop's node. The record and the
//! contacts are the mesh's, so a change to the mesh sends them again.

use super::team::{TeamCardsView, TeamView, invite_views, team_view};
use crate::fleet::{
    Fleet,
    events::{Events, Interest},
    projects::desk,
};
use beep_fleet::Session;
use beep_proto::node::{
    events::{TEAM_CARDS_TOPIC, TEAM_INVITES_TOPIC, Topic, Value},
    tools::{TeamCards, TeamInviteArgs},
};
use iroh::EndpointId;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, PoisonError},
};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

/// What the nodes said of a team's cards.
pub(crate) enum Heard {
    Cards(TeamCards),
    /// The node does not act for the team.
    Missing,
    /// The node is from before the topic.
    Refused,
}

impl Heard {
    /// What a snapshot of the cards topic says; `None` for one that is not the cards.
    pub(crate) fn of(value: Option<&Value>) -> Option<Self> {
        match value {
            Some(Value::TeamCards(cards)) => Some(Self::Cards(cards.clone())),
            Some(Value::Missing) => Some(Self::Missing),
            Some(_) => None,
            None => Some(Self::Refused),
        }
    }
}

#[derive(Default)]
pub(crate) struct TeamLive(Mutex<Held>);

#[derive(Default)]
struct Held {
    projects: BTreeMap<Uuid, Project>,
    /// The windows that read the invites.
    invites: BTreeSet<String>,
    waiting: Option<Vec<TeamInviteArgs>>,
    sent_invites: Option<serde_json::Value>,
}

struct Project {
    name: String,
    windows: BTreeSet<String>,
    /// The cards the machine that acts for the team gave last, and which machine it is.
    acting: Option<(EndpointId, TeamCards)>,
    /// The nodes from before the topic, which are asked for the cards instead.
    refused: BTreeSet<EndpointId>,
    /// What one of them answered when asked.
    asked: Option<TeamCards>,
    /// What the windows were last sent.
    sent: Option<serde_json::Value>,
}

impl Project {
    fn cards(&self) -> Option<&TeamCards> {
        self.acting
            .as_ref()
            .map(|(_, cards)| cards)
            .or(self.asked.as_ref())
    }
}

impl Held {
    /// Takes what `machine` said of the cards of `name`'s team; the projects it changes.
    fn cards_heard(&mut self, name: &str, machine: EndpointId, heard: Heard) -> Vec<Uuid> {
        let mut changed = Vec::new();

        for (id, project) in &mut self.projects {
            if project.name != name {
                continue;
            }

            match &heard {
                Heard::Cards(cards) => {
                    project.refused.remove(&machine);
                    project.acting = Some((machine, cards.clone()));
                }
                Heard::Missing => {
                    project.refused.remove(&machine);

                    if project
                        .acting
                        .as_ref()
                        .is_some_and(|(acting, _)| *acting == machine)
                    {
                        project.acting = None;
                    }
                }
                Heard::Refused => {
                    project.refused.insert(machine);
                }
            }

            changed.push(*id);
        }

        changed
    }

    /// The topics the nodes are asked for while the windows follow.
    fn interests(&self, me: EndpointId) -> Vec<Interest> {
        let cards = self.projects.values().map(|project| Interest {
            machine: None,
            topic: Topic::keyed(TEAM_CARDS_TOPIC, &project.name),
        });
        let invites = (!self.invites.is_empty()).then(|| Interest {
            machine: Some(me.to_string()),
            topic: Topic::new(TEAM_INVITES_TOPIC),
        });

        cards.chain(invites).collect()
    }

    fn forget(&mut self, window: &str) {
        self.invites.remove(window);

        for project in self.projects.values_mut() {
            project.windows.remove(window);
        }

        self.projects
            .retain(|_, project| !project.windows.is_empty());
    }
}

impl TeamLive {
    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `window` read project `id`'s team: it hears the changes until it closes.
    pub(crate) fn follow(&self, app: &AppHandle, window: &str, id: Uuid, name: &str) {
        self.held()
            .projects
            .entry(id)
            .or_insert_with(|| Project {
                name: name.to_owned(),
                windows: BTreeSet::new(),
                acting: None,
                refused: BTreeSet::new(),
                asked: None,
                sent: None,
            })
            .windows
            .insert(window.to_owned());
        self.hold(app);
    }

    /// `window` read the invites: it hears them change until it closes.
    pub(crate) fn follow_invites(&self, app: &AppHandle, window: &str) {
        self.held().invites.insert(window.to_owned());
        self.hold(app);
    }

    /// `window` closed.
    pub(crate) fn clear(&self, app: &AppHandle, window: &str) {
        self.held().forget(window);
        self.hold(app);
    }

    /// Keeps the nodes pushing what the windows follow, however the windows are shown.
    fn hold(&self, app: &AppHandle) {
        #[cfg(desktop)]
        {
            let fleet = app.state::<Arc<Fleet>>();
            let interests = self.held().interests(fleet.me);
            let events = app.state::<Events>();

            if let Err(error) = events.hold_set("team", interests) {
                log::warn!("a team page cannot follow its team: {error}");
            }

            if !self.held().projects.is_empty() || !self.held().invites.is_empty() {
                events.start(app.clone(), fleet.inner().clone());
            }
        }
        #[cfg(not(desktop))]
        let _ = app;
    }
}

/// A node answered on a project's cards topic.
pub(crate) fn cards_heard(app: &AppHandle, machine: EndpointId, topic: &Topic, heard: Heard) {
    let Some(name) = topic.key() else {
        return;
    };
    let refused = matches!(heard, Heard::Refused);
    let changed = app
        .state::<TeamLive>()
        .held()
        .cards_heard(name, machine, heard);

    if changed.is_empty() {
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        for id in changed {
            if refused {
                ask(&app, id).await;
            }

            emit(&app, id).await;
        }
    });
}

/// The invites waiting on this desktop's node changed.
pub(crate) fn invites_heard(app: &AppHandle, invites: Vec<TeamInviteArgs>) {
    app.state::<TeamLive>().held().waiting = Some(invites);
    let app = app.clone();
    tauri::async_runtime::spawn(async move { emit_invites(&app).await });
}

/// The mesh changed: a team's record or a contact may have.
pub(crate) fn mesh_changed(app: &AppHandle) {
    let ids: Vec<Uuid> = app
        .state::<TeamLive>()
        .held()
        .projects
        .keys()
        .copied()
        .collect();
    let invites = app.state::<TeamLive>().held().waiting.is_some();

    if ids.is_empty() && !invites {
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        for id in ids {
            let old_nodes = app
                .state::<TeamLive>()
                .held()
                .projects
                .get(&id)
                .is_some_and(|project| !project.refused.is_empty());

            if old_nodes {
                ask(&app, id).await;
            }

            emit(&app, id).await;
        }

        emit_invites(&app).await;
    });
}

/// The cards of a team from a node that predates the topic, as its request gives them.
async fn ask(app: &AppHandle, id: Uuid) {
    let Some(name) = app
        .state::<TeamLive>()
        .held()
        .projects
        .get(&id)
        .map(|project| project.name.clone())
    else {
        return;
    };
    let Ok(desk) = desk(&app.state::<Arc<Session>>()).await else {
        return;
    };
    let cards = desk.team_cards(&name).await.ok();

    if let Some(project) = app.state::<TeamLive>().held().projects.get_mut(&id) {
        project.asked = cards;
    }
}

/// Tells the windows of project `id`'s team, unless they were told this already.
async fn emit(app: &AppHandle, id: Uuid) {
    let Ok(desk) = desk(&app.state::<Arc<Session>>()).await else {
        return;
    };
    let Some(name) = app
        .state::<TeamLive>()
        .held()
        .projects
        .get(&id)
        .map(|project| project.name.clone())
    else {
        return;
    };
    let record = match team_view(&desk, &name) {
        Ok(record) => record,
        Err(error) => {
            log::debug!("a team record was not read: {error}");
            return;
        }
    };
    let payload = {
        let mut held = app.state::<TeamLive>().held();
        let Some(project) = held.projects.get_mut(&id) else {
            return;
        };
        let payload = payload(id, record, project.cards().cloned());

        if project.sent.as_ref() == Some(&payload) {
            return;
        }

        project.sent = Some(payload.clone());
        payload
    };

    if let Err(error) = app.emit("team-changed", payload) {
        log::debug!("the team page missed an update: {error}");
    }
}

/// What `team-changed` carries: the record, and the cards when the team's acting machine gave
/// them. A page keeps the cards it holds when they are null.
fn payload(id: Uuid, record: Option<TeamView>, cards: Option<TeamCards>) -> serde_json::Value {
    json!({
        "project_id": id,
        "record": record,
        "cards": cards.map(TeamCardsView::from),
    })
}

async fn emit_invites(app: &AppHandle) {
    let Some(waiting) = app.state::<TeamLive>().held().waiting.clone() else {
        return;
    };
    let Ok(desk) = desk(&app.state::<Arc<Session>>()).await else {
        return;
    };
    let payload = match serde_json::to_value(invite_views(&desk, waiting)) {
        Ok(payload) => payload,
        Err(error) => {
            log::debug!("the invites could not be encoded: {error}");
            return;
        }
    };
    {
        let mut held = app.state::<TeamLive>().held();

        if held.sent_invites.as_ref() == Some(&payload) {
            return;
        }

        held.sent_invites = Some(payload.clone());
    }

    if let Err(error) = app.emit("team-invites", payload) {
        log::debug!("the invites page missed an update: {error}");
    }
}

#[cfg(test)]
#[path = "../tests/projects_team_live.rs"]
mod tests;
