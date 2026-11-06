//! `beep-node status`: the running node, its mesh and each project's sync.

pub(crate) use beep_node::link::local_status::render;

/// This build's version.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What to print when no node runs from the folder.
pub(crate) fn not_running() -> String {
    format!(
        "beep-node {VERSION}\n\
         Node      not running\n\
         \n\
         Start it with `beep-node service install`, which keeps it running in the\n\
         background, or `beep-node run` to serve in this terminal.\n"
    )
}
