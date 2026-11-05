//! What the local assistant may ask about the user's work. Every answer only reads: machines,
//! projects, coding agent sessions and a session's recent log. The text goes back to the model
//! inside `<untrusted>` tags, added by `beep_assistant`.

use crate::{
    agents::sessions::machine_sessions,
    beep::{Beep, epoch_ms},
    fleet::{Fleet, agents as direct, projects::desk},
    machines::list::{Outside, machines},
};
use beep_assistant::WorkQuery;
use beep_fleet::Session;
use beep_proto::GetAgentSession;
use serde_json::{Value, json};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// Sessions listed to the model.
const SESSIONS_LISTED: usize = 30;
/// Log entries read from one session.
const LOG_ENTRIES: u32 = 20;
/// Characters kept of one log entry.
const ENTRY_CHARS: usize = 600;

/// The answer to `query`, as JSON text; an error is the reason, in words.
pub(crate) async fn answer(app: &AppHandle, query: WorkQuery) -> Result<String, String> {
    let rows = match query {
        WorkQuery::Machines => list_machines(app),
        WorkQuery::Projects => list_projects(app).await?,
        WorkQuery::Sessions => list_sessions(app).await,
        WorkQuery::ReadSession {
            machine_id,
            agent,
            session_id,
        } => read_session(app, machine_id, agent, session_id).await?,
    };
    Ok(rows.to_string())
}

fn short(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn list_machines(app: &AppHandle) -> Value {
    let fleet = app.state::<Arc<Fleet>>();
    let held = app.state::<Outside>();
    let rows: Vec<Value> = machines(app, &fleet, &held)
        .into_iter()
        .map(|machine| {
            json!({
                "id": machine.id,
                "name": machine.name,
                "online": machine.online,
                "last_seen_ms": epoch_ms(machine.seen_at),
                "version": machine.version,
            })
        })
        .collect();
    Value::Array(rows)
}

async fn list_projects(app: &AppHandle) -> Result<Value, String> {
    let session = app.state::<Arc<Session>>();
    let desk = desk(&session).await.map_err(|e| e.to_string())?;
    let rows: Vec<Value> = desk
        .imported()
        .await
        .into_iter()
        .map(|project| {
            json!({
                "project": project.project,
                "url": project.url,
                "machine": project.machine_name,
                "machine_id": project.machine_id,
                "folder": project.folder,
            })
        })
        .collect();
    Ok(Value::Array(rows))
}

async fn list_sessions(app: &AppHandle) -> Value {
    let beep = app.state::<Arc<Beep>>();
    let fleet = app.state::<Arc<Fleet>>();
    let held = app.state::<Outside>();
    let rows: Vec<Value> = machine_sessions(app, &beep, &fleet, &held)
        .await
        .into_iter()
        .take(SESSIONS_LISTED)
        .map(|session| {
            let status: &'static str = session.status.into();
            json!({
                "machine_id": session.machine_id,
                "machine": session.machine_name,
                "agent": session.agent,
                "session_id": session.id,
                "title": session.title,
                "folder": session.cwd,
                "status": status,
                "doing": session.doing,
                "last_activity_ms": epoch_ms(session.last_activity),
            })
        })
        .collect();
    Value::Array(rows)
}

async fn read_session(
    app: &AppHandle,
    machine_id: String,
    agent: String,
    session_id: String,
) -> Result<Value, String> {
    let beep = app.state::<Arc<Beep>>();
    let fleet = app.state::<Arc<Fleet>>();
    let request = GetAgentSession {
        machine_id,
        agent,
        session: session_id,
        last: LOG_ENTRIES,
        before: None,
    };
    let view = match fleet.member(&request.machine_id) {
        Some(machine) => direct::session(&fleet, machine, &request)
            .await
            .map_err(|e| e.to_string()),
        None => beep.call(request).await.map_err(|e| e.to_string()),
    }?;
    let entries: Vec<Value> = view
        .entries
        .into_iter()
        .map(|entry| {
            let kind: &'static str = entry.kind.into();
            json!({"kind": kind, "text": short(&entry.text, ENTRY_CHARS)})
        })
        .collect();
    Ok(json!({
        "title": view.session.title,
        "agent": view.session.agent,
        "error": view.error,
        "earlier_entries": view.earlier,
        "entries": entries,
    }))
}
