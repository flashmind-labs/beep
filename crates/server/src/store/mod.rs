use crate::protocol::MemorySource;
use anyhow::Result;
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use flashmind::memory::{EmbeddingProvider, MemoryStore};
use secrecy::{ExposeSecret, SecretString};
use sqlx::{PgPool, postgres::PgPoolOptions};
use sqlx_core::migrate::{Migration, MigrationType, Migrator};
use std::{borrow::Cow, sync::Arc, time::Duration};
use tokio::sync::Notify;

mod actions;
mod apps;
mod backfill;
mod blobs;
mod chats;
mod cron_jobs;
mod crons;
mod devices;
mod digest;
mod feeds;
mod files;
mod groups;
mod habits;
pub(crate) use habits::DuplicateHabit;
mod location;
mod memories;
mod merge;
mod messages;
mod places;
mod profile;
mod routes;
mod runs;
mod sessions;
mod settings;
mod shares;
mod skills;
mod test_db;
mod updates;
mod usage;
mod user_keys;
mod users;

pub use actions::ActionRow;
pub(crate) use apps::AppPermissionRow;
pub use apps::{CustomMcp, McpServerRow};
pub use blobs::{Blobs, PgBlobs};
pub(crate) use cron_jobs::{decode_schedule, encode_schedule};
pub use devices::PushDevice;
pub use digest::AttentionItem;
pub use feeds::Feed;
pub use files::{NewAttachment, NewFile, NewPicture, PicturesOf};
pub use groups::{GroupRow, Join, JoinedGroup, MemberRow, MessageRow, NewMessage, Posted};
pub use location::Location;
pub use memories::{Fact, Scored, WordEmbedder};
pub use merge::{Invite, Inviter, Merge, MergeInto};
pub use messages::{Role, Step, StepBody, StoredBody, StoredMessage, TurnAgentRow, TurnRecord};
pub use places::{Crossed, NewPlace, NewPlaceRoutine, PlaceRoutine, SavedPlace};
pub use profile::{ProfileNote, ProfileRead};
pub use runs::{CreateRun, FinishedTurn, RunStatus, SavedTurn};
pub use sessions::{DeletedTurn, FoundSession, SessionRef, StoredContext, TurnsToDelete};
pub use shares::{CopiedShare, Share};
pub use skills::{NewSkill, Review, SKILL_LIMIT, SkillOwner, SkillPatch, SkillRow};
pub use test_db::{TestDb, test_chat, test_steps, test_turn};
pub use updates::{Bumped, UpdateHub, start_pruning};
pub use user_keys::{Account, AccountKey, LinkKey, NewKey, SignIn};

/// The schema, oldest first. Embedded by hand because `sqlx::migrate!`
/// needs sqlx's `migrate` feature, which clashes with
/// flashmind's rusqlite.
const MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "init", include_str!("../../migrations/0001_init.sql")),
    (2, "users", include_str!("../../migrations/0002_users.sql")),
    (
        3,
        "accounts",
        include_str!("../../migrations/0003_accounts.sql"),
    ),
    (
        4,
        "cron_sessions",
        include_str!("../../migrations/0004_cron_sessions.sql"),
    ),
    (
        5,
        "app_permissions",
        include_str!("../../migrations/0005_app_permissions.sql"),
    ),
    (
        6,
        "drop_unused",
        include_str!("../../migrations/0006_drop_unused.sql"),
    ),
    (
        7,
        "user_model",
        include_str!("../../migrations/0007_user_model.sql"),
    ),
    (
        8,
        "digest",
        include_str!("../../migrations/0008_digest.sql"),
    ),
    (
        9,
        "turn_models",
        include_str!("../../migrations/0009_turn_models.sql"),
    ),
    (
        10,
        "drop_notifications",
        include_str!("../../migrations/0010_drop_notifications.sql"),
    ),
    (
        11,
        "user_thinking",
        include_str!("../../migrations/0011_user_thinking.sql"),
    ),
    (
        12,
        "token_usage",
        include_str!("../../migrations/0012_token_usage.sql"),
    ),
    (
        13,
        "auto_model",
        include_str!("../../migrations/0013_auto_model.sql"),
    ),
    (
        14,
        "actions",
        include_str!("../../migrations/0014_actions.sql"),
    ),
    (
        15,
        "account_keys",
        include_str!("../../migrations/0015_account_keys.sql"),
    ),
    (
        16,
        "user_keys",
        include_str!("../../migrations/0016_user_keys.sql"),
    ),
    (
        17,
        "shares",
        include_str!("../../migrations/0017_shares.sql"),
    ),
    (
        18,
        "user_timezone",
        include_str!("../../migrations/0018_user_timezone.sql"),
    ),
    (
        19,
        "groups",
        include_str!("../../migrations/0019_groups.sql"),
    ),
    (
        20,
        "group member names",
        include_str!("../../migrations/0020_group_member_names.sql"),
    ),
    (
        21,
        "group invite codes",
        include_str!("../../migrations/0021_group_invite_codes.sql"),
    ),
    (
        22,
        "profile",
        include_str!("../../migrations/0022_profile.sql"),
    ),
    (
        23,
        "group pins",
        include_str!("../../migrations/0023_group_pins.sql"),
    ),
    (
        24,
        "group shared apps",
        include_str!("../../migrations/0024_group_shared_apps.sql"),
    ),
    (25, "files", include_str!("../../migrations/0025_files.sql")),
    (
        26,
        "group reminders",
        include_str!("../../migrations/0026_group_reminders.sql"),
    ),
    (
        27,
        "places",
        include_str!("../../migrations/0027_places.sql"),
    ),
    (
        28,
        "pictures",
        include_str!("../../migrations/0028_pictures.sql"),
    ),
    (
        29,
        "messages",
        include_str!("../../migrations/0029_messages.sql"),
    ),
    (
        30,
        "drop transcripts",
        include_str!("../../migrations/0030_drop_transcripts.sql"),
    ),
    (
        31,
        "skills",
        include_str!("../../migrations/0031_skills.sql"),
    ),
    (
        32,
        "place once",
        include_str!("../../migrations/0032_place_once.sql"),
    ),
    (
        33,
        "agents",
        include_str!("../../migrations/0033_agents.sql"),
    ),
    (
        34,
        "account invites",
        include_str!("../../migrations/0034_account_invites.sql"),
    ),
    (
        35,
        "cron agents",
        include_str!("../../migrations/0035_cron_agents.sql"),
    ),
    (
        36,
        "usage agents",
        include_str!("../../migrations/0036_usage_agents.sql"),
    ),
    (
        37,
        "route cards",
        include_str!("../../migrations/0037_route_cards.sql"),
    ),
    (
        38,
        "custom mcp",
        include_str!("../../migrations/0038_custom_mcp.sql"),
    ),
    (
        39,
        "run prompts",
        include_str!("../../migrations/0039_run_prompts.sql"),
    ),
];

/// The last migration before the transcripts go. The backfill reads them
/// into message rows between this one and the next.
const BEFORE_BACKFILL: i64 = 29;

/// The migrations up to `last`.
fn migrator(last: i64) -> Migrator {
    let migrations = MIGRATIONS
        .iter()
        .filter(|&&(version, ..)| version <= last)
        .map(|&(version, description, sql)| {
            Migration::new(
                version,
                Cow::Borrowed(description),
                MigrationType::Simple,
                Cow::Borrowed(sql),
                false,
            )
        })
        .collect();
    Migrator {
        migrations: Cow::Owned(migrations),
        // A database already past `last` has migrations this one leaves out.
        ignore_missing: true,
        ..Migrator::DEFAULT
    }
}

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

/// The categories Jev files a memory under, with what each holds.
pub(crate) const MEMORY_CATEGORIES: &[&str] = &[
    "identity",
    "people",
    "preference",
    "work",
    "place",
    "plan",
    "routine",
];

/// What a memory is filed under until Jev files it, such as when Jev was
/// unreachable. The hourly sweep files these and any old category.
pub(crate) const UNFILED: &str = "unfiled";

/// The days a fact holds, like the dates of a trip. Both open by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Validity {
    pub from: Option<NaiveDate>,
    pub until: Option<NaiveDate>,
}

impl Validity {
    /// Whether the fact no longer holds on `today`.
    pub fn ended(&self, today: NaiveDate) -> bool {
        self.until.is_some_and(|until| until < today)
    }
}

/// Where a memory was picked up: the chat session, email or Slack thread it
/// came from, and the words there that show it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origin {
    /// The session id, email id or Slack thread id, by the memory's source.
    pub reference: String,
    pub quote: Option<String>,
}

/// A stored memory.
#[derive(Debug, Clone)]
pub struct MemoryRow {
    pub id: String,
    pub content: String,
    pub category: String,
    pub source: MemorySource,
    /// `YYYY-MM-DD HH:MM:SS` in UTC.
    pub updated_at: String,
    pub valid: Validity,
    pub origin: Option<Origin>,
    /// Picked by curation as part of the profile every chat starts with.
    pub core: bool,
}

impl MemoryRow {
    /// `YYYY-MM-DD` of the last change.
    pub fn updated(&self) -> &str {
        self.updated_at.get(..10).unwrap_or_default()
    }
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

/// `duration` in seconds, for `make_interval(secs => ...)`.
fn seconds(duration: Duration) -> f64 {
    duration.as_secs_f64()
}

impl Store {
    /// Connects to the database at `url`, brings its schema up to date and
    /// opens the memory store in the same database.
    pub async fn open(url: &SecretString, embedder: Arc<dyn EmbeddingProvider>) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .connect(url.expose_secret())
            .await?;
        Self::with_pool(pool, embedder).await
    }

    /// Runs the migrations on `pool` and opens the store on it.
    pub async fn with_pool(pool: PgPool, embedder: Arc<dyn EmbeddingProvider>) -> Result<Self> {
        migrator(BEFORE_BACKFILL).run(&pool).await?;
        backfill::run(&pool).await?;
        migrator(i64::MAX).run(&pool).await?;
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

    /// Tells the scheduler the `crons` table changed. `notify_one` keeps a permit
    /// when nobody is waiting, so a write between two waits is not lost.
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
