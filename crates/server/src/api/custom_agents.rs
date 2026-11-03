//! The custom agents a project's folder defines on one machine, for the
//! project page. The node reads them and keeps every path inside the
//! folder and the harnesses' own folders; the server passes them through
//! and keeps nothing.

use super::{Reply, agent_control::live_host_back};
use crate::{
    nodes::NodeWork,
    protocol::{ApiError, CustomAgent, ListCustomAgents},
    router::Cx,
};

pub(super) async fn list_custom_agents(cx: Cx, list: ListCustomAgents) -> Reply<Vec<CustomAgent>> {
    let live = live_host_back(&cx, &list.machine_id).await?;
    let machine = &live.host.machine_name;
    let read = live.online.custom_agents(&list.folder).await.map_err(|e| {
        ApiError::bad_request(format!(
            "Could not read the custom agents on {machine}: {e:#}. Updating beep-node there may help."
        ))
    })?;
    match read {
        NodeWork::Done(agents) => Ok(agents),
        NodeWork::Refused(message) => Err(ApiError::bad_request(format!("{machine}: {message}"))),
    }
}
