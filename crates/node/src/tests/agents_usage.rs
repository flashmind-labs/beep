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
