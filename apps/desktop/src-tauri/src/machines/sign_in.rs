//! Signing a machine's coding agent in from here (`sign-in/1`): the
//! machine runs the agent's own sign-in, this Mac shows its link and code
//! and passes back the code the user pastes. Over the mesh, no server.

use crate::{
    error::{DesktopError, Result},
    fleet::Machines,
};
use beep_mesh::machine;
use beep_proto::node::sign_in::{Request, State};
use serde::{Deserialize, Serialize};

/// What the sheet asks of the sign-in.
#[derive(Debug, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    Start,
    State,
    Answer { text: String },
    Cancel,
}

/// How the sign-in stands, for the sheet: `idle`, `starting`, `waiting`,
/// `signed_in` or `failed`.
#[derive(Debug, Serialize)]
pub struct SignInView {
    state: &'static str,
    url: Option<String>,
    code: Option<String>,
    asks_code: bool,
    reason: Option<String>,
}

impl From<State> for SignInView {
    fn from(state: State) -> Self {
        let mut view = Self {
            state: "idle",
            url: None,
            code: None,
            asks_code: false,
            reason: None,
        };
        match state {
            State::Starting {} => view.state = "starting",
            State::Waiting {
                url,
                code,
                asks_code,
            } => {
                view.state = "waiting";
                view.url = Some(url);
                view.code = code;
                view.asks_code = asks_code;
            }
            State::SignedIn {} => view.state = "signed_in",
            State::Failed { reason } => {
                view.state = "failed";
                view.reason = Some(reason);
            }
            // `Idle`, and whatever a newer node says.
            _ => {}
        }
        view
    }
}

/// Takes one `step` of `tool`'s sign-in on the machine `machine_id`.
#[tauri::command]
pub async fn machine_sign_in(
    fleet: Machines<'_>,
    machine_id: String,
    tool: String,
    step: Step,
) -> Result<SignInView> {
    let machine = fleet.member(&machine_id).ok_or_else(|| {
        DesktopError::BadRequest("That machine is not in this Mac's mesh.".into())
    })?;
    let request = match step {
        Step::Start => Request::Start { tool },
        Step::State => Request::State { tool },
        Step::Answer { text } => Request::Answer { tool, text },
        Step::Cancel => Request::Cancel { tool },
    };
    let client = fleet.client(machine).await?;
    let state = machine::sign_in(&client, &request)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("{}: {e}", fleet.name_of(machine))))?;
    Ok(state.into())
}
