use super::*;
use std::time::Duration;

use crate::ShellOutput;
use reqwest::Client;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

/// A shell that answers each script by what it is for, on a machine whose home is `/home/me` and
/// where only the `taken` folders hold files and `checkouts` hold `acme/widget`.
#[derive(Default)]
struct Scripted {
    taken: Vec<&'static str>,
    checkouts: Vec<&'static str>,
}

#[async_trait]
impl crate::Shell for Scripted {
    async fn run(&self, script: &str, _: Duration) -> Result<ShellOutput> {
        // Looking for checkouts finds `checkouts`; the clone lands.
        if script.contains("BEEP_IMPORT:") {
            return Ok(ShellOutput::exited(0, "\nBEEP_IMPORT: cloned\n"));
        }

        if script.contains("@@dir ") {
            let out: String = self
                .checkouts
                .iter()
                .map(|dir| {
                    format!("@@dir {dir}\nremote.origin.url https://github.com/acme/widget.git\n")
                })
                .collect();
            return Ok(ShellOutput::exited(0, &out));
        }

        if !script.contains("BEEP_FOLDER:") {
            return Ok(ShellOutput::exited(0, ""));
        }

        let word = script.lines().find_map(|line| line.strip_prefix("d="));
        let path = shlex::split(word.unwrap_or_default())
            .unwrap_or_default()
            .concat()
            .replace("$HOME", "/home/me");
        Ok(if self.taken.contains(&path.as_str()) {
            ShellOutput::exited(
                0,
                &format!("\nBEEP_FOLDER: path {path}\nBEEP_FOLDER: full\n"),
            )
        } else {
            ShellOutput::exited(0, &format!("\nBEEP_FOLDER: missing {path}\n"))
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

impl Hosts for NoHosts {
    fn client(&self) -> Result<Client> {
        Ok(beep_proto::https::client().build()?)
    }
    fn calls(&self) -> &Arc<Semaphore> {
        &self.0
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

fn machine(id: &str, projects_in: Option<&str>, taken: Vec<&'static str>) -> Machine {
    Machine {
        id: id.into(),
        name: id.into(),
        online: true,
        os: Some("linux".into()),
        home: Some("/home/me".into()),
        projects_in: projects_in.map(str::to_owned),
        shell: Some(Arc::new(Scripted {
            taken,
            checkouts: Vec::new(),
        })),
        sessions: None,
    }
}

/// Imports `acme/widget` onto `machine` under `parent`, and gives its folder.
async fn imported_folder(machine: Machine, parent: Option<&str>) -> String {
    let store = Memory::default();
    let hosts = NoHosts(Arc::new(Semaphore::new(1)));
    let effects = Noted::default();
    let id = machine.id.clone();
    let machines = [machine];
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
            machine_id: Some(&id),
            folder: None,
            parent,
            nesting: Nesting::Org,
        },
    )
    .await
    .expect("the import lands");
    assert!(landed.cloned);
    assert_eq!(landed.project, "acme/widget");
    assert_eq!(
        store.saved.lock().expect("not poisoned").as_slice(),
        [("acme/widget".to_owned(), id.clone(), landed.folder.clone())]
    );
    assert_eq!(*effects.0.lock().expect("not poisoned"), [id]);
    landed.folder
}

#[tokio::test]
async fn an_import_goes_in_the_machines_projects_folder() {
    let home = machine("box", None, Vec::new());
    assert_eq!(imported_folder(home, None).await, "/home/me/acme/widget");
    let code = machine("box", Some("~/code"), Vec::new());
    assert_eq!(
        imported_folder(code, None).await,
        "/home/me/code/acme/widget"
    );
    let srv = machine("box", Some("/srv/work/"), Vec::new());
    assert_eq!(imported_folder(srv, None).await, "/srv/work/acme/widget");
}

#[tokio::test]
async fn an_import_goes_under_the_folder_given_and_inside_a_folder_with_files() {
    let free = machine("box", Some("~/code"), Vec::new());
    assert_eq!(
        imported_folder(free, Some("/srv/x")).await,
        "/srv/x/acme/widget"
    );
    let taken = machine("box", None, vec!["/home/me/acme/widget"]);
    assert_eq!(
        imported_folder(taken, None).await,
        "/home/me/acme/widget/widget"
    );
}

#[tokio::test]
async fn an_explicit_target_folder_does_not_nest_org_or_divert_to_another_checkout() {
    let store = Memory::default();
    let hosts = NoHosts(Arc::new(Semaphore::new(1)));
    let effects = Noted::default();
    let mut box_with_other = machine("box", None, vec!["/home/me/work"]);
    box_with_other.shell = Some(Arc::new(Scripted {
        taken: vec!["/home/me/work"],
        checkouts: vec!["/home/me/acme/widget"],
    }));
    let machines = [box_with_other];
    let env = Env {
        store: &store,
        hosts: &hosts,
        effects: &effects,
        credentials: &[],
        machines: &machines,
    };

    let direct = import(
        &env,
        Import {
            repo: "acme/widget",
            machine_id: Some("box"),
            folder: Some("~/work/widget"),
            parent: None,
            nesting: Nesting::Org,
        },
    )
    .await
    .expect("direct target folder imports");
    assert!(direct.cloned);
    assert_eq!(direct.folder, "/home/me/work/widget");

    let inside = import(
        &env,
        Import {
            repo: "acme/widget",
            machine_id: Some("box"),
            folder: Some("~/work"),
            parent: None,
            nesting: Nesting::Org,
        },
    )
    .await
    .expect("non-empty folder places repo inside without org");
    assert!(inside.cloned);
    assert_eq!(inside.folder, "/home/me/work/widget");
}

#[tokio::test]
async fn a_new_project_goes_in_each_machines_own_folder() {
    let store = Memory::default();
    let hosts = NoHosts(Arc::new(Semaphore::new(1)));
    let effects = Noted::default();
    let machines = [
        machine("a", Some("~/code"), Vec::new()),
        machine("b", Some("~/code"), Vec::new()),
    ];
    let env = Env {
        store: &store,
        hosts: &hosts,
        effects: &effects,
        credentials: &[],
        machines: &machines,
    };
    let repo = RepoRef::parse("acme/widget", &[]).expect("org/repo parses");
    let chosen: Vec<&Machine> = machines.iter().collect();
    let folders = [ProjectFolder {
        machine_id: "b".into(),
        folder: "/srv/elsewhere".into(),
    }];
    let made = clone_onto(&env, &repo, &chosen, &folders).await;
    let landed: Vec<(&str, &str)> = made
        .imported
        .iter()
        .map(|p| (p.machine_id.as_str(), p.folder.as_str()))
        .collect();
    assert_eq!(
        landed,
        [
            ("a", "/home/me/code/widget"),
            ("b", "/srv/elsewhere/widget")
        ]
    );
}
