use super::*;
use serde_json::json;
use uuid::Uuid;

/// A path in a fresh directory of its own.
fn path() -> PathBuf {
    std::env::temp_dir()
        .join(format!("beep-views-{}", Uuid::new_v4()))
        .join("views.json")
}

#[test]
fn a_missing_file_starts_empty() {
    let cache = ViewCache::load(path(), "1.0.0");
    assert!(cache.views().is_empty());
}

#[test]
fn saved_views_come_back() {
    let path = path();
    let cache = ViewCache::load(path.clone(), "1.0.0");
    assert!(cache.put(View::Home, json!({"machines": []})));
    cache.write_unsaved();
    assert!(cache.put(View::Top, json!(["org/repo"])));
    cache.write_unsaved();

    let again = ViewCache::load(path.clone(), "1.0.0");
    assert_eq!(again.get(View::Home), Some(json!({"machines": []})));
    assert_eq!(again.get(View::Top), Some(json!(["org/repo"])));
    assert_eq!(again.get(View::Groups), None);
    again.clear();
    assert!(!path.exists());
    assert!(again.views().is_empty());
}

#[test]
fn an_unchanged_view_is_not_written_again() {
    let cache = ViewCache::load(path(), "1.0.0");
    assert!(cache.put(View::Sync, json!([])));
    // A writer is due and has not run: it will write this one too.
    assert!(!cache.put(View::Top, json!([])));
    cache.write_unsaved();
    assert!(!cache.put(View::Sync, json!([])));
    assert!(cache.put(View::Sync, json!([1])));
    cache.write_unsaved();
    cache.clear();
}

#[test]
fn another_version_keeps_only_local_search_lists() {
    let path = path();
    let cache = ViewCache::load(path.clone(), "1.0.0");
    cache.put(View::Home, json!({}));
    cache.put(View::Palette, json!({"projects": []}));
    cache.put(View::AgentSessions, json!([]));
    cache.write_unsaved();
    let upgraded = ViewCache::load(path.clone(), "1.0.1");
    assert_eq!(upgraded.get(View::Home), None);
    assert_eq!(upgraded.get(View::Palette), Some(json!({"projects": []})));
    assert_eq!(upgraded.get(View::AgentSessions), Some(json!([])));

    let old = json!({"format": FORMAT + 1, "app": "1.0.0", "views": {"home": {}}});
    fs::write(&path, old.to_string()).expect("the file is written");
    assert!(ViewCache::load(path.clone(), "1.0.0").views().is_empty());
    cache.clear();
}

#[test]
fn disabling_server_views_keeps_the_mesh_views_across_restart() {
    let path = path();
    let cache = ViewCache::load(path.clone(), "0.2.0");
    cache.put(
        View::Home,
        json!({
            "machines": [{"id": "a", "joined": true}, {"id": "b", "joined": false}],
            "running": [{"kind": "agent", "id": "s1"}, {"kind": "research", "topic": "t"}],
            "needs_you": [{"kind": "agent", "session": "s1"}, {"kind": "chat", "session_id": "c"}],
            "coming_up": [{"id": "cron"}],
            "updates": [{"id": 1}],
            "projects": [{"name": "acme/widget"}],
        }),
    );
    cache.put(View::Top, json!(["acme/widget"]));
    cache.put(View::Agents, json!([{"name": "acme/widget"}]));
    cache.put(View::Overview, json!([]));
    cache.put(
        View::Palette,
        json!({"projects": [{"project": "acme/widget"}]}),
    );
    cache.put(View::AgentSessions, json!([]));
    cache.write_unsaved();
    cache.keep_local();
    let reopened = ViewCache::load(path, "0.2.0");
    assert_eq!(
        reopened.get(View::Home),
        Some(json!({
            "machines": [{"id": "a", "joined": true}],
            "running": [{"kind": "agent", "id": "s1"}],
            "needs_you": [{"kind": "agent", "session": "s1"}],
            "coming_up": null,
            "updates": null,
            "projects": [{"name": "acme/widget"}],
        }))
    );
    assert_eq!(reopened.get(View::Top), Some(json!(["acme/widget"])));
    assert_eq!(reopened.get(View::Agents), None);
    assert_eq!(reopened.get(View::Overview), None);
    assert_eq!(reopened.get(View::Palette), cache.get(View::Palette));
    assert_eq!(reopened.get(View::AgentSessions), Some(json!([])));
    reopened.clear();
}

#[test]
fn keeping_local_views_writes_nothing_when_nothing_goes() {
    let path = path();
    let cache = ViewCache::load(path.clone(), "0.2.0");
    cache.put(View::Top, json!([]));
    cache.keep_local();
    assert!(!path.exists());
}

#[test]
fn a_broken_file_is_dropped() {
    let path = path();
    let cache = ViewCache::load(path.clone(), "1.0.0");
    cache.put(View::Home, json!({}));
    cache.write_unsaved();

    for broken in [&b"{\"format\":1,"[..], b"", b"\x00\xff", b"{\"views\":{}}"] {
        fs::write(&path, broken).expect("the file is written");
        assert!(ViewCache::load(path.clone(), "1.0.0").views().is_empty());
    }
    // One naming a view this version does not know.
    let unknown = json!({"format": FORMAT, "app": "1.0.0", "views": {"later": {}}});
    fs::write(&path, unknown.to_string()).expect("the file is written");
    assert!(ViewCache::load(path.clone(), "1.0.0").views().is_empty());
    // Writing again replaces it.
    let fresh = ViewCache::load(path.clone(), "1.0.0");
    fresh.put(View::Home, json!({"a": 1}));
    fresh.write_unsaved();
    assert_eq!(
        ViewCache::load(path.clone(), "1.0.0").get(View::Home),
        Some(json!({"a": 1}))
    );
    fresh.clear();
}

#[tokio::test(start_paused = true)]
async fn a_burst_of_changes_is_written_once_after_the_wait() {
    let path = path();
    let cache = Arc::new(ViewCache::load(path.clone(), "1.0.0"));
    assert!(cache.put(View::Home, json!({"machines": [1]})));
    let writer = tokio::spawn(write_later(cache.clone()));
    tokio::task::yield_now().await;
    tokio::time::advance(WRITE_AFTER / 2).await;

    assert!(!cache.put(View::Home, json!({"machines": [2]})));
    assert!(!path.exists());

    writer.await.unwrap();
    let saved = ViewCache::load(path.clone(), "1.0.0");
    assert_eq!(saved.get(View::Home), Some(json!({"machines": [2]})));
    assert!(cache.put(View::Home, json!({"machines": [3]})));
    cache.clear();
}

#[test]
fn quitting_writes_what_waits_and_nothing_when_all_is_written() {
    let path = path();
    let cache = ViewCache::load(path.clone(), "1.0.0");
    cache.flush();
    assert!(!path.exists());

    assert!(cache.put(View::Sync, json!([1])));
    cache.flush();
    let saved = ViewCache::load(path.clone(), "1.0.0");
    assert_eq!(saved.get(View::Sync), Some(json!([1])));
    cache.clear();
}
