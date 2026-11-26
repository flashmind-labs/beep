use super::*;
use std::time::Duration;

#[test]
fn a_mac_reopens_what_was_open_and_notes_it_on_quit() {
    let dir = std::env::temp_dir().join(format!("beep-this-mac-reopen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the node's folder");
    let endpoint = async_runtime::block_on(
        iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
            .secret_key(iroh::SecretKey::generate())
            .bind(),
    )
    .expect("the endpoint binds");
    let machine = Machine::new(&endpoint, &dir, None);
    machine.reopen();
    // Nothing was open: moves stop waiting at once, settings or not.
    let agents = machine.offers().agents.clone();
    async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(20), async {
            while !agents.reopened() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("reopened without waiting for settings");
    });
    let note = dir.join("reopen.json");
    let _ = std::fs::remove_file(&note);
    async_runtime::block_on(machine.quit());
    assert!(note.exists(), "quitting notes what is open");
    let _ = std::fs::remove_dir_all(&dir);
}
