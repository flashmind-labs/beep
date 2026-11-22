//! Tests of the root modules through the public API.

mod names {
    use beep_proto::names::*;
    use iroh::SecretKey;

    #[test]
    fn names_are_stable() {
        let id = SecretKey::from_bytes(&[7; 32]).public();
        assert_eq!(username(&id), "sturdy-monkey-swim");
    }
}

mod lenient {
    use beep_proto::lenient::*;
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Inner {
        a: u8,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Older {
        name: String,
        inner: Inner,
        list: Vec<Inner>,
        maybe: Option<Inner>,
        what: Choice,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum Choice {
        Unit,
        New(Inner),
        Tuple(u8, u8),
        Struct { x: u8 },
    }

    #[derive(Serialize)]
    struct NewerInner {
        a: u8,
        added: u8,
    }

    #[derive(Serialize)]
    struct Newer {
        name: String,
        inner: NewerInner,
        list: Vec<NewerInner>,
        maybe: Option<NewerInner>,
        what: NewerChoice,
        added: String,
    }

    #[derive(Serialize)]
    enum NewerChoice {
        New(NewerInner),
        Tuple(u8, u8, u8),
        Struct { x: u8, added: u8 },
    }

    fn newer(what: NewerChoice) -> Newer {
        let inner = || NewerInner { a: 1, added: 9 };
        Newer {
            name: "n".into(),
            inner: inner(),
            list: vec![inner(), inner()],
            maybe: Some(inner()),
            what,
            added: "extra".into(),
        }
    }

    fn older(what: Choice) -> Older {
        let inner = || Inner { a: 1 };
        Older {
            name: "n".into(),
            inner: inner(),
            list: vec![inner(), inner()],
            maybe: Some(inner()),
            what,
        }
    }

    #[test]
    fn plain_rmp_refuses_what_this_reader_skips() {
        let bytes = rmp_serde::to_vec(&newer(NewerChoice::New(NewerInner { a: 2, added: 3 })))
            .expect("encodes");
        assert!(rmp_serde::from_slice::<Older>(&bytes).is_err());
        assert_eq!(
            from_slice::<Older>(&bytes).expect("decodes"),
            older(Choice::New(Inner { a: 2 }))
        );
    }

    #[test]
    fn unknown_trailing_elements_are_skipped_at_every_depth() {
        let cases = [
            (NewerChoice::Tuple(1, 2, 3), Choice::Tuple(1, 2)),
            (
                NewerChoice::Struct { x: 5, added: 6 },
                Choice::Struct { x: 5 },
            ),
        ];

        for (sent, expect) in cases {
            let bytes = rmp_serde::to_vec(&newer(sent)).expect("encodes");
            assert_eq!(from_slice::<Older>(&bytes).expect("decodes"), older(expect));
        }
    }

    #[test]
    fn maps_arrays_and_short_arrays_still_read() {
        #[derive(Debug, PartialEq, Deserialize)]
        struct Grown {
            a: u8,
            #[serde(default)]
            b: u8,
        }
        let named = rmp_serde::to_vec_named(&Inner { a: 4 }).expect("encodes");
        let compact = rmp_serde::to_vec(&Inner { a: 4 }).expect("encodes");

        for bytes in [named, compact] {
            assert_eq!(
                from_slice::<Grown>(&bytes).expect("decodes"),
                Grown { a: 4, b: 0 }
            );
        }

        let unknown_key = rmp_serde::to_vec_named(&NewerInner { a: 1, added: 2 }).expect("encodes");
        assert_eq!(
            from_slice::<Inner>(&unknown_key).expect("decodes"),
            Inner { a: 1 }
        );
        let map = BTreeMap::from([(1u8, "x".to_owned())]);
        let bytes = rmp_serde::to_vec(&map).expect("encodes");
        assert_eq!(
            from_slice::<BTreeMap<u8, String>>(&bytes).expect("decodes"),
            map
        );
    }

    #[test]
    fn what_is_wrong_is_still_refused() {
        let unknown_variant = rmp_serde::to_vec(&"Nope").expect("encodes");
        assert!(from_slice::<Choice>(&unknown_variant).is_err());
        let wrong_type = rmp_serde::to_vec(&("x", 1u8)).expect("encodes");
        assert!(from_slice::<Inner>(&wrong_type).is_err());
        let too_short = rmp_serde::to_vec(&()).expect("encodes");
        assert!(from_slice::<Inner>(&too_short).is_err());
        assert!(from_slice::<Inner>(&[]).is_err());
    }

    #[derive(Serialize)]
    struct OldInner {
        a: u8,
    }

    #[derive(Serialize)]
    struct OldOuter {
        name: String,
        inner: OldInner,
        list: Vec<OldInner>,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct NewInner {
        a: u8,
        #[serde(default)]
        added: u8,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct NewOuter {
        name: String,
        inner: NewInner,
        list: Vec<NewInner>,
        #[serde(default)]
        added: String,
    }

    /// The property compact msgpack is chosen for: a new reader given a
    /// shorter message from an older sender fills the defaulted fields.
    #[test]
    fn a_shorter_message_fills_defaulted_fields_at_every_depth() {
        let old = OldOuter {
            name: "n".into(),
            inner: OldInner { a: 1 },
            list: vec![OldInner { a: 2 }, OldInner { a: 3 }],
        };
        let bytes = rmp_serde::to_vec(&old).expect("encodes");
        let read: NewOuter = from_slice(&bytes).expect("an older message reads");
        let inner = |a| NewInner { a, added: 0 };
        assert_eq!(
            read,
            NewOuter {
                name: "n".into(),
                inner: inner(1),
                list: vec![inner(2), inner(3)],
                added: String::new(),
            }
        );
    }
}

mod secret {
    use beep_proto::secret::*;

    #[test]
    fn debug_hides_the_secret() {
        let secret = SecretText::new("ghp_example1234");
        assert!(!format!("{secret:?}").contains("1234"));
        let body = rmp_serde::to_vec(&secret).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<String>(&body).unwrap(),
            "ghp_example1234"
        );
    }
}

mod transport {
    mod port {
        use beep_proto::port::*;
        use iroh::Endpoint;
        use iroh::endpoint::presets;
        use std::net::{Ipv4Addr, UdpSocket};

        #[tokio::test]
        async fn binds_the_port_asked_for_or_a_random_one_when_it_is_taken() {
            let taken = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("a free port binds");
            let port = taken
                .local_addr()
                .expect("a bound socket has an address")
                .port();
            let builder = || Endpoint::builder(presets::Minimal);

            let fallback = bind_on(builder, port).await.expect("falls back");
            assert!(
                fallback
                    .bound_sockets()
                    .iter()
                    .all(|addr| addr.port() != port)
            );
            fallback.close().await;

            drop(taken);
            let fixed = bind_on(builder, port).await.expect("binds the free port");
            assert!(
                fixed
                    .bound_sockets()
                    .iter()
                    .any(|addr| addr.is_ipv4() && addr.port() == port)
            );
            fixed.close().await;
        }
    }
}

mod agents {
    mod flsh {
        use beep_proto::*;

        #[test]
        fn sorts_and_quantizations_spell_as_flsh_does() {
            use strum::VariantArray;

            for q in FlshQuantization::VARIANTS {
                let text: &str = q.into();
                let wire = rmp_serde::to_vec(q).expect("encodes");
                assert_eq!(wire, rmp_serde::to_vec(text).expect("encodes"));
                assert_eq!(text.parse::<FlshQuantization>().ok(), Some(*q));
            }

            assert!("fp7".parse::<FlshQuantization>().is_err());
            // flsh's own enum reads what Beep writes.
            for sort in FlshSort::VARIANTS {
                let text: &str = sort.into();
                let wire = rmp_serde::to_vec(sort).expect("encodes");
                assert_eq!(wire, rmp_serde::to_vec(text).expect("encodes"));
                let theirs: flashmind_types::ProviderSort =
                    rmp_serde::from_slice(&wire).expect("flsh reads it");
                assert_eq!(rmp_serde::to_vec(&theirs).expect("encodes"), wire);
            }
        }
    }

    mod notice {
        use beep_proto::*;

        fn key(question: Option<u32>) -> AgentNoticeKey {
            AgentNoticeKey {
                machine_id: "ab12".into(),
                agent: "claude".into(),
                session: "a3f9c1".into(),
                question,
                machine_name: "build-box: lab".into(),
            }
        }

        #[test]
        fn a_key_reads_back_as_written() {
            for question in [None, Some(7)] {
                let written = key(question).to_string();
                assert!(written.starts_with("agent:"), "{written}");
                assert_eq!(written.parse::<AgentNoticeKey>(), Ok(key(question)));
            }

            assert_eq!(
                key(Some(7)).to_string(),
                "agent:7:ab12:claude:a3f9c1:build-box: lab"
            );
        }

        #[test]
        fn chat_and_group_ids_are_not_keys() {
            for id in [
                "",
                "6f1c1d36-3f0e-4c55-9d6b-2b8f3c1d9e10",
                "g1",
                "agent:",
                "agent:x:ab12:claude:a3f9c1:box",
                "agent::ab12::a3f9c1:box",
                "agent::ab12:claude",
            ] {
                assert_eq!(id.parse::<AgentNoticeKey>(), Err(NotAgentKey), "{id}");
            }
        }
    }

    mod sessions {
        use beep_proto::*;

        fn image(name: &str, mime: &str, data: Vec<u8>) -> Upload {
            Upload {
                name: name.into(),
                mime: mime.into(),
                data,
            }
        }

        fn png(size: usize) -> Vec<u8> {
            let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
            data.resize(size.max(data.len()), 0);
            data
        }

        #[test]
        fn agent_images_of_the_four_types_pass() {
            let mut webp = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
            webp.push(0);
            let images = [
                image("a.png", "image/png", png(100)),
                image("b.jpg", "image/jpeg", vec![0xff, 0xd8, 0xff, 0xe0]),
                image("c.gif", "image/gif", b"GIF89a..".to_vec()),
                image("d.webp", "image/webp", webp),
            ];
            assert_eq!(check_agent_images(&images), Ok(()));
            assert_eq!(check_agent_images(&[]), Ok(()));
        }

        #[test]
        fn agent_images_over_the_limits_are_refused() {
            let six: Vec<Upload> = (0..6)
                .map(|_| image("a.png", "image/png", png(10)))
                .collect();
            let error = check_agent_images(&six).unwrap_err().to_string();
            assert!(error.contains("At most 5 images"), "{error}");

            let big = [image("big.png", "image/png", png(MAX_AGENT_IMAGE + 1))];
            let error = check_agent_images(&big).unwrap_err().to_string();
            assert!(error.contains("big.png is over 5 MB"), "{error}");
            let at_limit = [image("ok.png", "image/png", png(MAX_AGENT_IMAGE))];
            assert_eq!(check_agent_images(&at_limit), Ok(()));

            let two = [
                image("a.png", "image/png", png(4 << 20)),
                image("b.png", "image/png", png(4 << 20)),
            ];
            let error = check_agent_images(&two).unwrap_err().to_string();
            assert!(error.contains("over 7 MB together"), "{error}");
        }

        #[test]
        fn agent_images_of_other_types_are_refused() {
            let svg = [image("a.svg", "image/svg+xml", b"<svg/>".to_vec())];
            let error = check_agent_images(&svg).unwrap_err().to_string();
            assert!(error.contains("a.svg is not a PNG"), "{error}");
            let lying = [image("a.png", "image/png", vec![0xff, 0xd8, 0xff])];
            let error = check_agent_images(&lying).unwrap_err().to_string();
            assert!(error.contains("not the image/png it says"), "{error}");
        }
    }
}

mod projects {
    mod groups {
        use beep_proto::ApiError;
        use beep_proto::groups::*;

        fn given(list: &[&str]) -> Vec<String> {
            list.iter().map(|p| (*p).to_owned()).collect()
        }

        #[test]
        fn a_home_folder_the_root_or_nothing_is_refused() {
            for list in [
                vec![],
                vec!["~"],
                vec!["/Users/someone/Projects"],
                vec!["/"],
            ] {
                assert!(
                    matches!(group_paths(&given(&list)), Err(ApiError::BadRequest { .. })),
                    "{list:?}"
                );
            }
            let many: Vec<String> = (0..=16).map(|i| format!("~/f{i}")).collect();
            assert!(group_paths(&many).is_err());
        }
    }
}

mod tool_details {
    use beep_proto::{AgentEntry, AgentEntryKind, AgentToolCall};
    use serde::{Deserialize, Serialize};

    #[test]
    fn named_transcript_entries_work_with_released_readers_and_older_nodes() {
        #[derive(Serialize, Deserialize)]
        struct OldEntry {
            kind: AgentEntryKind,
            text: String,
        }
        let older = OldEntry {
            kind: AgentEntryKind::Tool {},
            text: "Wait [completed]".into(),
        };
        let bytes = rmp_serde::to_vec_named(&older).unwrap();
        let mut current: AgentEntry = rmp_serde::from_slice(&bytes).unwrap();
        assert!(current.tool.is_none());
        current.tool = Some(AgentToolCall {
            name: Some("Sleep".into()),
            ..AgentToolCall::default()
        });
        let bytes = rmp_serde::to_vec_named(&current).unwrap();
        let released: OldEntry = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(released.kind, older.kind);
        assert_eq!(released.text, older.text);
    }
}
