//! What an app may ask of a session, checked before a node is asked.

use super::agent_name;
use beep_proto::{
    AgentCheckoutPlan, AgentEffort, AgentHost, AgentSteer, ApiError, ModelId, check_agent_images,
    node::agent::{self, Request},
};

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

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::Upload;

    fn png(name: &str, size: usize) -> Upload {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.resize(size, 0);
        Upload {
            name: name.into(),
            mime: "image/png".into(),
            data,
        }
    }

    #[test]
    fn a_named_branch_needs_a_branch_name_and_a_node_that_makes_it() {
        let named = |branch: &str| {
            Some(AgentCheckoutPlan::NamedWorktree {
                base: Some("main".into()),
                branch: branch.into(),
            })
        };
        let new = Makes {
            checkouts: true,
            branches: true,
        };
        let plain = Makes {
            checkouts: true,
            branches: false,
        };
        assert!(
            checkout_for(new, "box", named("issue-52-crash"))
                .unwrap()
                .is_some()
        );
        for bad in [
            "", "-b", "a..b", "HEAD", "x~1", "a/.b", "a.lock", "a//b", "a b",
        ] {
            assert!(
                matches!(
                    checkout_for(new, "box", named(bad)),
                    Err(ApiError::BadRequest { .. })
                ),
                "{bad:?} is refused"
            );
        }
        let Err(ApiError::BadRequest { message }) = checkout_for(plain, "old-box", named("ok"))
        else {
            panic!("a bad request");
        };
        assert!(message.contains("old-box is too old"), "{message}");
    }

    #[test]
    fn a_steer_goes_to_the_node_and_falls_back_to_a_prompt() {
        let steer = node_request(AgentSteer::Steer {
            text: "Use the fixture instead".into(),
            images: vec![png("shot.png", 1024)],
        })
        .unwrap();
        assert!(matches!(&steer, Request::Steer { images, .. } if images.len() == 1));
        assert!(matches!(
            queued_instead(&steer),
            Some(Request::PromptImages { text, images }) if text == "Use the fixture instead" && images.len() == 1
        ));
        assert_eq!(queued_instead(&Request::Cancel {}), None);
        let empty = node_request(AgentSteer::Steer {
            text: " ".into(),
            images: Vec::new(),
        });
        assert!(empty.is_err(), "nothing to send");
    }
}
