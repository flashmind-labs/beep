//! Joining a team, both sides. The joiner sends `Join` with an invite's
//! secret; the inviter answers `Challenge`, the joiner pushes the proof ref
//! and sends `Proved`, and the inviter checks the ref and that the login is
//! a collaborator, deletes the ref and answers `Roster` or `Refused`.

use super::{
    link::{TeamError, TeamLink, finish},
    proof::{Nonce, ProofError, drop_proof, proof_seen, push_proof},
    roster::{record, was_removed},
};
use beep_mesh::{JoinSecret, TeamTicket, secret_hash};
use beep_proto::{
    node::{
        LinkRx, LinkTx,
        tools::{Reply, Request, TeamJoinArgs},
    },
    team::{ForgeLogin, Member, TeamMsg, TeamRecord, TeamRepo},
};
use iroh::{EndpointAddr, EndpointId};
use std::{
    future::Future,
    path::Path,
    time::{Duration, SystemTime},
};
use tokio::time::timeout;

/// How long a whole join may take, as the joiner waits.
const JOIN_LIMIT: Duration = Duration::from_secs(60);
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
    /// `checkout` to `remote`.
    pub(crate) async fn join(
        &self,
        ticket: TeamTicket,
        checkout: &Path,
        remote: &str,
    ) -> Result<TeamRecord, TeamError> {
        timeout(JOIN_LIMIT, self.joining(ticket, checkout, remote))
            .await
            .map_err(|_| TeamError::TimedOut)?
    }

    async fn joining(
        &self,
        ticket: TeamTicket,
        checkout: &Path,
        remote: &str,
    ) -> Result<TeamRecord, TeamError> {
        let repo = ticket.repo;
        let login = self
            .forge()
            .login(&repo)
            .await
            .map_err(TeamError::Forge)?;
        let (mesh, devices) = self.with(|teams| (teams.home.mesh, teams.home.devices(&repo)));
        let mesh = mesh.ok_or(TeamError::NoMesh)?;

        let (mut tx, mut rx) = self.dial(&[EndpointAddr::new(ticket.inviter)]).await?;
        tx.send(&TeamMsg::Join {
            secret: *ticket.secret.as_bytes(),
            repo: repo.clone(),
            login,
            mesh,
            devices,
        })
        .await?;

        let nonce = match rx.recv().await? {
            Some(TeamMsg::Challenge { nonce }) => nonce
                .parse::<Nonce>()
                .map_err(|_| TeamError::Refused("The inviter's challenge does not read".into()))?,
            other => return Err(unexpected(other)),
        };
        git_step(push_proof(checkout, remote, &repo, &nonce)).await?;
        tx.send(&TeamMsg::Proved {
            nonce: nonce.to_string(),
        })
        .await?;

        match rx.recv().await? {
            Some(TeamMsg::Roster { repo: of, members }) if of == repo => Ok(record(repo, members)),
            other => Err(unexpected(other)),
        }
    }

    /// The inviter's side of a join from `peer`: the team's roster or why not.
    pub(super) async fn admit(
        &self,
        peer: EndpointId,
        join: Join,
        mut tx: LinkTx,
        mut rx: LinkRx,
    ) -> Result<(), TeamError> {
        let answer = match self.check(peer, &join, &mut tx, &mut rx).await {
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

        tx.send(&answer).await?;
        finish(tx).await?;
        Ok(())
    }

    /// Checks the invite, then the proof and the login, and adds the member.
    async fn check(
        &self,
        peer: EndpointId,
        join: &Join,
        tx: &mut LinkTx,
        rx: &mut LinkRx,
    ) -> Result<TeamRecord, String> {
        let repo = &join.repo;
        let (team, checkout) = self.with(|teams| {
            let team = teams.rosters.get(repo).cloned();
            (team, teams.home.checkout(repo))
        });
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
        if was_removed(&team, &join.login, join.mesh) {
            return Err(format!("{} was removed from {repo}", join.login));
        }
        if !join.devices.iter().any(|device| device.id == peer) {
            return Err("The joining device is not among the devices it named".to_owned());
        }

        let checkout =
            checkout.ok_or_else(|| format!("This device has no checkout of {repo} to check with"))?;
        let nonce = Nonce::generate();
        let challenge = TeamMsg::Challenge {
            nonce: nonce.to_string(),
        };
        tx.send(&challenge).await.map_err(|e| e.to_string())?;
        let proved = timeout(PROVE_LIMIT, rx.recv::<TeamMsg>())
            .await
            .map_err(|_| "The proof took too long".to_owned())?
            .map_err(|e| e.to_string())?;

        if proved != Some(TeamMsg::Proved {
            nonce: nonce.to_string(),
        }) {
            return Err("The joiner did not prove push access".to_owned());
        }

        let (folder, remote) = (&checkout.folder, &checkout.remote);
        let seen = git_step(proof_seen(folder, remote, &nonce))
            .await
            .map_err(|e| format!("Could not check the proof: {e}"))?;

        if !seen {
            return Err(ProofError::Denied(repo.to_string()).to_string());
        }

        let collaborator = self.forge().collaborator(repo, &join.login).await;

        if let Err(e) = git_step(drop_proof(folder, remote, &nonce)).await {
            log::warn!("the proof ref of a join to {repo} stays: {e}");
        }

        match collaborator {
            Ok(true) => {}
            Ok(false) => return Err(format!("{} is not a collaborator on {repo}", join.login)),
            Err(e) => return Err(e),
        }

        let member = Member {
            login: join.login.clone(),
            mesh: join.mesh,
            devices: join.devices.clone(),
            joined_at: SystemTime::now(),
        };
        self.with(|teams| teams.rosters.admit(repo, member))
            .ok_or_else(|| format!("The team for {repo} is gone from this device"))
    }
}

/// One git command of the proof, within `GIT_LIMIT`.
async fn git_step<T>(
    step: impl Future<Output = Result<T, ProofError>>,
) -> Result<T, ProofError> {
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
