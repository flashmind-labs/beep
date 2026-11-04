//! Postgres: `Store` and its queries, one file per kind of row, and the migrations list.

mod memory_rows;
mod migrations;
pub(crate) use memory_rows::{MEMORY_CATEGORIES, UNFILED};
pub use memory_rows::{MemoryRow, Origin, Validity};
use migrations::{MIGRATIONS, migrate};

use anyhow::Result;
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use flashmind::memory::{EmbeddingProvider, MemoryStore};
use secrecy::{ExposeSecret, SecretString};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::sync::Arc;
use tokio::sync::Notify;

mod actions;
mod agent_images;
mod agent_session_starts;
pub(crate) use agent_session_starts::SessionOn;
mod agent_spend;
mod agent_usage;
mod apps;
mod backfill;
mod blobs;
mod chat_allows;
pub(crate) use chat_allows::ChatAllow;
mod chats;
mod credentials;
mod cron_jobs;
mod crons;
mod devices;
mod digest;
mod events;
mod feeds;
mod files;
mod findings;
mod flsh_settings;
mod grants;
mod groups;
mod habits;
mod harness_settings;
mod instance;
mod instructions;
pub(crate) use habits::DuplicateHabit;
pub(crate) use instructions::{InstructionsRow, Saved};
pub(crate) use reviews::{
    Judged, PrRow, ReviewFindingRow, ReviewRow, ShardRow, ShardState, Verdict,
};
pub(crate) use terminals::TerminalRow;
mod location;
mod memories;
mod merge;
mod messages;
mod node_tokens;
mod node_traffic;
mod nodes;
mod places;
mod profile;
mod project_groups;
mod project_imports;
mod project_removal;
mod project_sync;
mod project_webhooks;
pub(crate) use project_webhooks::{NewWebhook, OrphanWebhooks};
mod projects;
mod research;
mod reviews;
mod routes;
mod runs;
mod sessions;
mod shares;
mod skills;
mod sources;
mod terminals;
mod thread_chats;
pub(crate) use thread_chats::{NewThreadChat, ThreadChatRow};
mod test_db;
mod update_chats;
pub(crate) use update_chats::{UpdateChat, UpdateChatSeed};
pub use update_chats::{UpdateDetail, UpdateEvent};
mod updates;
mod usage;
mod user_keys;
mod users;
mod watchers;

pub use actions::ActionRow;
pub(crate) use agent_images::{AgentImageBytes, AgentImageOf};
pub(crate) use agent_spend::{DayModelRow, DaySpendRow, Periods};
pub(crate) use agent_usage::{ModelRow, PlanRow, SpendRow};
pub(crate) use apps::AppPermissionRow;
pub use apps::{CustomMcp, McpServerRow};
pub use blobs::{Blobs, PgBlobs};
pub(crate) use credentials::ApiKey;
pub use credentials::{
    AgentTokenRow, CredentialRow, GitHubAccountRow, GitLogin, NewCredential, NewGitHubAccount,
};
pub(crate) use cron_jobs::{decode_schedule, encode_schedule};
pub use devices::{PushDevice, PushService, PushToken};
pub use digest::AttentionItem;
pub use events::{EventRow, EventState, NewTriage, RecentEvent, TriageRow, TriggerRow, UpdateRow};
pub use feeds::Feed;
pub use files::{NewAttachment, NewFile, NewPicture, PicturesOf};
pub use findings::{FindingRow, NewFinding, SeenIn, Sighting};
pub(crate) use grants::{GrantRow, NewGrant};
pub use groups::{GroupRow, Join, JoinedGroup, MemberRow, MessageRow, NewMessage, Posted};
pub(crate) use harness_settings::{HarnessRow, OffHarness, SaveHarness};
pub use location::Location;
pub(crate) use memories::MAX_ROWS as MAX_MEMORY_ROWS;
pub use memories::{Fact, Scope, Scored, Superseded, WordEmbedder};
pub use merge::{Invite, Inviter, Merge, MergeInto};
pub use messages::{Role, Step, StepBody, StoredBody, StoredMessage, TurnRecord};
pub use node_tokens::{Claim, ClaimNode, ClaimedNode};
pub use nodes::{NewNode, NodeRow};
pub use places::{Crossed, NewPlace, NewPlaceRoutine, PlaceRoutine, SavedPlace};
pub use profile::{ProfileNote, ProfileRead};
pub(crate) use project_groups::SaveGroup;
pub use project_imports::{ImportRow, NewImport};
pub(crate) use project_removal::{DroppedMachine, RemovedProject};
pub use project_sync::SyncRow;
pub(crate) use projects::{
    After, CheckoutRow, IssueAuthor, NewTask, NewWorker, OpenCounts, OverviewRow, ProjectRow,
    StartRefused, TaskRow, TrackedIssue, WorkerRow,
};
pub use research::{EntryRow, ListedResearch, NewSource, ResearchRow, SourceRow};
pub use runs::{CreateRun, FinishedTurn, RunStatus, SavedTurn};
pub use sessions::{
    DeletedTurn, FoundSession, QuietSession, SessionRef, StoredContext, TurnsToDelete,
};
pub use shares::{CopiedShare, Share};
pub use skills::{NewSkill, Review, SKILL_LIMIT, SkillOwner, SkillPatch, SkillRow};
pub use test_db::{TestDb, TestTurn, test_chat, test_steps, test_turn, test_turn_with};
pub use updates::{Bumped, UpdateHub, start_pruning};
pub use user_keys::{Account, AccountKey, LinkKey, LiveKey, NewKey, SignIn};

/// Most connections one server process opens.
const MAX_CONNECTIONS: u32 = 10;

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
    /// Every user's memories, each tagged with its owner.
    memories: MemoryStore,
    /// Woken after any write to `crons`, so the scheduler picks the change up.
    crons_changed: Arc<Notify>,
    /// The bytes of kept files and pictures.
    blobs: Arc<dyn Blobs>,
    /// Wakes the update streams when a user's log moves.
    updates: Arc<UpdateHub>,
}

fn parse_date(text: Option<&str>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text?, "%Y-%m-%d").ok()
}

fn format_date(date: Option<NaiveDate>) -> Option<String> {
    date.map(|d| d.format("%Y-%m-%d").to_string())
}

/// RFC 3339 in UTC to the second, as clients get times.
fn format_time(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Secs, true)
}

impl Store {
    /// Connects to `url`, migrates the schema and opens the memory store.
    pub async fn open(url: &SecretString, embedder: Arc<dyn EmbeddingProvider>) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .connect(url.expose_secret())
            .await?;
        Self::with_pool(pool, embedder).await
    }

    /// Runs the migrations on `pool` and opens the store on it.
    pub async fn with_pool(pool: PgPool, embedder: Arc<dyn EmbeddingProvider>) -> Result<Self> {
        migrate(&pool).await?;
        let memories = MemoryStore::connect_postgres(pool.clone(), embedder).await?;
        Ok(Self {
            blobs: Arc::new(PgBlobs::new(pool.clone())),
            pool,
            memories,
            crons_changed: Arc::new(Notify::new()),
            updates: Arc::default(),
        })
    }

    /// Woken after any write to `crons`.
    pub fn crons_changed(&self) -> Arc<Notify> {
        self.crons_changed.clone()
    }

    /// Wakes the scheduler after a `crons` change; `notify_one` keeps a permit so no write is lost.
    pub(crate) fn notify_crons_changed(&self) {
        self.crons_changed.notify_one();
    }

    #[cfg(test)]
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::MIGRATIONS;
    use std::fs;

    #[test]
    fn every_migration_file_is_listed() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/migrations");
        let mut files: Vec<i64> = fs::read_dir(dir)
            .expect("the migrations folder is in the crate")
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".sql")?.split('_').next()?.parse().ok()
            })
            .collect();
        files.sort_unstable();
        let listed: Vec<i64> = MIGRATIONS.iter().map(|(version, ..)| *version).collect();
        assert_eq!(listed, files);
    }
}
