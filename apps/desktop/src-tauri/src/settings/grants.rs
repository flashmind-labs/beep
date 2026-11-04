//! Routine grants: what the user let a routine do without asking, until the
//! grant ends, and revoking one so the routine asks again.

use crate::{
    beep::{Field, Service, epoch_ms},
    error::Result,
};
use beep_proto::{Grant, ListGrants, RevokeGrant};
use serde::Serialize;

/// A grant as the bar lists it. Times are in milliseconds since the Unix
/// epoch.
#[derive(Debug, Serialize)]
pub struct GrantView {
    id: String,
    routine_id: String,
    /// The routine's label, or its prompt when it has none.
    routine: String,
    app: String,
    action: String,
    fields: Vec<Field>,
    per_run: u32,
    expires_at: u64,
    created_at: u64,
    last_used_at: Option<u64>,
}

impl From<Grant> for GrantView {
    fn from(grant: Grant) -> Self {
        Self {
            id: grant.id,
            routine_id: grant.routine_id,
            routine: grant.routine,
            app: grant.app,
            action: grant.action,
            fields: grant.fields.into_iter().map(Field::from).collect(),
            per_run: grant.per_run,
            expires_at: epoch_ms(grant.expires_at),
            created_at: epoch_ms(grant.created_at),
            last_used_at: grant.last_used_at.map(epoch_ms),
        }
    }
}

/// The grants still in force, newest first.
#[tauri::command]
pub async fn list_grants(beep: Service<'_>) -> Result<Vec<GrantView>> {
    let grants = beep.call(ListGrants {}).await?;
    Ok(grants.into_iter().map(GrantView::from).collect())
}

#[tauri::command]
pub async fn revoke_grant(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(RevokeGrant { id }).await
}
