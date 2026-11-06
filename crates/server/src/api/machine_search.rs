//! Where a machine looks for git checkouts: set from a desktop now.

use super::{Reply, own_nodes::retired};
use crate::{api::router::Cx, protocol::*};

pub(super) async fn get_machine_search(_: Cx, _: GetMachineSearch) -> Reply<String> {
    retired()
}

pub(super) async fn set_machine_search(_: Cx, _: SetMachineSearch) -> Reply<String> {
    retired()
}
