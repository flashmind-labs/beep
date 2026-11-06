//! Machines nearest first: this one, then peers by the round trip measured on their mesh
//! connection, then those with none measured.

use beep_mesh::Mesh;
use iroh::EndpointId;
use std::time::Duration;

/// Orders `machines` nearest first, by `id` of each (see [`rtt`]).
pub fn nearest_first<T>(mesh: &Mesh, machines: &mut [T], id: impl Fn(&T) -> EndpointId) {
    by_rtt(machines, |machine| rtt(mesh, id(machine)));
}

/// The round trip to `machine`: zero for this one, else iroh's estimate on the path its mesh
/// connection uses (the fastest open path when none is selected). `None` with no connection.
fn rtt(mesh: &Mesh, machine: EndpointId) -> Option<Duration> {
    if machine == mesh.me() {
        return Some(Duration::ZERO);
    }
    let connection = mesh.peer(machine)?;
    let paths = connection.paths();
    paths
        .iter()
        .find(|path| path.is_selected())
        .map(|path| path.rtt())
        .or_else(|| paths.iter().map(|path| path.rtt()).min())
}

/// Lowest round trip first, unknown last; equal ones keep their order.
fn by_rtt<T>(machines: &mut [T], rtt: impl Fn(&T) -> Option<Duration>) {
    machines.sort_by_cached_key(|machine| match rtt(machine) {
        Some(rtt) => (false, rtt),
        None => (true, Duration::ZERO),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_round_trip_first_and_unknown_last() {
        let ms = Duration::from_millis;
        let mut machines = [
            ("far", Some(ms(80))),
            ("unknown", None),
            ("here", Some(Duration::ZERO)),
            ("near", Some(ms(4))),
            ("also unknown", None),
        ];
        by_rtt(&mut machines, |(_, rtt)| *rtt);
        let order: Vec<&str> = machines.iter().map(|(name, _)| *name).collect();
        assert_eq!(order, ["here", "near", "far", "unknown", "also unknown"]);
    }
}
