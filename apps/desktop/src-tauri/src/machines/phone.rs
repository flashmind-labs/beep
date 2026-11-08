//! A phone is no machine of the mesh: it embeds no node, so there is nothing to lend. This
//! stands in for `this_mac` where the rest of the app expects it, and lends nothing.

use crate::beep::Beep;
use beep_proto::node::desktop::{Permission, Permissions};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Off,
}

/// What Settings shows under This Mac: a phone lends nothing.
#[derive(Debug, Serialize)]
pub struct View {
    id: String,
    name: String,
    enabled: bool,
    status: Status,
    permissions: Permissions,
}

#[derive(Default)]
pub struct Machine;

impl Machine {
    pub fn start(&self) {}

    // The desktop's future is 'static, which `async fn` here would not be.
    #[allow(clippy::manual_async_fn)]
    pub fn quit(&self) -> impl Future<Output = ()> + Send + 'static {
        async {}
    }

    pub fn name(&self) -> &str {
        "Phone"
    }

    pub fn view(&self, beep: &Beep) -> View {
        View {
            id: beep.id().to_string(),
            name: self.name().to_owned(),
            enabled: false,
            status: Status::Off,
            permissions: Permissions::default(),
        }
    }

    pub fn sync(&self, _: &Beep) {}
}

pub fn allow(_: Permission) {}
