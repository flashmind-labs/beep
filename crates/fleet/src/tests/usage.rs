use super::*;
use beep_proto::{CreditScope, CreditUnit, ProviderCredits, node::usage::Billing};

fn hour(billing: Billing) -> HourSpend {
    HourSpend {
        hour: 0,
        model: ModelId::new("some-model"),
        input_tokens: 1_000,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        cost_usd: 5.0,
        unpriced_tokens: 0,
        billing,
    }
}

#[test]
fn a_plan_that_could_not_be_told_reports_tokens_not_dollars() {
    let unknown = hour(Billing::Unknown);
    let paid = hour(Billing::Metered);
    let rows = vec![
        Row {
            project: Some("widget".to_owned()),
            agent: &AgentSlug::CLAUDE,
            hour: &unknown,
        },
        Row {
            project: Some("widget".to_owned()),
            agent: &AgentSlug::CLAUDE,
            hour: &paid,
        },
    ];

    let bill = bill(&rows);

    assert_eq!(bill.cost_usd, 5.0);
    assert_eq!(bill.metered_tokens, 1_000);
    assert_eq!(bill.subscription_tokens, 1_000);
    let covered = bill.harnesses.iter().find(|h| h.subscription);
    assert_eq!(covered.and_then(|h| h.cost_usd), Some(5.0));
    let metered = bill.harnesses.iter().find(|h| !h.subscription);
    assert_eq!(metered.and_then(|h| h.cost_usd), Some(5.0));
}

#[test]
fn a_credit_only_plan_reaches_the_usage_view_without_becoming_spend() {
    let credits = ProviderCredits {
        provider: "openrouter".into(),
        scope: CreditScope::ApiKey,
        unit: CreditUnit::Usd,
        remaining: None,
        total: None,
        used: Some("42.125".into()),
        resets: None,
    };
    let plan = PlanUsage {
        agent: "flsh".into(),
        credits: vec![credits.clone()],
        ..Default::default()
    };
    let limits = plan_limits("build-box", &plan).unwrap();
    assert!(limits.windows.is_empty());
    assert_eq!(limits.credits, [credits]);
}
