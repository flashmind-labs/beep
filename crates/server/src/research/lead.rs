//! The lead's tools for starting, continuing and stopping the team, and adding scouts.

use super::{
    MAX_FOLLOW_UPS, MAX_GAP_SCOUTS, MAX_ROUNDS, MAX_SCOUTS, MAX_TEAM_SCOUTS, MIN_SCOUTS, Plan,
    RoleModel, Scout, Status,
};
use crate::{app::App, models::ModelChoices, user::User};
use anyhow::Result;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult, parse_args};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    ops::RangeInclusive,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

/// The longest question a scout takes.
const MAX_QUESTION: usize = 300;

#[derive(Deserialize)]
struct QuestionsArgs {
    questions: Vec<QuestionArgs>,
}

#[derive(Deserialize)]
struct QuestionArgs {
    question: String,
    /// A model id for this question's scout, else the default scout model.
    model: Option<String>,
}

/// The `questions` parameter, `min` to `max` of them.
fn questions_schema(min: usize, max: usize) -> Value {
    json!({
        "type": "object",
        "properties": {
            "questions": {
                "type": "array",
                "minItems": min,
                "maxItems": max,
                "items": {
                    "type": "object",
                    "properties": {
                        "question": { "type": "string" },
                        "model": {
                            "type": "string",
                            "description": "A scout model id from the list you were given; omit for the default"
                        }
                    },
                    "required": ["question"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["questions"],
        "additionalProperties": false
    })
}

/// The scouts for `asked`, in `round`, or what is wrong with them.
fn scouts_for(
    models: &ModelChoices,
    asked: Vec<QuestionArgs>,
    count: RangeInclusive<usize>,
    round: u32,
) -> Result<Vec<Scout>, String> {
    let Some(config) = models.research() else {
        return Err("Research is not set up on this server.".to_owned());
    };
    if !count.contains(&asked.len()) {
        return Err(format!(
            "Give {} to {} questions.",
            count.start(),
            count.end()
        ));
    }
    let mut scouts = Vec::new();
    for asked in asked {
        let question = asked.question.trim();
        if question.is_empty() || question.len() > MAX_QUESTION {
            return Err(format!("Each question must be 1-{MAX_QUESTION} bytes."));
        }
        let model = match asked.model.as_deref() {
            None => config.scout.clone(),
            Some(id) => match models.find(id) {
                Some(model) if config.scout_may_use(model) => RoleModel {
                    model: model.clone(),
                    thinking: config.scout.thinking,
                },
                _ => return Err(format!("{id} is not a scout model. Use one from the list.")),
            },
        };
        scouts.push(Scout {
            question: question.to_owned(),
            model,
            round,
        });
    }
    Ok(scouts)
}

pub struct StartResearchTool {
    pub app: Arc<App>,
    pub user: Arc<User>,
    pub research: Uuid,
}

#[async_trait]
impl Tool for StartResearchTool {
    fn name(&self) -> &str {
        "start_research"
    }
    fn description(&self) -> &str {
        "Start the research team on the plan the user approved: one scout per question, in \
         the order given. Call it only after the user agreed to the plan in this chat. The \
         team works in the background and you will be told when the paper is ready. Once a \
         paper is written, use research_more instead."
    }
    fn parameters(&self) -> Value {
        questions_schema(MIN_SCOUTS, MAX_SCOUTS)
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let args: QuestionsArgs = parse_args(self.name(), ctx.args)?;
        let Some(row) = self.app.store.research(self.research).await? else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "The research is gone.",
            ));
        };
        if row.plan.as_ref().is_some_and(|plan| plan.round() > 1) || row.status == Status::Done {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "The paper is written. Use research_more for new questions.",
            ));
        }
        let scouts = match scouts_for(&self.app.models, args.questions, MIN_SCOUTS..=MAX_SCOUTS, 1)
        {
            Ok(scouts) => scouts,
            Err(why) => return Ok(ToolResult::failure(ctx.tool_call_id, why)),
        };
        let Some(config) = self.app.models.research() else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "Research is not set up on this server.",
            ));
        };
        let plan = Plan {
            scouts,
            critic: config.critic.clone(),
            writer: config.writer.clone(),
        };
        if !self.app.start_team(&self.user, self.research, plan).await? {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "The team is already working.",
            ));
        }
        Ok(ToolResult::success(
            ctx.tool_call_id,
            "The team started. Tell the user in one short sentence that it is under way and \
             that the result will arrive in this chat. Do not repeat the plan.",
        ))
    }
}

/// Sends the team on another round, on questions the paper does not answer.
pub struct ResearchMoreTool {
    pub app: Arc<App>,
    pub user: Arc<User>,
    pub research: Uuid,
}

#[async_trait]
impl Tool for ResearchMoreTool {
    fn name(&self) -> &str {
        "research_more"
    }
    fn description(&self) -> &str {
        "Send the team on a follow-up round: one new scout per question, then the critic \
         checks the findings and the writer revises the same paper to answer them. For a \
         follow-up the paper and notes cannot answer. You will be told when the revised \
         paper is ready."
    }
    fn parameters(&self) -> Value {
        questions_schema(1, MAX_FOLLOW_UPS)
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let args: QuestionsArgs = parse_args(self.name(), ctx.args)?;
        let Some(row) = self.app.store.research(self.research).await? else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "The research is gone.",
            ));
        };
        let Some(mut plan) = row.plan else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "The team has not started. Plan it with the user and call start_research.",
            ));
        };
        if row.status.is_running() {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "The team is working on a round. Tell the user you will look into it once \
                 this round ends, then call research_more when the result comes in.",
            ));
        }
        let round = plan.round() + 1;
        if round > MAX_ROUNDS {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!(
                    "This research had its {MAX_ROUNDS} rounds. Suggest a new research for it."
                ),
            ));
        }
        let room = MAX_TEAM_SCOUTS.saturating_sub(plan.scouts.len());
        let scouts = match scouts_for(
            &self.app.models,
            args.questions,
            1..=MAX_FOLLOW_UPS.min(room),
            round,
        ) {
            Ok(scouts) => scouts,
            Err(why) => return Ok(ToolResult::failure(ctx.tool_call_id, why)),
        };
        plan.scouts.extend(scouts);
        if !self.app.start_team(&self.user, self.research, plan).await? {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "The team is already working.",
            ));
        }
        Ok(ToolResult::success(
            ctx.tool_call_id,
            "The team started the follow-up round. Tell the user in one short sentence what \
             it will look into.",
        ))
    }
}

/// Adds scouts for gaps to the running round, run once the lead's turn ends.
pub struct AddScoutsTool {
    pub app: Arc<App>,
    pub round: u32,
    /// Room left for scouts in the research.
    pub room: usize,
    pub added: Arc<Mutex<Vec<Scout>>>,
}

#[async_trait]
impl Tool for AddScoutsTool {
    fn name(&self) -> &str {
        "add_scouts"
    }
    fn description(&self) -> &str {
        "Put new scouts on questions this round left open, one per question. Only for a gap \
         that matters to the answer and that a search could close. Call it at most once."
    }
    fn parameters(&self) -> Value {
        questions_schema(1, MAX_GAP_SCOUTS)
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let args: QuestionsArgs = parse_args(self.name(), ctx.args)?;
        let max = MAX_GAP_SCOUTS.min(self.room);
        let scouts = match scouts_for(&self.app.models, args.questions, 1..=max, self.round) {
            Ok(scouts) => scouts,
            Err(why) => return Ok(ToolResult::failure(ctx.tool_call_id, why)),
        };
        let count = scouts.len();
        {
            let Ok(mut added) = self.added.lock() else {
                anyhow::bail!("the added scouts are poisoned");
            };
            if !added.is_empty() {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "You added this round's scouts already.",
                ));
            }
            added.extend(scouts);
        }
        Ok(ToolResult::success(
            ctx.tool_call_id,
            format!("Added {count} scouts. They start when your turn ends; brief the writer now."),
        ))
    }
}

pub struct StopResearchTool {
    pub app: Arc<App>,
    pub research: Uuid,
}

#[async_trait]
impl Tool for StopResearchTool {
    fn name(&self) -> &str {
        "stop_research"
    }
    fn description(&self) -> &str {
        "Stop the research team. What it found so far stays. Only when the user asks."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {}, "additionalProperties": false })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        Ok(if self.app.stop_team(self.research).await? {
            ToolResult::success(ctx.tool_call_id, "Stopped the team.")
        } else {
            ToolResult::failure(ctx.tool_call_id, "The team was not working.")
        })
    }
}
