//! Server-only Jev decisions through OpenRouter. The key and upstream error bodies never reach clients.
use crate::{agents::Agent, models::AutoModel};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use flashmind::llm::{JEV_LATEST_MODEL, JevQuestion, JevRequest, JevResponse, OpenRouterProvider};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use flashmind::types::{LlmError, LlmErrorKind, ReasoningLevel};
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

/// The agent and thinking Jev picked for a prompt.
pub struct AgentRouted<'a> {
    pub agent: &'a Agent,
    pub thinking: ReasoningLevel,
}

/// The model and thinking Jev picked for a prompt.
pub struct Routed<'a> {
    pub model: &'a AutoModel,
    pub thinking: ReasoningLevel,
}

const PICK_MODEL: &str = "Which model should answer the user's latest message? Pick the one whose description fits the task best; prefer the faster model when both would do it well.";
const PICK_AGENT: &str = "Which agent should answer the user's latest message? Pick the one whose description fits the task. When the message continues the task of the current agent, like a follow-up question, keep the current agent. Pick general when no other agent clearly fits.";
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

    /// Jev's probability that the answer to `instructions` about `state` is yes.
    pub async fn probability(&self, state: &str, instructions: &str) -> Result<f64> {
        self.judge(state, instructions, None).await
    }

    /// Like `probability`, with a short question and what counts as yes and as no.
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

    /// Jev's probability of yes for each of `questions`, asked together in
    /// one request and keyed as given. A question Jev left unanswered is
    /// missing from the result.
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

    /// Jev's pick of one of `options`, each named with what it fits, for
    /// `instructions` about `state`.
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

    /// Jev's pick of one of `options` for each of `questions`, asked together
    /// in one request about `state` and keyed as given. A question Jev left
    /// unanswered, or answered with an unknown option, is missing.
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

    /// Jev's pick of one of `models` and a thinking level for the prompt in
    /// `state`. With a single model only the thinking is asked.
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

    /// Jev's pick of one of `agents` and a thinking level for the prompt in
    /// `state`, asked together in one request.
    pub async fn route_agent<'a>(
        &self,
        state: &str,
        agents: &'a [Agent],
    ) -> Result<AgentRouted<'a>> {
        let criteria = agents
            .iter()
            .map(|agent| (agent.id.as_str().to_owned(), agent.when.clone()))
            .collect();
        let questions = BTreeMap::from([
            ("thinking".to_owned(), thinking_question()),
            (
                "agent".to_owned(),
                JevQuestion::Choice {
                    instructions: PICK_AGENT.into(),
                    criteria,
                },
            ),
        ]);
        let response = self.provider.decide(request(state, questions)).await?;
        let thinking = choice(&response, "thinking")
            .and_then(|level| level.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("Jev picked no thinking level"))?;
        let picked =
            choice(&response, "agent").ok_or_else(|| anyhow::anyhow!("Jev picked no agent"))?;
        let agent = agents
            .iter()
            .find(|agent| agent.id.as_str() == picked)
            .ok_or_else(|| anyhow::anyhow!("Jev picked an unknown agent: {picked}"))?;
        Ok(AgentRouted { agent, thinking })
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

/// Turns a provider error into a client-safe message. The provider already
/// leaves upstream bodies out of its errors; transport errors are not echoed.
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
mod tests {
    use super::*;
    use crate::agents::AgentId;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    fn args() -> Value {
        json!({
            "state": "Customer was charged twice and wants a refund.",
            "questions": {
                "needs_reply": {"type": "noul", "instructions": "Does this need a reply?"},
                "team": {
                    "type": "choice",
                    "instructions": "Which team should handle this?",
                    "criteria": {"billing": "Charges and refunds", "technical": "Product defects"}
                }
            }
        })
    }

    async fn read_request(stream: &mut TcpStream) -> String {
        let mut data = Vec::new();
        loop {
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            assert!(n > 0);
            data.extend_from_slice(&buf[..n]);
            if data.ends_with(b"}") {
                break;
            }
        }
        String::from_utf8(data).unwrap()
    }

    /// Answers every connection with the same response, so client retries see it too.
    async fn mock(
        status: &'static str,
        body: &'static str,
    ) -> (DecisionTool, UnboundedReceiver<String>) {
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nRetry-After: 0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/v1/systemone", listener.local_addr().unwrap());
        let tool = DecisionTool::new("private-test-key".into())
            .unwrap()
            .with_system_one_url(url);
        let (tx, rx) = unbounded_channel();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let request = read_request(&mut stream).await;
                stream.write_all(response.as_bytes()).await.unwrap();
                let _ = tx.send(request);
            }
        });
        (tool, rx)
    }

    #[tokio::test]
    async fn returns_answers_from_jev() {
        let body = r#"{"id":"r1","model":"jev-1.13","answers":{"needs_reply":{"p":0.93},"team":{"choice":"billing"}},"usage":{"input_tokens":12}}"#;
        let (tool, mut requests) = mock("200 OK", body).await;
        let output = tool.decide(&args()).await.unwrap();
        let request = requests.recv().await.unwrap();
        assert!(request.contains("Bearer private-test-key"));
        assert!(request.contains("\"model\":\"jev-latest\""));
        assert!(request.contains("\"type\":\"choice\""));
        assert_eq!(output["answers"]["team"]["choice"], "billing");
        assert_eq!(output["answers"]["needs_reply"]["p"], 0.93);
        assert!(output.get("usage").is_none());
    }

    #[tokio::test]
    async fn reads_one_probability() {
        let body = r#"{"model":"jev-1.13","answers":{"yes":{"type":"noul","noul":0.81}}}"#;
        let (tool, mut requests) = mock("200 OK", body).await;
        let p = tool.probability("User: I moved to Porto", "Worth remembering?");
        assert_eq!(p.await.unwrap(), 0.81);
        let request = requests.recv().await.unwrap();
        assert!(request.contains("\"type\":\"noul\""));
        assert!(request.contains("I moved to Porto"));
    }

    #[tokio::test]
    async fn chooses_only_a_known_option() {
        let options = [("plan", "Trips"), ("work", "Jobs")];
        let body = r#"{"model":"jev-1.13","answers":{"choice":{"choice":"plan"}}}"#;
        let (tool, mut requests) = mock("200 OK", body).await;
        let picked = tool.choose("Is in Lisbon until Friday.", "Which?", &options);
        assert_eq!(picked.await.unwrap(), "plan");
        assert!(requests.recv().await.unwrap().contains("\"Trips\""));
        let body = r#"{"model":"jev-1.13","answers":{"choice":{"choice":"travel"}}}"#;
        let (tool, _) = mock("200 OK", body).await;
        assert!(
            tool.choose("Is in Lisbon.", "Which?", &options)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn chooses_for_each_question_in_one_request() {
        let options = [("plan", "Trips"), ("work", "Jobs")];
        let body =
            r#"{"model":"jev-1.13","answers":{"1":{"choice":"plan"},"2":{"choice":"travel"}}}"#;
        let (tool, mut requests) = mock("200 OK", body).await;
        let questions = [("1", "Memory 1?"), ("2", "Memory 2?"), ("3", "Memory 3?")];
        let filed = tool
            .choose_each("1. Is in Lisbon.\n2. Works at Acme.", &questions, &options)
            .await
            .unwrap();
        assert_eq!(filed, BTreeMap::from([("1".to_owned(), "plan")]));
        let request = requests.recv().await.unwrap();
        assert!(request.contains("Memory 3?"));
        assert!(requests.try_recv().is_err(), "one request for all three");
    }

    #[tokio::test]
    async fn routes_to_a_model_and_thinking() {
        let body = r#"{"model":"jev-1.13","answers":{"model":{"choice":"openai/gpt-6-luna"},"thinking":{"choice":"high"}}}"#;
        let (tool, mut requests) = mock("200 OK", body).await;
        let models = [
            AutoModel {
                model: "openrouter:z-ai/glm-5.3-flash".parse().unwrap(),
                name: "GLM 5.3 Flash".into(),
                when: "Quick lookups".into(),
            },
            AutoModel {
                model: "openrouter:openai/gpt-6-luna".parse().unwrap(),
                name: "GPT-6 Luna".into(),
                when: "Demanding work".into(),
            },
        ];
        let routed = tool.route("User: plan my week", &models).await.unwrap();
        assert_eq!(routed.model.name, "GPT-6 Luna");
        assert_eq!(routed.thinking, ReasoningLevel::High);
        let request = requests.recv().await.unwrap();
        assert!(request.contains("\"Demanding work\""));
        assert!(request.contains("\"medium\""));
        assert!(request.contains("\"off\""));
    }

    #[tokio::test]
    async fn routes_without_thinking() {
        let body = r#"{"model":"jev-1.13","answers":{"thinking":{"choice":"off"}}}"#;
        let (tool, _requests) = mock("200 OK", body).await;
        let models = [AutoModel {
            model: "openrouter:z-ai/glm-5.3-flash".parse().unwrap(),
            name: "GLM 5.3 Flash".into(),
            when: "Quick lookups".into(),
        }];
        let routed = tool.route("User: thanks!", &models).await.unwrap();
        assert_eq!(routed.thinking, ReasoningLevel::Off);
    }

    #[tokio::test]
    async fn routes_to_an_agent_and_thinking() {
        let body = r#"{"model":"jev-1.13","answers":{"agent":{"choice":"getting_around"},"thinking":{"choice":"medium"}}}"#;
        let (tool, mut requests) = mock("200 OK", body).await;
        let agent = |id, when: &str| Agent {
            id,
            name: when.into(),
            when: when.into(),
            model: "openrouter:z-ai/glm-5.3-flash".parse().unwrap(),
            thinking: ReasoningLevel::Low,
        };
        let agents = [
            agent(AgentId::General, "Anything else"),
            agent(AgentId::GettingAround, "Directions and transit"),
        ];
        let routed = tool
            .route_agent(
                "Current agent: getting_around\n\nLatest message from the user: which exit",
                &agents,
            )
            .await
            .unwrap();
        assert_eq!(routed.agent.id, AgentId::GettingAround);
        assert_eq!(routed.thinking, ReasoningLevel::Medium);
        let request = requests.recv().await.unwrap();
        assert!(request.contains("\"Directions and transit\""));
        assert!(request.contains("keep the current agent"));
    }

    #[tokio::test]
    async fn redacts_upstream_errors() {
        let (tool, mut requests) = mock("429 Too Many Requests", "private-test-key").await;
        let error = tool.decide(&args()).await.unwrap_err();
        requests.recv().await.unwrap();
        assert!(error.contains("rate limited"));
        assert!(!error.contains("private-test-key"));
    }

    #[test]
    fn rejects_bad_requests() {
        assert!(parse(&json!({"state": " ", "questions": args()["questions"]})).is_err());
        assert!(parse(&json!({"state": "x", "questions": {}})).is_err());
        assert!(
            parse(
                &json!({"state": "x", "questions": {"q": {"type": "choice", "instructions": "?"}}})
            )
            .is_err()
        );
        assert!(
            parse(
                &json!({"state": "x", "questions": {"q": {"type": "maybe", "instructions": "?"}}})
            )
            .is_err()
        );
    }
}
