//! A headless stand-in for the desktop, for walking a mesh by hand over
//! real machines with no server. It keeps a mesh in `--dir` under a key of
//! its own, serves the join and peer protocols, and takes one command per
//! line on stdin. Every command prints what it did and how long it took.
//!
//!     e2e_desktop --dir ~/.d1 --name mac < commands
//!
//! Commands: `ticket <label>`, `wait <machine> [secs]`, `machines`,
//! `agent <label> <machine> <cwd> <prompt>`, `answer <label> <option>`,
//! `attach <label> <machine> <session>`, `sessions <machine>`,
//! `term <machine> <cwd|-> <command>`, `diff <machine> <root>`,
//! `read <machine> <root> <path>`, `bench <n> list|read <machine> [root path]`,
//! `exec <machine> <command>`, `import <repo> <machine> [folder]`,
//! `syncset <project> <machine>=<folder>...`, `syncstatus`,
//! `remove <machine>`, `quit`.
//!
//! A ticket is written to `<dir>/ticket-<label>`, never printed.

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use beep_fleet::{Desk, FleetError, ImportJob, Peers};
use beep_mesh::{
    DeviceKind, JOIN_ALPN, Key, Mesh, PEER_ALPN, PeerClient,
    machine::{
        self,
        agent::{Opened, open_agent},
        sessions::{self, NodeFile, NodeWork, WorkRead},
        terminals,
    },
    transport_config,
};
use beep_proto::client::bind_endpoint;
use beep_proto::node::{
    agent::{DiffAsk, Event, Request},
    mesh_state::{StateValue, SyncSpec},
    process::TaskId,
    status::{self, Report},
    sync::{PROTOCOL as SYNC, Reply as SyncReply, Request as SyncRequest},
    terminal::{Reply as TermReply, Size},
};
use iroh::{Endpoint, EndpointId, SecretKey, protocol::Router};
use iroh_tickets::Ticket;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    time::timeout,
};

struct Mine {
    mesh: Mesh,
    me: EndpointId,
}

#[async_trait]
impl Peers for Mine {
    async fn mesh(&self) -> beep_fleet::Result<Mesh> {
        Ok(self.mesh.clone())
    }

    fn me(&self) -> EndpointId {
        self.me
    }

    async fn client(&self, machine: EndpointId) -> beep_fleet::Result<PeerClient> {
        beep_mesh::with_deadline(machine, &self.mesh)
            .await
            .map_err(|_| FleetError::Offline(machine.fmt_short().to_string()))
    }
}

fn key_in(dir: &Path) -> Result<SecretKey> {
    let file = dir.join("key");
    match fs::read(&file) {
        Ok(bytes) => {
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| anyhow!("key is not 32 bytes"))?;
            Ok(SecretKey::from_bytes(&bytes))
        }
        Err(_) => {
            let key = SecretKey::generate();
            fs::write(&file, key.to_bytes())?;
            Ok(key)
        }
    }
}

struct Driver {
    dir: PathBuf,
    mesh: Mesh,
    me: EndpointId,
    desk: Desk,
    sessions: HashMap<String, Opened>,
}

impl Driver {
    fn id_of(&self, name: &str) -> Result<EndpointId> {
        self.mesh
            .devices()
            .into_iter()
            .find(|d| d.device.name == name)
            .map(|d| d.device.id)
            .with_context(|| format!("no machine called {name}"))
    }

    async fn client(&self, name: &str) -> Result<PeerClient> {
        let id = self.id_of(name)?;
        beep_mesh::with_deadline(id, &self.mesh)
            .await
            .map_err(|e| anyhow!("{name}: {e}"))
    }

    async fn run(&mut self, line: &str) -> Result<()> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let rest = |n: usize| words[n..].join(" ");
        match words.as_slice() {
            ["ticket", label] => {
                let ticket = self
                    .mesh
                    .invite(DeviceKind::Node, Duration::from_secs(3600))
                    .await?;
                let file = self.dir.join(format!("ticket-{label}"));
                fs::write(&file, ticket.encode_string())?;
                println!("ticket written to {}", file.display());
            }
            ["wait", name, rest @ ..] => {
                let secs: u64 = rest.first().and_then(|s| s.parse().ok()).unwrap_or(60);
                let id = loop_until(secs, || {
                    let want = *name;
                    self.mesh
                        .devices()
                        .into_iter()
                        .find(|d| d.device.name == want && d.online)
                        .map(|d| d.device.id)
                })
                .await
                .with_context(|| format!("{name} did not come online in {secs}s"))?;
                println!("{name} online as {}", id.fmt_short());
            }
            ["machines"] => {
                for d in self.mesh.devices() {
                    let mut said = String::new();
                    if d.online
                        && let Ok(client) = beep_mesh::with_deadline(d.device.id, &self.mesh).await
                    {
                        said = probe(&client).await;
                    }
                    println!(
                        "  {:<10} {:?} {} {}",
                        d.device.name,
                        d.device.kind,
                        if d.online { "online" } else { "offline" },
                        said
                    );
                }
            }
            ["agent", label, name, cwd, ..] => {
                let client = self.client(name).await?;
                let prompt = rest(4);
                let opened = open_agent(
                    &client,
                    "codex",
                    Request::Start {
                        cwd: Some((*cwd).to_owned()),
                        prompt,
                        model: None,
                        effort: None,
                        checkout: None,
                    },
                )
                .await?;
                println!("session {} on {name}", opened.info.id);
                self.sessions.insert((*label).to_owned(), opened);
                self.pump(label).await?;
            }
            ["answer", label, option] => {
                let opened = self.sessions.get_mut(*label).context("no such session")?;
                let id = opened
                    .info
                    .question
                    .as_ref()
                    .map(|q| q.id)
                    .context("the session asks nothing")?;
                opened
                    .tx
                    .send(&Request::Answer {
                        question: id,
                        reply: (*option).to_owned(),
                    })
                    .await?;
                opened.info.question = None;
                self.pump(label).await?;
            }
            ["attach", label, name, session] => {
                let client = self.client(name).await?;
                let opened = open_agent(
                    &client,
                    "codex",
                    Request::Attach {
                        session: (*session).to_owned(),
                    },
                )
                .await?;
                println!(
                    "attached {} turns={} status={:?}",
                    opened.info.id, opened.info.turns, opened.info.state.status
                );
                self.sessions.insert((*label).to_owned(), opened);
            }
            ["sessions", name] => {
                let client = self.client(name).await?;
                for s in sessions::list(&client).await? {
                    println!(
                        "  {} {} turns={} status={:?} cwd={}",
                        s.id, s.agent, s.turns, s.state.status, s.cwd
                    );
                }
            }
            ["term", name, cwd, ..] => {
                let client = self.client(name).await?;
                let cwd = (*cwd != "-").then(|| (*cwd).to_owned());
                let task = terminals::open(&client, Some(rest(3)), cwd, Size::DEFAULT).await?;
                println!("terminal {} started", task.id.0);
                let (_tx, mut rx) =
                    terminals::attach(&client, TaskId(task.id.0), 0, Size::DEFAULT).await?;
                let mut text = Vec::new();
                loop {
                    match timeout(Duration::from_secs(10), rx.recv::<TermReply>()).await {
                        Ok(Ok(Some(TermReply::Output { data, .. }))) => {
                            text.extend_from_slice(&data)
                        }
                        Ok(Ok(Some(TermReply::Ended { .. }))) | Ok(Ok(None)) => break,
                        Ok(Ok(Some(_))) => {}
                        Ok(Err(e)) => bail!("terminal stream: {e}"),
                        Err(_) => break,
                    }
                }
                println!("terminal output: {:?}", String::from_utf8_lossy(&text));
            }
            ["diff", name, root] => {
                let client = self.client(name).await?;
                let ask = DiffAsk {
                    root: (*root).to_owned(),
                    base: String::new(),
                    head: String::new(),
                    only: Vec::new(),
                    lockfiles: false,
                };
                match sessions::diff(&client, ask).await? {
                    NodeWork::Done(diff) => {
                        println!(
                            "diff: {} files +{} -{}",
                            diff.files.len(),
                            diff.additions,
                            diff.deletions
                        );
                        for f in &diff.files {
                            println!("  {f:?}");
                        }
                    }
                    NodeWork::Refused(why) => bail!("diff refused: {why}"),
                }
            }
            ["read", name, root, path] => {
                let client = self.client(name).await?;
                println!("{}", read_one(&client, root, path).await?);
            }
            ["bench", n, what, name, rest @ ..] => {
                let n: usize = n.parse()?;
                let client = self.client(name).await?;
                // One call first, so a connection that has to open does not count.
                let mut times = Vec::new();
                for i in 0..=n {
                    let t = Instant::now();
                    match *what {
                        "list" => {
                            sessions::list(&client).await?;
                        }
                        "read" => {
                            let [root, path] = rest else {
                                bail!("bench read needs root and path")
                            };
                            read_one(&client, root, path).await?;
                        }
                        other => bail!("bench what? {other}"),
                    }
                    if i > 0 {
                        times.push(t.elapsed());
                    }
                }
                times.sort();
                println!(
                    "bench {what} on {name}: n={n} min={:?} median={:?} max={:?}",
                    times[0],
                    times[times.len() / 2],
                    times[times.len() - 1]
                );
            }
            ["exec", name, ..] => {
                let client = self.client(name).await?;
                let done = machine::exec(&client, &rest(2), Duration::from_secs(60)).await?;
                println!("exit={:?} output={:?}", done.exit_code, done.output);
            }
            ["import", repo, name, rest @ ..] => {
                let id = self.id_of(name)?.to_string();
                let folder = rest.first().copied();
                let landed = self
                    .desk
                    .import(ImportJob {
                        repo,
                        machine_id: Some(&id),
                        folder,
                        parent: None,
                        worktrees: &[],
                    })
                    .await?;
                println!(
                    "imported {} on {} at {} cloned={}",
                    landed.project, landed.machine_name, landed.folder, landed.cloned
                );
                for p in self.desk.imported().await {
                    println!(
                        "  recorded: {} on {} {}",
                        p.project, p.machine_name, p.folder
                    );
                }
            }
            ["syncset", project, places @ ..] => {
                let mut spec = SyncSpec::default();
                for place in places {
                    let (name, folder) = place.split_once('=').context("name=folder")?;
                    spec.folders.insert(self.id_of(name)?, folder.to_owned());
                }
                self.mesh
                    .set(Key::Sync((*project).to_owned()), Some(spec.to_bytes()?))
                    .await?;
                println!("sync spec for {project} written to the mesh");
            }
            ["syncstatus"] => {
                for d in self.mesh.devices() {
                    if !d.online || d.device.id == self.me {
                        continue;
                    }
                    let Ok(client) = beep_mesh::with_deadline(d.device.id, &self.mesh).await else {
                        continue;
                    };
                    let reply: Result<SyncReply, _> =
                        client.call(SYNC, &SyncRequest::Status {}).await;
                    match reply {
                        Ok(SyncReply::Status { projects }) => {
                            for p in projects {
                                println!(
                                    "  {} {} {:?} files={} pending={} err={:?}",
                                    d.device.name, p.name, p.state, p.files, p.pending, p.error
                                );
                            }
                        }
                        other => println!("  {} sync: {other:?}", d.device.name),
                    }
                }
            }
            ["remove", name] => {
                let id = self.id_of(name)?;
                self.mesh.remove(id).await?;
                println!("removed {name}");
            }
            ["members"] => {
                for d in self.mesh.devices() {
                    println!(
                        "  {} {}",
                        d.device.name,
                        if d.online { "online" } else { "offline" }
                    );
                }
            }
            [] => {}
            other => bail!("unknown command {other:?}"),
        }
        Ok(())
    }

    /// Prints the session's events until its turn ends, it asks, or it ends.
    async fn pump(&mut self, label: &str) -> Result<()> {
        let opened = self.sessions.get_mut(label).context("no such session")?;
        loop {
            let event = timeout(Duration::from_secs(90), opened.rx.recv::<Event>())
                .await
                .context("no event in 90s")??;
            match event {
                Some(Event::Question { question }) => {
                    println!(
                        "question {}: {} {:?} options={:?}",
                        question.id,
                        question.text,
                        question.detail,
                        question.options.iter().map(|o| &o.id).collect::<Vec<_>>()
                    );
                    opened.info.question = Some(question);
                    return Ok(());
                }
                Some(Event::TurnDone { .. }) => {
                    println!("turn done");
                    return Ok(());
                }
                Some(Event::Ended { reason }) => {
                    println!("ended: {reason}");
                    return Ok(());
                }
                Some(Event::Refused { reason }) => bail!("refused: {reason}"),
                Some(Event::State { state }) => {
                    println!("  state {:?} {}", state.status, state.summary)
                }
                Some(_) => {}
                None => bail!("stream closed"),
            }
        }
    }
}

async fn read_one(client: &PeerClient, root: &str, path: &str) -> Result<String> {
    let read = WorkRead {
        root,
        path,
        offset: 0,
        len: 64 * 1024,
    };
    Ok(match sessions::read_file(client, read).await? {
        NodeFile::Chunk { size, mime, data } => format!(
            "file size={size} mime={mime} head={:?}",
            String::from_utf8_lossy(&data[..data.len().min(60)])
        ),
        NodeFile::Missing => "file missing".to_owned(),
        NodeFile::Refused(why) => format!("refused: {why}"),
    })
}

async fn loop_until<T>(secs: u64, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return Some(v);
        }
        if Instant::now() > end {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn probe(client: &PeerClient) -> String {
    let read = async {
        let (_tx, mut rx) = client.open(status::PROTOCOL).await.ok()?;
        let (mut offers, mut env) = (None, None);
        while offers.is_none() || env.is_none() {
            match rx.recv::<Report>().await.ok()?? {
                Report::Offers(o) => offers = Some(o),
                Report::Env(e) => env = Some(e),
                _ => {}
            }
        }
        let env = env?;
        Some(format!(
            "os={} {} host={} offers={}",
            env.os,
            env.arch,
            env.hostname,
            offers?.len()
        ))
    };
    timeout(Duration::from_secs(5), read)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "(no report)".to_owned())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let dir = PathBuf::from(flag("--dir").context("--dir")?);
    let name = flag("--name").unwrap_or_else(|| "desktop".to_owned());
    fs::create_dir_all(&dir)?;
    let key = key_in(&dir)?;
    let endpoint: Endpoint = bind_endpoint(key, transport_config()).await?;
    let mesh = match Mesh::open(&dir, endpoint.clone()).await? {
        Some(mesh) => {
            println!("opened the mesh");
            mesh
        }
        None => {
            println!("made a new mesh");
            Mesh::create(&dir, endpoint.clone(), &name).await?
        }
    };
    let (peer, join) = mesh.protocols();
    let router = Router::builder(endpoint.clone())
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .spawn();
    let me = endpoint.id();
    let desk = Desk::open(Arc::new(Mine {
        mesh: mesh.clone(),
        me,
    }))
    .await?;
    println!("up as {}", me.fmt_short());
    let mut driver = Driver {
        dir,
        mesh,
        me,
        desk,
        sessions: HashMap::new(),
    };
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line == "quit" {
            break;
        }
        println!("> {line}");
        let t = Instant::now();
        match driver.run(line).await {
            Ok(()) => println!("ok ({} ms)", t.elapsed().as_millis()),
            Err(e) => println!("FAIL ({} ms): {e:#}", t.elapsed().as_millis()),
        }
    }
    router.shutdown().await.ok();
    Ok(())
}
