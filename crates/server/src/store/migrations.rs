//! The schema: every migration in order, applied on start.

use super::backfill;
use anyhow::Result;
use sqlx::PgPool;
use sqlx_core::migrate::{Migration, MigrationType, Migrator};
use std::borrow::Cow;

/// The schema, oldest first; embedded by hand since `sqlx::migrate!` clashes with flashmind's rusqlite.
pub(super) const MIGRATIONS: &[(i64, &str, &str)] = &[
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
    (
        40,
        "attachments",
        include_str!("../../migrations/0040_attachments.sql"),
    ),
    (
        41,
        "mcp tools",
        include_str!("../../migrations/0041_mcp_tools.sql"),
    ),
    (
        42,
        "push service",
        include_str!("../../migrations/0042_push_service.sql"),
    ),
    (
        43,
        "research",
        include_str!("../../migrations/0043_research.sql"),
    ),
    (44, "nodes", include_str!("../../migrations/0044_nodes.sql")),
    (
        45,
        "memory curation scopes",
        include_str!("../../migrations/0045_memory_curation_scopes.sql"),
    ),
    (
        46,
        "events",
        include_str!("../../migrations/0046_events.sql"),
    ),
    (
        47,
        "event triage",
        include_str!("../../migrations/0047_event_triage.sql"),
    ),
    (
        48,
        "spoken turns",
        include_str!("../../migrations/0048_spoken_turns.sql"),
    ),
    (
        49,
        "event state time",
        include_str!("../../migrations/0049_event_state_at.sql"),
    ),
    (
        50,
        "grants",
        include_str!("../../migrations/0050_grants.sql"),
    ),
    (
        51,
        "tool descriptions",
        include_str!("../../migrations/0051_tool_descriptions.sql"),
    ),
    (
        52,
        "watchers",
        include_str!("../../migrations/0052_watchers.sql"),
    ),
    (
        53,
        "turn sources",
        include_str!("../../migrations/0053_turn_sources.sql"),
    ),
    (
        55,
        "project_sync",
        include_str!("../../migrations/0055_project_sync.sql"),
    ),
    (
        56,
        "node tokens",
        include_str!("../../migrations/0056_node_tokens.sql"),
    ),
    (
        58,
        "credentials",
        include_str!("../../migrations/0058_credentials.sql"),
    ),
    (
        59,
        "auto model set",
        include_str!("../../migrations/0059_auto_model_set.sql"),
    ),
    (
        60,
        "projects",
        include_str!("../../migrations/0060_projects.sql"),
    ),
    (
        61,
        "project imports",
        include_str!("../../migrations/0061_project_imports.sql"),
    ),
    (
        62,
        "checkout checks",
        include_str!("../../migrations/0062_checkout_checks.sql"),
    ),
    (
        63,
        "github accounts",
        include_str!("../../migrations/0063_github_accounts.sql"),
    ),
    (
        64,
        "project issues",
        include_str!("../../migrations/0064_project_issues.sql"),
    ),
    (
        65,
        "projects on top",
        include_str!("../../migrations/0065_projects_on_top.sql"),
    ),
    (
        66,
        "instructions",
        include_str!("../../migrations/0066_instructions.sql"),
    ),
    (
        67,
        "terminals",
        include_str!("../../migrations/0067_terminals.sql"),
    ),
    (
        68,
        "branch reviews",
        include_str!("../../migrations/0068_branch_reviews.sql"),
    ),
    (
        69,
        "chat allows",
        include_str!("../../migrations/0069_chat_allows.sql"),
    ),
    (
        70,
        "thread chats",
        include_str!("../../migrations/0070_thread_chats.sql"),
    ),
    (
        71,
        "review worktrees",
        include_str!("../../migrations/0071_review_worktrees.sql"),
    ),
    (
        72,
        "agent images",
        include_str!("../../migrations/0072_agent_images.sql"),
    ),
    (
        73,
        "project groups",
        include_str!("../../migrations/0073_project_groups.sql"),
    ),
    (
        74,
        "drop agent images",
        include_str!("../../migrations/0074_drop_agent_images.sql"),
    ),
    (
        75,
        "imported projects",
        include_str!("../../migrations/0075_imported_projects.sql"),
    ),
    (
        76,
        "harness_settings",
        include_str!("../../migrations/0076_harness_settings.sql"),
    ),
    (
        77,
        "paused syncs",
        include_str!("../../migrations/0077_paused_syncs.sql"),
    ),
    (
        78,
        "terminal ptys",
        include_str!("../../migrations/0078_terminal_ptys.sql"),
    ),
    (
        79,
        "agent images again",
        include_str!("../../migrations/0079_agent_images.sql"),
    ),
    (
        80,
        "agent usage",
        include_str!("../../migrations/0080_agent_usage.sql"),
    ),
    (
        81,
        "import forks",
        include_str!("../../migrations/0081_import_forks.sql"),
    ),
    (
        82,
        "agent session usage",
        include_str!("../../migrations/0082_agent_session_usage.sql"),
    ),
    (
        83,
        "update chats",
        include_str!("../../migrations/0083_update_chats.sql"),
    ),
    (
        86,
        "agent spend billing",
        include_str!("../../migrations/0086_agent_spend_billing.sql"),
    ),
    (
        92,
        "project webhooks",
        include_str!("../../migrations/0092_project_webhooks.sql"),
    ),
    (
        97,
        "flsh settings",
        include_str!("../../migrations/0097_flsh_settings.sql"),
    ),
    (
        98,
        "agent session starts",
        include_str!("../../migrations/0098_agent_session_starts.sql"),
    ),
    (
        99,
        "node traffic",
        include_str!("../../migrations/0099_node_traffic.sql"),
    ),
];

/// The last migration before the transcript backfill runs.
pub(super) const BEFORE_BACKFILL: i64 = 29;

/// The migrations up to `last`.
pub(super) fn migrator(last: i64) -> Migrator {
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

/// Brings the schema on `pool` up to date.
pub(super) async fn migrate(pool: &PgPool) -> Result<()> {
    migrator(BEFORE_BACKFILL).run(pool).await?;
    backfill::run(pool).await?;
    migrator(i64::MAX).run(pool).await?;
    Ok(())
}
