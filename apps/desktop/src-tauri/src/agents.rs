//! Coding-agent sessions on the user's machines, for the bar's and the
//! window's agent views: which machines can run one, starting one, reading
//! one in full and steering it. Answering a question goes through
//! `center_answer_agent`.

use crate::{
    beep::{Beep, epoch_ms},
    center::AgentsShown,
    commands::{GONE, file_name, free_name, image_mime},
    error::{DesktopError, Result},
    markdown,
    projects::unknown_op,
    work_copy,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    AGENT_FILE_CHUNK, AgentEntry, AgentEntryKind, AgentFileChunk, AgentHost, AgentModel,
    AgentModels, AgentQuestion, AgentSessionUsage, AgentSessionView, AgentSteer, ApiError,
    GetAgentFile, GetAgentImage, GetAgentSession, GetAgentSessionUsage, ListAgentHosts,
    ListAgentModels, ListAgentSessions, MAX_AGENT_FILE, MachinePick, MachineSession, ModelId,
    PreviewAgentMachine, ResumeAgentSession, StartAgentSession, SteerAgentSession, Upload,
    check_agent_images, image_key,
    node::agent::{CheckoutPlan, Effort},
};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, process::Command, sync::Arc, time::Instant};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use uuid::Uuid;

type Service<'a> = State<'a, Arc<Beep>>;

/// A machine that can run a coding agent.
#[derive(Debug, Serialize)]
pub struct HostRow {
    machine_id: String,
    machine_name: String,
    agents: Vec<String>,
    workspace: Option<String>,
    home: Option<String>,
    capabilities: Vec<&'static str>,
}

impl From<AgentHost> for HostRow {
    fn from(host: AgentHost) -> Self {
        Self {
            machine_id: host.machine_id,
            machine_name: host.machine_name,
            agents: host.agents,
            workspace: host.workspace,
            home: host.home,
            capabilities: host
                .capabilities
                .into_iter()
                .map(<&'static str>::from)
                .collect(),
        }
    }
}

/// One session as the views list it.
#[derive(Debug, Serialize)]
pub struct SessionRow {
    machine_id: String,
    machine_name: String,
    id: String,
    agent: String,
    title: Option<String>,
    cwd: String,
    status: &'static str,
    doing: Option<String>,
    /// In milliseconds since the Unix epoch.
    started_at: u64,
    last_activity: u64,
    external: bool,
    question: Option<AgentQuestion>,
}

impl From<MachineSession> for SessionRow {
    fn from(session: MachineSession) -> Self {
        Self {
            status: session.status.into(),
            started_at: epoch_ms(session.started_at),
            last_activity: epoch_ms(session.last_activity),
            machine_id: session.machine_id,
            machine_name: session.machine_name,
            id: session.id,
            agent: session.agent,
            title: session.title,
            cwd: session.cwd,
            doing: session.doing,
            external: session.external,
            question: session.question,
        }
    }
}

#[derive(Debug, Serialize)]
struct StepRow {
    content: String,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct EntryRow {
    kind: &'static str,
    text: String,
    /// What the agent said or thought, as the chat shows Beep's replies:
    /// its Markdown as cleaned HTML. Only on the agent's own entries and
    /// its thoughts.
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
    /// How long a thought took, in milliseconds, once it ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    ms: Option<u64>,
}

impl From<AgentEntry> for EntryRow {
    fn from(entry: AgentEntry) -> Self {
        // The bar draws the pictures of image lines after the text.
        let (html, ms) = match entry.kind {
            AgentEntryKind::Agent {} => (
                Some(markdown::to_html(&without_image_lines(&entry.text))),
                None,
            ),
            AgentEntryKind::Thought { ms } => (Some(markdown::to_html(&entry.text)), ms),
            _ => (None, None),
        };
        Self {
            kind: entry.kind.into(),
            text: entry.text,
            html,
            ms,
        }
    }
}

/// `text` without its `[image: name]` lines.
fn without_image_lines(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let line = line.trim();
            !(line.starts_with("[image: ") && line.ends_with(']'))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One session in full.
#[derive(Debug, Serialize)]
pub struct SessionPage {
    session: SessionRow,
    plan: Vec<StepRow>,
    files: Vec<String>,
    last_message: String,
    error: Option<String>,
    queued: u32,
    turns: u32,
    entries: Vec<EntryRow>,
    earlier: u32,
}

impl From<AgentSessionView> for SessionPage {
    fn from(view: AgentSessionView) -> Self {
        Self {
            session: view.session.into(),
            plan: view
                .plan
                .into_iter()
                .map(|step| StepRow {
                    content: step.content,
                    status: step.status.into(),
                })
                .collect(),
            files: view.files,
            last_message: view.last_message,
            error: view.error,
            queued: view.queued,
            turns: view.turns,
            entries: view.entries.into_iter().map(EntryRow::from).collect(),
            earlier: view.earlier,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Started {
    session: SessionRow,
    why: String,
}

/// What `agent_steer` does to a session, as the page sends it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Steer {
    Prompt {
        text: String,
        /// Pasted or dropped images, sent after the text.
        #[serde(default)]
        images: Vec<PastedImage>,
    },
    /// A message for the running turn, delivered once the agent's current
    /// tool call finishes.
    #[serde(rename = "steer")]
    InTurn {
        text: String,
        #[serde(default)]
        images: Vec<PastedImage>,
    },
    /// Takes back the steers not yet delivered.
    DropSteers,
    Cancel,
    Close,
    /// Switches the running session's model or thinking from its next turn.
    Configure {
        #[serde(default)]
        model: Option<ModelId>,
        #[serde(default)]
        effort: Option<Effort>,
    },
}

/// An image as the page sends it: its bytes in base64.
#[derive(Debug, Deserialize)]
pub struct PastedImage {
    name: String,
    mime: String,
    data: String,
}

impl TryFrom<Steer> for AgentSteer {
    type Error = DesktopError;

    /// Images go as `PromptImages`, checked against its limits here first;
    /// a message without them goes as `Prompt`, which every server reads.
    fn try_from(steer: Steer) -> Result<Self> {
        Ok(match steer {
            Steer::Prompt { text, images } if images.is_empty() => Self::Prompt { text },
            Steer::Prompt { text, images } => Self::PromptImages {
                text,
                images: uploads(images)?,
            },
            Steer::InTurn { text, images } => Self::Steer {
                text,
                images: uploads(images)?,
            },
            Steer::DropSteers => Self::DropSteers {},
            Steer::Cancel => Self::Cancel {},
            Steer::Close => Self::Close {},
            Steer::Configure { model, effort } => Self::Configure {
                model,
                effort: effort.map(Into::into),
            },
        })
    }
}

/// The pasted images as uploads, checked against the limits.
fn uploads(images: Vec<PastedImage>) -> Result<Vec<Upload>> {
    let images = images
        .into_iter()
        .map(|image| -> Result<Upload> {
            let data = STANDARD.decode(&image.data).map_err(|_| {
                DesktopError::BadRequest(format!("{} could not be read", image.name))
            })?;
            Ok(Upload {
                name: image.name,
                mime: image.mime,
                data,
            })
        })
        .collect::<Result<Vec<Upload>>>()?;
    check_agent_images(&images)?;
    Ok(images)
}

/// The same message as a prompt for after the turn, for a server from
/// before steering, which reads op 5 of `AgentSteer` as unknown.
fn queued_instead(steer: &AgentSteer) -> Option<AgentSteer> {
    let AgentSteer::Steer { text, images } = steer else {
        return None;
    };
    Some(if images.is_empty() {
        AgentSteer::Prompt { text: text.clone() }
    } else {
        AgentSteer::PromptImages {
            text: text.clone(),
            images: images.clone(),
        }
    })
}

/// True when the server did not know `Steer`, op 5 of `AgentSteer`.
fn older_than_steer(error: &DesktopError) -> bool {
    matches!(error, DesktopError::Api(ApiError::BadRequest { message }) if message.contains("unknown op 5") || message.contains("unknown op 6"))
}

/// True when the server did not know `PromptImages`: one from before it
/// reads op 3 of `AgentSteer` as unknown.
fn older_than_images(error: &DesktopError) -> bool {
    matches!(error, DesktopError::Api(ApiError::BadRequest { message }) if message.contains("unknown op 3"))
}

/// True when the server did not know `Configure`, op 4 of `AgentSteer`.
fn older_than_configure(error: &DesktopError) -> bool {
    matches!(error, DesktopError::Api(ApiError::BadRequest { message }) if message.contains("unknown op 4"))
}

#[tauri::command]
pub async fn agent_hosts(beep: Service<'_>) -> Result<Vec<HostRow>> {
    let hosts = beep.call(ListAgentHosts {}).await?;
    Ok(hosts.into_iter().map(HostRow::from).collect())
}

#[tauri::command]
pub async fn agent_sessions(beep: Service<'_>) -> Result<Vec<SessionRow>> {
    let sessions = beep.call(ListAgentSessions {}).await?;
    Ok(sessions.into_iter().map(SessionRow::from).collect())
}

/// Starts a session; no `machine_id` lets the server pick. `chat` hears
/// its turn ends and questions; `model`, from `agent_models`, picks the
/// agent's model, `effort` how hard it thinks and `checkout` the new
/// branch or worktree it starts on.
// The arguments are the fields of the window's `invoke` call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn agent_start(
    beep: Service<'_>,
    machine_id: Option<String>,
    agent: Option<String>,
    cwd: Option<String>,
    prompt: String,
    chat: Option<Uuid>,
    model: Option<String>,
    effort: Option<Effort>,
    checkout: Option<CheckoutPlan>,
) -> Result<Started> {
    let started = beep
        .call(StartAgentSession {
            machine_id,
            agent,
            cwd,
            prompt,
            needs: Vec::new(),
            chat,
            model,
            effort: effort.map(Into::into),
            checkout: checkout.map(Into::into),
        })
        .await?;
    Ok(Started {
        session: started.session.into(),
        why: started.why,
    })
}

/// The machine Auto would pick, as the start form shows it.
#[derive(Debug, Serialize)]
pub struct PickView {
    machine_id: String,
    machine_name: String,
    why: String,
}

impl From<MachinePick> for PickView {
    fn from(pick: MachinePick) -> Self {
        Self {
            machine_id: pick.machine_id,
            machine_name: pick.machine_name,
            why: pick.why,
        }
    }
}

/// Where Auto would start the form's session now; `None` from a server
/// too old to say. A start no machine fits is an error saying why.
#[tauri::command]
pub async fn agent_preview(
    beep: Service<'_>,
    agent: Option<String>,
    cwd: Option<String>,
    prompt: String,
) -> Result<Option<PickView>> {
    let asked = beep
        .call(PreviewAgentMachine {
            agent,
            cwd,
            prompt,
            needs: Vec::new(),
        })
        .await;
    match asked {
        Ok(pick) => Ok(Some(pick.into())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The models an agent can run on a machine, as the start form's Model
/// field lists them.
#[derive(Debug, Serialize)]
pub struct ModelsView {
    /// "listed", "named" or "fixed".
    pick: &'static str,
    current: Option<String>,
    models: Vec<ModelView>,
}

#[derive(Debug, Serialize)]
pub struct ModelView {
    id: String,
    name: String,
    description: Option<String>,
}

impl From<AgentModels> for ModelsView {
    fn from(list: AgentModels) -> Self {
        Self {
            pick: list.pick.into(),
            current: list.current,
            models: list.models.into_iter().map(ModelView::from).collect(),
        }
    }
}

impl From<AgentModel> for ModelView {
    fn from(model: AgentModel) -> Self {
        Self {
            id: model.id,
            name: model.name,
            description: model.description,
        }
    }
}

/// The models `agent` can run on `machine_id`; `None` from a server too
/// old to say, which the form takes as none to pick.
#[tauri::command]
pub async fn agent_models(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
) -> Result<Option<ModelsView>> {
    match beep.call(ListAgentModels { machine_id, agent }).await {
        Ok(list) => Ok(Some(list.into())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Resumes a session that ran outside Beep, as a project lists it; it then
/// runs as one Beep started.
#[tauri::command]
pub async fn agent_resume(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
    session: String,
    prompt: Option<String>,
) -> Result<Started> {
    let started = beep
        .call(ResumeAgentSession {
            machine_id,
            agent,
            session,
            prompt,
        })
        .await?;
    Ok(Started {
        session: started.session.into(),
        why: started.why,
    })
}

/// One session in full. The calling webview's session view shows it, so
/// its notices stay quiet while the view keeps reading it.
#[tauri::command]
pub async fn agent_view(
    window: WebviewWindow,
    beep: Service<'_>,
    shown: State<'_, AgentsShown>,
    machine_id: String,
    agent: String,
    session: String,
    last: u32,
) -> Result<SessionPage> {
    shown.read(
        window.label(),
        &machine_id,
        &agent,
        &session,
        Instant::now(),
    );
    let view = beep
        .call(GetAgentSession {
            machine_id,
            agent,
            session,
            last,
        })
        .await?;
    Ok(view.into())
}

/// How full a Beep session's context is and what its finished turns
/// used; `None` from a server too old to say.
#[tauri::command]
pub async fn agent_usage(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
    session: String,
) -> Result<Option<AgentSessionUsage>> {
    let usage = beep
        .call(GetAgentSessionUsage {
            machine_id,
            agent,
            session,
        })
        .await;
    match usage {
        Ok(usage) => Ok(Some(usage)),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The picture of the `[image: image-<key>]` line of `session`, read from
/// its machine through the server, as a data URL the bar can draw.
#[tauri::command]
pub async fn agent_image(
    beep: Service<'_>,
    machine_id: String,
    session: String,
    key: String,
) -> Result<String> {
    let image = beep
        .call(GetAgentImage {
            machine_id,
            session,
            key,
        })
        .await;
    let image = match image {
        Err(DesktopError::Api(ApiError::NotFound {})) => {
            return Err(DesktopError::BadRequest(GONE.into()));
        }
        other => other?,
    };
    if !image_mime(&image.mime) {
        return Err(DesktopError::BadRequest("bad picture".into()));
    }
    Ok(format!(
        "data:{};base64,{}",
        image.mime,
        STANDARD.encode(&image.data)
    ))
}

/// Part of a media file on a machine, for the bar's media pane: `data`
/// is base64, and the bar asks again from `offset` plus its length until
/// it has `size` bytes.
#[derive(Debug, Serialize)]
pub struct AgentFilePart {
    size: u64,
    mime: String,
    data: String,
}

/// Up to `AGENT_FILE_CHUNK` bytes of the media file `path` on `machine_id`
/// from `offset`, read from its machine through the server.
#[tauri::command]
pub async fn agent_file(
    beep: Service<'_>,
    machine_id: String,
    path: String,
    offset: u64,
) -> Result<AgentFilePart> {
    let chunk = file_chunk(&beep, machine_id, path, offset).await?;
    Ok(AgentFilePart {
        size: chunk.size,
        mime: chunk.mime,
        data: STANDARD.encode(&chunk.data),
    })
}

/// Opens the media file `path` on `machine_id` in the Mac's own app for
/// it, read whole into the cache first.
#[tauri::command]
pub async fn agent_file_open(
    app: AppHandle,
    beep: Service<'_>,
    machine_id: String,
    path: String,
) -> Result<()> {
    let data = whole_file(&beep, &machine_id, &path).await?;
    let dir = app.path().app_cache_dir()?.join("agent-files");
    fs::create_dir_all(&dir).map_err(anyhow::Error::from)?;
    let of = image_key(format!("{machine_id}\n{path}").as_bytes());
    let to = dir.join(format!("{of}-{}", file_name(&path)));
    fs::write(&to, &data).map_err(anyhow::Error::from)?;
    Command::new("open")
        .arg(&to)
        .spawn()
        .map_err(anyhow::Error::from)?;
    Ok(())
}

/// Saves the media file `path` on `machine_id` to Downloads, under a name
/// not taken there yet, and returns where it went.
#[tauri::command]
pub async fn agent_file_save(
    app: AppHandle,
    beep: Service<'_>,
    machine_id: String,
    path: String,
) -> Result<PathBuf> {
    let data = whole_file(&beep, &machine_id, &path).await?;
    let to = free_name(&app.path().download_dir()?, &file_name(&path));
    fs::write(&to, &data).map_err(anyhow::Error::from)?;
    Ok(to)
}

/// One `GetAgentFile`, with a file the machine no longer has as `GONE`.
async fn file_chunk(
    beep: &Beep,
    machine_id: String,
    path: String,
    offset: u64,
) -> Result<AgentFileChunk> {
    let len = u32::try_from(AGENT_FILE_CHUNK).map_err(anyhow::Error::from)?;
    if let Some(chunk) = work_copy::read(beep, &machine_id, &path, offset, len).await {
        return Ok(chunk);
    }
    let chunk = beep
        .call(GetAgentFile {
            machine_id,
            path,
            offset,
        })
        .await;
    match chunk {
        Err(DesktopError::Api(ApiError::NotFound {})) => Err(DesktopError::BadRequest(GONE.into())),
        other => other,
    }
}

/// The whole media file `path` on `machine_id`, read a chunk at a time.
async fn whole_file(beep: &Beep, machine_id: &str, path: &str) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    loop {
        let offset = u64::try_from(data.len()).map_err(anyhow::Error::from)?;
        let chunk = file_chunk(beep, machine_id.to_owned(), path.to_owned(), offset).await?;
        if chunk.size > MAX_AGENT_FILE || chunk.data.len() > AGENT_FILE_CHUNK {
            return Err(DesktopError::BadRequest("file too large".into()));
        }
        data.extend_from_slice(&chunk.data);
        let have = u64::try_from(data.len()).map_err(anyhow::Error::from)?;
        if have >= chunk.size {
            data.truncate(usize::try_from(chunk.size).map_err(anyhow::Error::from)?);
            return Ok(data);
        }
        if chunk.data.is_empty() {
            return Err(DesktopError::BadRequest(
                "the file changed while read".into(),
            ));
        }
    }
}

#[tauri::command]
pub async fn agent_steer(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
    session: String,
    steer: Steer,
) -> Result<Vec<String>> {
    let steer = AgentSteer::try_from(steer)?;
    // The keys of the images sent, which the session's log names them by.
    let keys: Vec<String> = match &steer {
        AgentSteer::PromptImages { images, .. } | AgentSteer::Steer { images, .. } => {
            images.iter().map(|image| image_key(&image.data)).collect()
        }
        _ => Vec::new(),
    };
    let images = !keys.is_empty();
    let configure = matches!(steer, AgentSteer::Configure { .. });
    let fallback = queued_instead(&steer);
    let call = |steer: AgentSteer| SteerAgentSession {
        machine_id: machine_id.clone(),
        agent: agent.clone(),
        session: session.clone(),
        steer,
    };
    let sent = match (beep.call(call(steer)).await, fallback) {
        // A server from before steering takes it as a message for after
        // the turn, which is the nearest it can do.
        (Err(e), Some(prompt)) if older_than_steer(&e) => beep.call(call(prompt)).await,
        (sent, _) => sent,
    };
    match sent {
        Err(e) if images && older_than_images(&e) => Err(DesktopError::BadRequest(
            "This Beep server does not take images yet. Send the message without them.".into(),
        )),
        Err(e) if configure && older_than_configure(&e) => Err(DesktopError::BadRequest(
            "This Beep server cannot switch a running session's model yet.".into(),
        )),
        Err(e) if older_than_steer(&e) => Err(DesktopError::BadRequest(
            "This Beep server cannot take a message back yet.".into(),
        )),
        other => other.map(|()| keys),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasted(data: &[u8]) -> PastedImage {
        PastedImage {
            name: "shot.png".into(),
            mime: "image/png".into(),
            data: STANDARD.encode(data),
        }
    }

    #[test]
    fn a_message_without_images_goes_as_text() {
        let steer = Steer::Prompt {
            text: "hi".into(),
            images: Vec::new(),
        };
        assert_eq!(
            AgentSteer::try_from(steer).unwrap(),
            AgentSteer::Prompt { text: "hi".into() }
        );
    }

    #[test]
    fn images_go_as_prompt_images_within_the_limits() {
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        let steer = Steer::Prompt {
            text: "look".into(),
            images: vec![pasted(&png)],
        };
        let AgentSteer::PromptImages { images, .. } = AgentSteer::try_from(steer).unwrap() else {
            panic!("an image goes as PromptImages");
        };
        assert_eq!(images[0].data, png);
        let six = Steer::Prompt {
            text: "look".into(),
            images: (0..6).map(|_| pasted(&png)).collect(),
        };
        assert!(AgentSteer::try_from(six).is_err());
    }

    #[test]
    fn a_steer_carries_its_images_and_falls_back_to_a_prompt() {
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        let steer = AgentSteer::try_from(Steer::InTurn {
            text: "use the fixture".into(),
            images: vec![pasted(&png)],
        })
        .unwrap();
        let AgentSteer::Steer { images, .. } = &steer else {
            panic!("a steer goes as Steer");
        };
        assert_eq!(images[0].data, png);
        assert!(matches!(
            queued_instead(&steer),
            Some(AgentSteer::PromptImages { text, .. }) if text == "use the fixture"
        ));
        let old = DesktopError::Api(ApiError::bad_request("unknown op 5"));
        assert!(older_than_steer(&old));
    }

    #[test]
    fn an_old_server_is_told_apart() {
        let old = DesktopError::Api(ApiError::bad_request("unknown op 3"));
        assert!(older_than_images(&old));
        assert!(!older_than_images(&DesktopError::Api(
            ApiError::bad_request("The message is empty.")
        )));
    }

    #[test]
    fn image_lines_stay_out_of_the_agents_html() {
        let said = EntryRow::from(AgentEntry {
            kind: AgentEntryKind::Agent {},
            text: "Saved chart.png\n[image: image-0123456789abcdef]".into(),
        });
        let html = said.html.unwrap();
        assert!(html.contains("Saved chart.png"), "{html}");
        assert!(!html.contains("image-0123"), "{html}");
        assert!(said.text.ends_with("[image: image-0123456789abcdef]"));
    }

    #[test]
    fn what_the_agent_says_comes_as_clean_html() {
        let said = EntryRow::from(AgentEntry {
            kind: AgentEntryKind::Agent {},
            text: "See [sqlx #19](https://example.com/19) and `cargo test`.<script>x()</script>"
                .into(),
        });
        let html = said.html.expect("an agent entry carries its HTML");
        assert!(
            html.contains(r#"<a href="https://example.com/19""#),
            "{html}"
        );
        assert!(html.contains("<code>cargo test</code>"), "{html}");
        assert!(!html.contains("<script"), "{html}");
        let tool = EntryRow::from(AgentEntry {
            kind: AgentEntryKind::Tool {},
            text: "Read file [completed]".into(),
        });
        assert_eq!(tool.html, None);
    }

    #[test]
    fn a_thought_comes_as_html_with_its_time() {
        let thought = EntryRow::from(AgentEntry {
            kind: AgentEntryKind::Thought { ms: Some(12_000) },
            text: "**Reading** the `tests`".into(),
        });
        assert_eq!(thought.kind, "thought");
        assert_eq!(thought.ms, Some(12_000));
        let html = thought.html.expect("a thought carries its HTML");
        assert!(html.contains("<strong>Reading</strong>"), "{html}");
    }
}
