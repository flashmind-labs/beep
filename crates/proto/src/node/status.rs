//! `status/1`: what a node reports to a peer in its mesh, in place of the
//! control stream it keeps with a server. The stream starts with `Open`;
//! the node answers `Opened`, then sends reports until the peer closes it.
//! `Offers` comes first, `Env` once probed and again when it changes, `Stats`
//! about once a minute, `Traffic` the same.

use super::{EnvReport, StatsReport, traffic::TrafficReport};
use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "status/1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
// Sent a few times an hour, never kept in bulk: the size does not matter.
#[allow(clippy::large_enum_variant)]
pub enum Report {
    /// The protocols the node serves.
    Offers(Vec<String>),
    Env(EnvReport),
    Stats(StatsReport),
    Traffic(TrafficReport),
}
