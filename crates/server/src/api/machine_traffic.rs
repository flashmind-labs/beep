//! What the caller's machines moved: no machine reports to the server now.

use super::Reply;
use crate::{
    api::router::Cx,
    protocol::{ListMachineTraffic, MachineTraffic},
};

pub(super) async fn list_machine_traffic(
    _: Cx,
    _: ListMachineTraffic,
) -> Reply<Vec<MachineTraffic>> {
    Ok(Vec::new())
}
