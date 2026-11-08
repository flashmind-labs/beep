use super::*;
use crate::protocol::node::usage::Billing;
use iroh::SecretKey;

fn machine() -> EndpointId {
    SecretKey::from_bytes(&[1; 32]).public()
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).expect("a real day")
}

fn spent(cwd: &str, agent: &str, billing: Billing, cost: Option<f64>, tokens: u64) -> DaySpendRow {
    DaySpendRow {
        endpoint: machine(),
        cwd: cwd.into(),
        agent: agent.into(),
        day: day(3),
        billing,
        cost_usd: cost,
        tokens,
        unpriced_tokens: if cost.is_some() { 0 } else { tokens },
    }
}

#[test]
fn dollars_count_only_metered_use() {
    let days = [
        spent(
            "/srv/widget",
            "claude",
            Billing::Unknown,
            Some(361.58),
            1_000_000,
        ),
        spent("/srv/widget", "claude", Billing::Metered, Some(2.0), 10_000),
        spent(
            "/srv/widget",
            "codex",
            Billing::Subscription,
            Some(13.22),
            50_000,
        ),
        spent("/srv/notes", "flsh", Billing::Unknown, Some(13.01), 20_000),
        spent("/srv/notes", "pi", Billing::Metered, None, 5_000),
    ];
    let models = [
        DayModelRow {
            agent: "claude".into(),
            model: "claude-opus-5-5".into(),
            day: day(3),
            billing: Billing::Unknown,
            tokens: 1_000_000,
            from_logs: true,
        },
        DayModelRow {
            agent: "claude".into(),
            model: "us.anthropic.claude-opus-5-5".into(),
            day: day(3),
            billing: Billing::Metered,
            tokens: 10_000,
            from_logs: true,
        },
    ];
    let period = spend_period(&days, &models, &[], day(1));
    assert!((period.cost_usd - 15.01).abs() < 1e-9);
    assert_eq!(period.metered_tokens, 35_000);
    assert_eq!(period.subscription_tokens, 1_050_000);

    let shown: Vec<(&str, bool, Option<f64>, u64)> = period
        .harnesses
        .iter()
        .map(|h| (h.agent.as_ref(), h.subscription, h.cost_usd, h.tokens))
        .collect();
    assert_eq!(
        shown,
        [
            ("flsh", false, Some(13.01), 20_000),
            ("claude", false, Some(2.0), 10_000),
            ("claude", true, None, 1_000_000),
            ("codex", true, None, 50_000),
            ("pi", false, None, 5_000),
        ]
    );
    assert_eq!(
        period.harnesses[1].models[0].model,
        "us.anthropic.claude-opus-5-5"
    );
    assert_eq!(period.harnesses[2].models[0].model, "claude-opus-5-5");

    let notes = &period.projects[0];
    assert_eq!(notes.project, "notes");
    assert!((notes.cost_usd - 13.01).abs() < 1e-9);
    assert_eq!(notes.metered_tokens, 25_000);
    let widget = &period.projects[1];
    assert!((widget.cost_usd - 2.0).abs() < 1e-9);
    let covered: Vec<(&str, u64)> = widget
        .subscription
        .iter()
        .map(|t| (t.agent.as_ref(), t.tokens))
        .collect();
    assert_eq!(covered, [("claude", 1_000_000), ("codex", 50_000)]);

    assert!(
        spend_period(&days, &models, &[], day(4))
            .projects
            .is_empty()
    );
}
