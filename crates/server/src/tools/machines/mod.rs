//! Tools that use the user's machines; each takes `in`, else picks one the way `Need` says.

use crate::{
    account::user::User,
    app::App,
    nodes::{DESKTOP, FILES, Online, SHELL, agent_name},
    protocol::node::agent,
    store::NodeRow,
};
use flashmind::types::{ToolRegistry, tool::ToolResult};
use futures::future::join_all;
use std::sync::Arc;
use uuid::Uuid;

pub(crate) mod add;
pub(crate) mod agents;
mod code_agent;
pub(crate) mod copy;
mod desktop;
pub(crate) mod exec;
pub(crate) mod mcp;
pub(crate) mod process;
pub(crate) mod transfer;

/// Registers each tool some machine of the user's online now can serve.
pub(crate) fn register(registry: &mut ToolRegistry, app: &Arc<App>, user: &Arc<User>, chat: Uuid) {
    let online = app.nodes.of_user(user.id());
    let machines = Machines {
        app: app.clone(),
        user: user.clone(),
    };
    if online.iter().any(|node| node.offers(SHELL)) {
        registry.register(Arc::new(exec::ExecTool {
            machines: machines.clone(),
        }));
        registry.register(Arc::new(process::ProcessTool {
            machines: machines.clone(),
        }));
    }
    // A Mac offers desktop/1 only while the user lets Beep use it.
    if online.iter().any(|node| node.offers(DESKTOP)) {
        registry.register(Arc::new(desktop::DesktopTool {
            machines: machines.clone(),
        }));
    }
    if online.iter().any(offers_an_agent) {
        registry.register(Arc::new(code_agent::CodeAgentTool { machines, chat }));
    }
}

fn offers_an_agent(online: &Online) -> bool {
    online
        .offers
        .iter()
        .any(|offer| agent::agent_of(offer).is_some())
}

/// The user's machines, looked up on each call.
#[derive(Clone)]
pub(crate) struct Machines {
    pub(crate) app: Arc<App>,
    pub(crate) user: Arc<User>,
}

/// A machine a call runs on, online now.
pub(crate) struct Picked {
    /// Its name as the machines list shows it.
    pub(crate) name: String,
    /// Its key, as the store keeps it.
    pub(crate) endpoint: String,
    pub(crate) online: Online,
    /// Its home folder, as it last reported it.
    pub(crate) home: Option<String>,
}

impl Picked {
    /// Where commands start: the environment's workspace, else the home folder (`None`).
    pub(crate) fn cwd(&self) -> Option<&str> {
        self.online.workspace.as_deref()
    }

    /// The name as one word, as `in` takes it.
    pub(crate) fn shown(&self) -> String {
        command_name(&self.name)
    }
}

/// What a call needs of its machine, which also decides the default.
#[derive(Clone, Copy)]
pub(crate) enum Need<'a> {
    /// A shell. The default is the user's environment.
    Shell,
    /// Files it shares, with the same default as `Shell`.
    Files,
    /// Desktop control; defaults to the only machine that has it.
    Desktop,
    /// A coding agent, `None` for any; defaults to the environment, else the only machine with it.
    Agent(Option<&'a str>),
}

impl Need<'_> {
    fn met(self, online: &Online) -> bool {
        match self {
            Self::Shell => online.offers(SHELL),
            Self::Files => online.offers(FILES),
            Self::Desktop => online.offers(DESKTOP),
            Self::Agent(Some(name)) => online.offers(&agent::protocol(name)),
            Self::Agent(None) => offers_an_agent(online),
        }
    }

    fn what(self) -> String {
        match self {
            Self::Shell => "a shell".to_owned(),
            Self::Files => "shared files".to_owned(),
            Self::Desktop => "desktop control".to_owned(),
            Self::Agent(Some(name)) => agent_name(name).to_owned(),
            Self::Agent(None) => "a coding agent".to_owned(),
        }
    }

    fn rule(self) -> Rule {
        match self {
            Self::Shell | Self::Files => Rule::Workspace,
            Self::Desktop => Rule::OnlyOne,
            Self::Agent(_) => Rule::WorkspaceThenOnlyOne,
        }
    }
}

/// How a call without `in` picks its machine.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Rule {
    Workspace,
    WorkspaceThenOnlyOne,
    OnlyOne,
}

/// What `choose` knows of a machine.
#[derive(Debug)]
struct Candidate<'a> {
    name: &'a str,
    online: bool,
    /// Online and offers what the call needs.
    meets: bool,
    /// The user's environment.
    workspace: bool,
}

/// The index in `candidates` of the machine `name` names, else `rule`'s default.
fn choose(
    candidates: &[Candidate<'_>],
    name: Option<&str>,
    rule: Rule,
    what: &str,
) -> Result<usize, String> {
    let names = || {
        let names: Vec<_> = candidates.iter().map(|c| command_name(c.name)).collect();
        if names.is_empty() {
            "The user has no machines running beep-node.".to_owned()
        } else {
            format!("Machines: {}.", names.join(", "))
        }
    };
    if let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) {
        let wanted = command_name(name);
        let Some(at) = candidates
            .iter()
            .position(|c| command_name(c.name) == wanted)
        else {
            return Err(format!("No machine called {name}. {}", names()));
        };
        let found = &candidates[at];
        if !found.online {
            return Err(format!("{} is offline.", found.name));
        }
        if !found.meets {
            return Err(format!("{} does not offer {what}.", found.name));
        }
        return Ok(at);
    }
    let meeting: Vec<usize> = (0..candidates.len())
        .filter(|&at| candidates[at].meets)
        .collect();
    let workspace = meeting.iter().copied().find(|&at| candidates[at].workspace);
    let choices = || {
        meeting
            .iter()
            .map(|&at| command_name(candidates[at].name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (rule, workspace, &meeting[..]) {
        (Rule::Workspace | Rule::WorkspaceThenOnlyOne, Some(at), _) => Ok(at),
        (Rule::WorkspaceThenOnlyOne | Rule::OnlyOne, _, [only]) => Ok(*only),
        (_, _, []) => Err(format!(
            "None of the user's machines online offers {what}. {}",
            names()
        )),
        (Rule::Workspace, None, _) => Err(format!(
            "The user's environment is offline. Name a machine with `in`: {}.",
            choices()
        )),
        _ => Err(format!(
            "Several machines offer {what}: {}. Name one with `in`.",
            choices()
        )),
    }
}

impl Machines {
    /// The machine a call runs on; the error is for the model.
    pub(crate) async fn pick(&self, name: Option<&str>, need: Need<'_>) -> Result<Picked, String> {
        let rows = self
            .user
            .nodes()
            .await
            .map_err(|e| format!("Could not list the user's machines: {e:#}"))?;
        let online: Vec<Option<Online>> = rows.iter().map(|row| self.online(row)).collect();
        let picked = pick_from(&rows, &online, name, need);
        if picked.is_ok() || online.iter().all(Option::is_some) {
            return picked;
        }
        // A machine whose link dropped a moment ago, as one changing
        // network does, is waited for before the call fails.
        let back = join_all(rows.iter().zip(online).map(|(row, now)| async move {
            match now {
                Some(now) => Some(now),
                None => self
                    .app
                    .nodes
                    .back_online(&row.endpoint)
                    .await
                    .filter(|online| online.user == self.user.id()),
            }
        }))
        .await;
        pick_from(&rows, &back, name, need)
    }

    /// The node's connection, only while it is signed in as this user.
    fn online(&self, row: &NodeRow) -> Option<Online> {
        self.app
            .nodes
            .online(&row.endpoint)
            .filter(|online| online.user == self.user.id())
    }
}

/// The machine a call runs on among `rows`, with each one's connection in
/// `online`; the error is for the model.
fn pick_from(
    rows: &[NodeRow],
    online: &[Option<Online>],
    name: Option<&str>,
    need: Need<'_>,
) -> Result<Picked, String> {
    let candidates: Vec<Candidate<'_>> = rows
        .iter()
        .zip(online)
        .map(|(row, online)| Candidate {
            name: &row.name,
            online: online.is_some(),
            meets: online.as_ref().is_some_and(|online| need.met(online)),
            workspace: online
                .as_ref()
                .is_some_and(|online| online.workspace.is_some()),
        })
        .collect();
    let at = choose(&candidates, name, need.rule(), &need.what())?;
    let row = &rows[at];
    let online = online[at]
        .clone()
        .ok_or_else(|| format!("{} is offline.", row.name))?;
    Ok(Picked {
        name: row.name.clone(),
        endpoint: row.endpoint.clone(),
        online,
        home: row.env.as_ref().map(|env| env.home.clone()),
    })
}

/// The node `name` names: by name, ignoring case and spaces against dashes.
pub(crate) fn find<'a>(nodes: &'a [NodeRow], name: &str) -> Option<&'a NodeRow> {
    nodes
        .iter()
        .find(|node| command_name(&node.name) == command_name(name))
}

/// A machine's name as one word, such as `build-box`.
pub(crate) fn command_name(name: &str) -> String {
    name.trim().to_lowercase().replace(' ', "-")
}

/// How the model calls `code_agent` for `action` on session `id`, for hints.
pub(crate) fn agent_call(node: &str, agent: &str, action: &str, id: &str) -> String {
    format!("code_agent with action `{action}`, agent `{agent}`, id `{id}` and in `{node}`")
}

fn finish(call_id: &str, result: Result<String, String>) -> ToolResult {
    match result {
        Ok(text) => ToolResult::success(call_id, text),
        Err(text) => ToolResult::failure(call_id, text),
    }
}

/// The `in` parameter every machine tool takes.
pub(crate) fn in_parameter(default: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "string",
        "description": format!("The machine to use, by its name in the machines list. Pick one \
            whose OS and `can` list fit the task: iOS, iPadOS and macOS work (Xcode, the iOS \
            simulator) runs on a macOS machine that can xcode. {default}")
    })
}

#[cfg(test)]
mod live_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(name: &str, online: bool, meets: bool, workspace: bool) -> Candidate<'_> {
        Candidate {
            name,
            online,
            meets,
            workspace,
        }
    }

    #[test]
    fn in_picks_a_machine_by_name() {
        let machines = [
            candidate("Build Box", true, true, true),
            candidate("mac", true, false, false),
            candidate("laptop", false, false, false),
        ];
        assert_eq!(
            choose(&machines, Some("build-box"), Rule::Workspace, "a shell"),
            Ok(0)
        );
        assert_eq!(
            choose(&machines, Some("mac"), Rule::OnlyOne, "desktop control"),
            Err("mac does not offer desktop control.".into())
        );
        assert_eq!(
            choose(&machines, Some("laptop"), Rule::Workspace, "a shell"),
            Err("laptop is offline.".into())
        );
        assert_eq!(
            choose(&machines, Some("phone"), Rule::Workspace, "a shell"),
            Err("No machine called phone. Machines: build-box, mac, laptop.".into())
        );
    }

    #[test]
    fn commands_default_to_the_users_environment() {
        let machines = [
            candidate("mac", true, true, false),
            candidate("home", true, true, true),
        ];
        assert_eq!(choose(&machines, None, Rule::Workspace, "a shell"), Ok(1));
        let no_environment = [candidate("mac", true, true, false)];
        assert_eq!(
            choose(&no_environment, None, Rule::Workspace, "a shell"),
            Err("The user's environment is offline. Name a machine with `in`: mac.".into())
        );
    }

    #[test]
    fn desktop_needs_a_name_when_several_machines_offer_it() {
        let one = [
            candidate("home", true, false, true),
            candidate("mac", true, true, false),
        ];
        assert_eq!(choose(&one, None, Rule::OnlyOne, "desktop control"), Ok(1));
        let two = [
            candidate("mac", true, true, false),
            candidate("studio", true, true, false),
        ];
        assert_eq!(
            choose(&two, None, Rule::OnlyOne, "desktop control"),
            Err("Several machines offer desktop control: mac, studio. Name one with `in`.".into())
        );
        let none = [candidate("home", true, false, true)];
        assert_eq!(
            choose(&none, None, Rule::OnlyOne, "desktop control"),
            Err(
                "None of the user's machines online offers desktop control. Machines: home.".into()
            )
        );
    }
}
