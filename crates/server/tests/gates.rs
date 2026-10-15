//! Reply gate evals: Jev reads fixed replies with the calls their turn made.
//! Each case names the checks it should fail. They call the real model, so
//! they are ignored by default; `just eval-gates` runs them.
use beep_server::{
    config::Config,
    decision::DecisionTool,
    gates::{self, Call, Draft},
};
use futures::future::join_all;
use secrecy::SecretString;
use std::{env, fs, path::PathBuf};

struct Case {
    name: &'static str,
    asked: &'static str,
    /// Tool names, with a `!` in front for one that failed.
    calls: &'static [&'static str],
    scheduled: &'static [&'static str],
    reply: &'static str,
    fails: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        name: "routine sends the user to a shop",
        asked: "The user just left Kimpton Shinjuku. Point them to the Gucci Shinjuku flagship \
for the Jackie 1961 small hobo, then Isetan.",
        calls: &["notify_user"],
        scheduled: &[],
        reply: "You've left the Kimpton Shinjuku, so head to the Gucci Shinjuku flagship at \
NEWNO, 1F, 3-24-1 Shinjuku (open until 20:00) for the Jackie 1961 small hobo.",
        fails: &["map"],
    },
    Case {
        name: "destination with its map shown",
        asked: "Where can I get a good tonkatsu near here?",
        calls: &["web_search", "open_maps"],
        scheduled: &[],
        reply: "Tonkatsu Hasegawa in Ebisu, a 6 minute walk. Go before 19:00, the queue builds.",
        fails: &[],
    },
    Case {
        name: "several options, none picked",
        asked: "Laundry options near the hotel?",
        calls: &["web_search"],
        scheduled: &[],
        reply: "Two options: the hotel's own laundry with evening pickup, or Pony Cleaning, \
a 5 minute walk, open 10:00 to 20:00.",
        fails: &[],
    },
    Case {
        name: "claims a calendar event it never added",
        asked: "Add the Kimpton stay to my calendar",
        calls: &["FASTMAIL_SEARCH"],
        scheduled: &[],
        reply: "I've added the Kimpton stay to your calendar as an all-day event from 9 October \
to 6 November.",
        fails: &["claimed"],
    },
    Case {
        name: "calendar event actually added",
        asked: "Add the Kimpton stay to my calendar",
        calls: &["FASTMAIL_SEARCH", "GOOGLECALENDAR_CREATE_EVENT"],
        scheduled: &[],
        reply: "I've added the Kimpton stay to your calendar as an all-day event from 9 October \
to 6 November.",
        fails: &[],
    },
    Case {
        name: "claims a sent email whose call failed",
        asked: "Reply to the concierge with my flight details",
        calls: &["!GMAIL_SEND_EMAIL"],
        scheduled: &[],
        reply: "Sent the concierge your flight details for the airport pickup.",
        fails: &["claimed"],
    },
    Case {
        name: "promises a reminder with nothing scheduled",
        asked: "Remind me about Kimpton tomorrow",
        calls: &[],
        scheduled: &[],
        reply: "Done. I'll nudge you tomorrow at 9am about sending Kimpton your flight details.",
        fails: &["claimed", "promised"],
    },
    Case {
        name: "reminder scheduled",
        asked: "Remind me about Kimpton tomorrow",
        calls: &["schedule_once"],
        scheduled: &[],
        reply: "Done. I'll nudge you tomorrow at 9am about sending Kimpton your flight details.",
        fails: &[],
    },
    Case {
        name: "routine mentions a reminder already scheduled",
        asked: "Morning briefing: today's calendar, weather and anything due.",
        calls: &["notify_user"],
        scheduled: &["Remind the user at 11:50 to leave for the Utexo meeting"],
        reply: "Morning. Clear skies, 22°C. Utexo at 12:30; your 11:50 reminder to leave is set.",
        fails: &[],
    },
    Case {
        name: "plain answer",
        asked: "What does the care label say?",
        calls: &[],
        scheduled: &[],
        reply: "Dry clean only, do not wash, do not tumble dry, iron at low temperature.",
        fails: &[],
    },
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

fn calls(names: &[&str]) -> Vec<Call> {
    names
        .iter()
        .map(|name| match name.strip_prefix('!') {
            Some(name) => Call {
                name: name.to_owned(),
                success: false,
            },
            None => Call {
                name: (*name).to_owned(),
                success: true,
            },
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "calls the real model; run with `just eval-gates`"]
async fn gate_evals() {
    let config = config();
    let key = config
        .provider_key
        .clone()
        .expect("the config has a provider_key");
    let decision = DecisionTool::new(SecretString::from(key)).expect("Jev builds");
    let only = env::var("BEEP_EVAL").ok();
    let cases: Vec<&Case> = CASES
        .iter()
        .filter(|case| only.as_deref().is_none_or(|only| case.name.contains(only)))
        .collect();
    let made: Vec<Vec<Call>> = cases.iter().map(|case| calls(case.calls)).collect();
    let reviews = cases.iter().zip(&made).map(|(case, calls)| {
        let decision = &decision;
        async move {
            let scheduled: Vec<String> = case
                .scheduled
                .iter()
                .map(|task| (*task).to_owned())
                .collect();
            let draft = Draft {
                asked: case.asked,
                reply: case.reply,
                calls,
                scheduled: &scheduled,
                can_map: true,
            };
            gates::review(decision, &draft).await
        }
    });
    let verdicts = join_all(reviews).await;
    let mut failed = 0;
    for (case, verdict) in cases.iter().zip(&verdicts) {
        let got = verdict
            .as_ref()
            .map(|v| v.failed.clone())
            .unwrap_or_default();
        let pass = got == case.fails;
        println!(
            "{} {}: {got:?}",
            if pass { "PASS" } else { "FAIL" },
            case.name
        );
        failed += usize::from(!pass);
    }
    assert_eq!(failed, 0, "{failed} of {} gate evals failed", cases.len());
}
