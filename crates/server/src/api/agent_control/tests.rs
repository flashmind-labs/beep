use super::{auto::folder_test, *};
use crate::{
    api::agent_control::{
        steer::{node_request, queued_instead},
        view::entries_for,
    },
    protocol::node::agent::{self, Request},
};

#[test]
fn only_an_app_that_reads_thoughts_gets_them() {
    let thought = agent::Entry {
        kind: agent::EntryKind::Thought,
        text: "hmm".into(),
        ms: Some(4000),
    };
    let entries = vec![
        agent::Entry::new(agent::EntryKind::User, "go".into()),
        thought,
        agent::Entry::new(agent::EntryKind::Agent, "done".into()),
    ];
    let kinds = |thoughts| -> Vec<AgentEntryKind> {
        entries_for(entries.clone(), thoughts)
            .into_iter()
            .map(|entry| entry.kind)
            .collect()
    };
    assert_eq!(
        kinds(true),
        [
            AgentEntryKind::User {},
            AgentEntryKind::Thought { ms: Some(4000) },
            AgentEntryKind::Agent {},
        ]
    );
    assert_eq!(
        kinds(false),
        [AgentEntryKind::User {}, AgentEntryKind::Agent {}]
    );
}

#[test]
fn a_named_branch_needs_a_branch_name_and_a_node_that_makes_it() {
    let named = |branch: &str| {
        Some(AgentCheckoutPlan::NamedWorktree {
            base: Some("main".into()),
            branch: branch.into(),
        })
    };
    let continued = Some(AgentCheckoutPlan::BranchWorktree {
        branch: "login-copy".into(),
    });
    let new = Makes {
        checkouts: true,
        branches: true,
    };
    let plain = Makes {
        checkouts: true,
        branches: false,
    };
    assert_eq!(
        checkout_for(new, "box", named("issue-52-crash")).unwrap(),
        Some(agent::CheckoutPlan::NamedWorktree {
            base: Some("main".into()),
            branch: "issue-52-crash".into(),
        })
    );
    assert_eq!(
        checkout_for(new, "box", continued.clone()).unwrap(),
        Some(agent::CheckoutPlan::BranchWorktree {
            branch: "login-copy".into(),
        })
    );
    for bad in [
        "", "-b", "a..b", "HEAD", "x~1", "a/.b", "a.lock", "a//b", "a b",
    ] {
        assert!(
            matches!(
                checkout_for(new, "box", named(bad)),
                Err(ApiError::BadRequest { .. })
            ),
            "{bad:?} is refused"
        );
    }
    let Err(ApiError::BadRequest { message }) = checkout_for(plain, "old-box", continued) else {
        panic!("a bad request");
    };
    assert!(message.contains("old-box is too old"), "{message}");
}

fn png(name: &str, size: usize) -> Upload {
    let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
    data.resize(size, 0);
    Upload {
        name: name.into(),
        mime: "image/png".into(),
        data,
    }
}

#[test]
fn a_steer_goes_to_the_node_and_falls_back_to_a_prompt() {
    let steer = node_request(AgentSteer::Steer {
        text: "Use the fixture instead".into(),
        images: vec![png("shot.png", 1024)],
    })
    .unwrap();
    assert!(matches!(&steer, Request::Steer { images, .. } if images.len() == 1));
    assert!(matches!(
        queued_instead(&steer),
        Some(Request::PromptImages { text, images }) if text == "Use the fixture instead" && images.len() == 1
    ));
    let words = node_request(AgentSteer::Steer {
        text: "stop editing tests".into(),
        images: Vec::new(),
    })
    .unwrap();
    assert_eq!(
        queued_instead(&words),
        Some(Request::Prompt {
            text: "stop editing tests".into()
        })
    );
    assert_eq!(queued_instead(&Request::Cancel {}), None);
    let empty = node_request(AgentSteer::Steer {
        text: " ".into(),
        images: Vec::new(),
    });
    assert!(empty.is_err(), "nothing to send");
}

#[test]
fn folder_tests_quote_the_path_and_keep_home() {
    assert_eq!(
        folder_test("/srv/project").as_deref(),
        Some("test -d /srv/project")
    );
    assert_eq!(
        folder_test("/srv/my project").as_deref(),
        Some("test -d '/srv/my project'")
    );
    assert_eq!(
        folder_test("~/code/app").as_deref(),
        Some("test -d \"$HOME\"/code/app")
    );
    assert_eq!(folder_test("~").as_deref(), Some("test -d \"$HOME\""));
}
