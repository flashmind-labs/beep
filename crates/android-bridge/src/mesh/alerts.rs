//! Questions and finished turns the machines of the mesh tell, as the phone shows them.

use crate::{AsyncMutex, BeepClient, BeepError};
use beep_fleet::{Peers, alerts::Heard};
use beep_proto::{RunState, node::agent::OptionKind};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NoticeKind {
    Ask,
    Finished,
    Run,
}

/// One answer the machine offers to a question.
#[derive(Debug, uniffi::Record)]
pub struct AskOption {
    pub id: String,
    pub name: String,
    pub allow: bool,
}

#[derive(Debug, uniffi::Record)]
pub struct MobileNotice {
    pub id: String,
    pub kind: NoticeKind,
    pub title: String,
    pub detail: String,
    pub machine_id: String,
    pub agent: String,
    pub session: String,
    pub question: Option<u32>,
    pub options: Vec<AskOption>,
    pub at_ms: u64,
}

/// The live feed. Dropping it closes the streams to the machines.
#[derive(uniffi::Object)]
pub struct AskFeed {
    heard: AsyncMutex<mpsc::Receiver<Heard>>,
    task: JoinHandle<()>,
}

impl Drop for AskFeed {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl AskFeed {
    /// The next question, finished turn or ended build; `None` once the feed ends.
    pub async fn next(&self) -> Result<Option<MobileNotice>, BeepError> {
        let mut heard = self.heard.lock().await;
        while let Some(told) = heard.recv().await {
            if let Some(notice) = mobile(told) {
                return Ok(Some(notice));
            }
        }
        Ok(None)
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl BeepClient {
    /// Follows the alerts and runs of every machine of the workspace.
    pub async fn follow_asks(&self) -> Result<Arc<AskFeed>, BeepError> {
        let Some(mesh) = self.mesh().await? else {
            self.client()?;
            return Err(crate::bad_request(
                "Link a desktop workspace to follow asks.",
            ));
        };
        let peers: Arc<dyn Peers> = mesh.clone();
        let (following, heard) = beep_fleet::alerts::follow(peers, mesh.mesh.clone());
        Ok(Arc::new(AskFeed {
            heard: AsyncMutex::new(heard),
            task: tokio::spawn(following),
        }))
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| u64::try_from(since.as_millis()).ok())
        .unwrap_or_default()
}

fn mobile(told: Heard) -> Option<MobileNotice> {
    match told {
        Heard::Connected => None,
        Heard::Notice {
            key,
            notice,
            options,
        } => Some(MobileNotice {
            id: notice.session_id,
            kind: if key.question.is_some() {
                NoticeKind::Ask
            } else {
                NoticeKind::Finished
            },
            title: notice.title,
            detail: notice.body,
            machine_id: key.machine_id,
            agent: key.agent,
            session: key.session,
            question: key.question,
            options: options
                .into_iter()
                .map(|option| AskOption {
                    allow: matches!(option.kind, OptionKind::AllowOnce | OptionKind::AllowAlways),
                    id: option.id,
                    name: option.name,
                })
                .collect(),
            at_ms: now_ms(),
        }),
        Heard::Run(news) => {
            let run = &news.run;
            let ended = match run.state {
                RunState::Passed {} => "passed",
                RunState::Failed {} => "failed",
                RunState::Cancelled {} => "cancelled",
                _ => return None,
            };
            let title = if run.branch.is_empty() {
                format!("Build {ended}")
            } else {
                format!("Build {ended} on {}", run.branch)
            };
            let detail = match run.number {
                0 => run.workflow.clone(),
                number => format!("{} #{number}", run.workflow),
            };
            Some(MobileNotice {
                id: format!("run:{}:{}:{}", news.project, news.host, run.id),
                kind: NoticeKind::Run,
                title,
                detail: format!("{}: {detail}", news.project),
                machine_id: String::new(),
                agent: String::new(),
                session: String::new(),
                question: None,
                options: Vec::new(),
                at_ms: now_ms(),
            })
        }
    }
}
