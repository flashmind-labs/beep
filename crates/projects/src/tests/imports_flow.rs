use super::*;

#[test]
fn a_parent_takes_the_owner_and_name() {
    let repo = RepoRef::parse("acme/widget", &[]).expect("org/repo parses");
    assert_eq!(under_parent("~/Projects/", &repo), "~/Projects/acme/widget");
    assert_eq!(under_parent("  ", &repo), "");
}

use crate::{ShellOutput, model::ForgejoUrl};
use reqwest::Client;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

/// A shell that answers each script by what it is for.
struct Scripted;

#[async_trait]
impl crate::Shell for Scripted {
    async fn run(&self, script: &str, _: std::time::Duration) -> Result<ShellOutput> {
        // Looking for checkouts finds none; the clone lands.
        Ok(if script.contains("BEEP_IMPORT:") {
            ShellOutput::exited(0, "\nBEEP_IMPORT: cloned\n")
        } else {
            ShellOutput::exited(0, "")
        })
    }
}

#[derive(Default)]
struct Memory {
    saved: Mutex<Vec<(String, String, String)>>,
}

#[async_trait]
impl ImportStore for Memory {
    async fn imports(&self) -> Result<Vec<ImportRow>> {
        Ok(Vec::new())
    }
    async fn save_import(&self, import: NewImport<'_>) -> Result<ImportRow> {
        self.saved.lock().expect("not poisoned").push((
            import.project.to_owned(),
            import.endpoint.to_owned(),
            import.folder.to_owned(),
        ));
        Ok(ImportRow {
            project: import.project.to_owned(),
            endpoint: import.endpoint.to_owned(),
            folder: import.folder.to_owned(),
            url: import.url.to_owned(),
            imported_at: chrono::Utc::now(),
            fork: None,
        })
    }
    async fn link_import(&self, _: NewImport<'_>) -> Result<bool> {
        Ok(false)
    }
    async fn set_import_fork(
        &self,
        _: &str,
        _: &str,
        _: Option<&crate::model::RepoName>,
    ) -> Result<()> {
        Ok(())
    }
}

struct NoHosts(Arc<Semaphore>);

#[async_trait]
impl Hosts for NoHosts {
    fn client(&self) -> Result<Client> {
        Ok(beep_proto::https::client().build()?)
    }
    fn calls(&self) -> &Arc<Semaphore> {
        &self.0
    }
    async fn is_public(&self, _: &ForgejoUrl) -> bool {
        false
    }
}

#[derive(Default)]
struct Noted(Mutex<Vec<String>>);

#[async_trait]
impl Effects for Noted {
    async fn imported(&self, machine: &str) {
        self.0
            .lock()
            .expect("not poisoned")
            .push(machine.to_owned());
    }
    async fn join_sync(&self, _: &[ImportedProject], _: &mut Vec<CheckoutFailure>) -> Result<()> {
        Ok(())
    }
}

fn box_machine() -> Machine {
    Machine {
        id: "box-id".into(),
        name: "build-box".into(),
        online: true,
        os: Some("linux".into()),
        home: Some("/home/me".into()),
        workspace: None,
        shell: Some(Arc::new(Scripted)),
        sessions: None,
    }
}

#[tokio::test]
async fn an_import_records_what_the_fake_shell_cloned() {
    let store = Memory::default();
    let hosts = NoHosts(Arc::new(Semaphore::new(1)));
    let effects = Noted::default();
    let machines = [box_machine()];
    let env = Env {
        store: &store,
        hosts: &hosts,
        effects: &effects,
        credentials: &[],
        machines: &machines,
    };
    let landed = import(
        &env,
        Import {
            repo: "acme/widget",
            machine_id: Some("box-id"),
            folder: None,
            parent: None,
        },
    )
    .await
    .expect("the import lands");
    assert!(landed.cloned);
    assert_eq!(landed.project, "acme/widget");
    assert_eq!(landed.folder, "/home/me/projects/acme/widget");
    assert_eq!(
        store.saved.lock().expect("not poisoned").as_slice(),
        [(
            "acme/widget".to_owned(),
            "box-id".to_owned(),
            "/home/me/projects/acme/widget".to_owned()
        )]
    );
    assert_eq!(*effects.0.lock().expect("not poisoned"), ["box-id"]);
}
