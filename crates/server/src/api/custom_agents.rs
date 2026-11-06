//! The custom agents a project's folder defines on one machine: a desktop reads them now.

use super::{Reply, own_nodes::retired};
use crate::{
    api::router::Cx,
    protocol::{CustomAgent, ListCustomAgents},
};

pub(super) async fn list_custom_agents(_: Cx, _: ListCustomAgents) -> Reply<Vec<CustomAgent>> {
    retired()
}
