use super::*;
use beep_proto::watched;
use serde_json::json;

#[test]
fn a_rate_limit_event_updates_its_windows_only() {
    let hub = UsageHub::default();
    hub.set_plan(PlanUsage {
        agent: AgentSlug::CLAUDE,
        plan: Some("max".into()),
        windows: vec![
            LimitWindow {
                minutes: 300,
                used_percent: 10.0,
                resets_at: Some(100),
                scope: None,
            },
            LimitWindow {
                minutes: 10080,
                used_percent: 1.0,
                resets_at: Some(200),
                scope: None,
            },
        ],
        credits: Vec::new(),
        at: 50,
    });
    let event = json!({
        "status": "allowed",
        "rateLimitType": "seven_day",
        "unifiedWindows": {"seven_day": {"utilization": 0.02, "resetsAt": 300, "status": "allowed"}},
    });
    hub.update_windows(&AgentSlug::CLAUDE, claude_rate_limit(&event), 60);
    let plans = hub.plans();
    let plan = watched::read(&plans, |plans| plans["claude"].clone());
    assert_eq!(plan.plan.as_deref(), Some("max"));
    assert_eq!(plan.at, 60);
    assert_eq!(plan.windows[0].used_percent, 10.0);
    assert_eq!(plan.windows[1].used_percent, 2.0);
    assert_eq!(plan.windows[1].resets_at, Some(300));
}

#[test]
fn only_changed_credits_move_the_plans() {
    use beep_proto::{CreditScope, CreditUnit};

    let hub = UsageHub::default();
    let credits = |remaining: &str| ProviderCredits {
        provider: "provider".into(),
        scope: CreditScope::Account,
        unit: CreditUnit::Usd,
        remaining: Some(remaining.into()),
        total: None,
        used: None,
        resets: None,
    };
    let mut plans = hub.plans();

    hub.update_credits(&AgentSlug::CLAUDE, credits("5.00"), 10);
    assert!(plans.has_changed().unwrap());
    plans.mark_unchanged();

    // The same balance seen later changes nothing, not even the timestamp.
    hub.update_credits(&AgentSlug::CLAUDE, credits("5.00"), 20);
    assert!(!plans.has_changed().unwrap());
    assert_eq!(watched::read(&plans, |plans| plans["claude"].at), 10);

    hub.update_credits(&AgentSlug::CLAUDE, credits("4.00"), 30);
    assert!(plans.has_changed().unwrap());
    assert_eq!(watched::read(&plans, |plans| plans["claude"].at), 30);
}
