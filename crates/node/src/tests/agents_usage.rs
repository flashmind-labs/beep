use super::*;
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
    let plan = &plans.borrow()["claude"];
    assert_eq!(plan.plan.as_deref(), Some("max"));
    assert_eq!(plan.at, 60);
    assert_eq!(plan.windows[0].used_percent, 10.0);
    assert_eq!(plan.windows[1].used_percent, 2.0);
    assert_eq!(plan.windows[1].resets_at, Some(300));
}

#[test]
fn claude_fable_window_is_kept_as_the_weekly_fable_limit() {
    let event = json!({
        "unifiedWindows": {
            "seven_day": {"utilization": 0.39, "resetsAt": 300},
            "seven_day_overage_included": {"utilization": 0.12, "resetsAt": 400},
        },
    });
    let windows = claude_rate_limit(&event);

    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].scope, None);
    assert_eq!(windows[1].scope.as_deref(), Some("Fable"));
    assert_eq!(windows[1].minutes, 7 * 24 * 60);
    assert_eq!(windows[1].used_percent, 12.0);
}
