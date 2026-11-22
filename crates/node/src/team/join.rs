//! Joining a team, both sides. The joiner sends `Join` with an invite's
//! secret and its forge CLI's login; the inviter answers `Challenge`, the
//! joiner pushes the proof ref, marks its commit in that login's name and
//! sends `Proved`, and the inviter checks the ref and that the login marked
//! it, deletes the ref and answers `Roster` or `Refused`. The joiner checks
//! every member of that roster on the forge in turn.

use super::{
    home::Checkout,
    link::{TeamError, TeamLink, finish},
    gateway::FORGE_LIMIT,
    proof::{Nonce, ProofError, drop_proof, proof_context, proof_seen, push_proof},
    roster::vet_roster,
};
use beep_mesh::{JoinSecret, TeamTicket, secret_hash};
use beep_proto::{
    node::{
        LinkRx, LinkTx,
        tools::{Reply, Request, TeamJoinArgs},
    },
    team::{ForgeLogin, Member, TeamMsg, TeamRecord, TeamRepo},
};
use futures::future::try_join_all;
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

/// A `Join` as the inviter takes it.
pub(super) struct Join {
    pub(super) secret: [u8; 32],
    pub(super) repo: TeamRepo,
    pub(super) login: ForgeLogin,
    pub(super) mesh: EndpointId,
    pub(super) devices: Vec<EndpointAddr>,
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
        let checkout = link
            .with(|teams| teams.home.checkout(&ticket.repo))
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
            .map_err(TeamError::Forge)?;
        let (mesh, devices) =
            self.with(|teams| (teams.home.mesh, teams.home.devices(&ticket.repo)));
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
            .map_err(|e| TeamError::Forge(format!("Could not mark the proof on {}: {e}", repo.host)))?;
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

    /// Checks the roster the inviter answered with before taking it, as a
    /// member checks a newcomer: the inviter and this mesh are on it, this
    /// mesh's entry is the one `sent`, every entry keeps the rules, and the
    /// forge says every member of another mesh may push.
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
        try_join_all(others.map(|member| self.vouch(repo, &member.login)))
            .await
            .map_err(TeamError::Refused)?;

        Ok(())
    }

    /// The inviter's side of a join from `peer`: the team's roster or why
    /// not, within `ADMIT_LIMIT`. A proof ref the joiner may have pushed is
    /// deleted after the answer, however the check ended.
    pub(super) async fn admit(
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
        let (team, checkout, own) = self.with(|teams| {
            let team = teams.rosters.get(repo).cloned();
            (team, teams.home.checkout(repo), teams.home.mesh)
        });
        let entry = Member {
            login: join.login.clone(),
            mesh: join.mesh,
            devices: join.devices.clone(),
            joined_at: SystemTime::now(),
            admitted_by: own,
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

        self.marked(repo, &sha, &nonce, &join.login).await?;
        self.with(|teams| teams.rosters.admit(repo, peer, own, entry))
            .map_err(|refusal| refusal.to_string())
    }

    /// Whether `login` marked the proof commit `sha` of `nonce`: the forge
    /// lets only those who may push mark a commit, and names who did. The
    /// reason to give when not, which says nothing of this machine.
    async fn marked(
        &self,
        repo: &TeamRepo,
        sha: &str,
        nonce: &Nonce,
        login: &ForgeLogin,
    ) -> Result<(), String> {
        let context = proof_context(nonce);
        let answer = timeout(FORGE_LIMIT, self.forge().stampers(repo, sha, &context)).await;
        let by = |stampers: &[String]| {
            let login = login.to_string();
            stampers.iter().any(|by| by.eq_ignore_ascii_case(&login))
        };

        match answer {
            Ok(Ok(stampers)) if by(&stampers) => Ok(()),
            Ok(Ok(_)) => Err(format!("{login} did not mark the proof on the forge")),
            Ok(Err(e)) => {
                log::warn!("could not read the proof of {login} on {repo}: {e}");
                Err("Could not check access on the forge".to_owned())
            }
            Err(_) => {
                log::warn!("the forge took too long to show the proof of {login} on {repo}");
                Err("Could not check access on the forge".to_owned())
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
