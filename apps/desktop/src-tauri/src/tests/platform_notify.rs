use super::*;

fn agent_notice(question: Option<u32>) -> (Notice, AgentNoticeKey) {
    let key = AgentNoticeKey {
        machine_id: "m1".into(),
        agent: "claude".into(),
        session: "s1".into(),
        question,
        machine_name: "build-box".into(),
    };
    let notice = Notice {
        title: "Claude Code on build-box asks".into(),
        body: "Run the tests?".into(),
        session_id: key.to_string(),
    };
    (notice, key)
}

#[test]
fn an_agent_notice_opens_its_session_and_a_question_has_answers() {
    let (asks, key) = agent_notice(Some(4));
    assert_eq!(
        click_target(&asks),
        CenterTarget::Agent(AgentTarget::notified(&key))
    );
    assert_eq!(ask(&asks), Some(key));
    let (done, key) = agent_notice(None);
    assert_eq!(
        click_target(&done),
        CenterTarget::Agent(AgentTarget::notified(&key))
    );
    assert_eq!(ask(&done), None, "a finished turn only opens");
    assert_eq!((agent_reply(true), agent_reply(false)), ("allow", "deny"));
}

#[test]
fn only_a_permission_prompt_is_answered_with_allow_and_deny() {
    let option = |id: &str, kind| AnswerOption {
        id: id.into(),
        name: id.into(),
        kind,
    };
    let allow = option("allow", OptionKind::AllowOnce);
    let always = option("always", OptionKind::AllowAlways);
    let reject = option("reject", OptionKind::RejectOnce);
    assert!(allow_deny(&[allow.clone(), always, reject.clone()]));
    assert!(
        !allow_deny(std::slice::from_ref(&allow)),
        "nothing to deny with"
    );
    let choices = [
        option("choice-0", OptionKind::AllowOnce),
        option("choice-1", OptionKind::AllowOnce),
        reject.clone(),
    ];
    assert!(!allow_deny(&choices), "Allow would pick the first choice");
    let picks = [allow, option("Card lists", OptionKind::Other), reject];
    assert!(!allow_deny(&picks));
}

#[test]
fn any_other_notice_opens_home() {
    let (mut notice, _) = agent_notice(None);
    notice.session_id = String::new();
    assert_eq!(click_target(&notice), CenterTarget::Home);
    assert_eq!(ask(&notice), None);
}

fn route(notice: &Notice) -> Route {
    Route {
        target: Some(click_target(notice)),
        buttons: ask(notice).map(|key| Buttons::Answer(key.to_string())),
    }
}

#[test]
fn notification_targets_survive_serialization_without_waiters() {
    // More than the previous waiter cap, with independent targets recovered from each notification.
    for question in 0..64 {
        let (notice, key) = agent_notice(Some(question));
        let stored = serde_json::to_string(&route(&notice)).unwrap();
        let recovered = || serde_json::from_str(&stored).unwrap();
        assert_eq!(
            response(recovered(), Interaction::Open),
            Response::Open(click_target(&notice))
        );
        assert_eq!(
            response(recovered(), Interaction::Action),
            Response::Answer(key.clone(), true)
        );
        assert_eq!(
            response(recovered(), Interaction::Other),
            Response::Answer(key, false)
        );
        assert_eq!(
            response(recovered(), Interaction::Ignore),
            Response::Nothing
        );
    }
}

#[test]
fn plain_and_finished_notifications_cannot_answer_a_question() {
    let (notice, _) = agent_notice(None);
    for interaction in [Interaction::Action, Interaction::Other] {
        assert_eq!(response(route(&notice), interaction), Response::Nothing);
        assert_eq!(
            response(
                Route {
                    target: None,
                    buttons: Some(Buttons::Answer("invalid".into()))
                },
                interaction
            ),
            Response::Nothing
        );
        let (_, finished) = agent_notice(None);
        assert_eq!(
            response(
                Route {
                    target: None,
                    buttons: Some(Buttons::Answer(finished.to_string()))
                },
                interaction
            ),
            Response::Nothing
        );
    }
    assert_eq!(
        response(
            Route {
                target: None,
                buttons: None
            },
            Interaction::Open
        ),
        Response::Nothing
    );
}

#[test]
fn a_node_release_updates_from_its_action_button_only() {
    let route = || Route {
        target: Some(CenterTarget::Machines),
        buttons: Some(Buttons::UpdateNodes),
    };
    let stored = serde_json::to_string(&route()).unwrap();
    let recovered = || serde_json::from_str(&stored).unwrap();
    assert_eq!(
        response(recovered(), Interaction::Action),
        Response::UpdateNodes
    );
    assert_eq!(
        response(recovered(), Interaction::Open),
        Response::Open(CenterTarget::Machines)
    );
    for interaction in [Interaction::Other, Interaction::Ignore] {
        assert_eq!(response(route(), interaction), Response::Nothing);
    }
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
#[test]
fn native_notification_keeps_its_actions_and_click_target() {
    use objc2_foundation::{NSString, ns_string};

    let (notice, key) = agent_notice(Some(3));
    let notification =
        mac::notification(&notice.title, &notice.body, &route(&notice), None).unwrap();
    assert!(notification.hasActionButton());
    assert_eq!(&*notification.actionButtonTitle(), ns_string!("Allow"));
    assert_eq!(&*notification.otherButtonTitle(), ns_string!("Deny"));
    let info = notification.userInfo().unwrap();
    let value = info
        .objectForKey(ns_string!("beep-route"))
        .unwrap()
        .downcast::<NSString>()
        .unwrap();
    let recovered: Route = serde_json::from_str(&value.to_string()).unwrap();
    assert_eq!(
        response(recovered, Interaction::Other),
        Response::Answer(key, false)
    );

    let plain = mac::notification(
        "Notice",
        "Finished",
        &Route {
            target: None,
            buttons: None,
        },
        None,
    )
    .unwrap();
    assert!(!plain.hasActionButton());
}

#[cfg(desktop)]
#[test]
fn icon_cache_refreshes_only_when_an_existing_install_changes_icon() {
    use std::path::Path;

    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let icon = tmp.path().join("icon.icns");
    std::fs::write(&icon, b"old-icon-bytes").unwrap();

    // Brand new install: records hash without refreshing system caches.
    let mut refreshes = 0;
    icon_cache::sync_icon_cache(&data_dir, false, &icon, || {
        refreshes += 1;
        true
    });
    assert_eq!(refreshes, 0);

    // Same icon on next launch: no refresh.
    icon_cache::sync_icon_cache(&data_dir, true, &icon, || {
        refreshes += 1;
        true
    });
    assert_eq!(refreshes, 0);

    // Icon bytes change on update: refreshes once and updates saved hash.
    std::fs::write(&icon, b"new-icon-bytes").unwrap();
    icon_cache::sync_icon_cache(&data_dir, true, &icon, || {
        refreshes += 1;
        true
    });
    assert_eq!(refreshes, 1);

    // Subsequent launch with the same new icon: no refresh.
    icon_cache::sync_icon_cache(&data_dir, true, &icon, || {
        refreshes += 1;
        true
    });
    assert_eq!(refreshes, 1);

    // Failed refresh does not record the new hash, so the next launch can retry.
    std::fs::write(&icon, b"third-icon-bytes").unwrap();
    icon_cache::sync_icon_cache(&data_dir, true, &icon, || {
        refreshes += 1;
        false
    });
    assert_eq!(refreshes, 2);
    icon_cache::sync_icon_cache(&data_dir, true, &icon, || {
        refreshes += 1;
        true
    });
    assert_eq!(refreshes, 3);

    // Existing install upgrading from before hash tracking existed: refreshes once.
    let upgrade_dir = tmp.path().join("upgrade");
    std::fs::create_dir_all(&upgrade_dir).unwrap();
    icon_cache::sync_icon_cache(&upgrade_dir, true, &icon, || {
        refreshes += 1;
        true
    });
    assert_eq!(refreshes, 4);
    icon_cache::sync_icon_cache(&upgrade_dir, true, &icon, || {
        refreshes += 1;
        true
    });
    assert_eq!(refreshes, 4);

    // App bundle path detection only matches `.app/Contents/MacOS/<bin>`.
    assert_eq!(
        icon_cache::app_bundle_dir(Path::new(
            "/Applications/Beep.app/Contents/MacOS/beep-desktop"
        )),
        Some(Path::new("/Applications/Beep.app"))
    );
    assert_eq!(
        icon_cache::app_bundle_dir(Path::new("/Users/test/target/debug/beep-desktop")),
        None
    );

    // Bundled icon lookup prefers `icon.icns` and falls back to another `.icns`.
    let bundle = tmp.path().join("Test.app");
    let resources = bundle.join("Contents/Resources");
    std::fs::create_dir_all(&resources).unwrap();
    let custom = resources.join("beep.icns");
    std::fs::write(&custom, b"custom").unwrap();
    assert_eq!(icon_cache::bundle_icon_path(&bundle), Some(custom));
    let default = resources.join("icon.icns");
    std::fs::write(&default, b"default").unwrap();
    assert_eq!(icon_cache::bundle_icon_path(&bundle), Some(default));
}
