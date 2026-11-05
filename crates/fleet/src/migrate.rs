//! Moving an account that lives on a server into the mesh: its projects, imports, sync, groups,
//! pins, credentials, agent settings and instructions are read from the server once and written
//! as mesh state. The server stays as it is.
//!
//! The move never overwrites: a key the mesh already holds keeps its value, so a newer value from
//! a desktop never gives way to an older one from the server, and running it twice writes nothing
//! the second time. Everything is read before anything is written, so a server that cannot be
//! reached leaves the mesh untouched and the move tries again on the next launch.
//!
//! The server never sends a secret back. A credential is copied without its value: the mesh holds
//! a record with its last four characters (`unmoved/credential/<id>`), which Settings lists as one
//! to enter again. A harness's env vars are the same, name only (`unmoved/harness/<agent>/<VAR>`). A node
//! counts a record as the mesh holding that family: it takes only the mesh's values at once and
//! never the server's.

use crate::{
    error::Result,
    records::{
        GroupRecord, PinRecord, Place, ProjectRecord, SavedVersion, WRITES, find_project, now_ms,
        place_key, project_key, time_ms,
    },
};
use async_trait::async_trait;
use beep_mesh::{Key, Mesh};
use beep_proto::{
    Credential, FlshSettings, ImportedProject, InstructionsVersion, MachineHarnessSettings,
    ProjectGroup, ProjectSync,
    node::{
        harness::{HARNESSES, HarnessConfig},
        mesh_state::{StateValue, SyncSpec},
    },
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// What an account has on its server: the reads the move makes.
#[async_trait]
pub trait Source: Send + Sync {
    /// Who the account is: the id of its own key, the same on every login of every server.
    async fn account(&self) -> Result<String>;
    async fn imports(&self) -> Result<Vec<ImportedProject>>;
    async fn sync(&self) -> Result<Vec<ProjectSync>>;
    async fn groups(&self) -> Result<Vec<ProjectGroup>>;
    /// The projects pinned to the top, in the order they were pinned.
    async fn on_top(&self) -> Result<Vec<String>>;
    /// The credentials, with their last four characters and no value.
    async fn credentials(&self) -> Result<Vec<Credential>>;
    async fn harness(&self, agent: &str) -> Result<Vec<MachineHarnessSettings>>;
    async fn flsh(&self) -> Result<FlshSettings>;
    /// Every saved version of the instructions, oldest first.
    async fn instructions(&self) -> Result<Vec<InstructionsVersion>>;
}

/// What a move did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// The account was moved before; nothing was read or written.
    pub already: bool,
    /// Keys written to the mesh.
    pub written: usize,
    /// Credentials copied without their value, which the user enters again.
    pub unmoved: usize,
}

/// `moved/<account>`: that this account's move finished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Moved {
    at_ms: u64,
}

impl StateValue for Moved {}

/// What an `unmoved/...` record says: a value the server holds that the mesh has none of.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum Unmoved {
    /// A credential, shown with its last four characters.
    Credential(Credential),
    /// The name of an env var of a harness, never its value.
    HarnessEnv { agent: String, name: String },
}

impl StateValue for Unmoved {}

pub(crate) const UNMOVED_CREDENTIAL: &str = "unmoved/credential/";

fn moved_key(account: &str) -> Key {
    Key::Other(format!("moved/{account}"))
}

pub(crate) fn unmoved_credential(id: &str) -> Key {
    Key::Unmoved(format!("credential/{id}"))
}

pub(crate) fn unmoved_env(agent: &str, name: &str) -> Key {
    Key::Unmoved(format!("harness/{agent}/{name}"))
}

/// Everything read from the server, before any of it is written.
struct Read {
    imports: Vec<ImportedProject>,
    sync: Vec<ProjectSync>,
    groups: Vec<ProjectGroup>,
    on_top: Vec<String>,
    credentials: Vec<Credential>,
    harness: Vec<(&'static str, Vec<MachineHarnessSettings>)>,
    flsh: FlshSettings,
    instructions: Vec<InstructionsVersion>,
}

async fn read(source: &dyn Source) -> Result<Read> {
    let mut harness = Vec::new();
    for known in HARNESSES {
        harness.push((known.agent, source.harness(known.agent).await?));
    }
    Ok(Read {
        imports: source.imports().await?,
        sync: source.sync().await?,
        groups: source.groups().await?,
        on_top: source.on_top().await?,
        credentials: source.credentials().await?,
        harness,
        flsh: source.flsh().await?,
        instructions: source.instructions().await?,
    })
}

/// Writes only keys the mesh does not hold, and counts them.
struct Writer<'a> {
    mesh: &'a Mesh,
    written: usize,
}

impl Writer<'_> {
    /// Whether the key was written. A key the mesh holds or deleted is left as it is.
    async fn absent<T: StateValue>(&mut self, key: Key, value: &T) -> Result<bool> {
        let bytes = value.to_bytes().map_err(anyhow::Error::from)?;
        let wrote = self.mesh.set_if_new(key, bytes).await?;
        self.written += usize::from(wrote);
        Ok(wrote)
    }
}

/// Moves the source's account into `mesh`. A desktop calls it once its mesh is open. The move is
/// kept per account, so another login, on the same server or not, is another move.
pub async fn run(mesh: &Mesh, source: &dyn Source) -> Result<Report> {
    let account = source.account().await?;
    if mesh.get(&moved_key(&account)).is_some() {
        return Ok(Report {
            already: true,
            ..Report::default()
        });
    }
    let found = read(source).await?;
    let _write = WRITES.lock().await;
    let mut out = Writer { mesh, written: 0 };
    let base = now_ms();
    projects(&mut out, &found.imports).await?;
    sync(&mut out, &found.sync).await?;
    for (index, group) in found.groups.iter().enumerate() {
        let record = GroupRecord {
            name: group.name.clone(),
            paths: group.paths.clone(),
            created_ms: base + offset(index),
        };
        out.absent(Key::Group(group.id.to_string()), &record)
            .await?;
    }
    for (index, name) in found.on_top.iter().enumerate() {
        let pin = PinRecord {
            at_ms: base + offset(index),
        };
        out.absent(Key::Pin(name.clone()), &pin).await?;
    }
    let unmoved = unmoved(&mut out, &found.credentials).await?;
    harness(&mut out, &found.harness).await?;
    out.absent(Key::Flsh, &found.flsh).await?;
    instructions(&mut out, &found.instructions).await?;
    out.absent(moved_key(&account), &Moved { at_ms: base })
        .await?;
    Ok(Report {
        already: false,
        written: out.written,
        unmoved,
    })
}

/// Keeps the server's order: one millisecond between neighbours.
fn offset(index: usize) -> u64 {
    u64::try_from(index).unwrap_or(u64::MAX)
}

fn machine(text: &str) -> Option<EndpointId> {
    match text.parse() {
        Ok(id) => Some(id),
        Err(_) => {
            log::warn!("left out a machine of the server's that is not an iroh id");
            None
        }
    }
}

async fn projects(out: &mut Writer<'_>, imports: &[ImportedProject]) -> Result<()> {
    for import in imports {
        let Some(machine) = machine(&import.machine_id) else {
            continue;
        };
        // The project keeps the name it was first stored under, in any case.
        let name = find_project(out.mesh, &import.project)
            .map_or_else(|| import.project.clone(), |stored| stored.name);
        let record = ProjectRecord {
            url: import.url.clone(),
        };
        out.absent(project_key(&name), &record).await?;
        let place = Place {
            folder: import.folder.clone(),
            imported_ms: time_ms(import.imported_at),
            fork: None,
        };
        out.absent(place_key(&name, machine), &place).await?;
    }
    Ok(())
}

async fn sync(out: &mut Writer<'_>, projects: &[ProjectSync]) -> Result<()> {
    for project in projects.iter().filter(|project| !project.pinned) {
        let mut folders = BTreeMap::new();
        for on in &project.machines {
            if let Some(machine) = machine(&on.machine_id) {
                folders.insert(machine, on.folder.clone());
            }
        }
        if folders.is_empty() {
            continue;
        }
        let spec = SyncSpec {
            folders,
            paused: false,
        };
        out.absent(Key::Sync(project.project.clone()), &spec)
            .await?;
    }
    Ok(())
}

/// A record for each credential the mesh has no saved one for.
async fn unmoved(out: &mut Writer<'_>, credentials: &[Credential]) -> Result<usize> {
    let mut count = 0;
    for credential in credentials {
        let mut shown = credential.clone();
        shown.unreadable = true;
        if out
            .absent(
                unmoved_credential(&credential.id),
                &Unmoved::Credential(shown),
            )
            .await?
        {
            count += 1;
        }
    }
    Ok(count)
}

async fn harness(
    out: &mut Writer<'_>,
    settings: &[(&'static str, Vec<MachineHarnessSettings>)],
) -> Result<()> {
    for (agent, machines) in settings {
        let Some(first) = machines.first() else {
            continue;
        };
        // On or off and the launch arguments are the same on every machine. The values of its env
        // vars never come back from the server, so a variable cannot be copied.
        let shared = &first.settings;
        for var in &shared.env {
            let record = Unmoved::HarnessEnv {
                agent: (*agent).to_owned(),
                name: var.name.clone(),
            };
            out.absent(unmoved_env(agent, &var.name), &record).await?;
        }
        if !shared.enabled || !shared.args.is_empty() {
            let config = HarnessConfig {
                enabled: shared.enabled,
                args: shared.args.clone(),
                ..HarnessConfig::new(agent)
            };
            out.absent(Key::Harness((*agent).to_owned()), &config)
                .await?;
        }
        for each in machines {
            let (program, home) = (&each.settings.program, &each.settings.home);
            let Some(id) = machine(&each.machine_id) else {
                continue;
            };
            if program.is_empty() && home.is_empty() {
                continue;
            }
            let own = HarnessConfig {
                program: Some(program.clone()).filter(|text| !text.is_empty()),
                home: Some(home.clone()).filter(|text| !text.is_empty()),
                ..HarnessConfig::new(agent)
            };
            out.absent(Key::Harness(format!("{agent}/{id}")), &own)
                .await?;
        }
    }
    Ok(())
}

/// Versions are numbered by their place in the list, so they come in only while the mesh holds
/// none but these: a version saved in the mesh first is the user's newer text. A version is the
/// same key each time, so a move that stopped halfway finishes the list.
async fn instructions(out: &mut Writer<'_>, versions: &[InstructionsVersion]) -> Result<()> {
    let keyed: Vec<(Key, SavedVersion)> = versions
        .iter()
        .map(|version| {
            let saved = SavedVersion {
                text: version.text.clone(),
                note: version.note.clone(),
                saved_ms: time_ms(version.saved_at),
            };
            let id = Uuid::new_v5(&Uuid::NAMESPACE_OID, &version.version.to_be_bytes());
            (
                Key::InstructionVersion(format!("{}.{id}", saved.saved_ms)),
                saved,
            )
        })
        .collect();
    let foreign = out
        .mesh
        .scan("instructions/")
        .into_iter()
        .any(|(held, _)| !keyed.iter().any(|(key, _)| *key == held));
    if foreign {
        return Ok(());
    }
    for (key, saved) in keyed {
        out.absent(key, &saved).await?;
    }
    Ok(())
}
