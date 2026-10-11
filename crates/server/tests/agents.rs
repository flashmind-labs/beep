//! Agent router evals: Jev picks an agent for fixed prompts, some of them
//! follow-ups whose agent depends on the chat's current one. They call the
//! real model, so they are ignored by default; `just eval-agents` runs them.
use beep_server::{
    agents::{Agent, AgentId, router_state},
    config::Config,
    decision::DecisionTool,
};
use futures::future::join_all;
use secrecy::SecretString;
use std::{env, fs, path::PathBuf};

/// The share of cases that must route right.
const PASS_RATE: f64 = 0.9;

struct Case {
    message: &'static str,
    /// The chat's agent before this message, and the turn before it.
    current: Option<(AgentId, &'static str)>,
    expected: AgentId,
}

const fn fresh(message: &'static str, expected: AgentId) -> Case {
    Case {
        message,
        current: None,
        expected,
    }
}

const fn after(
    current: AgentId,
    earlier: &'static str,
    message: &'static str,
    expected: AgentId,
) -> Case {
    Case {
        message,
        current: Some((current, earlier)),
        expected,
    }
}

use AgentId::{General, GettingAround, GoingOut, Inbox, Research, Shopping};

/// Reminders and alarms go to General whatever they mention.
const NEVER_SPECIALIST: &[&str] = &[
    "Remind me at 9 to call the bank",
    "Set an alarm for 6:30",
    "Remind me to leave for the airport two hours before my flight",
];

const CASES: &[Case] = &[
    // General
    fresh("Remind me at 9 to call the bank", General),
    fresh("Set an alarm for 6:30", General),
    fresh(
        "Remind me to leave for the airport two hours before my flight",
        General,
    ),
    fresh("What time is it in New York?", General),
    fresh("What's the capital of Honduras?", General),
    fresh("Good morning! What's on today?", General),
    fresh("Translate 'see you tomorrow' to Japanese", General),
    fresh("How long do I boil an egg for a runny yolk?", General),
    fresh("Tell me a joke", General),
    fresh("What's the weather this afternoon?", General),
    // Getting around
    fresh(
        "How do I get from Shinjuku to Harajuku by train?",
        GettingAround,
    ),
    fresh(
        "Which exit at Shibuya for the scramble crossing?",
        GettingAround,
    ),
    fresh("Is my flight to Berlin on time?", GettingAround),
    fresh("Do Canadian citizens need a visa for Japan?", GettingAround),
    fresh("Best way from the airport to my hotel?", GettingAround),
    fresh("What time is checkout at my hotel?", GettingAround),
    fresh(
        "How much is a metro ticket from Ginza to Asakusa?",
        GettingAround,
    ),
    fresh(
        "Can I bring 3 bottles of wine through customs in Japan?",
        GettingAround,
    ),
    fresh("Get me a taxi to the convention centre", GettingAround),
    // Shopping
    fresh("Where can I buy sunscreen near me?", Shopping),
    fresh(
        "Compare the best cordless stick vacuums under 300 euros",
        Shopping,
    ),
    fresh("What's the Japanese equivalent of ibuprofen gel?", Shopping),
    fresh(
        "Find me a black leather handbag like a designer one, but cheaper",
        Shopping,
    ),
    fresh("Which store downtown sells hiking boots?", Shopping),
    fresh(
        "Is the new iPhone cheaper in Japan than in Germany?",
        Shopping,
    ),
    // Going out
    fresh("Good ramen near me open now?", GoingOut),
    fresh(
        "Where should we go for drinks tonight in Shibuya?",
        GoingOut,
    ),
    fresh("Plan a birthday dinner for six on Saturday", GoingOut),
    fresh("Any tennis courts I can book this weekend?", GoingOut),
    fresh("Fun things to do this evening near the harbour", GoingOut),
    // Research
    fresh("How does threshold ECDSA signing work?", Research),
    fresh(
        "Compare Ethereum and Solana for building a payments app",
        Research,
    ),
    fresh("Explain the Lightning Network and its trade-offs", Research),
    fresh(
        "Are Schnorr signatures at risk from quantum computers?",
        Research,
    ),
    fresh("What are the main ideas behind JEPA?", Research),
    // Inbox and admin
    fresh("Clean up my inbox: archive all the newsletters", Inbox),
    fresh("Any unread emails I need to answer today?", Inbox),
    fresh(
        "My gym membership payment failed again, what do I do?",
        Inbox,
    ),
    fresh("Find the confirmation number for my hotel booking", Inbox),
    fresh("Summarize my unread Slack", Inbox),
    fresh(
        "I got a letter from the tax office, what do I need to file?",
        Inbox,
    ),
    // Follow-ups stay with the chat's agent
    after(
        GettingAround,
        "User: Directions to the ramen shop in Ueno\nAssistant: Take the Ginza Line to Ueno, exit 3.",
        "Hibiya line which exit",
        GettingAround,
    ),
    after(
        GettingAround,
        "User: How do I get to the airport?\nAssistant: The Narita Express from Tokyo Station, 53 minutes.",
        "and how much is it?",
        GettingAround,
    ),
    after(
        Shopping,
        "User: Where can I buy a hair straightener?\nAssistant: Two electronics stores nearby carry them.",
        "which one is open later?",
        Shopping,
    ),
    after(
        Research,
        "User: Explain how an on-chain order book works\nAssistant: Every order and match is a transaction on the chain.",
        "and how does it compare to an AMM?",
        Research,
    ),
    after(
        GoingOut,
        "User: Dinner ideas near the station?\nAssistant: Try the tonkatsu place on the corner or the Italian one upstairs.",
        "book the first one for 8",
        GoingOut,
    ),
    after(
        Inbox,
        "User: Any emails from my landlord?\nAssistant: One from yesterday about the deposit.",
        "reply that I'll send the photos tonight",
        Inbox,
    ),
    // A new topic leaves the chat's agent
    after(
        GettingAround,
        "User: How do I get to Shinjuku?\nAssistant: Take the JR Yamanote Line, 12 minutes.",
        "Remind me at 8 to call mum",
        General,
    ),
    after(
        Research,
        "User: Explain zk rollups\nAssistant: They post validity proofs to the base chain.",
        "Where can I get good sushi near me tonight?",
        GoingOut,
    ),
];

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
#[ignore = "calls the real model; run with `just eval-agents`"]
async fn router_evals() {
    let config = config();
    let key = config
        .provider_key
        .clone()
        .expect("the config has a provider_key");
    let decision = DecisionTool::new(SecretString::from(key)).expect("Jev builds");
    let mut agents: Vec<Agent> = config
        .agents
        .into_iter()
        .map(|(id, agent)| Agent {
            id,
            name: agent.name,
            when: agent.when,
            model: agent.model,
            thinking: agent.thinking,
        })
        .collect();
    agents.sort_by_key(|agent| (agent.id != General, agent.id.as_str()));
    assert!(
        agents.first().is_some_and(|agent| agent.id == General),
        "the config sets up agents with a general one"
    );
    let only = env::var("BEEP_EVAL").ok();
    let cases: Vec<&Case> = CASES
        .iter()
        .filter(|case| {
            only.as_deref()
                .is_none_or(|only| case.message.contains(only))
        })
        .collect();
    let states: Vec<String> = cases
        .iter()
        .map(|case| {
            router_state(
                case.current.map(|(agent, _)| agent),
                case.current.map(|(_, earlier)| earlier),
                case.message,
                0,
            )
        })
        .collect();
    let picks = join_all(
        states
            .iter()
            .map(|state| decision.route_agent(state, &agents)),
    )
    .await;
    let mut passed = 0;
    let mut specialist_reminders = Vec::new();
    for (case, pick) in cases.iter().zip(picks) {
        let got = pick.as_ref().map(|routed| routed.agent.id);
        let pass = got.as_ref().is_ok_and(|id| *id == case.expected);
        println!(
            "{} {:?} -> {:?} (expected {})",
            if pass { "PASS" } else { "FAIL" },
            case.message,
            got.map(AgentId::as_str).map_err(|e| e.to_string()),
            case.expected.as_str()
        );
        passed += usize::from(pass);
        if NEVER_SPECIALIST.contains(&case.message) && !pass {
            specialist_reminders.push(case.message);
        }
    }
    let rate = passed as f64 / cases.len() as f64;
    println!(
        "{passed} of {} routed right ({:.0}%)",
        cases.len(),
        rate * 100.0
    );
    assert!(
        specialist_reminders.is_empty(),
        "reminders went to a specialist: {specialist_reminders:?}"
    );
    assert!(rate >= PASS_RATE, "routed {:.0}% right", rate * 100.0);
}
