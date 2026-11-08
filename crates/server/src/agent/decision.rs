//! Server-only Jev decisions through OpenRouter. The key and upstream error bodies never reach clients.
use crate::agent::models::AutoModel;
use anyhow::{Result, ensure};
use async_trait::async_trait;
use flashmind::{
    llm::{JEV_LATEST_MODEL, JevQuestion, JevRequest, JevResponse, OpenRouterProvider},
    types::{
        LlmError, LlmErrorKind, ReasoningLevel,
        tool::{Tool, ToolContext, ToolResult},
    },
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const MAX_STATE_BYTES: usize = 20_000;
const MAX_QUESTIONS: usize = 16;
const MAX_ID_BYTES: usize = 64;

/// What counts as a yes and as a no for a yes/no question.
pub struct Criteria {
    pub yes: &'static str,
    pub no: &'static str,
}

/// The model and thinking Jev picked for a prompt.
pub struct Routed<'a> {
    pub model: &'a AutoModel,
    pub thinking: ReasoningLevel,
}

const PICK_MODEL: &str = "Which model should answer the user's latest message? Pick the one whose description fits the task best; prefer the faster model when both would do it well.";
const PICK_THINKING: &str =
    "How long should the model think before answering the user's latest message?";
/// The thinking levels Jev picks from, with what each suits.
const THINKING: [(ReasoningLevel, &str); 4] = [
    (
        ReasoningLevel::Off,
        "Replies that need no thought: small talk, thanks, a yes or no, rewording or translating a short text.",
    ),
    (
        ReasoningLevel::Low,
        "Quick lookups: a fact, a price, opening hours, the news, a short reply.",
    ),
    (
        ReasoningLevel::Medium,
        "A few steps or tools: comparing options, reading one email thread, a short plan.",
    ),
    (
        ReasoningLevel::High,
        "Demanding work: planning a week's schedule, organizing events, reading or writing email across threads, multi-step tasks.",
    ),
];

/// Holds the key only inside the provider, which has no `Debug` and never logs it.
pub struct DecisionTool {
    provider: OpenRouterProvider,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionArgs {
    state: String,
    questions: BTreeMap<String, JevQuestion>,
}

impl DecisionTool {
    pub fn new(key: SecretString) -> Result<Self> {
        let raw = key.expose_secret();
        ensure!(
            !raw.is_empty() && !raw.chars().any(char::is_whitespace),
            "invalid OpenRouter key"
        );
        let provider = OpenRouterProvider::new(raw.to_owned()).with_app_name("Beep".into());
        Ok(Self { provider })
    }

    #[cfg(test)]
    fn with_system_one_url(self, url: String) -> Self {
        Self {
            provider: self.provider.with_system_one_url(url),
        }
    }

    /// Jev's probability that the answer to `instructions` about `state` is yes, given a short
    /// question and what counts as yes and as no.
    pub async fn probability_with(
        &self,
        state: &str,
        instructions: &str,
        criteria: &Criteria,
    ) -> Result<f64> {
        let criteria = BTreeMap::from([
            ("true".to_owned(), criteria.yes.to_owned()),
            ("false".to_owned(), criteria.no.to_owned()),
        ]);
        self.judge(state, instructions, Some(criteria)).await
    }

    /// Jev's probability of yes for each of `questions`, in one request; unanswered ones are missing.
    pub async fn probabilities(
        &self,
        state: &str,
        questions: &[(&str, &str, &Criteria)],
    ) -> Result<BTreeMap<String, f64>> {
        let asked = questions
            .iter()
            .map(|(key, instructions, criteria)| {
                let question = JevQuestion::Noul {
                    instructions: (*instructions).into(),
                    criteria: Some(BTreeMap::from([
                        ("true".to_owned(), criteria.yes.to_owned()),
                        ("false".to_owned(), criteria.no.to_owned()),
                    ])),
                };
                ((*key).to_owned(), question)
            })
            .collect();
        let response = self.provider.decide(request(state, asked)).await?;
        Ok(response
            .answers
            .iter()
            .filter_map(|(key, answer)| Some((key.clone(), answer["noul"].as_f64()?)))
            .collect())
    }

    /// Jev's pick of one of `options` for `instructions` about `state`.
    pub async fn choose(
        &self,
        state: &str,
        instructions: &str,
        options: &[(&'static str, &str)],
    ) -> Result<&'static str> {
        let question = JevQuestion::Choice {
            instructions: instructions.into(),
            criteria: options
                .iter()
                .map(|(key, fits)| ((*key).to_owned(), (*fits).to_owned()))
                .collect(),
        };
        let questions = BTreeMap::from([("choice".to_owned(), question)]);
        let response = self.provider.decide(request(state, questions)).await?;
        let picked =
            choice(&response, "choice").ok_or_else(|| anyhow::anyhow!("Jev made no choice"))?;
        options
            .iter()
            .map(|(key, _)| *key)
            .find(|key| *key == picked)
            .ok_or_else(|| anyhow::anyhow!("Jev chose an unknown option: {picked}"))
    }

    /// Jev's pick for each of `questions`, in one request; unanswered or unknown picks are missing.
    pub async fn choose_each(
        &self,
        state: &str,
        questions: &[(&str, &str)],
        options: &[(&'static str, &str)],
    ) -> Result<BTreeMap<String, &'static str>> {
        let criteria: BTreeMap<String, String> = options
            .iter()
            .map(|(key, fits)| ((*key).to_owned(), (*fits).to_owned()))
            .collect();
        let asked = questions
            .iter()
            .map(|(key, instructions)| {
                let question = JevQuestion::Choice {
                    instructions: (*instructions).into(),
                    criteria: criteria.clone(),
                };
                ((*key).to_owned(), question)
            })
            .collect();
        let response = self.provider.decide(request(state, asked)).await?;
        Ok(questions
            .iter()
            .filter_map(|(key, _)| {
                let picked = choice(&response, key)?;
                let option = options
                    .iter()
                    .map(|(option, _)| *option)
                    .find(|o| *o == picked)?;
                Some(((*key).to_owned(), option))
            })
            .collect())
    }

    /// Jev's pick of a model and thinking level; with one model only the thinking is asked.
    pub async fn route<'a>(&self, state: &str, models: &'a [AutoModel]) -> Result<Routed<'a>> {
        let mut questions = BTreeMap::from([("thinking".to_owned(), thinking_question())]);
        if models.len() > 1 {
            let criteria = models
                .iter()
                .map(|auto| (auto.model.name().to_owned(), auto.when.clone()))
                .collect();
            questions.insert(
                "model".to_owned(),
                JevQuestion::Choice {
                    instructions: PICK_MODEL.into(),
                    criteria,
                },
            );
        }
        let response = self.provider.decide(request(state, questions)).await?;
        let thinking = choice(&response, "thinking")
            .and_then(|level| level.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("Jev picked no thinking level"))?;
        let model = match models {
            [only] => only,
            _ => {
                let picked = choice(&response, "model")
                    .ok_or_else(|| anyhow::anyhow!("Jev picked no model"))?;
                models
                    .iter()
                    .find(|auto| auto.model.name() == picked)
                    .ok_or_else(|| anyhow::anyhow!("Jev picked an unknown model: {picked}"))?
            }
        };
        Ok(Routed { model, thinking })
    }

    async fn judge(
        &self,
        state: &str,
        instructions: &str,
        criteria: Option<BTreeMap<String, String>>,
    ) -> Result<f64> {
        let question = JevQuestion::Noul {
            instructions: instructions.into(),
            criteria,
        };
        let questions = BTreeMap::from([("yes".to_owned(), question)]);
        let response = self.provider.decide(request(state, questions)).await?;
        response
            .answers
            .get("yes")
            .and_then(|answer| answer["noul"].as_f64())
            .ok_or_else(|| anyhow::anyhow!("Jev gave no probability"))
    }

    async fn decide(&self, args: &Value) -> Result<Value, String> {
        let request = parse(args)?;
        let response = self.provider.decide(request).await.map_err(describe)?;
        Ok(json!({"answers": response.answers}))
    }
}

/// How long the model should think, with what each level suits.
fn thinking_question() -> JevQuestion {
    JevQuestion::Choice {
        instructions: PICK_THINKING.into(),
        criteria: THINKING
            .iter()
            .map(|(level, when)| (level.to_string(), (*when).to_owned()))
            .collect(),
    }
}

/// A request for `questions` about `state`, cut to what Jev takes.
fn request(state: &str, questions: BTreeMap<String, JevQuestion>) -> JevRequest {
    JevRequest {
        model: JEV_LATEST_MODEL.into(),
        state: state[..state.floor_char_boundary(MAX_STATE_BYTES)].into(),
        questions,
    }
}

/// The option Jev picked for the choice question `key`.
fn choice<'a>(response: &'a JevResponse, key: &str) -> Option<&'a str> {
    response
        .answers
        .get(key)
        .and_then(|answer| answer["choice"].as_str())
}

/// Turns a provider error into a client-safe message; transport errors are not echoed.
fn describe(error: anyhow::Error) -> String {
    match error.downcast_ref::<LlmError>() {
        Some(error) if error.kind == LlmErrorKind::RateLimited => {
            "Decision is rate limited. Try again shortly.".into()
        }
        Some(error) => format!("Decision failed: {}.", error.message),
        None => "Decision could not reach Jev. Try again shortly.".into(),
    }
}

fn parse(args: &Value) -> Result<JevRequest, String> {
    let args = DecisionArgs::deserialize(args).map_err(|e| format!("Invalid decision: {e}"))?;
    let state = args.state.trim();
    if state.is_empty() || state.len() > MAX_STATE_BYTES {
        return Err(format!(
            "Decision state must contain 1-{MAX_STATE_BYTES} bytes."
        ));
    }
    if args.questions.is_empty() || args.questions.len() > MAX_QUESTIONS {
        return Err(format!("Decision needs 1-{MAX_QUESTIONS} questions."));
    }
    if let Some(id) = args
        .questions
        .keys()
        .find(|id| id.is_empty() || id.len() > MAX_ID_BYTES)
    {
        return Err(format!(
            "Question ID {id:?} must contain 1-{MAX_ID_BYTES} bytes."
        ));
    }
    Ok(JevRequest {
        model: JEV_LATEST_MODEL.into(),
        state: state.into(),
        questions: args.questions,
    })
}

#[async_trait]
impl Tool for DecisionTool {
    fn name(&self) -> &str {
        "decision"
    }
    fn description(&self) -> &str {
        "Make fast, calibrated judgments over some text with Jev. Use it to categorize, route, filter or answer yes/no questions about a message, email, note or other item, instead of guessing. Ask several related questions in one call. Types: noul gives the probability that the answer is yes; choice picks one key from criteria; score rates the text against an ordered list of criteria. Treat answers as recommendations. Keep secrets and unnecessary personal data out of state."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "state": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": MAX_STATE_BYTES,
                    "description": "The text to judge."
                },
                "questions": {
                    "type": "object",
                    "minProperties": 1,
                    "maxProperties": MAX_QUESTIONS,
                    "description": "Questions keyed by short IDs such as is_spam or team.",
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "type": {"type": "string", "enum": ["noul", "choice", "score"]},
                            "instructions": {"type": "string", "description": "The question to answer about the state."},
                            "criteria": {
                                "description": "choice: object mapping each option key to its description (required). score: ordered array of level descriptions (required). noul: optional object of hints.",
                                "anyOf": [
                                    {"type": "object", "additionalProperties": {"type": "string"}},
                                    {"type": "array", "items": {"type": "string"}}
                                ]
                            }
                        },
                        "required": ["type", "instructions"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["state", "questions"],
            "additionalProperties": false
        })
    }
    fn humanize(&self, _args: &Value) -> String {
        "Deciding…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        Ok(match self.decide(&ctx.args).await {
            Ok(output) => ToolResult::success(ctx.tool_call_id, output.to_string()),
            Err(error) => ToolResult::failure(ctx.tool_call_id, error),
        })
    }
}

#[cfg(test)]
#[path = "../tests/agent_decision.rs"]
mod tests;
