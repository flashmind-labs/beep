use super::*;

fn alert(title: Option<&str>, kind: AlertKind) -> Alert {
    Alert {
        session: "s1".into(),
        agent: "claude".into(),
        title: title.map(str::to_owned),
        cwd: "/srv/widget".into(),
        kind,
    }
}

fn ended(stop_reason: StopReason, last_message: &str, error: Option<&str>) -> AlertKind {
    AlertKind::TurnEnded {
        stop_reason,
        last_message: last_message.into(),
        error: error.map(str::to_owned),
    }
}

#[test]
fn the_title_is_the_project_and_the_session() {
    let done = alert(
        Some("Fix the login"),
        ended(StopReason::EndTurn, "Done.", None),
    );
    assert_eq!(
        done.notice_title(Some("acme/widget")),
        "acme/widget: Fix the login"
    );
    assert_eq!(done.notice_title(None), "Fix the login");
    assert_eq!(
        alert(None, ended(StopReason::EndTurn, "", None)).notice_title(None),
        "widget"
    );
}

#[test]
fn the_body_is_what_the_agent_said_or_why_it_stopped() {
    let body = |kind| alert(Some("Fix the login"), kind).notice_body();
    assert_eq!(
        body(ended(StopReason::EndTurn, "**Done**, see `a.rs`.\n", None)),
        "**Done**, see `a.rs`."
    );
    assert_eq!(body(ended(StopReason::EndTurn, " ", None)), "Finished");
    assert_eq!(
        body(ended(StopReason::Failed, "", Some("no network"))),
        "Failed: no network"
    );
    assert_eq!(
        body(ended(StopReason::Cancelled, "", None)),
        "Stopped: cancelled"
    );
    assert_eq!(
        body(AlertKind::Failed {
            reason: "exited".into()
        }),
        "Failed: exited"
    );
}
