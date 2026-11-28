//! Starting, finding and stopping the sessions this node runs.

use super::{
    AgentKind, AlertHub, Session,
    catalog::Catalog,
    checkout,
    checkouts::Checkouts,
    delete::Delete,
    family,
    family::{
        ChildStart, Family,
        links::{Links, MAX_DEPTH},
    },
    images::Images,
    listings::Listings,
    live::Mark,
    machine_list, projects,
    reopen::Reopen,
    reviews::kept::Reviews,
    session::{Begin, Start},
    sessions::Sessions,
    sessions::new_id,
    usage::Usage,
};
use crate::events::Hub;
use crate::machine::env::locate::folder;
use crate::occupancy::Occupancy;
use crate::process::spawn_env;
use anyhow::{Context, Result};
use beep_proto::SessionId;
use beep_proto::node::agent::{CheckoutPlan, ParentLink, Persona, Status};
use beep_proto::watched;
use beep_proto::{AccountName, AgentAccount};
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

/// Starts sessions: new ones, resumed ones, reopened ones and reviewers,
/// and puts each in its family.
pub(crate) struct Starter {
    pub(super) catalog: Arc<Catalog>,
    pub(super) sessions: Arc<Sessions>,
    pub(super) links: Arc<Links>,
    pub(super) family: Arc<Family>,
    pub(super) occupancy: Occupancy,
    pub(super) usage: Arc<Usage>,
    pub(super) images: Arc<Images>,
    pub(super) alerts: Arc<AlertHub>,
    pub(super) checkouts: Arc<Checkouts>,
    pub(super) events: Hub,
    pub(super) reopen: Arc<Reopen>,
    pub(super) listings: Arc<Listings>,
    pub(super) reviews: Arc<Reviews>,
    pub(super) delete: Arc<Delete>,
}

impl Starter {
    /// Starts a session in the branch or worktree `checkout` makes in
    /// `cwd`, or in `cwd` itself without one. A start that would be refused
    /// is refused before anything is made.
    pub(super) async fn start_in(
        &self,
        kind: &AgentKind,
        cwd: Option<&str>,
        checkout: Option<CheckoutPlan>,
        mut begin: Begin,
    ) -> Result<Arc<Session>> {
        self.catalog.prepare(kind).await?;
        self.admits(kind, cwd, &begin)?;

        let prompt = begin.prompt.as_deref().unwrap_or_default();
        let place = checkout::place(cwd.map(str::to_owned), checkout, prompt).await?;
        begin.note = place.note;
        begin.brief = place.brief.map(Cow::Owned);
        begin.worktrees.extend(place.worktree);
        self.start(kind, place.folder.as_deref(), begin).await
    }

    /// Refuses what `start` refuses of a session asked to run in `cwd`,
    /// beyond what `prepare` refuses, so that `start_in` makes nothing for
    /// it: an account `kind` lacks here, or a checkout git sync is
    /// preserving local edits in.
    fn admits(&self, kind: &AgentKind, cwd: Option<&str>, begin: &Begin) -> Result<()> {
        signs_in(&kind.name, begin.account.as_ref())?;
        let (cwd, _) = in_project(cwd, begin.resume.is_some(), None)?;

        self.occupancy.holds().check_start(&cwd)
    }

    /// Starts an issue's automatic session: the `auto_harness`, on the
    /// worktree `plan` makes in `cwd`, titled `title`. Its id, once the
    /// agent names it as listings and cards do, and the worktree it works
    /// in (the session itself runs in the project folder).
    pub(crate) async fn start_on_issue(
        &self,
        cwd: &str,
        plan: CheckoutPlan,
        title: String,
        prompt: String,
    ) -> Result<(SessionId, String)> {
        let kind = self
            .catalog
            .auto_harness()
            .context("this machine has no coding agent")?;
        let begin = Begin {
            prompt: Some(prompt),
            title: Some(title),
            ..Begin::default()
        };
        let session = self.start_in(&kind, Some(cwd), Some(plan), begin).await?;
        let named = session.named_info().await.map_err(anyhow::Error::msg)?;

        let worktree = named.worktrees.into_iter().next();

        Ok((named.id, worktree.unwrap_or(named.cwd)))
    }

    pub(crate) async fn start(
        &self,
        kind: &AgentKind,
        cwd: Option<&str>,
        mut begin: Begin,
    ) -> Result<Arc<Session>> {
        let kind = &self.catalog.settings.configured(kind)?;
        signs_in(&kind.name, begin.account.as_ref())?;
        let own = own_folder(cwd, &begin)?;
        let (cwd, brief) = in_project(
            own.as_deref().or(cwd),
            begin.resume.is_some(),
            begin.brief.take(),
        )?;
        let brief = match begin.instructions.take() {
            Some(instructions) => {
                super::session::standing(brief.map(Cow::into_owned), Some(&instructions))
                    .map(Cow::Owned)
            }
            None => brief,
        };
        // Only the first start reads the earlier session; a resume has it.
        let brief = match begin.continues.as_ref().filter(|_| begin.resume.is_none()) {
            Some(earlier) => {
                if begin.title.is_none()
                    && let Some(title) = self
                        .sessions
                        .get(earlier)
                        .and_then(|session| session.info().title)
                        .or_else(|| {
                            self.reopen
                                .saved(earlier)
                                .and_then(|saved| saved.info.title)
                        })
                        .filter(|title| !title.trim().is_empty())
                {
                    begin.title = Some(title);
                    begin.named = true;
                }
                let said = super::brief::continues(earlier);
                Some(Cow::Owned(match brief {
                    Some(brief) => format!("{brief}\n\n{said}"),
                    None => said,
                }))
            }
            None => brief,
        };
        let team = self
            .links
            .mesh()
            .and_then(|mesh| crate::membership::team_repo_of(&mesh, &cwd));
        let brief = match team {
            Some(_) => Some(Cow::Owned(match brief {
                Some(brief) => format!("{brief}\n\n{}", super::brief::TEAM),
                None => super::brief::TEAM.to_owned(),
            })),
            None => brief,
        };

        // Only a new session's first prompt names its issue.
        if begin.resume.is_none()
            && begin.issue.is_none()
            && let (Some(repo), Some(prompt)) = (&team, &begin.prompt)
        {
            begin.issue = super::start::issue_of_prompt(repo, prompt);
        }

        if let Some(mesh) = self.links.mesh() {
            projects::may_run(&mesh, &cwd, &kind.name).map_err(anyhow::Error::msg)?;
        }

        let resume = begin.resume.clone();
        // The id is taken first, so no other start takes it while this one
        // spawns. Recovery reads the starts under way.
        let reservation = self.sessions.reserve(resume.clone())?;
        let id = reservation.id.clone();
        let running = self.occupancy.holds().running(cwd.clone());
        self.occupancy.holds().check_start(&cwd)?;
        // A reviewer changes nothing, so it needs no git rule.
        let rule = checkout::rule(&cwd)
            .filter(|_| !begin.review)
            .map(str::to_owned);
        let instructions = crate::lock(&self.catalog.instructions)
            .clone()
            .filter(|text| !text.trim().is_empty());
        let resumed = begin.resume.is_some();
        // A reviewer tells the user nothing itself: the review does.
        let alerts = if begin.review {
            Arc::default()
        } else {
            self.alerts.clone()
        };
        let mesh = self.links.mesh();
        let context = super::brief::Context {
            machine: mesh
                .as_ref()
                .map(|mesh| machine_list::machine_name(Some(mesh)))
                .filter(|name| !name.is_empty()),
            project: mesh
                .as_ref()
                .and_then(|mesh| projects::of_folder(mesh, &cwd.to_string_lossy(), mesh.me())),
            multi_machine: mesh.as_ref().is_some_and(|mesh| !machine_list::alone(mesh)),
            rust_or_c: super::brief::has_native_build(&cwd),
        };
        begin.brief = brief;
        let review = begin.review;
        // A WSL agent cannot reach a socket on Windows.
        let docker = match !super::kinds::in_wsl(kind) && docker_containers(mesh.as_ref()) {
            true => {
                super::processes::Docker::open(kind.env.get("DOCKER_HOST").map(String::as_str))
                    .await
            }
            false => None,
        };
        let mut session = Session::start(Start {
            id: id.clone(),
            kind,
            cwd: &cwd,
            begin,
            instructions,
            rule,
            skills: self
                .catalog
                .skills_dir
                .as_deref()
                .and_then(crate::machine::provision::skills_brief),
            mcp: self.catalog.mcp.as_ref().map(|launch| launch.server(&id)),
            context,
            images: self.images.clone(),
            usage: self.usage.hub.clone(),
            alerts,
            checkouts: self.checkouts.clone(),
            stopping: self.sessions.stopping.clone(),
            running: Some(running),
            docker,
        })?;
        session.reviewer = review;
        let session = Arc::new(session);
        let how = if resumed { "took over" } else { "started" };
        log::info!("{how} {} session {id} in {}", kind.name, cwd.display());
        let events = self.events.clone();
        let mut updates = session.live.clone();
        session.keep(tokio::spawn(async move {
            let mut previous = None;
            let mut saved = None;

            loop {
                let (info, effort, fast) = watched::read_and_update(&mut updates, |live| {
                    (live.info.clone(), live.effort, live.fast)
                });

                // What the saved session keeps, noted as it changes.
                let keeps = (
                    info.id.clone(),
                    info.title.clone(),
                    info.model.clone(),
                    effort,
                    fast,
                    info.mode.clone(),
                    info.device.clone(),
                    info.worktrees.clone(),
                );

                if saved.as_ref() != Some(&keeps) {
                    crate::events::open_changed();
                    saved = Some(keeps);
                }
                events.changed(beep_proto::node::events::Topic::keyed("agent", &info.id));
                let summary = (
                    info.id.clone(),
                    info.state.status,
                    info.title.clone(),
                    info.turns,
                    info.state.tasks.clone(),
                    info.place.clone(),
                );

                if previous.as_ref() != Some(&summary) {
                    events.changed(beep_proto::node::events::Topic::new("agents"));
                    // An ended session lists no processes; its watch reports the rest.
                    let ended = |status| status == Status::Ended;

                    if previous
                        .as_ref()
                        .is_none_or(|(_, status, ..)| ended(*status) != ended(info.state.status))
                    {
                        events.changed(beep_proto::node::events::Topic::new("processes"));
                    }

                    previous = Some(summary);
                }

                if updates.changed().await.is_err() {
                    return;
                }
            }
        }));
        self.sessions.insert(reservation, resume, session.clone());
        crate::events::open_changed();
        Ok(session)
    }
}

/// Refuses a start as `account` that this node cannot sign `agent` in as,
/// rather than run it on another account.
fn signs_in(agent: &str, account: Option<&AccountName>) -> Result<()> {
    let Some(account) = account else {
        return Ok(());
    };
    let Ok(keeps) = AgentAccount::from_str(agent) else {
        anyhow::bail!("{agent} has no named accounts; start it on its default account");
    };
    anyhow::ensure!(
        spawn_env::has_account(keeps, account),
        "this machine does not have {agent}'s account {account} yet; try again once it has synced, or add it in Agent accounts"
    );
    Ok(())
}

/// The folder a session asked to run in `cwd` runs in, with Beep's brief
/// to it: the project folder when `cwd` is in a linked worktree, even a
/// removed one, so the session outlives its worktrees. The brief then names
/// the worktree, unless one came already.
fn in_project(
    cwd: Option<&str>,
    resumed: bool,
    brief: Option<Cow<'static, str>>,
) -> Result<(PathBuf, Option<Cow<'static, str>>)> {
    let asked = folder(cwd).or_else(|e| {
        cwd.map(PathBuf::from)
            .filter(|gone| checkout::project_of(gone).is_some())
            .ok_or(e)
    })?;
    let Some(project) = checkout::project_of(&asked) else {
        return Ok((asked, brief));
    };

    let (project_shown, tree) = (project.display(), asked.display());
    let said = match (resumed, asked.exists()) {
        (false, _) => format!(
            "This session runs in the project folder {project_shown}. Work and commit in the worktree {tree}."
        ),
        (true, true) => format!(
            "This session now runs in the project folder {project_shown}. Its work so far is in the worktree {tree}; continue there."
        ),
        (true, false) => format!(
            "This session now runs in the project folder {project_shown}. The worktree it worked in, {tree}, was removed."
        ),
    };
    Ok((project, brief.or(Some(Cow::Owned(said)))))
}

/// The folder of its own a new assistant conversation starts in, which its
/// threads share. `None` for every other start.
/// Whether new sessions list the Docker containers they start: the shared
/// setting, on until turned off, and on without a mesh.
fn docker_containers(mesh: Option<&beep_mesh::Mesh>) -> bool {
    use beep_proto::node::mesh_state::{DockerSettings, StateValue};

    mesh.and_then(|mesh| mesh.get(&beep_mesh::Key::Docker))
        .and_then(|bytes| DockerSettings::from_bytes(&bytes).ok())
        .and_then(|settings| settings.containers)
        .unwrap_or(true)
}

fn own_folder(cwd: Option<&str>, begin: &Begin) -> Result<Option<String>> {
    if !begin.assistant || begin.resume.is_some() {
        return Ok(None);
    }

    let home = dirs::home_dir().context("this machine has no home folder")?;
    let made =
        super::conversation::own(&home, cwd, new_id).context("making the conversation's folder")?;
    Ok(made.map(|folder| folder.to_string_lossy().into_owned()))
}

impl Starter {
    /// Starts the child `link` describes, here. The node that starts it
    /// checks the limits, whichever machine the parent is on.
    pub(crate) async fn start_child(
        &self,
        link: ParentLink,
        start: ChildStart,
    ) -> Result<Arc<Session>, String> {
        if link.depth == 0 || link.depth > MAX_DEPTH {
            return Err(family::too_deep());
        }

        let kind = self.catalog.kind_or_refusal(&start.harness)?;
        let mode = match &start.review {
            Some(_) => family::review_mode(&link, &kind.name)?,
            None => family::child_mode(&link, &kind.name)?,
        };
        let follows = self.family.parent_live_of(&link);
        let parent = (link.machine == self.links.me())
            .then(|| self.sessions.get(&link.session))
            .flatten();
        let root = (link.root_machine, link.root.clone());
        let title = start.title.filter(|title| !title.trim().is_empty());
        let begin = Begin {
            prompt: Some(start.prompt),
            named: title.is_some(),
            title,
            model: start.model,
            effort: start.review.as_ref().and_then(|review| review.effort),
            fast: start.review.as_ref().and_then(|review| review.fast),
            mode,
            parent: Some(link),
            follows,
            review: start.review.is_some(),
            account: start
                .account
                .filter(|_| AgentAccount::from_str(&kind.name).is_ok()),
            persona: start.persona.as_ref().map(Into::<Persona>::into),
            instructions: start
                .persona
                .as_ref()
                .map(crate::custom_agents::persona_brief),
            ..Begin::default()
        };
        let elsewhere = self.family.live_elsewhere(&root).await;
        // Held until the child is listed. It then counts twice for an
        // instant, which can refuse a start but never admit one too many.
        let place = self.family.take_place(&root, elsewhere)?;
        let session = self
            .start_in(&kind, start.cwd.as_deref(), None, begin)
            .await
            .map_err(|e| format!("{e:#}"))?;
        drop(place);
        self.family.watch_parent(&session);

        // Its prompt says what the parent knew: it has seen the parent so far.
        if let Some(parent) = parent {
            session
                .see(vec![(
                    parent.id.clone(),
                    watched::read(&parent.live, |live| Into::<Mark>::into(live)),
                )])
                .await;
        }

        session.named_info().await?;
        Ok(session)
    }
}

/// The subfolder of its own, made now, that a child called `title`
/// starts in when its parent names none and `parent` is in an assistant
/// conversation's folder. `None` anywhere else: it starts in the parent's.
pub(crate) fn child_folder(parent: &Session, title: Option<&str>) -> Result<Option<PathBuf>> {
    let Some(home) = dirs::home_dir() else {
        return Ok(None);
    };

    super::conversation::thread_folder(&home, Path::new(&parent.info().cwd), title, new_id)
        .context("making the thread's folder")
}
