//! `branch_review`: `post` the open findings as one inline PR review, or `merge` (squash) the PR.
//! Both always ask, and the card shows exactly what goes out.

use super::{Planned, ProjectTools, card, field, gated, leaked, on_home};
use crate::projects::stored::StoredProject;
use crate::{
    projects::{
        fleet,
        gate::Asks,
        review::rules::{merge_blocked, open},
    },
    protocol::Approval,
    store::{ProjectRow, ReviewFindingRow, ReviewRow},
};
use anyhow::Result;
use async_trait::async_trait;
use beep_projects::issues::issue_host;
use beep_projects::{
    commands::{HostCommand, InlineComment},
    model::IssueHost,
};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

pub(crate) const REVIEW: &str = "branch_review";
/// Never saved: posting a review always asks.
const POST_SLUG: &str = "PROJECT_REVIEW_POST";
/// The longest summary taken.
const MAX_BODY: usize = 2000;

pub(super) struct ReviewTool {
    pub(super) tools: Arc<ProjectTools>,
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

/// The inline comment for one finding; `None` without a line (it goes in the summary).
fn inline(finding: &ReviewFindingRow) -> Option<InlineComment> {
    let severity: &'static str = finding.severity.into();
    Some(InlineComment {
        path: finding.file.clone(),
        line: finding.line?,
        body: format!("{severity}: {}\n\n{}", finding.title, finding.why),
    })
}

/// The review's summary: the agent's text, then the findings without a line.
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
        Ok(self.run(id, &args).await?.into_result(id))
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
            None => match issue_host(&StoredProject::new(&tools.user, &project)).await? {
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
                // The reviewed head is checked on the review's machine before a merge.
                let why = merge_blocked(&review, &shards, &findings)
                    .unwrap_or_else(|| fleet::UNREACHABLE.to_owned());
                Ok(Planned::Refused(why))
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
}
