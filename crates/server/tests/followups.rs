//! Follow-up evals: fixed turns through the follow-up writer, each chip
//! checked by a judge model. They call the real model, so they are ignored by
//! default. `just eval-followups` runs them with the dev config.
use beep_server::{
    config::Config,
    decision::DecisionTool,
    followups::{self, Input, ToolOutput},
    memories::Turn,
    starters::Generator,
};
use flashmind::{
    llm::OpenRouterProvider,
    types::{AgentLlmConfig, Model},
};
use secrecy::SecretString;
use serde::Deserialize;
use std::{env, fs, path::PathBuf};

/// What `suggest_replies` keeps; the writer is asked for 40.
const MAX_CHARS: usize = 50;

const JUDGE: &str = "You grade follow-up chips shown under an assistant's reply. Each chip \
is a message the user could tap to send next. A chip passes only when all of these hold:
- the assistant can answer or do it with what it has (reminders, alarms, calendar events, \
directions, web search, email in a connected app);
- it is not already answered by the reply, the tool results or what the assistant knew;
- it is specific to this reply, not one that would fit any reply;
- a user in this situation would plausibly tap it.
Answer with one line per chip, in order, each \"PASS\" or \"FAIL: <short reason>\", and \
nothing else.";

struct Case {
    name: &'static str,
    context: Option<&'static str>,
    message: &'static str,
    reply: &'static str,
    results: &'static [(&'static str, &'static str)],
    /// At least one chip contains one of these, ignoring case.
    wants_any: &'static [&'static str],
    /// Every chip is plain ASCII, for an English chat about a place or sources
    /// in another script.
    ascii: bool,
}

const CASES: &[Case] = &[
    Case {
        name: "meeting later today",
        context: Some(
            "Time: Thursday 2026-03-12 16:04 +08\nLocation: Singapore\n\
             Connected apps: Gmail, Google Calendar.\n\
             What Beep remembers about the user:\n\
             - The user has a meeting with Acme Labs on March 12, 2026, from 17:30 to 18:00 at the Grand Hotel.",
        ),
        message: "Meeting with Acme today. Where",
        reply: "It's at the **Grand Hotel, 1 Main Rd**, in **Room 1253**. The invite says to \
                message Alex when you're at the elevator bay. It starts at **17:30**. The \
                location is on the map card below.",
        results: &[
            (
                "google_calendar_list_events",
                r#"[{"summary":"Acme sync","start":"2026-03-12T17:30:00+08:00","end":"2026-03-12T18:00:00+08:00","location":"Grand Hotel, 1 Main Rd, Room 1253","description":"Message Alex at the elevator bay."}]"#,
            ),
            (
                "open_maps",
                r#"{"action":"open_maps","message":"Shown in the chat as a map card with Directions and Open in Maps buttons."}"#,
            ),
        ],
        wants_any: &["remind", "alarm", "leave", "directions", "traffic"],
        ascii: false,
    },
    Case {
        name: "explains a topic",
        context: Some(
            "Time: Saturday 2026-03-14 10:12 +01\nLocation: Lisbon\nConnected apps: none.",
        ),
        message: "Why do onions make you cry?",
        reply: "Cutting an onion breaks its cells, and enzymes turn its sulfur compounds into \
                a gas called syn-propanethial-S-oxide. It drifts up, reacts with the water in \
                your eyes and irritates the nerves there, so they water to wash it out.",
        results: &[],
        wants_any: &[],
        ascii: false,
    },
    Case {
        name: "english chat about japanese sources",
        context: Some("Time: Monday 2026-03-16 09:30 +09\nLocation: Tokyo\nConnected apps: none."),
        message: "I got a letter from the ward office about my residence tax, what is it?",
        reply: "It's your **residence tax** bill for this year, from your ward office. It \
                lists the total and four payment slips; the first is due **March 31**. You can \
                pay at a convenience store, a bank or online with the QR code on each slip.",
        results: &[(
            "web_search",
            "住民税の納付について: 納付書は年4回に分けて送付されます。第1期の納期限は3月31日です。コンビニエンスストア、金融機関、スマートフォン決済で納付できます。",
        )],
        wants_any: &[],
        ascii: true,
    },
];

#[derive(Deserialize)]
struct Suggestions {
    replies: Vec<String>,
}

fn config() -> Config {
    let path = env::var_os("BEEP_EVAL_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.dev/android-server.toml")
        });
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    toml::from_str(&text).expect("the eval config parses")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "calls the real model; run with `just eval-followups`"]
async fn follow_up_evals() {
    let config = config();
    let key = config
        .provider_key
        .as_deref()
        .expect("the config has a provider_key");
    let model: Model = config.model.parse().expect("the config model parses");
    let mut llm = AgentLlmConfig::new(model).with_reasoning(config.reasoning);
    llm.provider_preferences = Some(config.routing.clone());
    let provider = OpenRouterProvider::new(key.to_owned());
    let generator = Generator {
        provider: &provider,
        llm: &llm,
        user: "eval",
    };
    let decision = DecisionTool::new(SecretString::from(key)).expect("Jev builds");
    let only = env::var("BEEP_EVAL").ok();

    let mut failed = 0;
    let mut ran = 0;
    for case in CASES
        .iter()
        .filter(|case| only.as_deref().is_none_or(|only| case.name.contains(only)))
    {
        ran += 1;
        let latest = Turn {
            message: case.message.into(),
            reply: case.reply.into(),
            actions: Vec::new(),
        };
        let input = Input {
            context: case.context,
            earlier: None,
            latest: &latest,
            results: case
                .results
                .iter()
                .map(|(tool, output)| ToolOutput { tool, output })
                .collect(),
        }
        .to_string();
        let score = followups::score(&decision, &input).await;
        let chips = followups::draft(generator, input.clone())
            .await
            .and_then(|output| serde_json::from_str::<Suggestions>(&output).ok())
            .map(|s| s.replies)
            .unwrap_or_default();

        let mut failures = Vec::new();
        if !(2..=3).contains(&chips.len()) {
            failures.push(format!("{} chips", chips.len()));
        }
        failures.extend(
            chips
                .iter()
                .filter(|chip| chip.chars().count() > MAX_CHARS)
                .map(|chip| format!("over {MAX_CHARS} chars: {chip}")),
        );
        let lower: Vec<String> = chips.iter().map(|c| c.to_lowercase()).collect();
        if !case.wants_any.is_empty()
            && !lower
                .iter()
                .any(|chip| case.wants_any.iter().any(|word| chip.contains(word)))
        {
            failures.push(format!("no chip mentions any of {:?}", case.wants_any));
        }
        if case.ascii {
            failures.extend(
                chips
                    .iter()
                    .filter(|chip| !chip.is_ascii())
                    .map(|chip| format!("not in the chat's language: {chip}")),
            );
        }
        if !chips.is_empty() {
            let listed: String = chips.iter().map(|chip| format!("\n- {chip}")).collect();
            let verdicts = generator
                .text(JUDGE, format!("{input}\n\nChips:{listed}"))
                .await
                .expect("the judge answers");
            failures.extend(
                chips
                    .iter()
                    .zip(verdicts.lines().filter(|l| !l.trim().is_empty()))
                    .filter(|(_, verdict)| !verdict.trim().starts_with("PASS"))
                    .map(|(chip, verdict)| format!("{chip}: {}", verdict.trim())),
            );
        }

        let verdict = if failures.is_empty() { "PASS" } else { "FAIL" };
        println!("{verdict} {}", case.name);
        println!("  score: {score:?}");
        println!("  chips: {chips:?}");
        for failure in &failures {
            println!("  - {failure}");
        }
        failed += usize::from(!failures.is_empty());
    }
    assert_eq!(failed, 0, "{failed} of {ran} follow-up evals failed");
}
