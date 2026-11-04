//! The user's runs and research papers.

use super::User;
use crate::{
    protocol::Run,
    research::pdf::{self, Paper, Pdf},
    store::{CreateRun, ListedResearch, ResearchRow},
};
use anyhow::Result;
use chrono::Utc;
use uuid::Uuid;

impl User {
    /// Starts a run for `message` in `session`; a repeated `request` returns its existing run.
    pub async fn create_run(
        &self,
        session: Uuid,
        request: &str,
        message: &str,
    ) -> Result<CreateRun> {
        self.store
            .create_run(&self.key, session, request, message)
            .await
    }

    /// Shows the prompt of `run`, just started, on every device while it runs.
    pub async fn save_prompt(&self, run: &Run, prompt: &str) -> Result<()> {
        self.store.save_prompt(&self.key, run, prompt).await?;
        self.changed(&run.session_id);
        Ok(())
    }

    /// The run going on in `session`, if any.
    pub async fn active_run(&self, session: Uuid) -> Result<Option<Run>> {
        self.store.active_run(&self.key, session).await
    }

    /// The runs going on in any of the user's sessions.
    pub async fn active_runs(&self) -> Result<Vec<Run>> {
        self.store.active_runs(&self.key).await
    }

    /// The run already started for `request`, without starting one.
    pub async fn existing_run(
        &self,
        session: Uuid,
        request: &str,
        message: &str,
    ) -> Result<Option<CreateRun>> {
        self.store
            .run_for_request(self.key.as_str(), session, request, Some(message))
            .await
    }

    /// The run already started for `request` in `session`, whatever its message.
    pub(crate) async fn run_for_spoken(
        &self,
        session: Uuid,
        request: &str,
    ) -> Result<Option<CreateRun>> {
        self.store
            .run_for_request(self.key.as_str(), session, request, None)
            .await
    }

    /// The research `chat` runs, if it is one.
    pub async fn research_in(&self, chat: Uuid) -> Result<Option<ResearchRow>> {
        self.store.research_in(&self.key, chat).await
    }

    /// The user's researches, newest first.
    pub(crate) async fn researches(&self) -> Result<Vec<ListedResearch>> {
        self.store.list_researches(&self.key).await
    }

    /// The research paper of `chat` as a PDF; `None` when there is none yet.
    pub(crate) async fn research_pdf(&self, chat: Uuid) -> Result<Option<Pdf>> {
        let Some(research) = self.research_in(chat).await? else {
            return Ok(None);
        };
        if research.draft.trim().is_empty() {
            return Ok(None);
        }
        let sources = self.store.research_sources(research.id).await?;
        let paper = Paper {
            topic: research.topic,
            draft: research.draft,
            sources,
            date: Utc::now().date_naive(),
        };
        pdf::render(paper).await.map(Some)
    }

    /// Makes `chat` a research on `topic`; a chat that is one keeps its own.
    pub async fn start_research(&self, chat: Uuid, topic: &str) -> Result<ResearchRow> {
        self.store.start_research(&self.key, chat, topic).await
    }

    /// The run with `id`, if it is the user's.
    pub async fn run(&self, id: &str) -> Result<Option<Run>> {
        self.store.run(&self.key, id).await
    }
}
