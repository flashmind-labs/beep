//! Fixed replies for released apps that still send machine requests to the server.

use super::{
    Reply,
    router::{Cx, Router, Sink, Uplink},
};
use crate::protocol::*;

pub(super) fn routes(router: Router) -> Router {
    router
        .call(machine::<StartCodexLogin>)
        .call(machine::<CancelCodexLogin>)
        .call(empty::<ListAgentHosts>)
        .call(machine::<ResumeAgentSession>)
        .call(machine::<StartAgentSession>)
        .call(machine::<PreviewAgentMachine>)
        .call(machine::<GetAgentSession>)
        .call(machine::<GetAgentSessionUsage>)
        .call(machine::<GetAgentSessionModes>)
        .call(machine::<GetAgentSessionTasks>)
        .call(machine::<SteerAgentSession>)
        .call(empty::<ListAgentImages>)
        .call(machine::<GetAgentFile>)
        .call(machine::<ListAgentModels>)
        .call(empty::<ListFoundFolders>)
        .call(empty::<ListProjectSessions>)
        .call(empty::<ListAgentSessions>)
        .call(machine::<AnswerAgentQuestion>)
        .call(machine::<AddBranchWorktree>)
        .call(machine::<ListCustomAgents>)
        .call(machine::<ListAgentHarnesses>)
        .call(machine::<UpdateAgentHarness>)
        .call(empty::<ReadAgentFiles>)
        .call(machine::<SyncAgentFiles>)
        .call(machine::<GetMachineSearch>)
        .call(machine::<SetMachineSearch>)
        .call(empty::<ListToolUpdates>)
        .call(machine::<UpdateMachineTool>)
        .call(empty::<ListMachineTraffic>)
        .call(empty::<ListMachines>)
        .call(empty::<ListSshHosts>)
        .call(machine::<UpdateMachine>)
        .call(machine::<GetMachineLog>)
        .call(empty::<ListUpdatableMachines>)
        .call(empty::<ListMachineDetails>)
        .call(empty::<ListMachineStats>)
        .call(machine::<CreateNodeToken>)
        .call(machine::<CopyProject>)
        .call(machine::<ImportProject>)
        .call(empty::<ListProjectForges>)
        .call(machine::<CreateProject>)
        .call(machine::<PreviewImport>)
        .call(machine::<FindImportFork>)
        .call(machine::<ListMachineFolder>)
        .call(empty::<ListHostRepos>)
        .call(machine::<ResolveSyncConflict>)
        .call(machine::<GetSyncSkipped>)
        .call(machine::<ListProjectBranches>)
        .call(machine::<StartBranchReview>)
        .call(machine::<CheckBranchReview>)
        .call(machine::<MergeReviewedBranch>)
        .call(empty::<ListTerminals>)
        .call(empty::<ListAllTerminals>)
        .call(machine::<StartTerminal>)
        .call(machine::<OpenTerminal>)
        .call(machine::<GetTerminal>)
        .call(machine::<CloseTerminal>)
        .call(empty::<ListWork>)
        .call(empty::<ListQuestions>)
        .call(machine::<GetWorkDiff>)
        .call(machine::<GetBranchDiff>)
        .call(machine::<ListWorkFolder>)
        .call(machine::<ListWorkTree>)
        .call(machine::<ReadWorkFile>)
        .call(machine::<WriteWorkFile>)
        .call(add_machine_over_ssh)
        .call(get_node_token)
        .call(revoke_node_token)
        .call(cancel_work)
        .call(get_codex_login)
        .duplex(attach_terminal)
}

pub(super) fn refused<T>() -> Reply<T> {
    Err(ApiError::bad_request(
        "Machines are reached through a desktop now.",
    ))
}

async fn machine<R: Call>(_: Cx, _: R) -> Reply<R::Reply> {
    refused()
}

async fn empty<R: Call>(_: Cx, _: R) -> Reply<R::Reply>
where
    R::Reply: Default,
{
    Ok(R::Reply::default())
}

/// Adding a machine over ssh was removed; released apps still ask.
async fn add_machine_over_ssh(_: Cx, _: AddMachineOverSsh) -> Reply<SshAdd> {
    Ok(SshAdd {
        machine_id: None,
        system: String::new(),
        error: "Adding a machine over SSH was removed. Add machines from a desktop.".into(),
        output: Vec::new(),
    })
}

/// No token is claimed any more.
async fn get_node_token(_: Cx, _: GetNodeToken) -> Reply<Option<String>> {
    Err(ApiError::NotFound {})
}

/// No token can be claimed, so there is nothing to revoke.
async fn revoke_node_token(_: Cx, _: RevokeNodeToken) -> Reply<()> {
    Ok(())
}

async fn cancel_work(_: Cx, _: CancelWork) -> Reply<bool> {
    Err(ApiError::NotFound {})
}

async fn get_codex_login(_: Cx, _: GetCodexLogin) -> Reply<CodexLoginState> {
    Ok(CodexLoginState::Gone {})
}

/// Ends at once: the terminal's machine is out of the server's reach.
async fn attach_terminal(
    _: Cx,
    _: AttachTerminal,
    _: Uplink<TerminalInput>,
    sink: Sink<TerminalEvent>,
) -> Reply<()> {
    sink.send(&TerminalEvent::Ended {
        state: TerminalState::Offline {},
    })
    .await?;
    Ok(())
}
