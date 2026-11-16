//! ACP requests and bounded adapter transport.

mod elicitation;
mod permissions;
mod prompt;
mod session;
mod tasks;
mod transport;
mod version;
pub(super) use version::{Capabilities, Protocol};

pub(super) use elicitation::{Answer, Fields, Form};
pub(super) use permissions::{Permission, permission_v2};
pub(super) use permissions::{allow_option, cancelled, plan_of, selected};
pub(super) use prompt::{Prompt, SteeringOutcome, prompt, steering, takes_images, takes_steering};
pub(super) use session::{
    MAX_MODELS, ModelSwitch, Offered, PLAN_MODE, ResumeMethod, initialize, new_session, offers,
    resume_method, resume_session, session_created, set_effort, set_fast, switch_model,
    takes_system_prompt, wire_name,
};
pub(super) use tasks::{AsyncTask, async_task, stop_task};
pub(crate) use transport::{Line, LineReader};
pub(super) use transport::{MAX_MESSAGE, transport};
