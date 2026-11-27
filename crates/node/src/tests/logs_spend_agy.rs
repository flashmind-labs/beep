//! Passes over fabricated Antigravity conversations.

use super::*;
use crate::agents::spend::{Heard, Spend};
use crate::logs::found::agy::Workspace;
use crate::logs::spend::Roots;
use rusqlite::Connection;

/// 2026-11-03T09:00:00Z.
const NOW: u64 = 1_793_696_400;
const HOUR: u64 = 3600;

struct Call {
    step: i64,
    at: u64,
    root: &'static str,
    input: u64,
    output: u64,
    cache_read: u64,
}

/// A conversation at `path` that ran in `/srv/project` and made `calls`.
fn conversation(path: &Path, calls: &[Call]) {
    let conn = Connection::open(path).expect("a database");
    conn.execute_batch(
        "CREATE TABLE steps (idx INTEGER PRIMARY KEY, metadata BLOB);
         CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB, size INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE trajectory_metadata_blob (id TEXT DEFAULT 'main' PRIMARY KEY, data BLOB);",
    )
    .expect("the schema");
    let trajectory = Trajectory {
        workspaces: vec![Workspace {
            uri: "file:///srv/project".to_owned(),
        }],
        created: None,
    };
    conn.execute(
        "INSERT INTO trajectory_metadata_blob (data) VALUES (?1)",
        [trajectory.encode_to_vec()],
    )
    .expect("the trajectory");
    let label = |key: &str, value: String| Label {
        key: key.to_owned(),
        value,
    };

    for (idx, call) in (0i64..).zip(calls) {
        let step = Step {
            created: Some(Time {
                seconds: i64::try_from(call.at).expect("a time"),
            }),
        };
        conn.execute(
            "INSERT INTO steps VALUES (?1, ?2)",
            rusqlite::params![call.step, step.encode_to_vec()],
        )
        .expect("a step");
        let generation = Generation {
            request: Some(Request {
                usage: Some(Usage {
                    input: call.input,
                    output: call.output,
                    cache_write: 0,
                    cache_read: call.cache_read,
                }),
                model: "gemini-4-carbon".to_owned(),
                labels: vec![
                    label("root_cascade_id", call.root.to_owned()),
                    label("last_step_index", call.step.to_string()),
                ],
            }),
        };
        conn.execute(
            "INSERT INTO gen_metadata (idx, data) VALUES (?1, ?2)",
            rusqlite::params![idx, generation.encode_to_vec()],
        )
        .expect("a request");
    }
}

#[test]
fn a_subagents_requests_count_toward_the_session_that_started_it() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let conversations = dir.path().join("conversations");
    std::fs::create_dir_all(&conversations).expect("folders");
    let hour = NOW - 2 * HOUR;
    conversation(
        &conversations.join("c1.db"),
        &[
            Call {
                step: 0,
                at: hour + 60,
                root: "c1",
                input: 1000,
                output: 100,
                cache_read: 0,
            },
            // Before the window: left out.
            Call {
                step: 2,
                at: NOW - 40 * 24 * HOUR,
                root: "c1",
                input: 9,
                output: 9,
                cache_read: 9,
            },
        ],
    );
    conversation(
        &conversations.join("c2.db"),
        &[Call {
            step: 0,
            at: hour + HOUR + 60,
            root: "c1",
            input: 10,
            output: 5,
            cache_read: 500,
        }],
    );
    let roots = Roots {
        agy: Some(conversations),
        ..Roots::default()
    };
    let spend = Spend::at(roots, dir.path().join("agent-spend.json"));

    let reports = spend.pass(NOW, &mut Heard::default());

    assert_eq!(reports.len(), 1, "{reports:?}");
    let report = &reports[0];
    assert_eq!(
        (
            report.agent.as_ref(),
            report.session.as_ref(),
            report.cwd.as_str()
        ),
        ("agy", "c1", "/srv/project")
    );
    let hours: Vec<_> = report
        .hours
        .iter()
        .map(|spent| {
            (
                spent.hour,
                spent.model.as_ref(),
                spent.input_tokens,
                spent.output_tokens,
                spent.cache_read_tokens,
                spent.billing,
            )
        })
        .collect();
    assert_eq!(
        hours,
        [
            (hour, "gemini-4-carbon", 1000, 100, 0, Billing::Subscription),
            (
                hour + HOUR,
                "gemini-4-carbon",
                10,
                5,
                500,
                Billing::Subscription
            ),
        ]
    );
}
