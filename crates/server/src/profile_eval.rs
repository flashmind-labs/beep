//! Local experiments on what the profile pass should read: Jev per email
//! against one model call per batch, with prompts from a variants file.
//!
//! `profile_eval_sample` caches a year of a real mailbox to `BEEP_EVAL_DIR/sample.json`;
//! `profile_eval_run` runs every variant in `BEEP_EVAL_DIR/variants.json` not yet in
//! `BEEP_EVAL_DIR/results.json` and adds it there.

use crate::{
    config::Config,
    decision::{Criteria, DecisionTool},
    feeds::mail::{Email, Mailbox, Source, Window},
    memories,
    starters::{Generator, unfence},
    store::Store,
    user::{Shared, User},
};
use anyhow::{Context, Result};
use chrono::{Duration as Days, Utc};
use flashmind::{
    llm::OpenRouterProvider,
    types::{AgentLlmConfig, Model},
};
use futures::{StreamExt, TryStreamExt, stream};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, fmt::Write, path::PathBuf, time::Instant};
use tokio::sync::broadcast;

const MONTHS: i64 = 12;
const PER_MONTH: usize = 10;
const EMAIL_CHARS: usize = 2500;
const TODAY: &str = "Friday 2026-10-09";

#[derive(Serialize, Deserialize, Clone)]
struct Sampled {
    n: usize,
    date: String,
    sent: bool,
    from: String,
    subject: String,
    text: String,
}

#[derive(Deserialize)]
struct Variants {
    #[serde(default)]
    jev: Vec<JevVariant>,
    #[serde(default)]
    batch: Vec<BatchVariant>,
}

#[derive(Deserialize)]
struct JevVariant {
    name: String,
    question: String,
    yes: Option<String>,
    no: Option<String>,
}

#[derive(Deserialize)]
struct BatchVariant {
    name: String,
    system: String,
    #[serde(default = "twenty")]
    size: usize,
    model: Option<String>,
    /// Only emails this Jev variant scored at least `threshold` are sent.
    filter: Option<String>,
    #[serde(default)]
    threshold: f64,
}

fn twenty() -> usize {
    20
}

#[derive(Deserialize)]
struct Verdicts {
    emails: Vec<Verdict>,
}

#[derive(Deserialize, Serialize)]
struct Verdict {
    n: usize,
    relevant: bool,
    #[serde(default)]
    notes: Vec<String>,
}

struct Env {
    dir: PathBuf,
    config: Config,
    key: String,
    user_id: String,
}

fn env() -> Result<Env> {
    let dir = PathBuf::from(std::env::var("BEEP_EVAL_DIR").context("set BEEP_EVAL_DIR")?);
    let path = std::env::var("BEEP_EVAL_CONFIG").context("set BEEP_EVAL_CONFIG")?;
    let config: Config = toml::from_str(&std::fs::read_to_string(&path)?)?;
    let key = config.provider_key.clone().context("no provider_key")?;
    let user_id = std::env::var("BEEP_EVAL_USER").unwrap_or_default();
    Ok(Env {
        dir,
        config,
        key,
        user_id,
    })
}

#[tokio::test]
#[ignore = "reads a real mailbox"]
async fn profile_eval_sample() -> Result<()> {
    let env = env()?;
    let store = Store::open(
        &env.config.database_url()?,
        memories::embedder(&env.config)?,
    )
    .await?;
    let account = store
        .accounts()
        .await?
        .into_iter()
        .find(|account| account.id == env.user_id)
        .context("no such user")?;
    let (notifications, _) = broadcast::channel(4);
    let user = User::new(
        &account,
        &Shared {
            store,
            notifications,
            composio: None,
            decision: None,
        },
    );
    let source = Source::new(&user, Mailbox::Fastmail).await?;
    let mut emails: Vec<Email> = Vec::new();
    for month in 0..MONTHS {
        let window = Window {
            after: Utc::now() - Days::days(30 * (month + 1)),
            before: Some(Utc::now() - Days::days(30 * month)),
        };
        let mut listed = Vec::new();
        let mut pages = source.history(window).boxed();
        while listed.len() < 200 {
            let Some(page) = pages.try_next().await? else {
                break;
            };
            listed.extend(page);
        }
        drop(pages);
        let step = (listed.len() / PER_MONTH).max(1);
        eprintln!("month {month}: listed {}", listed.len());
        emails.extend(listed.into_iter().step_by(step).take(PER_MONTH));
    }
    stream::iter(emails.iter_mut())
        .for_each_concurrent(16, |email| source.fill_body(email))
        .await;
    let sampled: Vec<Sampled> = emails
        .iter()
        .enumerate()
        .map(|(i, email)| Sampled {
            n: i + 1,
            date: email.received.format("%Y-%m-%d").to_string(),
            sent: email.sent,
            from: email.from.clone(),
            subject: email.subject.clone(),
            text: email.render(EMAIL_CHARS),
        })
        .collect();
    std::fs::write(
        env.dir.join("sample.json"),
        serde_json::to_string_pretty(&sampled)?,
    )?;
    eprintln!("sampled {}", sampled.len());
    Ok(())
}

fn leak(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}

#[tokio::test]
#[ignore = "costs money"]
async fn profile_eval_run() -> Result<()> {
    let env = env()?;
    let set = std::env::var("BEEP_EVAL_SET").unwrap_or_else(|_| "sample".into());
    let sample: Vec<Sampled> = serde_json::from_str(&std::fs::read_to_string(
        env.dir.join(format!("{set}.json")),
    )?)?;
    let variants: Variants = serde_json::from_str(&std::fs::read_to_string(
        env.dir.join(format!("{set}-variants.json")),
    )?)?;
    let results_path = env.dir.join(format!("{set}-results.json"));
    let mut results: Map<String, Value> = std::fs::read_to_string(&results_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let decision = DecisionTool::new(SecretString::from(env.key.as_str()))?;
    let provider = OpenRouterProvider::new(env.key.clone());

    for variant in &variants.jev {
        let name = format!("jev:{}", variant.name);
        if results.contains_key(&name) {
            continue;
        }
        let criteria = variant
            .yes
            .as_ref()
            .zip(variant.no.as_ref())
            .map(|(yes, no)| Criteria {
                yes: leak(yes),
                no: leak(no),
            });
        let started = Instant::now();
        let scores: Vec<Option<f64>> = stream::iter(&sample)
            .map(|email| {
                let (decision, criteria, name) = (&decision, &criteria, &name);
                let state = format!("Today: {TODAY}\n{}", email.text);
                async move {
                    match criteria {
                        Some(criteria) => {
                            decision
                                .probability_with(&state, &variant.question, criteria)
                                .await
                        }
                        None => decision.probability(&state, &variant.question).await,
                    }
                    .inspect_err(|error| eprintln!("{name}: {error:#}"))
                    .ok()
                }
            })
            .buffered(16)
            .collect()
            .await;
        let seconds = started.elapsed().as_secs_f64();
        eprintln!("{name}: {seconds:.1}s");
        results.insert(name, json!({"seconds": seconds, "scores": scores}));
        std::fs::write(&results_path, serde_json::to_string_pretty(&results)?)?;
    }

    for variant in &variants.batch {
        let name = format!("batch:{}", variant.name);
        if results.contains_key(&name) {
            continue;
        }
        let picked: Vec<&Sampled> = match &variant.filter {
            Some(filter) => {
                let scores = results[&format!("jev:{filter}")]["scores"]
                    .as_array()
                    .context("filter not run")?
                    .clone();
                sample
                    .iter()
                    .zip(scores)
                    .filter(|(_, score)| score.as_f64().is_some_and(|s| s >= variant.threshold))
                    .map(|(email, _)| email)
                    .collect()
            }
            None => sample.iter().collect(),
        };
        let model: Model = variant
            .model
            .as_deref()
            .unwrap_or(&env.config.model)
            .parse()?;
        let mut llm = AgentLlmConfig::new(model);
        llm.provider_preferences = Some(env.config.routing.clone());
        let generator = Generator {
            provider: &provider,
            llm: &llm,
            user: "eval",
        };
        let started = Instant::now();
        let batches: Vec<Result<Verdicts>> = stream::iter(picked.chunks(variant.size.max(1)))
            .map(|chunk| {
                let (system, name) = (&variant.system, &name);
                async move {
                    let mut prompt = format!("Today: {TODAY}\n");
                    for email in chunk {
                        let _ = write!(prompt, "\n=== Email {} ===\n{}\n", email.n, email.text);
                    }
                    let mut last = None;
                    for _ in 0..2 {
                        let answer = generator.text_up_to(system, prompt.clone(), 12000).await;
                        match answer.and_then(|answer| {
                            serde_json::from_str::<Verdicts>(unfence(&answer)).context("not JSON")
                        }) {
                            Ok(verdicts) => return Ok(verdicts),
                            Err(error) => {
                                eprintln!("{name}: {error:#}");
                                last = Some(error);
                            }
                        }
                    }
                    Err(last.expect("two tries ran"))
                }
            })
            .buffered(8)
            .collect()
            .await;
        let seconds = started.elapsed().as_secs_f64();
        let mut verdicts: BTreeMap<usize, Verdict> = BTreeMap::new();
        let mut failed = 0;
        for batch in batches {
            match batch {
                Ok(found) => verdicts.extend(found.emails.into_iter().map(|v| (v.n, v))),
                Err(_) => failed += 1,
            }
        }
        eprintln!(
            "{name}: {seconds:.1}s, {} sent, {failed} batches failed",
            picked.len()
        );
        results.insert(
            name,
            json!({"seconds": seconds, "sent": picked.len(), "failed": failed, "verdicts": verdicts}),
        );
        std::fs::write(&results_path, serde_json::to_string_pretty(&results)?)?;
    }
    Ok(())
}

/// Runs the chat learner on each turn of `chat.json` with every system prompt
/// in `learn-variants.json` (`{"name": "system"}`, `"current"` for the
/// shipped one) and writes what it would store to `learn-results.json`.
#[tokio::test]
#[ignore = "costs money"]
async fn profile_eval_learn() -> Result<()> {
    let env = env()?;
    let chat: Vec<Sampled> =
        serde_json::from_str(&std::fs::read_to_string(env.dir.join("chat.json"))?)?;
    let variants: BTreeMap<String, String> = serde_json::from_str(&std::fs::read_to_string(
        env.dir.join("learn-variants.json"),
    )?)?;
    let provider = OpenRouterProvider::new(env.key.clone());
    let model: Model = env.config.model.parse()?;
    let mut llm = AgentLlmConfig::new(model).with_reasoning(env.config.reasoning);
    llm.max_tokens = Some(8192);
    llm.provider_preferences = Some(env.config.routing.clone());
    let generator = Generator {
        provider: &provider,
        llm: &llm,
        user: "eval",
    };
    let mut results: Map<String, Value> = Map::new();
    for (name, system) in &variants {
        let system: &str = if system == "current" {
            memories::LEARN_SYSTEM
        } else {
            system
        };
        let ops: Vec<Value> = stream::iter(&chat)
            .map(|turn| async move {
                let latest = turn.text.split("\n\nUser: ").nth(1).unwrap_or_default();
                let (message, reply) = latest.split_once("\n\nAssistant: ").unwrap_or((latest, ""));
                let exchange = memories::Exchange {
                    session_id: "eval".into(),
                    today: Utc::now().date_naive(),
                    earlier: Vec::new(),
                    latest: memories::Turn {
                        message: message.into(),
                        reply: reply.into(),
                        actions: Vec::new(),
                    },
                };
                let prompt = memories::learn_prompt(&[], &exchange);
                match generator.text(system, prompt).await {
                    Ok(answer) => {
                        serde_json::from_str(unfence(&answer)).unwrap_or(Value::String(answer))
                    }
                    Err(error) => Value::String(format!("{error:#}")),
                }
            })
            .buffered(16)
            .collect()
            .await;
        eprintln!("learn:{name} done");
        results.insert(name.clone(), Value::Array(ops));
    }
    std::fs::write(
        env.dir.join("learn-results.json"),
        serde_json::to_string_pretty(&results)?,
    )?;
    Ok(())
}

/// Runs the shipped email learner on every email of `sample.json` that
/// `jev:life` scored at least 0.3, into `reading-<BEEP_EVAL_TAG>.json`.
#[tokio::test]
#[ignore = "costs money"]
async fn profile_eval_reading() -> Result<()> {
    let env = env()?;
    let tag = std::env::var("BEEP_EVAL_TAG").context("set BEEP_EVAL_TAG")?;
    let sample: Vec<Sampled> =
        serde_json::from_str(&std::fs::read_to_string(env.dir.join("sample.json"))?)?;
    let results: Map<String, Value> = serde_json::from_str(&std::fs::read_to_string(
        env.dir.join("sample-results.json"),
    )?)?;
    let scores = results["jev:life"]["scores"]
        .as_array()
        .context("no jev:life")?;
    let provider = OpenRouterProvider::new(env.key.clone());
    let model: Model = env.config.model.parse()?;
    let mut llm = AgentLlmConfig::new(model).with_reasoning(env.config.reasoning);
    llm.max_tokens = Some(8192);
    llm.provider_preferences = Some(env.config.routing.clone());
    let generator = Generator {
        provider: &provider,
        llm: &llm,
        user: "eval",
    };
    let system = format!(
        "{}\n{}",
        memories::ReadingKind::Email.intro(),
        memories::LEARN_RULES
    );
    let picked: Vec<&Sampled> = sample
        .iter()
        .zip(scores)
        .filter(|(_, score)| score.as_f64().is_some_and(|s| s >= 0.3))
        .map(|(email, _)| email)
        .collect();
    let out: BTreeMap<usize, Value> = stream::iter(picked)
        .map(|email| {
            let system = &system;
            async move {
                let reading = memories::Reading {
                    kind: memories::ReadingKind::Email,
                    id: email.n.to_string(),
                    query: String::new(),
                    text: format!("Email:\n{}", email.text),
                };
                let prompt = memories::reading_prompt(&[], &reading);
                let answer = match generator.text(system, prompt).await {
                    Ok(answer) => {
                        serde_json::from_str(unfence(&answer)).unwrap_or(Value::String(answer))
                    }
                    Err(error) => Value::String(format!("{error:#}")),
                };
                (email.n, answer)
            }
        })
        .buffered(16)
        .collect()
        .await;
    std::fs::write(
        env.dir.join(format!("reading-{tag}.json")),
        serde_json::to_string_pretty(&out)?,
    )?;
    Ok(())
}
