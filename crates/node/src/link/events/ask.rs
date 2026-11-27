//! `execute`, the call dispatcher, and `ask`, which answers one request on an
//! in-memory link.

use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::SessionsRequest,
    events::{CallReply, Operation},
};
use std::{future::Future, sync::Arc};
use tokio_util::task::AbortOnDropHandle;

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

pub(in crate::link) async fn execute(
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
