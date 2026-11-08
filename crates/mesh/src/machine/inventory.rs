//! Device classification for inventories, separate from signed membership roles.

use crate::{DeviceKind, DeviceStatus, Mesh};

/// A member as machine inventories describe it. Phones retain controller
/// permissions in signed membership, but do not host sessions or files.
#[derive(Clone, Debug, strum::Display, strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum Device {
    Node(DeviceStatus),
    Desktop(DeviceStatus),
    Phone(DeviceStatus),
}

impl AsRef<DeviceStatus> for Device {
    fn as_ref(&self) -> &DeviceStatus {
        match self {
            Self::Node(status) | Self::Desktop(status) | Self::Phone(status) => status,
        }
    }
}

impl From<Device> for DeviceStatus {
    fn from(device: Device) -> Self {
        match device {
            Device::Node(status) | Device::Desktop(status) | Device::Phone(status) => status,
        }
    }
}

impl Mesh {
    /// Every member, classified without changing the signed membership record.
    pub fn inventory(&self) -> Vec<Device> {
        self.machines()
            .into_iter()
            .map(|status| {
                if self.is_observer(status.device.id) {
                    Device::Phone(status)
                } else {
                    match status.device.kind {
                        DeviceKind::Node => Device::Node(status),
                        DeviceKind::Desktop => Device::Desktop(status),
                    }
                }
            })
            .collect()
    }
}
