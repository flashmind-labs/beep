use super::*;
use crate::usage_cache::UsageCache;
use beep_proto::{CreditScope, CreditUnit, ProviderCredits, node::usage::Billing};
use beep_proto::{SessionId, node::usage::PlanUsage};

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

#[test]
fn daily_rows_group_by_local_day_and_project_and_only_charge_metered_usage() {
    let day = NaiveDate::from_ymd_opt(2026, 3, 9).unwrap();
    let midnight = start(day) as u64;
    let mut yesterday = hour(Billing::Metered);
    yesterday.hour = midnight - 3600;
    let mut today = hour(Billing::Metered);
    today.hour = midnight;
    let mut covered = hour(Billing::Subscription);
    covered.hour = midnight + 3600;
    let rows = [&yesterday, &today, &covered].map(|hour| Row {
        project: Some("sample".to_owned()),
        agent: &AgentSlug::CLAUDE,
        hour,
    });
    let period = bill(&rows);
    assert_eq!(period.daily.len(), 2);
    assert_eq!(period.daily[0].first.as_deref(), Some("2026-03-09"));
    assert_eq!(period.daily[1].first.as_deref(), Some("2026-03-08"));
    assert_eq!(period.daily[0].tokens, 2_000);
    assert_eq!(period.daily[0].cost_usd, 5.0);
    assert_eq!(period.cost_usd, 10.0);
}

#[test]
fn partial_usage_pass_preserves_cached_sessions_until_complete_pass() {
    let cache = UsageCache::default();
    let machine = iroh::SecretKey::from_bytes(&[3; 32]).public();
    let s1 = SessionId::from("s1");
    let s2 = SessionId::from("s2");

    let mut initial = HashMap::new();
    let mut h1 = BTreeMap::new();
    h1.insert((100, ModelId::new("m")), hour(Billing::Metered));
    initial.insert(
        (machine, AgentSlug::CLAUDE, s1.clone()),
        Session {
            cwd: "/work".into(),
            hours: h1,
            historical: Vec::new(),
        },
    );
    let mut said = Said::default();
    cache.merge_and_fill(machine, true, Vec::new(), initial, &mut said);
    assert_eq!(said.sessions.len(), 1);

    // A partial pass that only saw PlanUsage (or one new session) before timing out keeps `s1`.
    let mut partial = HashMap::new();
    let mut h2 = BTreeMap::new();
    h2.insert((100, ModelId::new("m")), hour(Billing::Metered));
    partial.insert(
        (machine, AgentSlug::CLAUDE, s2.clone()),
        Session {
            cwd: "/work".into(),
            hours: h2,
            historical: Vec::new(),
        },
    );
    let plan = PlanUsage {
        agent: AgentSlug::CLAUDE,
        at: 10,
        ..Default::default()
    };
    let mut after_partial = Said::default();
    cache.merge_and_fill(
        machine,
        false,
        vec![("box".into(), plan)],
        partial,
        &mut after_partial,
    );
    assert_eq!(after_partial.sessions.len(), 2);
    assert!(
        after_partial
            .sessions
            .contains_key(&(machine, AgentSlug::CLAUDE, s1))
    );
    assert!(
        after_partial
            .sessions
            .contains_key(&(machine, AgentSlug::CLAUDE, s2))
    );
}
