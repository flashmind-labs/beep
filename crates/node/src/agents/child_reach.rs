//! How a caller reaches its children (`Reach`), and the subagent card its
//! transcript shows for each `start_agent` call (`show_children`).

use crate::calls::command_name;
use super::{Agents, Session, child_view::ChildView, family::{RemoteKid, Who}, live::Mark, machine_list::{Machine, find_named, machine_name}};
use beep_mesh::Mesh;
use beep_proto::SessionId;
use beep_proto::node::agent::{Entry, EntryKind};
use beep_proto::watched;
use std::sync::Arc;

/// The call that started a child, as its parent's transcript shows it:
/// `Beep: Start <harness> agent ...`, then the JSON `start_agent` returned.
const STARTED: &str = "Beep: Start ";

/// Shows each `start_agent` call among `entries` as the subagent card the
/// apps draw: its title, harness and machine, how it stands now and what it
/// last said. The child's own log is its own session, which the card links
/// to with a `[child]` line. A call whose result cannot be read stays a
/// plain call. A child on another machine shows as last read: the read
/// never waits for the machine. A child that is gone shows as ended.
pub(crate) fn show_children(agents: &Agents, session: &Session, entries: &mut [Entry]) {
    let calls: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.kind == EntryKind::Tool && entry.text.starts_with(STARTED))
        .map(|(at, _)| at)
        .collect();

    if calls.is_empty() {
        return;
    }

    let Some((_, who)) = agents.caller(&session.id) else {
        return;
    };
    let reach = Reach {
        agents,
        mesh: agents.mesh(),
        who: &who,
    };

    for at in calls {
        let text = entries[at].text.clone();
        let Some(first) = started_view(&text) else {
            continue;
        };
        entries[at].text = child_entry(&text, &reach.known_view(&first));
    }
}

/// The caller, and the ways it has to its children.
pub(crate) struct Reach<'a> {
    pub(crate) agents: &'a Agents,
    pub(crate) mesh: Option<Mesh>,
    pub(crate) who: &'a Who,
}

/// A child's id split into its session and the machine it runs on.
pub(crate) struct ChildId<'a> {
    pub(crate) session: SessionId,
    pub(crate) machine: Option<&'a str>,
}

impl<'a> From<&'a str> for ChildId<'a> {
    fn from(text: &'a str) -> Self {
        match text.trim().split_once('@') {
            Some((session, machine)) => Self {
                session: Into::<SessionId>::into(session),
                machine: Some(machine),
            },
            None => Self {
                session: Into::<SessionId>::into(text.trim()),
                machine: None,
            },
        }
    }
}

pub(crate) enum Found {
    Here(Arc<Session>),
    There(RemoteKid),
}

impl Reach<'_> {
    pub(crate) fn my_name(&self) -> String {
        machine_name(self.mesh.as_ref())
    }
    pub(crate) fn mesh(&self) -> Result<&Mesh, String> {
        self.mesh
            .as_ref()
            .ok_or_else(|| "This machine is in no mesh.".to_owned())
    }
    /// `child` as it stands on this machine, which the caller has now seen.
    pub(crate) fn view_here(&self, child: &Session) -> ChildView {
        self.shown([child]);
        ChildView::of(&child.info(), &Agents::name_of(child)).named(&self.my_name())
    }
    /// Notes that the caller was shown `children` as they stand, so its next
    /// turn is not told of them again.
    pub(crate) fn shown<'a>(&self, children: impl IntoIterator<Item = &'a Session>) {
        if let Some(caller) = self.agents.get(&self.who.session) {
            caller.see_soon(
                children
                    .into_iter()
                    .map(|child| {
                        (
                            child.id.clone(),
                            watched::read(&child.live, |live| Into::<Mark>::into(live)),
                        )
                    })
                    .collect(),
            );
        }
    }
    /// The machine `name` names: `None` is this one.
    pub(crate) fn target(&self, name: Option<&str>) -> Result<Option<Machine>, String> {
        let Some(name) = name.map(command_name).filter(|name| !name.is_empty()) else {
            return Ok(None);
        };
        let mesh = self.mesh.as_ref().ok_or_else(|| {
            format!("This machine is in no mesh, so it reaches no machine called {name}.")
        })?;
        let machine = find_named(mesh, &name)?;

        Ok((machine.id != mesh.me()).then_some(machine))
    }
    /// Where a child named by `id` is, and the child itself on this machine.
    /// The same refusal whether it is not there or not the caller's.
    pub(crate) fn find(&self, id: &str) -> Result<Found, String> {
        let id = ChildId::from(id);
        let none = || format!("You have no agent {}.", id.session);

        match self.target(id.machine).map_err(|_| none())? {
            None => Ok(Found::Here(self.agents.child_of(self.who, &id.session)?)),
            Some(machine) => self
                .agents
                .remote_kids(&self.who.session)
                .into_iter()
                .find(|kid| kid.machine == machine.id && kid.id == id.session)
                .map(Found::There)
                .ok_or_else(none),
        }
    }
    /// How the child `started` names stands, without waiting on its machine.
    /// One elsewhere that was never read shows as it started; one this parent
    /// no longer has has ended.
    pub(crate) fn known_view(&self, started: &ChildView) -> ChildView {
        let Ok(found) = self.find(&started.id) else {
            return ChildView {
                state: "ended".to_owned(),
                ..started.clone()
            };
        };

        match found {
            Found::Here(child) => self.view_here(&child),
            Found::There(kid) => self
                .mesh
                .as_ref()
                .and_then(|mesh| self.agents.family.views.of(mesh, &self.who.session, &kid))
                .map_or_else(|| started.clone(), |view| view.named(&kid.name)),
        }
    }}

/// The child a `start_agent` entry's text names, from the JSON line it holds.
fn started_view(text: &str) -> Option<ChildView> {
    text.lines()
        .find(|line| line.starts_with('{'))
        .and_then(|line| serde_json::from_str(line).ok())
}

/// A child as its parent's transcript entry writes it: the lines the apps
/// read as a subagent, and `[child]` with the id to open and its state.
fn child_entry(call: &str, view: &ChildView) -> String {
    let title = view.title.clone().unwrap_or_else(|| {
        let line = call.lines().next().unwrap_or_default();
        let line = line
            .strip_suffix(']')
            .and_then(|l| l.rsplit_once(" ["))
            .map_or(line, |(l, _)| l);
        let line = line.strip_prefix(STARTED).unwrap_or(line);
        match line.split_once(": ") {
            Some((_, prompt)) => prompt.to_owned(),
            None if line == "an agent" => format!("{} agent", view.harness),
            None if line == "thread" => "Thread".to_owned(),
            None => line.to_owned(),
        }
    });
    let status = match view.state.as_str() {
        "working" => "in_progress",
        "ended" if view.error.is_some() => "failed",
        _ => "completed",
    };
    // Keep the original prompt below the display title. The call's final
    // line is its JSON result, so prompt lines may themselves contain JSON.
    let prompt = call
        .rsplit_once("\n{")
        .and_then(|(head, _)| head.rsplit_once(" ["))
        .and_then(|(title, _)| title.strip_prefix(STARTED))
        .and_then(|start| start.split_once(": "))
        .map(|(_, prompt)| prompt);
    let title = title.lines().next().unwrap_or_default();
    let title = prompt.map_or_else(|| title.to_owned(), |prompt| format!("{title}\n{prompt}"));
    let mut text = format!(
        "{title} [{status}]\n[steps] {}\n[agent] {} on {}\n[child] {} {} {}",
        view.turns, view.harness, view.machine, view.id, view.state, view.key
    );

    // What the child spent, for the card only: it is the child's session's
    // own spend, which the parent's numbers never include.
    if view.tokens > 0 || view.cost_usd.is_some() {
        let cost = view
            .cost_usd
            .map_or_else(|| "-".to_owned(), |usd| format!("{usd:.4}"));
        text.push_str(&format!("\n[spend] {} {cost}", view.tokens));
    }

    if let Some(question) = &view.question {
        text.push_str(&format!("\nWaiting for you: {question}"));
    } else if !view.last_message.is_empty() {
        text.push('\n');
        text.push_str(&view.last_message);
    }

    text
}

#[cfg(test)]
#[path = "../tests/agents_child_reach.rs"]
mod tests;
