use super::*;
use beep_proto::watched;
use serde_json::json;
use std::time::Duration;
use tokio::time::timeout;

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
        account: None,
    });
    let event = json!({
        "status": "allowed",
        "rateLimitType": "seven_day",
        "unifiedWindows": {"seven_day": {"utilization": 0.02, "resetsAt": 300, "status": "allowed"}},
    });
    let default = (AgentSlug::CLAUDE, None);
    let work = (AgentSlug::CLAUDE, Some("Work".parse().unwrap()));
    hub.update_windows(&default, claude_rate_limit(&event), 60);
    // A session on another account reports that account's plan.
    hub.update_windows(&work, claude_rate_limit(&json!({"rateLimitType": "five_hour", "unifiedWindows": {"five_hour": {"utilization": 0.9}}})), 70);
    let plans = hub.plans();
    let plan = watched::read(&plans, |plans| plans[&default].clone());
    let other = watched::read(&plans, |plans| plans[&work].clone());
    assert_eq!(other.account, work.1);
    assert_eq!(other.windows.len(), 1);
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
    let claude = (AgentSlug::CLAUDE, None);

    hub.update_credits(&claude, credits("5.00"), 10);
    assert!(plans.has_changed().unwrap());
    plans.mark_unchanged();

    // The same balance seen later changes nothing, not even the timestamp.
    hub.update_credits(&claude, credits("5.00"), 20);
    assert!(!plans.has_changed().unwrap());
    assert_eq!(watched::read(&plans, |plans| plans[&claude].at), 10);

    hub.update_credits(&claude, credits("4.00"), 30);
    assert!(plans.has_changed().unwrap());
    assert_eq!(watched::read(&plans, |plans| plans[&claude].at), 30);
}

#[test]
fn set_plan_ignores_timestamp_only_updates() {
    let hub = UsageHub::default();
    let mut plans = hub.plans();
    let claude = (AgentSlug::CLAUDE, None);
    let make = |at: u64, used_percent: f64| PlanUsage {
        agent: AgentSlug::CLAUDE,
        plan: Some("max".into()),
        windows: vec![LimitWindow {
            minutes: 300,
            used_percent,
            resets_at: Some(100),
            scope: None,
        }],
        credits: Vec::new(),
        at,
        account: None,
    };

    hub.set_plan(make(10, 12.0));
    assert!(plans.has_changed().unwrap());
    plans.mark_unchanged();

    hub.set_plan(make(20, 12.0));
    assert!(!plans.has_changed().unwrap());
    assert_eq!(watched::read(&plans, |plans| plans[&claude].at), 10);

    hub.set_plan(make(30, 15.0));
    assert!(plans.has_changed().unwrap());
    assert_eq!(watched::read(&plans, |plans| plans[&claude].at), 30);
}

#[tokio::test]
async fn every_follower_gets_every_finished_turn() {
    let hub = UsageHub::default();
    let (a, b) = (
        hub.turns.follow(iroh::SecretKey::generate().public()),
        hub.turns.follow(iroh::SecretKey::generate().public()),
    );

    for tokens in [1, 2] {
        hub.record(TurnUsage {
            tokens,
            ..TurnUsage::default()
        });
    }

    for follow in [&a, &b] {
        for tokens in [1, 2] {
            let turn = timeout(Duration::from_secs(10), follow.next())
                .await
                .expect("every follower gets the turn");
            assert_eq!(turn.tokens, tokens);
            follow.sent();
        }
    }
}

#[tokio::test]
async fn a_follower_that_connects_late_gets_all_the_turns_kept() {
    let hub = UsageHub::default();

    for tokens in 1..=120 {
        hub.record(TurnUsage {
            tokens,
            ..TurnUsage::default()
        });
    }

    let follow = hub.turns.follow(iroh::SecretKey::generate().public());

    for tokens in 1..=120 {
        assert_eq!(follow.next().await.tokens, tokens);
        follow.sent();
    }
}

#[tokio::test]
async fn streams_share_one_plan_timer_until_the_last_ends() {
    let hub = UsageHub::default();
    let started = std::sync::atomic::AtomicUsize::new(0);
    let start = || {
        started.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        AbortOnDropHandle::new(tokio::spawn(std::future::pending()))
    };

    let desktop = hub.timer(start);
    let phone = hub.timer(start);
    assert_eq!(started.load(std::sync::atomic::Ordering::Relaxed), 1);

    drop(desktop);
    let _ = hub.timer(start);
    assert_eq!(started.load(std::sync::atomic::Ordering::Relaxed), 1);

    drop(phone);
    let _again = hub.timer(start);
    assert_eq!(started.load(std::sync::atomic::Ordering::Relaxed), 2);
}
