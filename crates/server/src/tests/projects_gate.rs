use super::*;
use crate::store::AppPermissionRow;

fn rules(level: PermissionLevel, allowed: &[&str]) -> Rules {
    Rules::from(vec![AppPermissionRow {
        app: "project".into(),
        level,
        allowed: allowed.iter().map(|tool| (*tool).to_owned()).collect(),
    }])
}

fn card() -> Approval {
    Approval {
        app: "Project".into(),
        action: "Create 1 task".into(),
        fields: Vec::new(),
    }
}

fn parks(gate: &ProjectGate, call: &str, asks: Asks) -> bool {
    match gate.check(call, "project_plan", asks, card) {
        Gate::Park(result) => {
            let payload = result.payload().expect("a parked call has a payload");
            payload.as_any().downcast_ref::<ApprovalPayload>().is_some()
        }
        Gate::Run => false,
        Gate::Refuse(reason) => panic!("refused: {reason}"),
    }
}

#[test]
fn gate_rules() {
    let ask = ProjectGate::new(Rules::default(), None);
    assert!(parks(&ask, "c1", Asks::ByLevel));
    let approved = ProjectGate::new(Rules::default(), Some("c1".into()));
    assert!(!parks(&approved, "c1", Asks::ByLevel));
    assert!(parks(&approved, "c2", Asks::ByLevel));
    assert!(!parks(&approved, "c1", Asks::Always));
    let always = ProjectGate::new(rules(PermissionLevel::Ask {}, &["PROJECT_PLAN"]), None);
    assert!(!parks(&always, "c1", Asks::ByLevel));
    assert!(parks(&always, "c1", Asks::Always));
    let allow = ProjectGate::new(rules(PermissionLevel::Allow {}, &[]), None);
    assert!(!parks(&allow, "c1", Asks::ByLevel));
    assert!(parks(&allow, "c1", Asks::Always));
    let read_only = ProjectGate::new(rules(PermissionLevel::ReadOnly {}, &[]), Some("c1".into()));
    for asks in [Asks::ByLevel, Asks::Always] {
        assert!(matches!(
            read_only.check("c1", "project_plan", asks, card),
            Gate::Refuse(_)
        ));
    }
}
