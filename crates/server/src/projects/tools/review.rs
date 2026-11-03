//! `branch_review`: act on a finished branch review. `post` puts its open
//! findings on the pull request as one review with inline comments, and
//! `merge` squash-merges the pull request. Both always ask, and the card
//! shows what goes out: every comment, or the pull request with its checks.

use super::{Planned, ProjectTools, card, field, gated, leaked, on_home};
use crate::{
    app::App,
    projects::{
        commands::{ChecksState, HostCommand, InlineComment, Shell, parse_pr},
        gate::Asks,
        issues::issue_host,
        model::IssueHost,
        review::{
            drive::online,
            git::{head_script, parse_head},
            rules::{merge_blocked, open},
        },
    },
    protocol::Approval,
    store::{ProjectRow, ReviewFindingRow, ReviewRow},
};
use anyhow::Result;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub(crate) const REVIEW: &str = "branch_review";
/// Never saved: posting a review always asks.
const POST_SLUG: &str = "PROJECT_REVIEW_POST";
/// Never saved: a merge always asks.
const MERGE_SLUG: &str = "PROJECT_REVIEW_MERGE";
/// How long reading where the branch is now may take.
const HEAD_LIMIT: Duration = Duration::from_secs(30);
/// The longest summary taken.
const MAX_BODY: usize = 2000;

pub(super) struct ReviewTool {
    pub(super) tools: Arc<ProjectTools>,
    pub(super) app: Arc<App>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Post,
    Merge,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewArgs {
    action: Action,
    review: Uuid,
    #[serde(default)]
    body: Option<String>,
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// The inline comment for one finding; `None` for one with no line, which
/// goes in the review's summary instead.
fn inline(finding: &ReviewFindingRow) -> Option<InlineComment> {
    let severity: &'static str = finding.severity.into();
    Some(InlineComment {
        path: finding.file.clone(),
        line: finding.line?,
        body: format!("{severity}: {}\n\n{}", finding.title, finding.why),
    })
}

/// The review's summary: the agent's text, then the findings that name no
/// line.
fn summary(body: &str, open: &[&ReviewFindingRow]) -> String {
    let mut out = body.trim().to_owned();
    let loose: Vec<String> = open
        .iter()
        .filter(|finding| finding.line.is_none())
        .map(|finding| {
            let severity: &'static str = finding.severity.into();
            format!(
                "- {severity}: {} ({}): {}",
                finding.title, finding.file, finding.why
            )
        })
        .collect();
    if !loose.is_empty() {
        out.push_str("\n\n");
        out.push_str(&loose.join("\n"));
    }
    out
}

/// The card for posting: the pull request, the summary and each comment.
fn post_card(url: &str, head: &str, body: &str, comments: &[InlineComment]) -> Approval {
    let mut fields = vec![
        field("Pull request", url),
        field("On commit", short(head)),
        field("Summary", body),
    ];
    fields.extend(
        comments
            .iter()
            .map(|comment| field(format!("{}:{}", comment.path, comment.line), &comment.body)),
    );
    card("Post review".to_owned(), fields)
}

#[async_trait]
impl Tool for ReviewTool {
    fn name(&self) -> &str {
        REVIEW
    }

    fn description(&self) -> &str {
        "Act on a finished review of a branch, by its id. `post` puts the review's open \
         findings on the branch's pull request as one review, each as an inline comment, with \
         `body` as a one or two sentence summary; the user sees every comment first. `merge` \
         squash-merges the pull request after checking that the branch has not moved since the \
         review, nothing is open, nothing conflicts and checks do not fail; it always asks."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["post", "merge"]},
                "review": {"type": "string", "description": "The review's id."},
                "body": {"type": "string", "description": "For post: the review's summary."}
            },
            "required": ["action", "review"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let id = ctx.tool_call_id;
        let args: ReviewArgs = match ctx.parse_args(REVIEW) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::failure(id, error.to_string())),
        };
        Ok(match self.run(id, &args).await? {
            Planned::Done(text) => ToolResult::success(id, text),
            Planned::Refused(text) => ToolResult::failure(id, text),
            Planned::Gated(result) => result,
        })
    }

    fn humanize(&self, args: &Value) -> String {
        match args.get("action").and_then(Value::as_str) {
            Some("merge") => "Merging the reviewed branch".to_owned(),
            _ => "Posting the review".to_owned(),
        }
    }
}

impl ReviewTool {
    async fn run(&self, id: &str, args: &ReviewArgs) -> Result<Planned> {
        let tools = &self.tools;
        let project = tools.project().await?;
        let Some(review) = tools.user.review(args.review).await?.filter(|review| {
            review
                .project
                .eq_ignore_ascii_case(&project.name.to_string())
        }) else {
            return Ok(Planned::Refused(
                "This project has no review with that id.".to_owned(),
            ));
        };
        let shards = tools.user.review_shards(&review).await?;
        let findings = tools.user.review_findings(&review).await?;
        if super::super::review::drive::running(&shards) {
            return Ok(Planned::Refused("The review is still running.".to_owned()));
        }
        let Some(pr) = review.pr.clone() else {
            return Ok(Planned::Refused(
                "The branch has no open pull request.".to_owned(),
            ));
        };
        let host = match &project.roles.prs {
            Some(host) => host.clone(),
            None => match issue_host(&tools.user, &project).await? {
                Some(host) => host,
                None => {
                    return Ok(Planned::Refused(
                        "The project has no GitHub or Forgejo host.".to_owned(),
                    ));
                }
            },
        };
        match args.action {
            Action::Post => {
                let body = args.body.as_deref().unwrap_or_default();
                self.post(
                    id, &project, &host, &review, &pr.url, pr.number, body, &findings,
                )
                .await
            }
            Action::Merge => {
                if let Some(why) = merge_blocked(&review, &shards, &findings) {
                    return Ok(Planned::Refused(why));
                }
                self.merge(id, &project, &host, &review, &pr.url, pr.number)
                    .await
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one call site, each a different part"
    )]
    async fn post(
        &self,
        id: &str,
        project: &ProjectRow,
        host: &IssueHost,
        review: &ReviewRow,
        url: &str,
        number: u64,
        body: &str,
        findings: &[ReviewFindingRow],
    ) -> Result<Planned> {
        let tools = &self.tools;
        let open = open(findings);
        let mut text = summary(body, &open);
        if text.trim().is_empty() {
            text = if open.is_empty() {
                format!(
                    "Reviewed {} and found nothing to change.",
                    short(&review.head)
                )
            } else {
                format!("Review of {}.", short(&review.head))
            };
        }
        if text.len() > MAX_BODY {
            return Ok(Planned::Refused(format!(
                "Keep the summary under {MAX_BODY} characters."
            )));
        }
        let comments: Vec<InlineComment> = open.iter().filter_map(|f| inline(f)).collect();
        let command = HostCommand::PostReview {
            number,
            commit: &review.head,
            body: &text,
            comments: &comments,
        };
        let hits = command.leaks(&tools.public_text().await?);
        if !hits.is_empty() {
            return Ok(Planned::Refused(leaked(&hits)));
        }
        let make_card = || post_card(url, &review.head, &text, &comments);
        if let Some(stop) = gated(tools.gate.check(id, POST_SLUG, Asks::Always, make_card)) {
            return Ok(stop);
        }
        Ok(
            match on_home(tools.fleet.as_ref(), &tools.user, project, host, command).await? {
                Ok(_) => Planned::Done(format!(
                    "Posted the review on #{number} with {} inline {}.",
                    comments.len(),
                    if comments.len() == 1 {
                        "comment"
                    } else {
                        "comments"
                    }
                )),
                Err(why) => Planned::Refused(format!("The review was not posted: {why}")),
            },
        )
    }

    async fn merge(
        &self,
        id: &str,
        project: &ProjectRow,
        host: &IssueHost,
        review: &ReviewRow,
        url: &str,
        number: u64,
    ) -> Result<Planned> {
        let tools = &self.tools;
        let Some(node) = online(&self.app, &tools.user, &review.endpoint) else {
            return Ok(Planned::Refused(
                "The machine the review ran on is offline, so the branch cannot be checked."
                    .to_owned(),
            ));
        };
        let out = node
            .run(
                &head_script(&review.folder, &review.branch, &review.head),
                HEAD_LIMIT,
            )
            .await?;
        if out.code != Some(0) || parse_head(&out.out, &review.head).stale {
            return Ok(Planned::Refused(format!(
                "{} moved since the review of {} (or could not be read): review it again first.",
                review.branch,
                short(&review.head)
            )));
        }
        let status = match on_home(
            tools.fleet.as_ref(),
            &tools.user,
            project,
            host,
            HostCommand::PrStatus { number },
        )
        .await?
        {
            Ok(out) => parse_pr(host, &out).ok().flatten(),
            Err(why) => {
                return Ok(Planned::Refused(format!(
                    "The pull request was not read: {why}"
                )));
            }
        };
        let Some(status) = status.filter(|pr| pr.open) else {
            return Ok(Planned::Refused(format!(
                "Pull request #{number} is not open."
            )));
        };
        if status.checks == ChecksState::Failing {
            return Ok(Planned::Refused(
                "Checks fail on the pull request; merge refused.".to_owned(),
            ));
        }
        let make_card = || {
            let checks: &'static str = match status.checks {
                ChecksState::None => "none reported",
                other => other.into(),
            };
            let mut fields = vec![
                field("Pull request", url),
                field("Branch", format!("{} into {}", review.branch, review.base)),
                field("Reviewed", format!("{}, nothing open", short(&review.head))),
                field("Checks", checks),
                field("Method", "squash"),
            ];
            if let Some(decision) = &status.review {
                fields.push(field("Review", decision.to_lowercase().replace('_', " ")));
            }
            card("Merge pull request".to_owned(), fields)
        };
        if let Some(stop) = gated(tools.gate.check(id, MERGE_SLUG, Asks::Always, make_card)) {
            return Ok(stop);
        }
        if let Err(why) = on_home(
            tools.fleet.as_ref(),
            &tools.user,
            project,
            host,
            HostCommand::MergePr { number },
        )
        .await?
        {
            return Ok(Planned::Refused(format!("The merge failed: {why}")));
        }
        // A task that opened this pull request is done with it.
        let tasks = tools.user.project_tasks(project.id).await?;
        if let Some(task) = tasks
            .iter()
            .find(|task| task.pr_url.as_deref() == Some(url) && task.state.is_open())
        {
            crate::projects::finish_task(tools.fleet.as_ref(), &tools.user, project, task).await?;
        }
        Ok(Planned::Done(format!("Merged {url}.")))
    }
}
