//! The session rows as this app shows them: the mirror's rows with presence, deletes and moves
//! under way, the user's titles and the starts it holds on top.

use super::starts::Starts;
use crate::fleet::{
    Fleet,
    session_lists::overlay,
    session_mirror::{Link, Rows, showing},
};
use beep_proto::MachineSession;
use iroh::EndpointId;
use std::{collections::HashSet, sync::Arc};
use tauri::{AppHandle, Manager};

/// `rows` with what this app knows on top: presence, deletes and moves under
/// way, the user's titles and the starts it holds.
pub(crate) fn overlaid(app: &AppHandle, fleet: &Fleet, rows: &Rows) -> Vec<MachineSession> {
    let mut sessions: Vec<MachineSession> = rows.iter_of(shown(fleet).as_ref()).cloned().collect();
    sessions.retain_mut(|session| overlay(fleet, session));

    sessions.extend(app.state::<Arc<Starts>>().rows());
    sessions.sort_by_key(|session| std::cmp::Reverse(session.last_activity));
    sessions
}

/// The child and reviewer sessions in `rows`, under the same overlays as
/// the other rows.
pub(super) fn overlaid_children(fleet: &Fleet, rows: &Rows) -> Vec<(MachineSession, Link)> {
    rows.children_of(shown(fleet).as_ref())
        .map(|(session, link)| (session.clone(), link.clone()))
        .filter_map(|(mut session, link)| overlay(fleet, &mut session).then_some((session, link)))
        .collect()
}

/// The machines whose rows show. Once the mesh is open, a machine that is
/// off shows no sessions: the mirror keeps them for when it is back.
fn shown(fleet: &Fleet) -> Option<HashSet<EndpointId>> {
    fleet
        .mesh()
        .map(|_| showing(&fleet.online(), &fleet.status().connecting(), fleet.id()))
}
