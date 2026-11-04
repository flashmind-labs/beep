//! The merge against the schema: every user table is merged and keeps its rows by the merge's rules.

use super::{account, key};
use crate::{
    account::{credentials::CredentialKey, user::UserKey},
    store::{
        NewImport, Store, TestDb,
        merge::{Merge, MergeInto, handled, projects, summed},
        test_chat, test_turn,
    },
};
use beep_projects::model::RepoName;
use sqlx::PgPool;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use uuid::Uuid;

/// Every table with a column naming a user, and that column.
async fn owner_columns(pool: &PgPool) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT c.relname::text, a.attname::text
         FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid
         WHERE c.relnamespace = current_schema()::regnamespace AND c.relkind = 'r'
           AND a.attname = 'user_id' AND NOT a.attisdropped
         UNION
         SELECT c.relname::text, a.attname::text
         FROM pg_constraint k JOIN pg_class c ON c.oid = k.conrelid
         JOIN pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = ANY(k.conkey)
         WHERE k.contype = 'f' AND k.confrelid = 'users'::regclass
           AND c.relnamespace = current_schema()::regnamespace
         ORDER BY 1, 2",
    )
    .fetch_all(pool)
    .await
    .expect("the catalog reads")
}

#[tokio::test]
async fn every_table_with_an_owner_is_merged() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let tables: BTreeSet<String> = owner_columns(&store.pool)
        .await
        .into_iter()
        .map(|(table, _)| table)
        .collect();
    let handled: BTreeSet<String> = handled().into_iter().map(str::to_owned).collect();
    let missing: Vec<_> = tables.difference(&handled).collect();
    assert!(
        missing.is_empty(),
        "merge_into does not move {missing:?}: add each to MOVED, OWN or the code that moves it"
    );
    let stale: Vec<_> = handled.difference(&tables).collect();
    assert!(
        stale.is_empty(),
        "merge_into names tables with no owner: {stale:?}"
    );

    // A count table's columns are all listed, or a new one would be lost.
    for summed in summed::SUMMED {
        let columns: Vec<String> = sqlx::query_scalar(
            "SELECT column_name::text FROM information_schema.columns
             WHERE table_schema = current_schema() AND table_name = $1
               AND column_name <> 'user_id'
             ORDER BY 1",
        )
        .bind(summed.table)
        .fetch_all(&store.pool)
        .await
        .unwrap();
        let mut listed: Vec<String> = summed.columns().map(str::to_owned).collect();
        listed.sort();
        assert_eq!(listed, columns, "the columns of {}", summed.table);
    }
}

/// Rows both accounts get (`{t}` tag, `{chat}` their chat); per-user keys match.
const BOTH: &[&str] = &[
    "INSERT INTO actions (user_id, description) VALUES ($1, 'action {t}')",
    "INSERT INTO files (id, user_id, name, mime, data)
     VALUES ('file-{t}', $1, 'a.png', 'image/png', decode('00', 'hex'))",
    "INSERT INTO agent_images (user_id, key, file_id, machine_id, session)
     VALUES ($1, 'same', 'file-{t}', 'm', 's')",
    "INSERT INTO agent_session_starts (user_id, machine, agent, session, device)
     VALUES ($1, 'm', 'claude', 's-{t}', 'd-{t}')",
    "INSERT INTO agent_model_usage (user_id, day, agent, model, tokens)
     VALUES ($1, '2026-01-01', 'claude', 'opus', 5)",
    "INSERT INTO agent_session_usage
         (user_id, endpoint_id, agent, session, hour, model, cwd, input_tokens, cost_usd)
     VALUES ($1, 'e', 'claude', 's', '2026-01-01T00:00:00Z', 'opus', '/w', 3, 0.5)",
    "INSERT INTO agent_usage (user_id, endpoint_id, session, day, agent, cwd, tokens, turns, cost_usd)
     VALUES ($1, 'e', 's', '2026-01-01', 'claude', '/w-{t}', 10, 1,
         CASE WHEN '{t}' = 's' THEN NULL ELSE 2.0 END)",
    "INSERT INTO app_events (user_id, app, kind, dedupe_key, summary, ids, raw, source, occurred_at)
     VALUES ($1, 'gmail', 'new_mail', 'same', '', '[]', '[]', 'poll', now())",
    "INSERT INTO app_permissions (user_id, app, level) VALUES ($1, 'gmail', 'ask')",
    "INSERT INTO attention_items (user_id, mailbox, message_id, received_at, text)
     VALUES ($1, 'inbox', 'm1', now(), '{t}')",
    "INSERT INTO branch_reviews (id, user_id, project, branch, base, endpoint_id, folder, head,
         merge_base, head_at, changed_lines, files)
     VALUES (gen_random_uuid(), $1, 'acme/app', 'b', 'main', 'e', '/w', 'h', 'm', now(), 1, 1)",
    "INSERT INTO chat_allows (chat_id, user_id, app, tool) VALUES ('{chat}', $1, 'gmail', 'send')",
    "INSERT INTO checkout_checks (user_id, endpoint_id, checked_at) VALUES ($1, 'e', now())",
    "INSERT INTO credentials (id, user_id, kind, provider, host_url, to_machines, last4, secret)
     VALUES ('cred-{t}', $1, 'api_key', 'openai', '', false, '0000', decode('00', 'hex'))",
    "INSERT INTO crons (user_id, id, label, prompt, schedule)
     VALUES ($1, 'cron-{t}', '', '', '0 9 * * *')",
    "INSERT INTO devices (user_id, device_id, fcm_token, endpoint_id)
     VALUES ($1, 'phone', 'tok-{t}', 'e-{t}')",
    "INSERT INTO event_polls (user_id, source, polled_at) VALUES ($1, 'github', now())",
    "INSERT INTO event_topics (user_id, topic, enabled) VALUES ($1, 'github_ci', false)",
    "INSERT INTO event_triage (user_id, decision, reason) VALUES ($1, 'ignore', '')",
    "INSERT INTO event_triggers (user_id, slug, account_id, trigger_id)
     VALUES ($1, 's', 'a', 't-{t}')",
    "INSERT INTO findings (id, fingerprint, kind, summary, steps, cause, fix)
     VALUES (gen_random_uuid(), 'fp-{t}', 'k', 's', 's', 'c', 'f')",
    "INSERT INTO finding_sightings (finding_id, user_id, chat_id, first_seq, last_seq)
     VALUES ((SELECT id FROM findings WHERE fingerprint = 'fp-{t}'), $1, '{chat}', 1, 2)",
    "INSERT INTO grants (id, user_id, routine_id, tool, constraints, per_run, shown, call_id, expires_at)
     VALUES (gen_random_uuid(), $1, 'cron-{t}', 'GMAIL_SEND', '[]', 1, '[]', 'call-{t}',
         now() + interval '1 day')",
    "INSERT INTO habits (id, user_id, name) VALUES ('habit-{t}', $1, 'Run')",
    "INSERT INTO habit_logs (id, habit_id, user_id) VALUES ('log-{t}', 'habit-{t}', $1)",
    "INSERT INTO flsh_settings (user_id, settings) VALUES ($1, jsonb_build_object('tag', '{t}'))",
    "INSERT INTO harness_settings (user_id, machine_id, agent, program)
     VALUES ($1, 'm1', 'codex', '{t}-codex')",
    "INSERT INTO instructions_versions (user_id, version, text) VALUES ($1, 1, '{t}1')",
    "INSERT INTO locations (user_id, latitude, longitude, accuracy) VALUES ($1, 1, 1, 1)",
    "INSERT INTO mail_seen (user_id, mailbox, message_id, received_at)
     VALUES ($1, 'inbox', 'm1', now())",
    "INSERT INTO mail_sync (user_id, mailbox, synced_at) VALUES ($1, 'inbox', now())",
    "INSERT INTO mcp_servers (user_id, name) VALUES ($1, 'fastmail')",
    "INSERT INTO memory_curation (user_id) VALUES ($1)",
    "INSERT INTO messages (chat_id, role, author_id) VALUES ('{chat}', 'member', $1)",
    "INSERT INTO node_tokens (hash, user_id, expires_at) VALUES ('hash-{t}', $1, now())",
    "INSERT INTO nodes (endpoint_id, user_id, name, version) VALUES ('node-{t}', $1, 'n', '0.1.0')",
    "INSERT INTO paused_syncs (user_id, project) VALUES ($1, 'acme/app')",
    "INSERT INTO pinned_projects (user_id, project) VALUES ($1, 'acme/app')",
    "INSERT INTO projects_on_top (user_id, project) VALUES ($1, 'acme/app')",
    "INSERT INTO project_webhooks (id, user_id, host, repo, endpoint_id, hook_id, secret)
     VALUES (gen_random_uuid(), $1, 'github', 'acme/app', 'e', 7, decode('00', 'hex'))",
    "INSERT INTO user_logins (sub, user_id) VALUES ('google:{t}', $1)",
    "INSERT INTO places (user_id, id, name, latitude, longitude, radius)
     VALUES ($1, 'place-{t}', 'Home', 1, 1, 100)",
    "INSERT INTO plan_usage (user_id, agent, endpoint_id, windows, read_at)
     VALUES ($1, 'claude', 'e-{t}', '[]', now())",
    "INSERT INTO profile_notes (user_id, source, note) VALUES ($1, 'mail', '{t}')",
    "INSERT INTO profile_reads (user_id, source) VALUES ($1, 'mail')",
    "INSERT INTO project_groups (id, user_id, name, paths)
     VALUES (gen_random_uuid(), $1, 'g', ARRAY['work'])",
    "INSERT INTO project_imports (user_id, project, endpoint_id, folder, url)
     VALUES ($1, 'acme/app', 'e', '/w', 'u')",
    "INSERT INTO project_sync (user_id, project, endpoint_id, folder)
     VALUES ($1, 'acme/app', 'e', '/w')",
    "INSERT INTO projects (id, user_id, name) VALUES (gen_random_uuid(), $1, 'acme/app')",
    "INSERT INTO project_tasks (id, project_id, slug, title)
     VALUES (gen_random_uuid(),
         (SELECT id FROM projects WHERE user_id = $1 AND name = 'acme/app'), 'task', 'Task')",
    "INSERT INTO project_workers (id, task_id, project_id, user_id, machine, agent, folder)
     SELECT gen_random_uuid(), t.id, p.id, $1, 'm', 'claude', '/w-{t}'
     FROM projects p JOIN project_tasks t ON t.project_id = p.id WHERE p.user_id = $1",
    "INSERT INTO researches (id, chat_id, user_id, topic)
     VALUES (gen_random_uuid(), '{chat}', $1, 't')",
    "INSERT INTO route_cards (user_id, call_id, chat_id, card) VALUES ($1, 'card', '{chat}', '[]')",
    "INSERT INTO shares (token, user_id, title, conversation, chat_id)
     VALUES ('share-{t}', $1, '', '', '{chat}')",
    "INSERT INTO skills (id, user_id, name, description, body)
     VALUES (gen_random_uuid(), $1, 'Deploy', '', '')",
    "INSERT INTO terminals (id, user_id, project, endpoint_id, task_id, name, command, cwd)
     VALUES (gen_random_uuid(), $1, 'acme/app', 'e', 1, 'n', 'c', '/w')",
    "INSERT INTO token_usage (user_id, day, model, agent, calls)
     VALUES ($1, '2026-01-01', 'opus', 'beep', 2)",
];

/// Rows only the source has, none clashing with the target's but the one grant.
const SOURCE_ONLY: &[&str] = &[
    "INSERT INTO agent_images (user_id, key, file_id, machine_id, session)
     VALUES ($1, 'own', 'file-s', 'm', 's')",
    "INSERT INTO harness_settings (user_id, machine_id, agent, program)
     VALUES ($1, 'm2', 'claude', 's-claude')",
    "INSERT INTO instructions_versions (user_id, version, text) VALUES ($1, 3, 's2')",
    // Granted for a call the target granted too: the target's stays.
    "INSERT INTO grants (id, user_id, routine_id, tool, constraints, per_run, shown, call_id, expires_at)
     VALUES (gen_random_uuid(), $1, 'cron-s', 'GMAIL_SEND', '[]', 1, '[]', 'call-t',
         now() + interval '1 day')",
    "INSERT INTO projects (id, user_id, name) VALUES (gen_random_uuid(), $1, 'solo/tool')",
];

/// Rows only the target has.
const TARGET_ONLY: &[&str] = &[
    // `acme/app-copy` is taken, without case, so the moved one is
    // `acme/app-copy-2`.
    "INSERT INTO projects (id, user_id, name) VALUES (gen_random_uuid(), $1, 'Acme/App-Copy')",
    "INSERT INTO instructions_versions (user_id, version, text) VALUES ($1, 2, 't2')",
];

/// How many source rows each table drops: key clashes and summed counts.
const DROPPED: &[(&str, i64)] = &[
    ("agent_images", 1),
    ("agent_model_usage", 1),
    ("agent_session_usage", 1),
    ("agent_usage", 1),
    ("app_events", 1),
    ("app_permissions", 1),
    ("attention_items", 1),
    ("checkout_checks", 1),
    ("credentials", 1),
    ("devices", 1),
    ("event_polls", 1),
    ("event_topics", 1),
    ("event_triggers", 1),
    ("grants", 1),
    ("habits", 1),
    ("flsh_settings", 1),
    ("harness_settings", 1),
    ("locations", 1),
    ("mail_seen", 1),
    ("mail_sync", 1),
    ("mcp_servers", 1),
    ("memory_curation", 1),
    ("places", 1),
    ("plan_usage", 1),
    ("profile_reads", 1),
    ("route_cards", 1),
    ("skills", 1),
    ("sync_counters", 1),
    ("token_usage", 1),
];

async fn seed(pool: &PgPool, statements: &[&str], user: &str, tag: &str, chat: Uuid) {
    for statement in statements {
        let sql = statement
            .replace("{t}", tag)
            .replace("{chat}", &chat.to_string());
        if let Err(e) = sqlx::query(&sql).bind(user).execute(pool).await {
            panic!("{sql}: {e}");
        }
    }
}

/// Each owner table's rows owned by `user`.
async fn counts(pool: &PgPool, owners: &[(String, String)], user: &str) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    for (table, column) in owners {
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE {column}=$1"))
                .bind(user)
                .fetch_one(pool)
                .await
                .unwrap();
        *out.entry(table.clone()).or_default() += count;
    }
    out
}

async fn texts(pool: &PgPool, sql: &str, user: &str) -> Vec<String> {
    sqlx::query_scalar(sql)
        .bind(user)
        .fetch_all(pool)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

async fn merged(store: &Store) -> (String, String) {
    let credential_key = CredentialKey::random();
    let (mac, phone) = (key(31), key(32));
    let target = account(store, &mac).await;
    let source = account(store, &phone).await;
    let (target_key, source_key) = (UserKey::for_test(&target), UserKey::for_test(&source));
    let pool = &store.pool;
    let (source_chat, target_chat) = (test_chat("merge source"), test_chat("merge target"));
    test_turn(store, &source_key, source_chat, "from the phone", 1000)
        .await
        .unwrap();
    test_turn(store, &target_key, target_chat, "from the mac", 2000)
        .await
        .unwrap();
    seed(pool, BOTH, &source, "s", source_chat).await;
    seed(pool, BOTH, &target, "t", target_chat).await;
    seed(pool, SOURCE_ONLY, &source, "s", source_chat).await;
    seed(pool, TARGET_ONLY, &target, "t", target_chat).await;
    store
        .create_invite(&source_key, &phone, "Phone", Duration::from_secs(600))
        .await
        .unwrap();
    let invite = store
        .create_invite(&target_key, &mac, "MacBook", Duration::from_secs(600))
        .await
        .unwrap();

    let owners = owner_columns(pool).await;
    let source_before = counts(pool, &owners, &source).await;
    let target_before = counts(pool, &owners, &target).await;
    let empty: Vec<_> = source_before.iter().filter(|(_, n)| **n == 0).collect();
    assert!(empty.is_empty(), "seed rows of the source in {empty:?}");

    let into = MergeInto {
        code: &invite.code,
        target: &target,
        adopt_external_id: false,
        credential_key: &credential_key,
    };
    assert!(matches!(
        store.merge_into(&source_key, into).await.unwrap(),
        Merge::Merged { .. }
    ));

    let source_after = counts(pool, &owners, &source).await;
    let left: Vec<_> = source_after.iter().filter(|(_, n)| **n > 0).collect();
    assert!(
        left.is_empty(),
        "rows left with the merged account: {left:?}"
    );
    let target_after = counts(pool, &owners, &target).await;
    for (table, after) in &target_after {
        let expected = if table == "account_invites" {
            // The source's codes go with it, and the target's was used.
            target_before[table] - 1
        } else {
            let dropped = DROPPED
                .iter()
                .find(|(name, _)| name == table)
                .map_or(0, |(_, n)| *n);
            target_before[table] + source_before[table] - dropped
        };
        assert_eq!(*after, expected, "{table} rows of the kept account");
    }
    (source, target)
}

#[tokio::test]
async fn a_merge_keeps_the_rows_of_every_table() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let (_, target) = merged(&store).await;
    let pool = &store.pool;

    // Both projects stay, the moved one renamed everywhere it is named.
    let names = texts(
        pool,
        "SELECT name FROM projects WHERE user_id=$1 ORDER BY name COLLATE \"C\"",
        &target,
    )
    .await;
    assert_eq!(
        names,
        ["Acme/App-Copy", "acme/app", "acme/app-copy-2", "solo/tool"]
    );
    for (table, column) in projects::NAMED.iter().filter(|(t, _)| *t != "projects") {
        let sql = format!("SELECT {column} FROM {table} WHERE user_id=$1 ORDER BY 1");
        let named = texts(pool, &sql, &target).await;
        assert_eq!(named, ["acme/app", "acme/app-copy-2"], "{table}");
    }
    let worker_project = texts(
        pool,
        "SELECT p.name FROM project_workers w JOIN projects p ON p.id = w.project_id
         WHERE w.user_id=$1 AND w.folder = '/w-s'",
        &target,
    )
    .await;
    assert_eq!(worker_project, ["acme/app-copy-2"]);

    // Counts of one key add up; the target's other columns stay.
    let tokens: i64 = sqlx::query_scalar("SELECT tokens FROM agent_model_usage WHERE user_id=$1")
        .bind(&target)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(tokens, 10);
    let session: (i64, f64) =
        sqlx::query_as("SELECT input_tokens, cost_usd FROM agent_session_usage WHERE user_id=$1")
            .bind(&target)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(session, (6, 1.0));
    let usage: (i64, i32, Option<f64>, String) =
        sqlx::query_as("SELECT tokens, turns, cost_usd, cwd FROM agent_usage WHERE user_id=$1")
            .bind(&target)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(usage, (20, 2, Some(2.0), "/w-t".to_owned()));
    let calls: i64 = sqlx::query_scalar("SELECT calls FROM token_usage WHERE user_id=$1")
        .bind(&target)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(calls, 4);

    // A setting both have keeps the target's; the source's others move.
    let programs = texts(
        pool,
        "SELECT program FROM harness_settings WHERE user_id=$1 ORDER BY machine_id",
        &target,
    )
    .await;
    assert_eq!(programs, ["t-codex", "s-claude"]);

    // Every version of the instructions is kept, the target's newest last.
    let versions = texts(
        pool,
        "SELECT version || ' ' || text FROM instructions_versions WHERE user_id=$1
         ORDER BY version",
        &target,
    )
    .await;
    assert_eq!(versions, ["1 s1", "2 s2", "3 t1", "4 t2"]);

    // The routine came with its grant, and the clashing grant went.
    let grants = texts(
        pool,
        "SELECT routine_id || ' ' || call_id FROM grants WHERE user_id=$1 ORDER BY 1",
        &target,
    )
    .await;
    assert_eq!(grants, ["cron-s call-s", "cron-t call-t"]);
}

#[tokio::test]
async fn an_import_step_works_on_a_renamed_project() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let (_, target) = merged(&store).await;
    let user = UserKey::for_test(&target);
    let renamed = "acme/app-copy-2";
    RepoName::parse(renamed).expect("the copy is still owner/repo");

    // Moving the import to another folder finds the moved row and the
    // moved project, and makes no other.
    let saved = store
        .save_import(
            &user,
            NewImport {
                project: renamed,
                endpoint: "e",
                folder: "/w-moved",
                url: "u",
            },
        )
        .await
        .unwrap();
    assert_eq!(saved.folder, "/w-moved");
    let fork = RepoName::parse("octo/app").unwrap();
    store
        .set_import_fork(&user, renamed, "e", Some(&fork))
        .await
        .unwrap();
    // A checkout found on another machine links to the same project.
    let linked = store
        .link_import(
            &user,
            NewImport {
                project: renamed,
                endpoint: "e2",
                folder: "/w2",
                url: "u",
            },
        )
        .await
        .unwrap();
    assert!(linked);
    let imports: Vec<_> = store
        .imports(&user)
        .await
        .unwrap()
        .into_iter()
        .filter(|row| row.project == renamed)
        .map(|row| (row.endpoint, row.folder, row.fork))
        .collect();
    assert_eq!(
        imports,
        [
            ("e".to_owned(), "/w-moved".to_owned(), Some(fork)),
            ("e2".to_owned(), "/w2".to_owned(), None),
        ]
    );
    let copies = texts(
        &store.pool,
        "SELECT name FROM projects WHERE user_id=$1 AND lower(name)='acme/app-copy-2'",
        &target,
    )
    .await;
    assert_eq!(copies, [renamed]);
}
