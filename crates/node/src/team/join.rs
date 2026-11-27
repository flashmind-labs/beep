//! Joining a team, both sides. The joiner sends `Join` with an invite's
//! secret and its forge CLI's login; the inviter answers `Challenge`, the
//! joiner pushes the proof ref, marks its commit in that login's name and
//! sends `Proved`, and the inviter checks the ref and that the login marked
//! it, deletes the ref and answers `Roster` or `Refused`. The joiner checks
//! every member of that roster on the forge in turn.

use super::{
    home::{Checkout, FORGE_LIMIT, Home},
    link::{TeamError, TeamLink, finish},
    proof::{Nonce, ProofError, drop_proof, proof_context, proof_seen, push_proof},
    roster::vet_roster,
};
use beep_mesh::{JoinSecret, TeamTicket, secret_hash};
use beep_proto::watched;
use beep_proto::{
    node::{
        LinkRx, LinkTx,
        tools::{Reply, Request, TeamJoinArgs},
    },
    team::{ForgeLogin, Member, TeamMsg, TeamRecord, TeamRepo},
};
use futures::future::join_all;
use iroh::{EndpointAddr, EndpointId};
use std::{
    collections::BTreeSet,
    future::Future,
    path::Path,
    time::{Duration, SystemTime},
};
use tokio::time::timeout;

/// How long a whole join may take, as the joiner waits.
const JOIN_LIMIT: Duration = Duration::from_secs(60);
/// How long the inviter takes for a whole join. It starts after the joiner
/// dialed and sent `Join`, and is shorter than `JOIN_LIMIT`, which starts
/// at the dial: the inviter never admits a joiner who gave up. Short in tests.
const ADMIT_LIMIT: Duration = Duration::from_secs(if cfg!(test) { 5 } else { 50 });
/// How long the inviter waits for `Proved`.
const PROVE_LIMIT: Duration = Duration::from_secs(45);
/// How long one git command of the proof may take.
const GIT_LIMIT: Duration = Duration::from_secs(20);
/// How long `answer` waits for a just-imported project to reach this node's
/// mesh state before refusing with `NotImported`.
const CHECKOUT_WAIT: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 5 });

/// A `Join` as the inviter takes it.
pub(crate) struct Join {
    pub(crate) secret: [u8; 32],
    pub(crate) repo: TeamRepo,
    pub(crate) login: ForgeLogin,
    pub(crate) mesh: EndpointId,
    pub(crate) devices: Vec<EndpointAddr>,
}

/// What the joiner sent in its `Join`: the entry it takes on the team.
pub(super) struct Sent {
    pub(super) login: ForgeLogin,
    pub(super) mesh: EndpointId,
    pub(super) devices: Vec<EndpointAddr>,
}

/// Answers `Tool::TeamJoin`: joins on this machine, which has the checkout.
pub(crate) async fn answer(link: Option<&TeamLink>, request: &Request) -> Reply {
    let joined = async {
        let link = link.ok_or("Teams are joined from a desktop of the mesh")?;
        let args: TeamJoinArgs = serde_json::from_str(&request.args)
            .map_err(|e| format!("The arguments do not fit: {e}"))?;
        let ticket = TeamTicket::parse(&args.ticket).map_err(|e| e.to_string())?;
        let checkout = checkout_for(link, &ticket.repo)
            .await
            .ok_or_else(|| TeamError::NotImported(ticket.repo.clone()).to_string())?;
        let record = link
            .join(ticket, &checkout.folder, &checkout.remote)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string(&record).map_err(|e| e.to_string())
    };

    match joined.await {
        Ok(text) => Reply::Done {
            text,
            failed: false,
        },
        Err(text) => Reply::Done { text, failed: true },
    }
}

/// The checkout of `repo` on this machine. When a project was just imported,
/// its `Key::Project` can still be replicating or waiting for `home::follow`,
/// so this waits briefly for the home task to publish it.
pub(super) async fn checkout_for(link: &TeamLink, repo: &TeamRepo) -> Option<Checkout> {
    let mut changes = link.home_changes();
    let ready = |home: &Home| home.checkout(repo).is_some();
    timeout(CHECKOUT_WAIT, watched::wait_for(&mut changes, ready))
        .await
        .ok()?
        .ok()?;

    link.home(|home| home.checkout(repo))
}

impl TeamLink {
    /// Joins the team `ticket` invites to, proving push access from
    /// `checkout` to `remote`. `JOIN_LIMIT` runs from the dial, so the
    /// inviter's shorter limit always ends first.
    pub(crate) async fn join(
        &self,
        ticket: TeamTicket,
        checkout: &Path,
        remote: &str,
    ) -> Result<TeamRecord, TeamError> {
        let login = self
            .forge()
            .login(&ticket.repo)
            .await
            .map_err(TeamError::Refused)?;
        let (mesh, devices) = self.home(|home| (home.mesh, home.devices(&ticket.repo)));
        let sent = Sent {
            login,
            mesh: mesh.ok_or(TeamError::NoMesh)?,
            devices,
        };

        timeout(JOIN_LIMIT, self.joining(ticket, &sent, checkout, remote))
            .await
            .map_err(|_| TeamError::TimedOut)?
    }

    async fn joining(
        &self,
        ticket: TeamTicket,
        sent: &Sent,
        checkout: &Path,
        remote: &str,
    ) -> Result<TeamRecord, TeamError> {
        let repo = ticket.repo;
        let join = TeamMsg::Join {
            secret: *ticket.secret.as_bytes(),
            repo: repo.clone(),
            login: sent.login.clone(),
            mesh: sent.mesh,
            devices: sent.devices.clone(),
        };
        let (mut tx, mut rx) = self.dial(&[EndpointAddr::new(ticket.inviter)]).await?;
        tx.send(&join).await?;

        let nonce = match rx.recv().await? {
            Some(TeamMsg::Challenge { nonce }) => nonce
                .parse::<Nonce>()
                .map_err(|_| TeamError::Refused("The inviter's challenge does not read".into()))?,
            other => return Err(unexpected(other)),
        };
        let sha = git_step(push_proof(checkout, remote, &repo, &nonce)).await?;
        self.forge()
            .stamp(&repo, &sha, &proof_context(&nonce))
            .await
            .map_err(|e| {
                TeamError::Refused(format!("Could not mark the proof on {}: {e}", repo.host))
            })?;
        tx.send(&TeamMsg::Proved {
            nonce: nonce.to_string(),
        })
        .await?;

        match rx.recv().await? {
            Some(TeamMsg::Roster { repo: of, members }) if of == repo => {
                self.check_roster(&repo, &members, ticket.inviter, sent)
                    .await?;
                Ok(TeamRecord::new(repo, members))
            }
            other => Err(unexpected(other)),
        }
    }

    /// Checks the roster the inviter answered with before taking it: the
    /// inviter and this mesh are on it, this mesh's entry is the one `sent`,
    /// every entry keeps the rules, and the forge says of no member of
    /// another mesh that they may not push. A member it cannot tell of is
    /// taken on the inviter's word: its mesh took them only on a definite
    /// yes, and the founder has no proof to check.
    pub(super) async fn check_roster(
        &self,
        repo: &TeamRepo,
        members: &[Member],
        inviter: EndpointId,
        sent: &Sent,
    ) -> Result<(), TeamError> {
        let names_inviter = members
            .iter()
            .any(|member| member.devices.iter().any(|device| device.id == inviter));
        let mut own = members.iter().filter(|member| member.mesh == sent.mesh);
        let ids = |devices: &[EndpointAddr]| -> BTreeSet<EndpointId> {
            devices.iter().map(|device| device.id).collect()
        };
        let own_as_sent = own.next().is_some_and(|entry| {
            entry.login == sent.login && ids(&entry.devices) == ids(&sent.devices)
        }) && own.next().is_none();

        if !names_inviter || !own_as_sent {
            return Err(TeamError::Refused(
                "The inviter's roster does not name the inviter and this mesh as it joined"
                    .to_owned(),
            ));
        }

        vet_roster(repo, members).map_err(|refusal| TeamError::Refused(refusal.to_string()))?;
        let others = members.iter().filter(|member| member.mesh != sent.mesh);
        let checks = others.map(|member| async move {
            let pushes = self.may_push(repo, &member.login).await;
            (member, pushes)
        });
        let answers = join_all(checks).await;

        match answers
            .into_iter()
            .find(|(_, pushes)| *pushes == Some(false))
        {
            Some((member, _)) => Err(TeamError::Refused(format!(
                "{} is not a collaborator on {repo}",
                member.login
            ))),
            None => Ok(()),
        }
    }

    /// The inviter's side of a join from `peer`: the team's roster or why
    /// not, within `ADMIT_LIMIT`. A proof ref the joiner may have pushed is
    /// deleted after the answer, however the check ended.
    pub(crate) async fn admit(
        &self,
        peer: EndpointId,
        join: Join,
        mut tx: LinkTx,
        mut rx: LinkRx,
    ) -> Result<(), TeamError> {
        let mut proof = None;
        let checked = timeout(
            ADMIT_LIMIT,
            self.check(peer, &join, &mut tx, &mut rx, &mut proof),
        )
        .await
        .unwrap_or_else(|_| Err("The join took too long".to_owned()));
        let answer = match checked {
            Ok(record) => {
                self.wake();
                self.spread(record.clone(), join.mesh);
                TeamMsg::Roster {
                    repo: record.repo,
                    members: record.members,
                }
            }
            Err(reason) => TeamMsg::Refused { reason },
        };

        let answered = async {
            tx.send(&answer).await?;
            finish(tx).await
        };
        let answered = answered.await;

        if let Some((checkout, nonce)) = proof
            && let Err(e) = git_step(drop_proof(&checkout.folder, &checkout.remote, &nonce)).await
        {
            log::debug!("no proof ref of a join to {} to delete: {e}", join.repo);
        }

        answered
    }

    /// Checks the invite and the joiner's entry, then the proof and the
    /// login, and adds the member. `proof` holds the challenge once sent. A
    /// refusal says nothing of this machine: details go to the log.
    async fn check(
        &self,
        peer: EndpointId,
        join: &Join,
        tx: &mut LinkTx,
        rx: &mut LinkRx,
        proof: &mut Option<(Checkout, Nonce)>,
    ) -> Result<TeamRecord, String> {
        let repo = &join.repo;
        let team = self.with(|teams| teams.rosters.get(repo).cloned());
        let (checkout, own) = self.home(|home| (home.checkout(repo), home.mesh));
        let entry = Member {
            login: join.login.clone(),
            mesh: join.mesh,
            devices: join.devices.clone(),
            joined_at: SystemTime::now(),
            admitted_by: own,
            proof: None,
        };
        let team = team.ok_or_else(|| format!("This device has no team for {repo}"))?;
        let hash = secret_hash(&JoinSecret::from(join.secret));
        let invite = team
            .invites
            .iter()
            .find(|invite| invite.secret_hash == hash)
            .ok_or_else(|| format!("This invite is not valid for {repo}"))?;

        if invite.expires_at <= SystemTime::now() {
            return Err("This invite expired: ask for a new one".to_owned());
        }

        self.with(|teams| teams.rosters.may_join(repo, peer, own, &entry))
            .map_err(|refusal| refusal.to_string())?;
        let checkout = checkout
            .ok_or_else(|| format!("This device has no checkout of {repo} to check with"))?;
        let nonce = Nonce::generate();
        let challenge = TeamMsg::Challenge {
            nonce: nonce.to_string(),
        };
        let proved = TeamMsg::Proved {
            nonce: nonce.to_string(),
        };
        let link_failed = |e: anyhow::Error| {
            log::debug!("a join to {repo} lost its link: {e:#}");
            "The link failed".to_owned()
        };
        tx.send(&challenge).await.map_err(link_failed)?;
        *proof = Some((checkout.clone(), nonce.clone()));
        let answer = timeout(PROVE_LIMIT, rx.recv::<TeamMsg>())
            .await
            .map_err(|_| "The proof took too long".to_owned())?
            .map_err(link_failed)?;

        if answer != Some(proved) {
            return Err("The joiner did not prove push access".to_owned());
        }

        let seen = git_step(proof_seen(&checkout.folder, &checkout.remote, &nonce))
            .await
            .map_err(|e| {
                log::warn!("could not check the proof of a join to {repo}: {e}");
                "Could not check the proof".to_owned()
            })?;
        let sha = seen.ok_or_else(|| ProofError::Denied(repo.to_string()).to_string())?;

        match self
            .marked(repo, &sha, &proof_context(&nonce), &join.login)
            .await
        {
            Some(true) => {}
            Some(false) => {
                return Err(format!(
                    "{} did not mark the proof on the forge",
                    join.login
                ));
            }
            None => return Err("Could not check access on the forge".to_owned()),
        }
        let entry = Member {
            proof: Some(sha),
            ..entry
        };
        let admitted = self
            .with(|teams| teams.rosters.admit(repo, peer, own, entry))
            .map_err(|refusal| refusal.to_string());

        if admitted.is_ok() {
            self.roster_edited();
        }

        admitted
    }

    /// Whether `login` marked commit `sha` of `repo` with a status whose
    /// context starts with `context`: the forge lets only those who may push
    /// mark a commit, and names who did. `None`, logged, when the forge
    /// cannot tell within `FORGE_LIMIT`.
    pub(super) async fn marked(
        &self,
        repo: &TeamRepo,
        sha: &str,
        context: &str,
        login: &ForgeLogin,
    ) -> Option<bool> {
        let answer = timeout(FORGE_LIMIT, self.forge().stampers(repo, sha, context)).await;
        let wanted = login.to_string();

        match answer {
            Ok(Ok(stampers)) => Some(stampers.iter().any(|by| by.eq_ignore_ascii_case(&wanted))),
            Ok(Err(e)) => {
                log::warn!("could not read the proof of {login} on {repo}: {e}");
                None
            }
            Err(_) => {
                log::warn!("the forge took too long to show the proof of {login} on {repo}");
                None
            }
        }
    }
}

/// One git command of the proof, within `GIT_LIMIT`.
async fn git_step<T>(step: impl Future<Output = Result<T, ProofError>>) -> Result<T, ProofError> {
    timeout(GIT_LIMIT, step)
        .await
        .map_err(|_| ProofError::Git("git took too long".to_owned()))?
}

/// The error for what the inviter answered instead of the next step.
fn unexpected(answer: Option<TeamMsg>) -> TeamError {
    match answer {
        Some(TeamMsg::Refused { reason }) => TeamError::Refused(reason),
        Some(_) => TeamError::Refused("The inviter answered out of turn".to_owned()),
        None => TeamError::Refused("The inviter closed the link".to_owned()),
    }
}
