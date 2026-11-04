//! Replays made-up group chats through Jev's two group questions and prints every wrong answer.
//! Calls OpenRouter, so it is ignored by default:
//!
//! `OPENROUTER_KEY=... cargo test -p beep-server group_eval -- --ignored --nocapture`

use super::{
    CALL_CRITERIA, CALL_QUESTION, CALL_THRESHOLD, MEMORY_CRITERIA, MEMORY_QUESTION, call_state,
    mentions_beep,
};
use crate::{
    agent::decision::DecisionTool,
    app::learning::MEMORY_THRESHOLD,
    store::{GroupRow, JoinedGroup, MemberRow, MessageRow},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use secrecy::SecretString;
use uuid::Uuid;

/// A chat and what should happen after its last message.
struct Case {
    name: &'static str,
    /// `(author, text)`, oldest first; "Beep" is Beep.
    chat: &'static [(&'static str, &'static str)],
    /// Whether Beep should answer the last message unmentioned (an @beep always answers).
    answer: bool,
    /// Whether the last message teaches something lasting about a member.
    learn: bool,
}

const CASES: &[Case] = &[
    // Questions the group puts to the air that Beep can answer.
    Case {
        name: "where to eat",
        chat: &[
            ("Dani", "Landed! Starving"),
            ("Sam", "Where should we eat near Shinjuku tonight?"),
        ],
        answer: true,
        learn: false,
    },
    Case {
        name: "opening hours",
        chat: &[("Ana", "Is the Ghibli museum open on Tuesdays?")],
        answer: true,
        learn: false,
    },
    Case {
        name: "weather",
        chat: &[("Sam", "Va a llover mañana en Kioto?")],
        answer: true,
        learn: false,
    },
    Case {
        name: "how to get there",
        chat: &[
            ("Dani", "How do we get from Narita to the hotel?"),
            ("Sam", "No idea"),
        ],
        answer: true,
        learn: false,
    },
    Case {
        name: "group planning",
        chat: &[
            ("Dani", "We need to plan the weekend in Hakone"),
            ("Ana", "Yes! Onsen and something with views"),
        ],
        answer: true,
        learn: true,
    },
    Case {
        name: "constraint on Beep",
        chat: &[
            ("Dani", "Que comemos? @beep"),
            ("Beep", "Probad el ramen de Ichiran, abre hasta tarde."),
            ("Sam", "Ramen no, Ana no puede tomar soja"),
        ],
        answer: true,
        learn: true,
    },
    Case {
        name: "objection to Beep",
        chat: &[
            ("Beep", "Take the Shinkansen at 7:03 to Kyoto."),
            ("Dani", "Too early, we're out late tonight"),
        ],
        answer: true,
        learn: false,
    },
    Case {
        name: "follow-up to Beep",
        chat: &[
            (
                "Beep",
                "Omoide Yokocho has small yakitori stalls with salt-only skewers.",
            ),
            ("Ana", "Do they take cards?"),
        ],
        answer: true,
        learn: false,
    },
    Case {
        name: "remember request",
        chat: &[
            ("Sam", "Ana es intolerante a la soja"),
            ("Dani", "Recuérdanoslo"),
        ],
        answer: true,
        learn: true,
    },
    Case {
        name: "price",
        chat: &[("Ana", "How much is a JR pass now?")],
        answer: true,
        learn: false,
    },
    Case {
        name: "itinerary",
        chat: &[("Dani", "Que hacemos el sábado?")],
        answer: true,
        learn: false,
    },
    // People talking to each other.
    Case {
        name: "greeting",
        chat: &[("Ana", "Buenos días!")],
        answer: false,
        learn: false,
    },
    Case {
        name: "cat noise",
        chat: &[
            ("Beep", "Try the yakitori at Omoide Yokocho."),
            ("Ana", "Miau"),
        ],
        answer: false,
        learn: false,
    },
    Case {
        name: "joke",
        chat: &[("Dani", "Sam fell asleep on the train again 😂")],
        answer: false,
        learn: false,
    },
    Case {
        name: "feelings",
        chat: &[("Sam", "I'm so tired today")],
        answer: false,
        learn: false,
    },
    Case {
        name: "member's whereabouts",
        chat: &[("Dani", "Ana where are you?")],
        answer: false,
        learn: false,
    },
    Case {
        name: "member's opinion",
        chat: &[("Sam", "Dani do you want to go to the onsen or not?")],
        answer: false,
        learn: false,
    },
    Case {
        name: "reply to member",
        chat: &[
            ("Dani", "I'll be at the hotel at 6"),
            ("Ana", "Ok see you there"),
        ],
        answer: false,
        learn: false,
    },
    Case {
        name: "thanks",
        chat: &[
            ("Beep", "The museum is closed on Tuesdays, go Wednesday."),
            ("Sam", "Thanks!"),
        ],
        answer: false,
        learn: false,
    },
    Case {
        name: "already answered",
        chat: &[
            ("Dani", "What time does the museum open?"),
            ("Ana", "10, I checked"),
        ],
        answer: false,
        learn: false,
    },
    Case {
        name: "news",
        chat: &[("Sam", "My sister just got engaged!!")],
        answer: false,
        learn: true,
    },
    Case {
        name: "logistics",
        chat: &[("Ana", "Running 10 min late")],
        answer: false,
        learn: false,
    },
    Case {
        name: "photo talk",
        chat: &[("Dani", "Look at this view"), ("Sam", "Wow")],
        answer: false,
        learn: false,
    },
    // Lasting facts, said by the member or about them.
    Case {
        name: "own diet",
        chat: &[("Sam", "I'm vegetarian btw, no fish either")],
        answer: false,
        learn: true,
    },
    Case {
        name: "allergy about another",
        chat: &[("Dani", "Careful, Ana is allergic to peanuts")],
        answer: false,
        learn: true,
    },
    Case {
        name: "lives in",
        chat: &[("Ana", "When I'm back in Valencia I'll show you the photos")],
        answer: false,
        learn: true,
    },
    Case {
        name: "partner",
        chat: &[("Sam", "My girlfriend Lea is joining us next week")],
        answer: false,
        learn: true,
    },
    Case {
        name: "dislike",
        chat: &[("Dani", "I hate crowded places, let's skip Shibuya crossing")],
        answer: false,
        learn: true,
    },
    Case {
        name: "trip plan",
        chat: &[(
            "Ana",
            "Ok it's decided, we go to Osaka on the 17th and back on the 19th",
        )],
        answer: false,
        learn: true,
    },
    Case {
        name: "question no fact",
        chat: &[("Dani", "Anyone want coffee?")],
        answer: false,
        learn: false,
    },
    Case {
        name: "Beep's suggestion",
        chat: &[
            ("Dani", "Ideas? @beep"),
            ("Beep", "Go to teamLab Planets, book ahead."),
        ],
        answer: false,
        learn: false,
    },
    Case {
        name: "opinion of world",
        chat: &[("Sam", "Tokyo is so clean compared to Paris")],
        answer: false,
        learn: false,
    },
    // Harder calls.
    Case {
        name: "fact asked of member",
        chat: &[("Sam", "Dani, sabes a qué hora sale el último tren a Kioto?")],
        answer: true,
        learn: false,
    },
    Case {
        name: "laugh at Beep",
        chat: &[
            ("Beep", "Kyoto has over 1,600 temples."),
            ("Dani", "jajaja"),
        ],
        answer: false,
        learn: false,
    },
    Case {
        name: "members discuss Beep's idea",
        chat: &[
            ("Beep", "Try teamLab Planets, book ahead."),
            ("Ana", "Dani te mola?"),
            ("Dani", "Sí, pero mejor el domingo"),
        ],
        answer: false,
        learn: false,
    },
    Case {
        name: "open question after chat",
        chat: &[
            ("Ana", "Qué tal se llega a Nara?"),
            ("Sam", "Ni idea"),
            ("Dani", "Yo tampoco jaja"),
        ],
        answer: true,
        learn: false,
    },
    Case {
        name: "rhetorical",
        chat: &[("Sam", "Why is everything so expensive here?")],
        answer: false,
        learn: false,
    },
    Case {
        name: "wrong fact from member",
        chat: &[("Dani", "The Shinkansen doesn't take the JR pass, right?")],
        answer: true,
        learn: false,
    },
    Case {
        name: "decision between members",
        chat: &[
            ("Ana", "Sushi or ramen tonight?"),
            ("Sam", "Ramen"),
            ("Dani", "Ramen too"),
        ],
        answer: false,
        learn: false,
    },
    Case {
        name: "need a booking",
        chat: &[("Dani", "Alguien puede mirar un hotel en Osaka para el 17?")],
        answer: true,
        learn: false,
    },
    Case {
        name: "translation",
        chat: &[("Sam", "What does 'okaikei onegaishimasu' mean?")],
        answer: true,
        learn: false,
    },
    Case {
        name: "complaint to Beep",
        chat: &[
            ("Beep", "Ichiran is open until 6am."),
            ("Ana", "It was closed, we walked for nothing"),
        ],
        answer: true,
        learn: false,
    },
    Case {
        name: "note the plan",
        chat: &[
            ("Ana", "Cena el viernes a las 9 en Gion"),
            ("Dani", "Apúntalo"),
        ],
        answer: true,
        learn: true,
    },
    Case {
        name: "birthday",
        chat: &[("Dani", "Ana's birthday is on the 20th, let's surprise her")],
        answer: false,
        learn: true,
    },
    Case {
        name: "remind us",
        chat: &[
            ("Ana", "Check-out is at 11 tomorrow"),
            ("Dani", "Avísanos a las 10"),
        ],
        answer: true,
        learn: false,
    },
    Case {
        name: "remind the group",
        chat: &[(
            "Sam",
            "Remind everyone to bring passports on Friday morning",
        )],
        answer: true,
        learn: false,
    },
    Case {
        name: "remind each other",
        chat: &[
            ("Dani", "Sam remind me to buy the tickets"),
            ("Sam", "Ok I will"),
        ],
        answer: false,
        learn: false,
    },
    // Mentions always answer; only learning is scored.
    Case {
        name: "mention with fact",
        chat: &[("Ana", "@beep I can't eat gluten, where can we have lunch?")],
        answer: true,
        learn: true,
    },
    Case {
        name: "bare mention",
        chat: &[("Dani", "Recuérdanoslo"), ("Dani", "@beep")],
        answer: true,
        learn: false,
    },
];

fn joined(chat: &[(&str, &str)]) -> JoinedGroup {
    let at = DateTime::<Utc>::UNIX_EPOCH;
    let mut names: Vec<&str> = chat
        .iter()
        .map(|(a, _)| *a)
        .filter(|a| *a != "Beep")
        .collect();
    names.sort_unstable();
    names.dedup();
    JoinedGroup {
        group: GroupRow {
            id: Uuid::nil(),
            name: "Japan".into(),
            updated_at: at,
        },
        members: names
            .into_iter()
            .map(|name| MemberRow {
                user_id: name.to_lowercase(),
                name: name.into(),
                creator: false,
                joined_at: at,
                pinned: false,
                shares_apps: true,
            })
            .collect(),
        last: None,
    }
}

fn history(chat: &[(&str, &str)]) -> Vec<MessageRow> {
    chat.iter()
        .zip(1..)
        .map(|((author, text), id)| MessageRow {
            id,
            author_id: (*author != "Beep").then(|| author.to_lowercase()),
            author_name: (*author).into(),
            content: (*text).into(),
            sent_at: DateTime::<Utc>::UNIX_EPOCH,
        })
        .collect()
}

#[tokio::test]
#[ignore = "calls OpenRouter; set OPENROUTER_KEY"]
async fn group_eval() -> Result<()> {
    let key = std::env::var("OPENROUTER_KEY").context("set OPENROUTER_KEY")?;
    let decision = DecisionTool::new(SecretString::from(key))?;
    let decision = &decision;
    let results: Vec<_> = stream::iter(CASES)
        .map(|case| async move {
            let state = call_state(&joined(case.chat), &history(case.chat));
            let last = case.chat.last().map(|(_, t)| *t).unwrap_or_default();
            let call = if mentions_beep(last) {
                None
            } else {
                Some(
                    decision
                        .probability_with(&state, CALL_QUESTION, &CALL_CRITERIA)
                        .await,
                )
            };
            let learn = decision
                .probability_with(&state, MEMORY_QUESTION, &MEMORY_CRITERIA)
                .await;
            (case, call, learn)
        })
        .buffered(8)
        .collect()
        .await;

    // An error counts as a wrong judgement; a mention has no call to judge.
    let (mut wrong, mut judged) = (0, 0);
    for (case, call, learn) in &results {
        let call = match call {
            Some(Ok(p)) => {
                let ok = (*p >= CALL_THRESHOLD) == case.answer;
                judged += 1;
                wrong += usize::from(!ok);
                format!("{}answer {p:.2}", if ok { " " } else { "!" })
            }
            Some(Err(e)) => {
                judged += 1;
                wrong += 1;
                format!("!answer error {e:#}")
            }
            None => " answer @beep".into(),
        };
        judged += 1;
        let learn = match learn {
            Ok(p) => {
                let ok = (*p >= MEMORY_THRESHOLD) == case.learn;
                wrong += usize::from(!ok);
                format!("{}learn {p:.2}", if ok { " " } else { "!" })
            }
            Err(e) => {
                wrong += 1;
                format!("!learn error {e:#}")
            }
        };
        println!("{call:<16} {learn:<14} {}", case.name);
    }
    println!("\n{wrong} wrong of {judged} judgements");
    Ok(())
}
