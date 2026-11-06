//! Tests of the app protocol's root modules through the public API.

mod names {
    use beep_proto::names::*;
    use iroh::SecretKey;

    #[test]
    fn names_are_stable() {
        let id = SecretKey::from_bytes(&[7; 32]).public();
        assert_eq!(username(&id), "sturdy-monkey-swim");
    }
}

mod link {
    use beep_proto::link::*;
    use iroh::{EndpointId, SecretKey};
    use std::time::{Duration, SystemTime};

    fn keys() -> (SecretKey, EndpointId) {
        let device = SecretKey::from_bytes(&[3; 32]);
        let server = SecretKey::from_bytes(&[4; 32]).public();
        (device, server)
    }

    #[test]
    fn an_offer_survives_its_uri() {
        let (device, server) = keys();
        let now = SystemTime::now();
        let offer = LinkOffer::sign(&device, server, "MacBook", now).unwrap();
        let parsed = LinkOffer::parse(&offer.to_uri()).unwrap();
        assert_eq!(parsed, offer);
        assert_eq!(parsed.device, device.public());
        parsed.verify(server, now).unwrap();
        let code = parsed.code();
        assert_eq!(code.len(), 7);
        assert_eq!(code, offer.code());
    }

    #[test]
    fn a_changed_offer_is_refused() {
        let (device, server) = keys();
        let now = SystemTime::now();
        let mut offer = LinkOffer::sign(&device, server, "MacBook", now).unwrap();
        offer.name = "Evil".into();
        assert_eq!(offer.verify(server, now), Err(LinkError::BadSignature));

        let offer = LinkOffer::sign(&device, server, "MacBook", now).unwrap();
        let other = SecretKey::from_bytes(&[5; 32]).public();
        assert_eq!(offer.verify(other, now), Err(LinkError::BadSignature));

        let mut offer = LinkOffer::sign(&device, server, "MacBook", now).unwrap();
        offer.device = SecretKey::from_bytes(&[6; 32]).public();
        assert_eq!(offer.verify(server, now), Err(LinkError::BadSignature));
    }

    #[test]
    fn an_old_offer_expires() {
        let (device, server) = keys();
        let then = SystemTime::now();
        let offer = LinkOffer::sign(&device, server, "MacBook", then).unwrap();
        offer.verify(server, then + OFFER_TTL).unwrap();
        assert_eq!(
            offer.verify(server, then + OFFER_TTL + Duration::from_secs(1)),
            Err(LinkError::Expired)
        );
        assert_eq!(
            offer.verify(server, then - Duration::from_secs(120)),
            Err(LinkError::Expired)
        );
    }

    #[test]
    fn a_join_code_reads_from_its_uri_or_typed() {
        let uri = join_uri("K7QM4X9TRP");
        assert_eq!(join_code(&uri).as_deref(), Some("K7QM4X9TRP"));
        assert_eq!(join_code(" k7qm4-x9trp ").as_deref(), Some("K7QM4X9TRP"));
        assert_eq!(format_join_code("K7QM4X9TRP"), "K7QM4-X9TRP");
        assert_eq!(join_code("beep-link:abc"), None);
        assert_eq!(join_code("short"), None);
    }

    #[test]
    fn a_server_offer_names_the_server() {
        let (device, server) = keys();
        let now = SystemTime::now();
        let own = LinkOffer::for_server(&device, "Beep", now).unwrap();
        let parsed = LinkOffer::parse(&own.to_uri()).unwrap();
        assert_eq!(parsed.server(now), Some(device.public()));
        // A device's offer for another server is not a server's.
        let offer = LinkOffer::sign(&device, server, "MacBook", now).unwrap();
        assert_eq!(offer.server(now), None);
        assert_eq!(own.server(now + OFFER_TTL + Duration::from_secs(1)), None);
    }
}

mod urls {
    use beep_proto::*;

    #[test]
    fn a_login_link_gives_its_token() {
        assert_eq!(
            login_token("beep://login?token=abc.def").as_deref(),
            Some("abc.def")
        );
        assert_eq!(
            login_token("beep://login/?x=1&token=a%2Db.c").as_deref(),
            Some("a-b.c")
        );
        assert_eq!(login_token("beep://login"), None);
        assert_eq!(login_token("beep://login?token="), None);
        assert_eq!(login_token("beep://oauth?token=abc"), None);
    }

    #[test]
    fn share_token_reads_only_share_links() {
        assert_eq!(share_url("ab12"), "https://beep.flashmind.com/s/ab12");
        assert_eq!(
            mcp_callback(
                "https://beep.flashmind.com/oauth/callback?code=a%2Fb&state=s1&iss=https%3A%2F%2Fid.example"
            ),
            Some(McpCallback {
                code: "a/b".to_owned(),
                state: "s1".to_owned(),
                issuer: Some("https://id.example".to_owned()),
            })
        );
        assert_eq!(
            mcp_callback("beep://oauth?state=s1&code=c"),
            Some(McpCallback {
                code: "c".to_owned(),
                state: "s1".to_owned(),
                issuer: None,
            })
        );
        assert_eq!(
            mcp_callback("https://beep.flashmind.com/oauth/callback?code=c"),
            None
        );
        assert_eq!(
            mcp_callback("https://beep.flashmind.com.evil/oauth/callback?code=c&state=s"),
            None
        );
        assert_eq!(mcp_callback("beep://oauth?code=%zz&state=s"), None);
        assert_eq!(share_token(&share_url("ab12")), Some("ab12"));
        assert_eq!(
            share_token(" https://beep.flashmind.com/s/ab12/\n"),
            Some("ab12")
        );
        assert_eq!(share_token(" beep://share/ab12\n"), Some("ab12"));
        assert_eq!(share_token("beep://share/"), None);
        assert_eq!(share_token("beep://share/../x"), None);
        assert_eq!(share_token("https://beep.flashmind.com/s/"), None);
        assert_eq!(share_token("https://beep.flashmind.com.evil/s/ab12"), None);
        assert_eq!(share_token("http://beep.flashmind.com/s/ab12"), None);
        assert_eq!(share_token("beep://apps"), None);
        assert_eq!(group_token(&group_url("cd34")), Some("cd34"));
        assert_eq!(group_token("beep://group/cd34"), Some("cd34"));
        assert_eq!(group_token(&share_url("cd34")), None);
    }

    #[test]
    fn group_code_reads_typed_codes_and_old_links() {
        assert_eq!(group_code("k7qm4-x9trp").as_deref(), Some("K7QM4X9TRP"));
        assert_eq!(group_code(" K7QM4 X9TRP\n").as_deref(), Some("K7QM4X9TRP"));
        assert_eq!(
            group_code(&group_url("7a2d5f90be13")).as_deref(),
            Some("7A2D5F90BE13")
        );
        assert_eq!(
            group_code("beep://group/7a2d5f90be13").as_deref(),
            Some("7A2D5F90BE13")
        );
        assert_eq!(group_code("hey there!"), None);
        assert_eq!(group_code("ab"), None);
        assert_eq!(group_code(&share_url("cd34ef")), None);
        assert_eq!(format_group_code("K7QM4X9TRP"), "K7QM4-X9TRP");
        assert_eq!(format_group_code("7A2D5F90BE13"), "7A2D5F90BE13");
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

mod requests {
    use beep_proto::{
        http::{decode, encode},
        *,
    };
    use serde::Serialize;
    use uuid::Uuid;

    #[test]
    fn chat_from_an_app_without_platform_decodes() {
        #[derive(Serialize)]
        struct Old {
            session_id: Uuid,
            request_id: Uuid,
            message: &'static str,
            images: Vec<String>,
            model: Option<String>,
            thinking: Option<ReasoningLevel>,
        }
        let old = Old {
            session_id: Uuid::nil(),
            request_id: Uuid::nil(),
            message: "hi",
            images: Vec::new(),
            model: None,
            thinking: None,
        };
        let buf = encode(&old).unwrap();
        assert_eq!(decode::<Chat>(&buf).unwrap().platform, None);
    }

    #[test]
    fn a_group_message_from_before_images_still_decodes() {
        #[derive(Serialize)]
        struct Old {
            group_id: String,
            request_id: Uuid,
            message: String,
        }
        let old = Old {
            group_id: "g1".into(),
            request_id: Uuid::new_v4(),
            message: "hi".into(),
        };
        let buf = encode(&old).unwrap();
        let decoded: SendToGroup = decode(&buf).unwrap();
        assert_eq!(decoded.message, "hi");
        assert!(decoded.images.is_empty());
    }

    #[test]
    fn a_request_skips_fields_a_newer_app_added() {
        #[derive(Serialize)]
        struct Newer {
            id: &'static str,
            extra: u8,
        }
        let buf = encode(&Newer { id: "r1", extra: 7 }).unwrap();
        assert_eq!(decode::<GetRun>(&buf).unwrap(), GetRun { id: "r1".into() });
        // A request with no fields yet, given one.
        #[derive(Serialize)]
        struct Given {
            extra: Option<u8>,
        }
        assert!(decode::<Health>(&encode(&Given { extra: None }).unwrap()).is_ok());
    }
}

mod chat {
    use beep_proto::*;

    #[test]
    fn only_a_resolved_notice_names_a_question() {
        let question = PendingApproval {
            run_id: String::new(),
            query_id: "q1".into(),
            approval: Approval {
                app: "Gmail".into(),
                action: "Send email".into(),
                fields: Vec::new(),
            },
        };
        let resolved = Notice::resolved("chat".into(), question.clone());
        assert_eq!(resolved.resolved_question(), Some("q1"));
        assert!(resolved.title.is_empty());
        let asking = Notice {
            title: "Allow Gmail: Send email?".into(),
            changed: false,
            ..resolved.clone()
        };
        assert_eq!(asking.resolved_question(), None);
        let group = Notice {
            group: true,
            ..resolved
        };
        assert_eq!(group.resolved_question(), None);
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
            let many: Vec<String> = (0..=MAX_PATHS).map(|i| format!("~/f{i}")).collect();
            assert!(group_paths(&many).is_err());
        }
    }
}
