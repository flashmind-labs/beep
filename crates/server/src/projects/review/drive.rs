//! Moves reviews on in the background, whether or not a screen is open:
//! a reviewer whose turn ended gives its findings (duplicates of another
//! shard's merged away), each finding is checked once by a verifier on the
//! cheapest offered model, and a done review's worktree is removed. Only
//! this loop writes a review's progress; every step is safe to repeat
//! after a restart.

use super::{
    git::remove_review_worktree,
    report::{
        MAX_CONTEXT, Reported, Subject, VERIFIER_SYSTEM, context_script, new_findings,
        parse_context, parse_findings, parse_verdicts, verifier_prompt,
    },
};
use crate::{
    app::App,
    nodes::Online,
    projects::commands::Shell,
    protocol::node::agent::{EntryKind, Request as AgentRequest, SessionInfo, Status},
    store::{Judged, ReviewFindingRow, ReviewRow, ShardRow, ShardState, Verdict},
    user::User,
};
use anyhow::Result;
use chrono::Utc;
use flashmind::types::AgentLlmConfig;
use futures::future::join_all;
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::time::{interval, timeout};

/// How often running reviews are moved on.
const EVERY: Duration = Duration::from_secs(15);
/// Reviews older than this are left as they are.
const DRIVE_FOR: chrono::Duration = chrono::Duration::days(2);
/// A reviewer still reading after this long is stopped.
const READ_LIMIT: chrono::Duration = chrono::Duration::minutes(90);
/// How long a node may take to list sessions or a transcript.
const NODE_LIMIT: Duration = Duration::from_secs(5);
/// How long reading a finding's code may take.
const CONTEXT_LIMIT: Duration = Duration::from_secs(20);
/// How long one verifier may take.
const VERIFY_LIMIT: Duration = Duration::from_secs(90);
/// The longest verifier answer, in tokens.
const VERIFY_TOKENS: u32 = 1200;
/// Findings verified at once.
const VERIFY_IN_FLIGHT: usize = 4;
/// Transcript entries read for a reviewer's reply.
const REPLY_ENTRIES: u32 = 30;
/// Finding ids are the shard's number times this, plus its place.
pub(crate) const SHARD_IDS: u32 = 100;

/// Runs until the app goes. Holds it weakly: the app owns the loop.
pub(crate) async fn run(app: Weak<App>) {
    let mut tick = interval(EVERY);
    loop {
        tick.tick().await;
        let Some(app) = app.upgrade() else { return };
        drive_all(&app).await;
    }
}

async fn drive_all(app: &Arc<App>) {
    let driven = match app.store.reviews_to_drive(Utc::now() - DRIVE_FOR).await {
        Ok(driven) => driven,
        Err(e) => {
            log::warn!("reviews: running reviews not read: {e:#}");
            return;
        }
    };
    for one in driven {
        let Some(user) = app.user(&one.user) else {
            continue;
        };
        let review = match user.review(one.review).await {
            Ok(Some(review)) => review,
            Ok(None) => continue,
            Err(e) => {
                log::warn!("reviews: review {} not read: {e:#}", one.review);
                continue;
            }
        };
        if let Err(e) = advance(app, &user, &review).await {
            log::warn!("reviews: review {} did not move on: {e:#}", review.id);
        }
    }
}

/// The user's machine `endpoint` when it is online now.
pub(crate) fn online(app: &App, user: &User, endpoint: &str) -> Option<Online> {
    app.nodes
        .online(endpoint)
        .filter(|node| node.user == user.id())
}

/// Whether any shard still reads or waits for its verifier.
pub(crate) fn running(shards: &[ShardRow]) -> bool {
    shards
        .iter()
        .any(|shard| matches!(shard.state, ShardState::Reading | ShardState::Verifying))
}

/// Whether a session's turn is over, so its reply can be read.
fn finished(info: &SessionInfo) -> bool {
    match info.state.status {
        Status::Ended => true,
        Status::Idle => info.turns >= 1,
        _ => false,
    }
}

/// What a running reviewer does now, by shard, read from its machine
/// without changing anything.
pub(crate) async fn doing(node: &Online, shards: &[ShardRow]) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    if !shards
        .iter()
        .any(|shard| shard.state == ShardState::Reading)
    {
        return out;
    }
    match timeout(NODE_LIMIT, node.agent_sessions()).await {
        Ok(Ok(sessions)) => {
            for shard in shards
                .iter()
                .filter(|shard| shard.state == ShardState::Reading)
            {
                if let Some(info) = sessions.iter().find(|info| info.id == shard.session) {
                    out.insert(shard.shard, info.state.summary.clone());
                }
            }
        }
        Ok(Err(e)) => log::info!("reviews: sessions not listed on {}: {e:#}", node.name),
        Err(_) => log::info!("reviews: sessions not listed on {} in time", node.name),
    }
    out
}

/// What the agent said in a session's last entries, oldest first, else
/// its last message.
async fn reply_text(node: &Online, info: &SessionInfo) -> String {
    match timeout(NODE_LIMIT, node.transcript(&info.id, REPLY_ENTRIES)).await {
        Ok(Ok((entries, _))) => {
            let said: Vec<String> = entries
                .into_iter()
                .filter(|entry| entry.kind == EntryKind::Agent)
                .map(|entry| entry.text)
                .collect();
            if !said.is_empty() {
                return said.join("\n");
            }
        }
        Ok(Err(e)) => log::info!("reviews: transcript not read: {e:#}"),
        Err(_) => log::info!("reviews: transcript not read in time"),
    }
    info.last_message.clone()
}

/// Ends a session whose reply was read; one that is already gone is fine.
pub(crate) async fn close_session(node: &Online, agent: &str, session: &str) {
    let closed = match node.agent_session(agent, session).await {
        Ok(link) => link.send(AgentRequest::Close {}).await,
        Err(e) => Err(e),
    };
    if let Err(e) = closed {
        log::debug!("reviews: session {session} not closed: {e:#}");
    }
}

fn subject(review: &ReviewRow) -> Subject<'_> {
    Subject {
        project: &review.project,
        branch: &review.branch,
        base: &review.base,
        head: &review.head,
        merge_base: &review.merge_base,
        at_head: review.worktree.is_some(),
    }
}

fn as_reported(finding: &ReviewFindingRow) -> Reported {
    Reported {
        severity: finding.severity,
        title: finding.title.clone(),
        file: finding.file.clone(),
        line: finding.line,
        why: finding.why.clone(),
    }
}

/// A reviewer's findings as rows of its shard, numbered in order.
pub(crate) fn finding_rows(shard: u32, reported: Vec<Reported>) -> Vec<ReviewFindingRow> {
    (0u32..)
        .zip(reported)
        .map(|(n, found)| ReviewFindingRow {
            id: shard * SHARD_IDS + n,
            shard,
            severity: found.severity,
            title: found.title,
            file: found.file,
            line: found.line,
            why: found.why,
            verdict: None,
            verdict_why: None,
            dismissed: false,
        })
        .collect()
}

/// Moves the review on as far as its machine allows. A machine that is
/// offline or does not answer leaves it as it is, for the next round.
pub(crate) async fn advance(app: &Arc<App>, user: &Arc<User>, review: &ReviewRow) -> Result<()> {
    let Some(node) = online(app, user, &review.endpoint) else {
        return Ok(());
    };
    let shards = user.review_shards(review).await?;
    if running(&shards) {
        let sessions = match timeout(NODE_LIMIT, node.agent_sessions()).await {
            Ok(Ok(sessions)) => sessions,
            Ok(Err(e)) => {
                log::info!("reviews: sessions not listed on {}: {e:#}", node.name);
                return Ok(());
            }
            Err(_) => return Ok(()),
        };
        for shard in &shards {
            match shard.state {
                ShardState::Reading => {
                    let info = sessions.iter().find(|info| info.id == shard.session);
                    read_shard(&node, user, review, shard, info).await?;
                }
                ShardState::Verifying => verify_shard(app, user, &node, review, shard).await?,
                ShardState::Done | ShardState::Failed => {}
            }
        }
    }
    if let Some(worktree) = &review.worktree
        && !running(&user.review_shards(review).await?)
    {
        let removed = node
            .run(
                &remove_review_worktree(&review.folder, worktree),
                CONTEXT_LIMIT,
            )
            .await?;
        // 3: the checkout itself is gone, and the worktree with it.
        if matches!(removed.code, Some(0 | 3)) {
            app.store.clear_review_worktree(review.id).await?;
        }
    }
    Ok(())
}

const GONE: &str = "The reviewer's session is gone from its machine.";
const TOO_LONG: &str = "The reviewer ran too long and was stopped.";
const NO_FINDINGS: &str = "The reviewer ended without listing its findings.";

/// Takes a finished reviewer's findings, or fails a shard whose reviewer
/// is gone, said nothing usable or read too long.
async fn read_shard(
    node: &Online,
    user: &User,
    review: &ReviewRow,
    shard: &ShardRow,
    info: Option<&SessionInfo>,
) -> Result<()> {
    let Some(info) = info else {
        user.fail_shard(review, shard.shard, ShardState::Reading, GONE)
            .await?;
        return Ok(());
    };
    if !finished(info) {
        if Utc::now() - review.started_at > READ_LIMIT
            && user
                .fail_shard(review, shard.shard, ShardState::Reading, TOO_LONG)
                .await?
        {
            close_session(node, &shard.agent, &shard.session).await;
        }
        return Ok(());
    }
    let text = reply_text(node, info).await;
    let Some(reported) = parse_findings(&text) else {
        if user
            .fail_shard(review, shard.shard, ShardState::Reading, NO_FINDINGS)
            .await?
        {
            close_session(node, &shard.agent, &shard.session).await;
        }
        return Ok(());
    };
    let earlier: Vec<Reported> = user
        .review_findings(review)
        .await?
        .iter()
        .filter(|finding| finding.shard != shard.shard)
        .map(as_reported)
        .collect();
    let rows = finding_rows(shard.shard, new_findings(&earlier, reported));
    let next = if rows.is_empty() {
        ShardState::Done
    } else {
        ShardState::Verifying
    };
    // False when a round before a restart already took them.
    user.report_shard(review, shard.shard, &rows, next).await?;
    close_session(node, &shard.agent, &shard.session).await;
    Ok(())
}

/// The verifier's model: the cheapest offered, with its routing.
fn verifier_llm(app: &App) -> AgentLlmConfig {
    let model = app.models.cheapest().clone();
    let mut llm = AgentLlmConfig::new(model.clone());
    llm.provider_preferences = app
        .models
        .routing(&model)
        .cloned()
        .or_else(|| app.llm().provider_preferences.clone());
    llm
}

/// Checks each of the shard's findings with a fresh verifier and marks the
/// shard done. A finding whose check fails stays unchecked.
async fn verify_shard(
    app: &Arc<App>,
    user: &Arc<User>,
    node: &Online,
    review: &ReviewRow,
    shard: &ShardRow,
) -> Result<()> {
    // Reviews from before the verifier moved to the server had one
    // session per shard; it is not needed any more.
    if let Some(old) = &shard.verifier {
        close_session(node, &shard.agent, old).await;
    }
    let findings: Vec<ReviewFindingRow> = user
        .review_findings(review)
        .await?
        .into_iter()
        .filter(|finding| finding.shard == shard.shard && finding.verdict.is_none())
        .collect();
    let llm = verifier_llm(app);
    let subject = subject(review);
    let mut judged: Vec<Judged> = Vec::with_capacity(findings.len());
    for batch in findings.chunks(VERIFY_IN_FLIGHT) {
        let verdicts = join_all(
            batch
                .iter()
                .map(|finding| verify(app, user, node, &llm, review, &subject, finding)),
        )
        .await;
        judged.extend(verdicts.into_iter().flatten());
    }
    user.judge_shard(review, shard.shard, &judged).await?;
    Ok(())
}

/// One verifier's verdict on one finding, read with the code it names;
/// `None` when the code or the model cannot be read.
async fn verify(
    app: &App,
    user: &User,
    node: &Online,
    llm: &AgentLlmConfig,
    review: &ReviewRow,
    subject: &Subject<'_>,
    finding: &ReviewFindingRow,
) -> Option<Judged> {
    let script = context_script(&review.folder, subject, &finding.file, finding.line);
    let context = match node.run(&script, CONTEXT_LIMIT).await {
        Ok(out) if out.code == Some(0) => out.out,
        Ok(out) => {
            log::info!(
                "reviews: a finding's code was not read: exit {:?}",
                out.code
            );
            return None;
        }
        Err(e) => {
            log::info!("reviews: a finding's code was not read: {e:#}");
            return None;
        }
    };
    let (code, diff) = parse_context(&context);
    let prompt = verifier_prompt(
        subject,
        finding,
        clip(&code, MAX_CONTEXT),
        clip(&diff, MAX_CONTEXT),
    );
    let generator = app.generator_on(user.id(), llm);
    let answer = match timeout(
        VERIFY_LIMIT,
        generator.text_up_to(VERIFIER_SYSTEM, prompt, VERIFY_TOKENS),
    )
    .await
    {
        Ok(Ok(answer)) => answer,
        Ok(Err(e)) => {
            log::info!("reviews: a verifier failed: {e:#}");
            return None;
        }
        Err(_) => {
            log::info!("reviews: a verifier did not answer in time");
            return None;
        }
    };
    let said = parse_verdicts(&answer)?
        .into_iter()
        .find(|said| said.n == 1)?;
    Some(Judged {
        id: finding.id,
        verdict: if said.real {
            Verdict::Verified
        } else {
            Verdict::Dropped
        },
        why: Some(said.why).filter(|why| !why.is_empty()),
    })
}

/// `text` cut to `max` bytes on a character boundary.
fn clip(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
