//! The machines of the mesh as agents name them.

use crate::calls::command_name;
use beep_mesh::{DeviceStatus, Mesh, machine::Device};
use iroh::EndpointId;

/// One device of the mesh.
#[derive(Clone)]
pub(crate) struct Machine {
    pub(crate) id: EndpointId,
    pub(crate) name: String,
}

impl From<DeviceStatus> for Machine {
    fn from(status: DeviceStatus) -> Self {
        Self {
            id: status.device.id,
            name: command_name(&status.device.name),
        }
    }
}

pub(crate) fn machines(mesh: &Mesh) -> Vec<Machine> {
    mesh.code_hosts().into_iter().map(Machine::from).collect()
}

/// The machine of `mesh` that `name` names, as agents say it.
pub(crate) fn find(mesh: &Mesh, name: &str) -> Option<Machine> {
    let wanted = command_name(name);
    machines(mesh).into_iter().find(|m| m.name == wanted)
}

/// The machine `name` names, or whose endpoint id it is.
pub(crate) fn find_or_id(mesh: &Mesh, name: &str) -> Option<Machine> {
    let wanted = command_name(name);
    machines(mesh)
        .into_iter()
        .find(|m| m.id.to_string() == name || m.name == wanted)
}

/// Why there is no machine called `name`.
pub(crate) fn no_machine(mesh: &Mesh, name: &str) -> String {
    let all = machines(mesh);
    let names: Vec<&str> = all.iter().map(|m| m.name.as_str()).collect();

    format!("No machine called {name}. Machines: {}.", names.join(", "))
}

pub(crate) fn find_named(mesh: &Mesh, name: &str) -> Result<Machine, String> {
    find(mesh, name).ok_or_else(|| no_machine(mesh, name))
}

pub(crate) fn find_or_id_named(mesh: &Mesh, name: &str) -> Result<Machine, String> {
    find_or_id(mesh, name).ok_or_else(|| no_machine(mesh, name))
}

/// Whether `mesh` has no machine but this one to run on; phones host nothing.
pub(crate) fn alone(mesh: &Mesh) -> bool {
    let me = mesh.me();

    mesh.inventory().iter().all(|device| match device {
        Device::Phone(_) => true,
        Device::Desktop(status) | Device::Node(status) => status.device.id == me,
    })
}

/// This machine's name in `mesh`, as agents say it; empty without one.
pub(crate) fn machine_name(mesh: Option<&Mesh>) -> String {
    let Some(mesh) = mesh else {
        return String::new();
    };

    machines(mesh)
        .into_iter()
        .find(|machine| machine.id == mesh.me())
        .map(|machine| machine.name)
        .unwrap_or_default()
}
