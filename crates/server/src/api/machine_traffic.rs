//! What the caller's machines moved over iroh.

use super::Reply;
use crate::{
    protocol::{ListMachineTraffic, MachineTraffic},
    router::Cx,
};

pub(super) async fn list_machine_traffic(
    cx: Cx,
    _: ListMachineTraffic,
) -> Reply<Vec<MachineTraffic>> {
    Ok(cx.user.machine_traffic().await?)
}
