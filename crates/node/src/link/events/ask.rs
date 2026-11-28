//! `execute`, the call dispatcher, and `ask`, which answers one request on an
//! in-memory link.

use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::SessionsRequest,
    events::{CallReply, Operation},
};
use std::{collections::HashSet, future::Future, sync::Arc, time::Duration};
use tokio::{task::JoinSet, time::timeout};
use tokio_util::task::AbortOnDropHandle;

/// How long a write to a stream may take before its reader is given up on.
pub(in crate::link) const WRITE_WAIT: Duration = Duration::from_secs(15);
const MAX_CALLS: usize = 16;

/// The calls a stream has running: at most `MAX_CALLS`, each with its own id.
#[derive(Default)]
pub(in crate::link) struct Calls {
    tasks: JoinSet<(u64, Result<Result<CallReply>, tokio::time::error::Elapsed>)>,
    pending: HashSet<u64>,
}

impl Calls {
    pub(in crate::link) fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Starts `operation` as call `id`, or says why it was refused.
    pub(in crate::link) fn start(
        &mut self,
        agents: Arc<crate::agents::Agents>,
        caller: iroh::EndpointId,
        id: u64,
        operation: Operation,
    ) -> Result<(), &'static str> {
        if self.tasks.len() >= MAX_CALLS || !self.pending.insert(id) {
            return Err("too many pending calls or duplicate call id");
        }

        self.tasks.spawn(async move {
            (
                id,
                timeout(Duration::from_secs(120), execute(agents, caller, operation)).await,
            )
        });

        Ok(())
    }

    /// The next call to finish, with its outcome as the text a reply carries.
    pub(in crate::link) async fn finished(&mut self) -> Result<(u64, Result<CallReply, String>)> {
        let done = self.tasks.join_next().await.context("no call is running")?;
        let (id, result) = done.context("a command stopped")?;
        self.pending.remove(&id);

        Ok((
            id,
            match result {
                Ok(done) => done.map_err(|error| format!("{error:#}")),
                Err(_) => Err("command timed out; its outcome may be unknown".into()),
            },
        ))
    }
}

/// Sends one `request` to `serve` on an in-memory link and reads its first
/// reply; `None` when the stream ends without one.
pub(super) async fn ask<Req, Rep, F>(
    request: &Req,
    serve: impl FnOnce(LinkTx, LinkRx) -> F,
) -> Result<Option<Rep>>
where
    Req: serde::Serialize,
    Rep: serde::de::DeserializeOwned,
    F: Future<Output = Result<()>> + Send + 'static,
{
    let link = beep_proto::node::local_link();
    let _task = AbortOnDropHandle::new(tokio::spawn(serve(link.node.0, link.node.1)));
    let (mut tx, mut rx) = link.caller;
    tx.send(request).await?;
    rx.recv().await
}

pub(super) async fn execute(
    agents: Arc<crate::agents::Agents>,
    caller: iroh::EndpointId,
    operation: Operation,
) -> Result<CallReply> {
    match operation {
        Operation::Agent {
            agent,
            session,
            request,
        } => {
            let info = crate::agents::send_session(&agents, &agent, &session, request)
                .await
                .map_err(anyhow::Error::msg)?;
            Ok(CallReply::Agent {
                info: Box::new(info),
            })
        }
        Operation::Process(request) => Ok(CallReply::Process(
            crate::run::tasks::answer(crate::run::tasks::Table::Shared, request).await,
        )),
        Operation::Sessions(request) => {
            if matches!(
                request,
                SessionsRequest::Watch { .. }
                    | SessionsRequest::FollowPresence {}
                    | SessionsRequest::FollowImages {}
                    | SessionsRequest::FollowAlerts {}
                    | SessionsRequest::FollowRuns {}
                    | SessionsRequest::FollowUsage { .. }
            ) {
                bail!("use a topic subscription for a following request");
            }
            let reply = ask(&request, |tx, rx| {
                crate::agents::serve_sessions(tx, rx, agents, caller, None)
            })
            .await?
            .context("session request closed without a reply")?;
            Ok(CallReply::Sessions(Box::new(reply)))
        }
    }
}
