//! One coding-agent session: its own stream (`<agent>/1`) for starting,
//! resuming or attaching to a session and giving it an order, and the checks
//! on what an app may ask of a session before a node is asked. A session
//! lives in the node, so dropping the stream leaves it running.

use super::{said, unexpected};
use crate::{CALL_TIMEOUT, MeshError, PeerClient};
use beep_proto::{
    AgentCheckoutPlan, AgentEffort, AgentHost, AgentSteer, ApiError, ModelId, agent_name,
    check_agent_images,
    node::{
        LinkRx, LinkTx,
        agent::{self, Event, Request, SessionInfo},
    },
};
use std::time::Duration;
use tokio::time::timeout;

/// What a session starts on besides its folder and prompt; `None` keeps the agent's default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartOn {
    /// An id from the agent's model list.
    pub model: Option<String>,
    /// One of `agent::efforts` for the agent.
    pub effort: Option<agent::Effort>,
    /// The branch or worktree to make first, for a node that makes them.
    pub checkout: Option<agent::CheckoutPlan>,
}

/// A session stream the node accepted.
pub struct Opened {
    pub info: SessionInfo,
    pub tx: LinkTx,
    pub rx: LinkRx,
}

/// Opens `agent`'s stream and sends `first`, which is `Start`, `Attach` or
/// `Resume`. The node answers with the session as it stands.
pub async fn open_agent(
    client: &PeerClient,
    agent: &str,
    first: Request,
) -> Result<Opened, MeshError> {
    let (tx, mut rx) = client.open_sending(&agent::protocol(agent), &first).await?;
    let event = timeout(CALL_TIMEOUT, rx.recv::<Event>())
        .await
        .map_err(|_| said("the node did not answer"))?
        .map_err(MeshError::Link)?
        .ok_or_else(|| said("the node closed the session stream"))?;
    match event {
        Event::Attached { session } => Ok(Opened {
            info: *session,
            tx,
            rx,
        }),
        Event::Refused { reason } => Err(said(reason)),
        other => Err(unexpected(&other)),
    }
}

/// Sends `request` down an attached stream and waits for the node's `Ack`,
/// skipping the state changes that come first. `Err` carries the node's
/// reason when it did nothing.
pub async fn order(
    opened: &mut Opened,
    request: Request,
    limit: Duration,
) -> Result<(), MeshError> {
    opened.tx.send(&request).await.map_err(MeshError::Link)?;
    let wait = async {
        loop {
            match opened.rx.recv::<Event>().await.map_err(MeshError::Link)? {
                Some(Event::Ack { error: None }) => return Ok(()),
                Some(Event::Ack { error: Some(error) }) => return Err(said(error)),
                Some(Event::Ended { reason }) => return Err(said(reason)),
                Some(_) => {}
                None => return Err(said("lost the session's stream")),
            }
        }
    };
    timeout(limit, wait)
        .await
        .map_err(|_| said("the node did not answer"))?
}

/// The prompt a node from before `Request::Steer` gets for one, sent after the running turn.
pub fn queued_instead(request: &Request) -> Option<Request> {
    let Request::Steer { text, images } = request else {
        return None;
    };
    Some(if images.is_empty() {
        Request::Prompt { text: text.clone() }
    } else {
        Request::PromptImages {
            text: text.clone(),
            images: images.clone(),
        }
    })
}

/// What the node is asked for `steer`; an empty message or images over the limits are refused.
pub fn node_request(steer: AgentSteer) -> Result<Request, ApiError> {
    Ok(match steer {
        AgentSteer::Prompt { text } if text.trim().is_empty() => {
            return Err(ApiError::bad_request("The message is empty."));
        }
        AgentSteer::Prompt { text } => Request::Prompt { text },
        AgentSteer::PromptImages { text, images } if images.is_empty() => {
            return node_request(AgentSteer::Prompt { text });
        }
        AgentSteer::PromptImages { text, images } => {
            check_agent_images(&images)?;
            Request::PromptImages { text, images }
        }
        AgentSteer::Steer { text, images } if text.trim().is_empty() && images.is_empty() => {
            return Err(ApiError::bad_request("The message is empty."));
        }
        AgentSteer::Steer { text, images } => {
            check_agent_images(&images)?;
            Request::Steer { text, images }
        }
        AgentSteer::DropSteers {} => Request::DropSteers {},
        AgentSteer::SetMode { mode } => Request::SetMode { mode },
        AgentSteer::StopTask { task } => Request::StopTask { task },
        AgentSteer::Cancel {} => Request::Cancel {},
        AgentSteer::Close {} => Request::Close {},
        AgentSteer::Configure {
            model: None,
            effort: None,
        } => return Err(ApiError::bad_request("Nothing to switch.")),
        AgentSteer::Configure { model, effort } => Request::Configure {
            model: model
                .as_ref()
                .map(|m| m.as_str().trim())
                .filter(|m| !m.is_empty())
                .map(ModelId::from),
            effort: effort.map(Into::into),
        },
    })
}

/// The model a start asks for: `None` for the agent's own.
pub fn picked_model(model: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) else {
        return Ok(None);
    };
    if !agent::valid_model(model) {
        return Err(ApiError::bad_request("That is not a model name."));
    }
    Ok(Some(model.to_owned()))
}

/// The thinking level a start asks of `agent`: `None` for the agent's own.
pub fn picked_effort(
    agent: &str,
    effort: Option<AgentEffort>,
) -> Result<Option<agent::Effort>, ApiError> {
    let Some(effort) = effort.map(agent::Effort::from) else {
        return Ok(None);
    };
    if !agent::efforts(agent).contains(&effort) {
        return Err(ApiError::bad_request(format!(
            "{} cannot be set to think at {}.",
            agent_name(agent),
            effort.as_str()
        )));
    }
    Ok(Some(effort))
}

/// The agent Auto starts when the request names none, most preferred first.
const PREFERRED: [&str; 6] = ["claude", "codex", "gemini", "opencode", "pi", "flsh"];

/// `named`, or the one `host` prefers when the call names none.
pub fn pick_agent(host: &AgentHost, named: Option<&str>) -> Result<String, ApiError> {
    let offers = |agent: &str| host.agents.iter().any(|offered| offered == agent);
    if let Some(named) = named {
        return if offers(named) {
            Ok(named.to_owned())
        } else {
            Err(ApiError::bad_request(format!(
                "{} does not offer {}.",
                host.machine_name,
                agent_name(named)
            )))
        };
    }
    PREFERRED
        .iter()
        .find(|agent| offers(agent))
        .map(|agent| (*agent).to_owned())
        .or_else(|| host.agents.first().cloned())
        .ok_or_else(|| {
            ApiError::bad_request(format!("{} offers no coding agent.", host.machine_name))
        })
}

/// Which checkout plans a node makes.
#[derive(Debug, Clone, Copy)]
pub struct Makes {
    /// A new branch or worktree named from the prompt.
    pub checkouts: bool,
    /// A worktree on a branch named by the start, new or existing.
    pub branches: bool,
}

/// The checkout a start asks node `name`, which makes what `makes` says, to
/// make; refused when the node is too old.
pub fn checkout_for(
    makes: Makes,
    name: &str,
    checkout: Option<AgentCheckoutPlan>,
) -> Result<Option<agent::CheckoutPlan>, ApiError> {
    let Some(plan) = checkout.map(agent::CheckoutPlan::from) else {
        return Ok(None);
    };
    if let Some(base) = plan.base()
        && !agent::valid_ref(base)
    {
        return Err(ApiError::bad_request(format!(
            "{base:?} is not a branch or commit name."
        )));
    }
    if let Some(branch) = plan.branch() {
        if !agent::valid_branch(branch) {
            return Err(ApiError::bad_request(format!(
                "{branch:?} is not a branch name git takes."
            )));
        }
        if !makes.branches {
            return Err(ApiError::bad_request(format!(
                "beep-node on {name} is too old to start on a named branch. Update it, or start on a new worktree."
            )));
        }
    }
    if !makes.checkouts {
        return Err(ApiError::bad_request(format!(
            "beep-node on {name} is too old to make a branch or worktree. Update it, or start in the checkout as it is."
        )));
    }
    Ok(Some(plan))
}
