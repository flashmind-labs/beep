//! A phone is no machine of the mesh: it embeds no node, so there is nothing to lend. This
//! stands in for `this_mac` where the rest of the app expects it, and lends nothing.

use crate::{
    beep::state::Prefs,
    machines::{ThisMacStatus, ThisMacView},
};
use beep_proto::node::desktop::{Permission, Permissions};
use iroh::EndpointId;

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

    pub fn view(&self, _: &Prefs, id: EndpointId) -> ThisMacView {
        ThisMacView {
            id: id.to_string(),
            name: self.name().to_owned(),
            enabled: false,
            status: ThisMacStatus::Off,
            permissions: Permissions::default(),
        }
    }

    pub fn sync(&self, _: &Prefs) {}
}

pub fn allow(_: Permission) {}
