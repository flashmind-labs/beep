//! What the caller's machines moved over iroh.

use super::Reply;
use crate::{
    api::router::Cx,
    protocol::{ListMachineTraffic, MachineTraffic},
};

pub(super) async fn list_machine_traffic(
    cx: Cx,
    _: ListMachineTraffic,
) -> Reply<Vec<MachineTraffic>> {
    Ok(cx.user.machine_traffic().await?)
}
