//! Tests of `beep_proto::node` through the public API.

mod skills {
    use beep_proto::node::skills::*;

    #[test]
    fn an_older_conflict_and_listing_still_read() {
        let older = (
            "notes",
            vec!["box-a ~/.claude/skills", "box-b ~/.claude/skills"],
            vec!["box-a", "box-b"],
            vec!["box-a ~/.claude/skills"],
        );
        let bytes = rmp_serde::to_vec(&older).expect("encodes");
        let read: Conflict = beep_proto::lenient::from_slice(&bytes).expect("decodes");
        assert_eq!(read.here, ["box-a ~/.claude/skills"]);
        assert!(read.held.is_empty());

        let older = ("notes", "00", vec!["~/.claude/skills"], 2u64, true);
        let bytes = rmp_serde::to_vec(&older).expect("encodes");
        let read: Listed = beep_proto::lenient::from_slice(&bytes).expect("decodes");
        assert_eq!((read.ver, read.edited), (2, true));
        assert_eq!((read.kept, read.changed, read.bytes), (false, 0, 0));
    }
}

mod admin {
    mod traffic {
        use beep_proto::node::traffic::*;
        use beep_proto::node::{EnvReport, Up};
        use serde::Deserialize;
        use std::collections::BTreeMap;

        #[test]
        fn traffic_round_trips_and_an_older_server_skips_it() {
            let up = Up::Traffic(TrafficReport {
                started_at_ms: 1_760_000_000_123,
                counters: BTreeMap::from([("alpn.beep-sync/1.tx".to_owned(), 42)]),
            });
            let body = rmp_serde::to_vec_named(&up).unwrap();
            assert_eq!(rmp_serde::from_slice::<Up>(&body).unwrap(), up);
            // A server from before `Up::Traffic` fails to read the message, which
            // its control loop skips.
            #[derive(Debug, Deserialize)]
            enum OldUp {
                #[allow(dead_code)]
                Env(EnvReport),
            }
            assert!(rmp_serde::from_slice::<OldUp>(&body).is_err());
        }
    }

    mod version {
        use beep_proto::node::version::*;

        #[test]
        fn ahead_is_positive() {
            assert_eq!(clock_offset_ms(1_000, 1_100, 11_050), Some(10_000));
        }

        #[test]
        fn behind_is_negative() {
            assert_eq!(clock_offset_ms(100_000, 100_200, 40_100), Some(-60_000));
        }

        #[test]
        fn a_clock_that_went_back_is_dropped() {
            assert_eq!(clock_offset_ms(2_000, 1_000, 1_500), None);
        }

        #[test]
        fn warns_from_five_seconds_either_way() {
            assert!(!clock_is_off(4_999));
            assert!(clock_is_off(5_000));
            assert!(clock_is_off(-5_000));
            assert!(!clock_is_off(-4_999));
            assert!(clock_is_off(i64::MIN));
        }

        #[test]
        fn an_older_node_has_no_time() {
            let old = rmp_serde::to_vec(&("0.2.3",)).expect("encodes");
            let read: Version = beep_proto::lenient::from_slice(&old).expect("decodes");
            assert_eq!(read.now_ms, None);
        }
    }

    mod update {
        use beep_proto::node::update::*;
        use serde::{Deserialize, Serialize};

        #[test]
        fn update_reads_across_versions() {
            #[derive(Serialize, Deserialize)]
            enum Older {
                Update {},
            }
            let body = rmp_serde::to_vec_named(&Older::Update {}).expect("encodes");
            assert_eq!(
                rmp_serde::from_slice::<Request>(&body).expect("decodes"),
                Request::Update { now: false }
            );
            let body = rmp_serde::to_vec_named(&Request::Update { now: true }).expect("encodes");
            assert!(matches!(
                rmp_serde::from_slice::<Older>(&body).expect("decodes"),
                Older::Update {}
            ));
        }

        #[test]
        fn events_round_trip() {
            for event in [
                Event::Newest {
                    version: "0.1.24".into(),
                },
                Event::Installed {
                    from: "0.1.20".into(),
                    to: "0.1.24".into(),
                    restarting: true,
                    note: String::new(),
                },
                Event::Failed {
                    reason: "no build".into(),
                },
            ] {
                let body = rmp_serde::to_vec_named(&event).expect("encodes");
                assert_eq!(
                    rmp_serde::from_slice::<Event>(&body).expect("decodes"),
                    event
                );
            }
        }
    }
}

mod control {
    use beep_proto::node::*;
    use serde::{Deserialize, Serialize};

    #[test]
    fn a_hello_without_a_workspace_still_reads() {
        #[derive(Serialize)]
        struct Older {
            version: &'static str,
            name: &'static str,
            offers: Vec<&'static str>,
        }
        let body = rmp_serde::to_vec_named(&Older {
            version: "0.1.0",
            name: "box",
            offers: vec!["shell/1"],
        })
        .unwrap();
        let hello: NodeHello = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(hello.workspace, None);
        assert_eq!(hello.setup, None);
        let newer = NodeHello {
            workspace: Some("/home/user".into()),
            setup: Some(NodeSetup {
                token: "bk1-test".into(),
                name: None,
            }),
            ..hello
        };
        let body = rmp_serde::to_vec_named(&newer).unwrap();
        assert_eq!(rmp_serde::from_slice::<NodeHello>(&body).unwrap(), newer);
    }

    #[test]
    fn an_older_node_skips_credentials() {
        // What a node from before `Down::Credentials` knew.
        #[derive(Debug, Deserialize)]
        enum OldDown {
            Welcome {},
        }
        let body =
            rmp_serde::to_vec_named(&Down::Credentials(MachineCredentials::default())).unwrap();
        assert!(rmp_serde::from_slice::<OldDown>(&body).is_err());
        let hello = NodeHello {
            version: "0.1.0".into(),
            name: "box".into(),
            offers: vec![],
            workspace: None,
            setup: None,
            credentials: true,
            checkouts: true,
            agent_settings: true,
            branch_checkouts: true,
        };
        let body = rmp_serde::to_vec_named(&hello).unwrap();
        assert_eq!(rmp_serde::from_slice::<NodeHello>(&body).unwrap(), hello);
    }

    #[test]
    fn an_older_node_reads_a_welcome_that_offers_tools() {
        // What a node from before `tools` knew.
        #[derive(Debug, Deserialize)]
        enum OldDown {
            Welcome {},
        }
        let body = rmp_serde::to_vec_named(&Down::Welcome { tools: true }).unwrap();
        assert!(matches!(
            rmp_serde::from_slice::<OldDown>(&body),
            Ok(OldDown::Welcome {})
        ));
        // And a newer node reads an older server's welcome.
        #[derive(Serialize)]
        enum Older {
            Welcome {},
        }
        let body = rmp_serde::to_vec_named(&Older::Welcome {}).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Down>(&body).unwrap(),
            Down::Welcome { tools: false }
        );
    }

    #[test]
    fn stats_round_trip_and_an_older_server_skips_them() {
        let up = Up::Stats(StatsReport {
            disks: vec![DiskUsage {
                path: "/home/user".into(),
                free_bytes: 212_000_000_000,
                total_bytes: 500_000_000_000,
            }],
            memory_used_mb: 9_300,
            memory_total_mb: 32_768,
            load: Some(1.25),
        });
        let body = rmp_serde::to_vec_named(&up).unwrap();
        assert_eq!(rmp_serde::from_slice::<Up>(&body).unwrap(), up);
        // What a server from before `Up::Stats` knew: it fails to read the
        // message, which its control loop skips.
        #[derive(Debug, Deserialize)]
        enum OldUp {
            #[allow(dead_code)]
            Env(EnvReport),
        }
        assert!(rmp_serde::from_slice::<OldUp>(&body).is_err());
    }
}

mod env {
    use beep_proto::node::*;
    use serde::Serialize;

    #[test]
    fn a_report_with_unknown_fields_still_reads() {
        #[derive(Serialize)]
        struct Newer {
            os: &'static str,
            displays: u32,
        }
        let body = rmp_serde::to_vec_named(&Newer {
            os: "linux",
            displays: 2,
        })
        .unwrap();
        let report: EnvReport = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(report.os, "linux");
        assert!(report.tools.is_empty());
    }

    #[test]
    fn capabilities_from_report() {
        let report = EnvReport {
            tools: vec![ToolInfo {
                name: "docker".into(),
                version: None,
            }],
            apps: vec![AppInfo {
                name: AppName::Blender,
                version: None,
            }],
            gpus: vec![GpuInfo {
                vendor: GpuVendor::Nvidia,
                model: "NVIDIA GeForce RTX 4090".into(),
                vram_mb: Some(24564),
                driver: None,
            }],
            ..EnvReport::default()
        };
        assert_eq!(
            report.capabilities(),
            [
                Capability::Gpu,
                Capability::Cuda,
                Capability::Blender,
                Capability::Docker
            ]
        );
        let name: &'static str = Capability::Cuda.into();
        assert_eq!(name, "cuda");
        assert_eq!(AppName::UnrealEngine.to_string(), "Unreal Engine");
    }

    #[test]
    fn a_syncthing_folder_holds_itself_and_what_is_inside() {
        let folder = |id: &str, path: &str| SyncthingFolder {
            id: id.into(),
            label: String::new(),
            path: path.into(),
        };
        let report = EnvReport {
            syncthing: vec![
                folder("work", "/srv/work/"),
                folder("game", "/srv/work/game"),
            ],
            ..EnvReport::default()
        };
        let found = |path| report.syncthing_folder(path).map(|f| f.id.as_str());
        assert_eq!(found("/srv/work"), Some("work"));
        assert_eq!(found("/srv/work/notes"), Some("work"));
        assert_eq!(found("/srv/work/game/src"), Some("game"));
        assert_eq!(found("/srv/workshop"), None);
        assert_eq!(found(""), None);
        let windows = folder("w", r"C:\Users\sam\work");
        assert!(windows.holds(r"C:\Users\sam\work\game"));
        assert_eq!(windows.name(), "w");
    }

    #[test]
    fn unknown_app_decodes_as_unknown() {
        #[derive(Serialize)]
        struct Newer {
            name: &'static str,
        }
        let body = rmp_serde::to_vec_named(&Newer { name: "figma" }).unwrap();
        let app: AppInfo = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(app.name, AppName::Unknown);
    }
}

mod stream {
    use beep_proto::node::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize)]
    struct NewerHello {
        version: String,
        name: String,
        offers: Vec<String>,
        workspace: Option<String>,
        setup: Option<String>,
        credentials: bool,
        checkouts: bool,
        agent_settings: bool,
        branch_checkouts: bool,
        added_later: u32,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Pair {
        a: u8,
    }

    #[derive(Serialize)]
    struct NewerPair {
        a: u8,
        b: u8,
    }

    #[tokio::test]
    async fn a_node_message_with_a_field_from_a_newer_build_is_read() {
        let LocalLink {
            caller: (mut tx, _),
            node: (_, mut rx),
        } = local_link();
        tx.send(&NewerHello {
            version: "9.0.0".into(),
            name: "box".into(),
            offers: vec!["shell/1".into()],
            workspace: None,
            setup: None,
            credentials: true,
            checkouts: true,
            agent_settings: true,
            branch_checkouts: true,
            added_later: 5,
        })
        .await
        .expect("sends");
        let hello: NodeHello = rx.recv().await.expect("reads").expect("a message");
        assert_eq!(hello.name, "box");
        assert!(hello.branch_checkouts);
        tx.send(&NewerPair { a: 1, b: 2 }).await.expect("sends");
        assert_eq!(rx.recv::<Pair>().await.expect("reads"), Some(Pair { a: 1 }));
    }
}

mod desktop {
    use beep_proto::node::desktop::*;
    use serde::Serialize;

    fn press(text: &str) -> KeyPress {
        text.parse().unwrap()
    }

    #[test]
    fn key_presses_read_like_shortcuts() {
        assert_eq!(
            press("cmd+t"),
            KeyPress {
                key: "t".into(),
                modifiers: vec![Modifier::Command]
            }
        );
        assert_eq!(
            press("Shift+CMD+Left"),
            KeyPress {
                key: "left".into(),
                modifiers: vec![Modifier::Command, Modifier::Shift]
            }
        );
        assert_eq!(press("enter").key, "return");
        assert_eq!(press("cmd++").key, "+");
        assert_eq!(press("cmd+plus").to_string(), "cmd++");
        assert_eq!(press("alt+ctrl+esc").to_string(), "option+ctrl+escape");
        assert!("cmd+".parse::<KeyPress>().is_err());
        assert!("hyper+t".parse::<KeyPress>().is_err());
    }

    #[test]
    fn a_failure_from_an_older_node_reads_without_its_permission() {
        #[derive(Serialize)]
        enum Older {
            Failed { reason: &'static str },
        }
        let body = rmp_serde::to_vec_named(&Older::Failed { reason: "no" }).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Reply>(&body).unwrap(),
            Reply::Failed {
                reason: "no".into(),
                missing: None
            }
        );
    }
}

mod files {
    use beep_proto::node::files::*;
    use serde::Serialize;

    #[test]
    fn a_read_without_its_newer_fields_still_reads() {
        #[derive(Serialize)]
        enum Older {
            Read { path: &'static str },
        }
        let bytes = rmp_serde::to_vec_named(&Older::Read { path: "/srv/a" }).expect("encodes");
        let read: Request = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(
            read,
            Request::Read {
                path: "/srv/a".into(),
                offset: 0,
                limit: 0
            }
        );
    }

    #[test]
    fn an_unknown_kind_reads_as_other() {
        #[derive(Serialize)]
        struct Newer {
            name: &'static str,
            kind: &'static str,
        }
        let bytes = rmp_serde::to_vec_named(&Newer {
            name: "fifo",
            kind: "socket",
        })
        .expect("encodes");
        let entry: FileEntry = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(entry.kind, FileKind::Other);
    }
}

mod harness {
    use beep_proto::SecretText;
    use beep_proto::node::EnvSecret;
    use beep_proto::node::harness::*;
    use serde::Serialize;

    #[test]
    fn versions_are_read_from_what_each_cli_prints() {
        assert_eq!(
            parse_version("2.1.312 (Claude Code)").as_deref(),
            Some("2.1.312")
        );
        assert_eq!(
            parse_version("codex-cli 0.166.0").as_deref(),
            Some("0.166.0")
        );
        assert_eq!(parse_version("v0.99.2\n").as_deref(), Some("0.99.2"));
        assert_eq!(
            parse_version("flsh 0.41.0-beta.1").as_deref(),
            Some("0.41.0-beta.1")
        );
        assert_eq!(parse_version("opencode"), None);
        assert_eq!(parse_version("error: 42 things"), None);
    }

    #[test]
    fn newer_compares_numbers_not_text() {
        assert!(is_newer("2.1.320", "2.1.312"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.2"));
        assert!(!is_newer("2.1.312", "2.1.312"));
        assert!(!is_newer("2.1.3", "2.1.312"));
        assert!(!is_newer("1.2.0-beta", "1.2.0"));
    }

    #[test]
    fn settings_round_trip_hide_their_values_and_default_to_on() {
        let settings = AgentSettings {
            harnesses: vec![HarnessConfig {
                env: vec![EnvSecret {
                    name: "HTTPS_PROXY".into(),
                    value: SecretText::new("http://user:pass7777@proxy"),
                }],
                args: vec!["--verbose".into()],
                ..HarnessConfig::new("codex")
            }],
            flsh: Some(beep_proto::FlshSettings {
                zdr: Some(true),
                quantizations: vec![beep_proto::FlshQuantization::Fp8],
                sort: Some(beep_proto::FlshSort::Throughput),
                models: vec![beep_proto::FlshModelRule {
                    model: "openrouter:vendor/model".parse().unwrap(),
                    autocompact: beep_proto::FlshAutoCompact {
                        enabled: true,
                        tokens: Some(120_000),
                    },
                }],
                ..beep_proto::FlshSettings::default()
            }),
        };
        let body = rmp_serde::to_vec_named(&settings).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<AgentSettings>(&body).unwrap(),
            settings
        );
        assert!(!format!("{settings:?}").contains("7777"));
        #[derive(Serialize)]
        struct Older {
            agent: &'static str,
        }
        let body = rmp_serde::to_vec_named(&Older { agent: "pi" }).unwrap();
        let read: HarnessConfig = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(read, HarnessConfig::new("pi"));
    }

    mod output {
        use beep_proto::node::harness::*;

        /// The end of what a failed `opencode upgrade` printed, cut to its last
        /// characters as an older node did: the tail of its logo in colored
        /// blocks, then its error.
        const OPENCODE: &str = "238m \u{1b}[0m\u{1b}[0m\u{2588}\u{1b}[0m\u{1b}[0m\u{2588}\u{1b}[0m\u{1b}[0m\u{1b}[48;5;238m\u{2580}\u{1b}[0m\u{1b}[0m\u{1b}[48;5;238m\u{2580}\u{1b}[0m\u{1b}[0m\u{1b}[48;5;238m\u{2580}\u{1b}[0m \u{1b}[90m\u{2580}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m \u{1b}[90m\u{2588}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m\u{1b}[38;5;235m\u{2580}\u{1b}[0m\u{1b}[38;5;235m\u{2580}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m \u{1b}[0m\u{2580}\u{1b}[0m\u{1b}[0m\u{2580}\u{1b}[0m\u{1b}[0m\u{2580}\u{1b}[0m \u{1b}[0m \u{1b}[91m\u{1b}[1mError: \u{1b}[0mUnexpected error, check log file at /path/to/home/.local/share/opencode/log/2026-11-03T112330.log for more details SyntaxError: JSON Parse error: Unexpected EOF";

        #[test]
        fn the_opencode_sample_comes_down_to_its_error() {
            let clean = clean_output(OPENCODE);
            let expected = "Error: Unexpected error, check log file at /path/to/home/.local/share/opencode/log/2026-11-03T112330.log for more details SyntaxError: JSON Parse error: Unexpected EOF";
            assert_eq!(clean, expected);
            assert_eq!(failure_line(&clean, 300), expected);
        }

        #[test]
        fn the_same_error_on_its_own_lines_keeps_both() {
            let text = "\u{1b}[48;5;238m\u{2580}\u{2580}\u{2580}\u{1b}[0m\n\u{1b}[90m\u{2588}\u{2588}\u{1b}[0m\n\n\u{1b}[91m\u{1b}[1mError: \u{1b}[0mUnexpected error, check log file\nSyntaxError: JSON Parse error: Unexpected EOF\n";
            let clean = clean_output(text);
            assert_eq!(
                clean,
                "Error: Unexpected error, check log file\nSyntaxError: JSON Parse error: Unexpected EOF"
            );
            assert_eq!(
                failure_line(&clean, 300),
                "Error: Unexpected error, check log file SyntaxError: JSON Parse error: Unexpected EOF"
            );
        }

        #[test]
        fn progress_bars_and_spinners_are_dropped() {
            let text = "Downloading\r[####      ] 40%\r[##########] 100%\n\
                \u{280b} Resolving\r\u{2819} Resolving\r\u{2839} Resolving\n\
                \u{2593}\u{2593}\u{2593}\u{2591}\u{2591} 60%\n\
                \u{1b}]0;npm install\u{7}\u{1b}[2K\u{1b}[1G\n\
                \u{1b}[?25l\u{1b}[?25h\n\
                added 3 packages in 2s\n\
                added 3 packages in 2s\n";
            assert_eq!(clean_output(text), "Resolving\nadded 3 packages in 2s");
        }

        #[test]
        fn the_failure_line_is_cut_to_length() {
            let long = format!("Error: {}", "word ".repeat(100));
            let line = failure_line(&long, 60);
            assert!(line.starts_with("Error: word"), "{line}");
            assert!(line.ends_with('…'), "{line}");
            assert!(line.chars().count() <= 61, "{line}");
            // Without an error line, the last words are kept.
            let words = (0..100)
                .map(|i| format!("w{i}"))
                .collect::<Vec<_>>()
                .join(" ");
            let line = failure_line(&words, 30);
            assert!(line.starts_with('…') && line.ends_with("w99"), "{line}");
            // The earliest error line that fits starts the line.
            let text = "Downloading formula\nError: refusing to load\nRun brew trust to trust it.";
            assert_eq!(
                failure_line(text, 300),
                "Error: refusing to load Run brew trust to trust it."
            );
        }
    }
}

mod mesh_state {
    use beep_proto::AgentSlug;
    use beep_proto::SecretText;
    use beep_proto::node::EnvSecret;
    use beep_proto::node::mesh_state::*;
    use iroh::{EndpointId, SecretKey};
    use serde::Serialize;
    use std::collections::BTreeMap;

    #[test]
    fn a_value_with_a_field_from_a_newer_build_is_read() {
        #[derive(Serialize)]
        struct NewerSpec {
            folders: BTreeMap<EndpointId, String>,
            paused: bool,
            added_later: u8,
        }
        let id = SecretKey::generate().public();
        let sent = NewerSpec {
            folders: BTreeMap::from([(id, "/srv/project".to_owned())]),
            paused: true,
            added_later: 1,
        };
        let bytes = rmp_serde::to_vec(&sent).unwrap();
        let spec = SyncSpec::from_bytes(&bytes).unwrap();
        assert!(spec.paused);
        assert_eq!(spec.folders.len(), 1);
        // A value from an older build, which wrote maps, still reads.
        let named = rmp_serde::to_vec_named(&sent).unwrap();
        assert!(SyncSpec::from_bytes(&named).unwrap().paused);
    }

    #[test]
    fn a_move_record_reads_with_a_field_a_newer_build_added() {
        #[derive(Serialize)]
        struct NewerMove {
            agent: AgentSlug,
            id: String,
            from: EndpointId,
            to: EndpointId,
            project: String,
            path: String,
            mode: Option<String>,
            stop: bool,
            at: u64,
            worktree: Option<MovedWorktree>,
            added_later: u8,
        }
        let (from, to) = (
            SecretKey::generate().public(),
            SecretKey::generate().public(),
        );
        let sent = NewerMove {
            agent: AgentSlug::CODEX,
            id: "s1".into(),
            from,
            to,
            project: "org/app".into(),
            path: "crates/a".into(),
            mode: None,
            stop: true,
            at: 7,
            worktree: None,
            added_later: 1,
        };
        let record = SessionMove::from_bytes(&rmp_serde::to_vec(&sent).unwrap()).unwrap();
        assert_eq!((record.from, record.to, record.stop), (from, to, true));
        assert_eq!(
            SessionMove::from_bytes(&record.to_bytes().unwrap()).unwrap(),
            record
        );
    }

    #[test]
    fn a_move_record_from_before_worktree_moves_reads_as_a_checkout_move() {
        #[derive(Serialize)]
        struct BeforeWorktrees {
            agent: AgentSlug,
            id: String,
            from: EndpointId,
            to: EndpointId,
            project: String,
            path: String,
            mode: Option<String>,
            stop: bool,
            at: u64,
        }
        let sent = BeforeWorktrees {
            agent: AgentSlug::CODEX,
            id: "s1".into(),
            from: SecretKey::generate().public(),
            to: SecretKey::generate().public(),
            project: "org/app".into(),
            path: String::new(),
            mode: None,
            stop: false,
            at: 7,
        };
        let record = SessionMove::from_bytes(&rmp_serde::to_vec(&sent).unwrap()).unwrap();
        assert_eq!(record.worktree, None);
        // A node from before it reads the new record's first nine fields.
        let moved = SessionMove {
            worktree: Some(MovedWorktree {
                branch: "feat".into(),
                from: "/srv/app/.worktrees/feat".into(),
            }),
            ..record
        };
        let older: (
            AgentSlug,
            String,
            EndpointId,
            EndpointId,
            String,
            String,
            Option<String>,
            bool,
            u64,
        ) = beep_proto::lenient::from_slice(&moved.to_bytes().unwrap()).unwrap();
        assert_eq!(older.8, 7);
    }

    #[test]
    fn values_round_trip_and_hide_secrets_in_debug() {
        let id = SecretKey::generate().public();
        let spec = SyncSpec {
            folders: BTreeMap::from([(id, "/srv/project".to_owned())]),
            paused: false,
        };
        assert_eq!(
            SyncSpec::from_bytes(&spec.to_bytes().unwrap()).unwrap(),
            spec
        );
        let credential = Credential::Env(EnvSecret {
            name: "GH_TOKEN".to_owned(),
            value: SecretText::new("ghp_example1234"),
        });
        let back = Credential::from_bytes(&credential.to_bytes().unwrap()).unwrap();
        assert_eq!(back, credential);
        assert!(!format!("{back:?}").contains("1234"));
    }
}

mod sync {
    use beep_proto::node::sync::*;
    use serde::{Deserialize, Serialize};

    #[test]
    fn a_newer_state_or_rule_still_reads() {
        #[derive(Serialize)]
        #[serde(rename_all = "snake_case")]
        enum Newer {
            Frozen,
        }
        let bytes = rmp_serde::to_vec_named(&Newer::Frozen).expect("encodes");
        assert_eq!(
            rmp_serde::from_slice::<State>(&bytes).expect("decodes"),
            State::Other
        );
        assert_eq!(
            rmp_serde::from_slice::<Rule>(&bytes).expect("decodes"),
            Rule::Other
        );
    }

    #[test]
    fn an_older_node_reads_a_paused_project_and_a_newer_status() {
        /// `Project` and `ProjectStatus` as a node before pausing read them.
        #[derive(Deserialize)]
        struct OldProject {
            name: String,
        }
        #[derive(Deserialize)]
        struct OldStatus {
            files: u64,
        }
        let project = Project {
            name: "game".into(),
            folder: "/srv/project".into(),
            peers: Vec::new(),
            paused: true,
        };
        let bytes = rmp_serde::to_vec_named(&project).expect("encodes");
        let old: OldProject = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(old.name, "game");
        let status = ProjectStatus {
            files: 3,
            paused: true,
            waiting: vec!["a".into()],
            ..ProjectStatus::default()
        };
        let bytes = rmp_serde::to_vec_named(&status).expect("encodes");
        let old: OldStatus = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(old.files, 3);
        // And a newer server reads an older node's status.
        #[derive(Serialize)]
        struct Before {
            name: String,
            files: u64,
        }
        let before = Before {
            name: "game".into(),
            files: 2,
        };
        let bytes = rmp_serde::to_vec_named(&before).expect("encodes");
        let now: ProjectStatus = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(now.files, 2);
        assert!(!now.paused && now.progress.is_none());
    }
}

mod tools {
    use beep_proto::node::tools::*;
    use serde::Serialize;

    #[test]
    fn the_agent_tools_go_by_their_names_and_an_older_server_reads_them_as_unknown() {
        for (tool, name) in [
            (Tool::StartAgent, "start_agent"),
            (Tool::AgentStatus, "agent_status"),
            (Tool::SendToAgent, "send_to_agent"),
            (Tool::StopAgent, "stop_agent"),
            (Tool::ListAgents, "list_agents"),
            (Tool::AgentParent, "agent_parent"),
            (Tool::AgentCount, "agent_count"),
        ] {
            assert_eq!(tool.to_string(), name);
            assert_eq!(name.parse::<Tool>(), Ok(tool));
            let bytes = rmp_serde::to_vec_named(&tool).expect("encodes");
            assert_eq!(
                rmp_serde::from_slice::<Tool>(&bytes).expect("decodes"),
                tool
            );
        }
        // What a node from before the tools sends for one it does not know.
        let bytes = rmp_serde::to_vec_named("schedule").expect("encodes");
        assert_eq!(
            rmp_serde::from_slice::<Tool>(&bytes).expect("decodes"),
            Tool::Unknown
        );
    }

    #[test]
    fn an_unknown_tool_and_a_missing_approval_still_read() {
        #[derive(Serialize)]
        struct Newer {
            session: &'static str,
            tool: &'static str,
            args: &'static str,
        }
        let bytes = rmp_serde::to_vec_named(&Newer {
            session: "a3f9c1",
            tool: "schedule",
            args: "{}",
        })
        .expect("encodes");
        let request: Request = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(request.tool, Tool::Unknown);
        assert_eq!(request.approved, Approved::No);
    }
}

mod transfer {
    use beep_proto::node::transfer::*;
    use iroh::{EndpointAddr, SecretKey};
    use serde::Serialize;

    #[test]
    fn taken_names_get_a_number_before_the_extension() {
        let file = ContentKind::File;
        assert_eq!(numbered("report.pdf", file, 1), "report.pdf");
        assert_eq!(numbered("report.pdf", file, 2), "report (2).pdf");
        assert_eq!(numbered("scene.tar.gz", file, 3), "scene.tar (3).gz");
        assert_eq!(numbered("Makefile", file, 2), "Makefile (2)");
        assert_eq!(numbered(".env", file, 2), ".env (2)");
        assert_eq!(
            numbered("renders.v2", ContentKind::Folder, 2),
            "renders.v2 (2)"
        );
    }

    #[test]
    fn an_offer_without_skipped_still_reads() {
        #[derive(Serialize)]
        enum Older {
            Offered {
                content: Content,
                from: EndpointAddr,
                name: &'static str,
                size: u64,
                files: u64,
            },
        }
        let from = EndpointAddr::new(SecretKey::generate().public());
        let content = Content {
            hash: [1; 32],
            kind: ContentKind::File,
        };
        let bytes = rmp_serde::to_vec_named(&Older::Offered {
            content,
            from: from.clone(),
            name: "a.txt",
            size: 1,
            files: 1,
        })
        .expect("encodes");
        let reply: Reply = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(
            reply,
            Reply::Offered {
                content,
                from,
                name: "a.txt".into(),
                size: 1,
                files: 1,
                skipped: 0,
                executable: false,
            }
        );
    }

    #[test]
    fn progress_without_a_stage_or_with_a_newer_one_still_reads() {
        #[derive(Serialize)]
        enum Older {
            Progress { done: u64, total: u64 },
        }
        let bytes =
            rmp_serde::to_vec_named(&Older::Progress { done: 1, total: 2 }).expect("encodes");
        let reply: Reply = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(
            reply,
            Reply::Progress {
                done: 1,
                total: 2,
                stage: Stage::Downloading,
            }
        );
        #[derive(Serialize)]
        #[serde(rename_all = "snake_case")]
        enum Newer {
            Verifying,
        }
        let bytes = rmp_serde::to_vec_named(&Newer::Verifying).expect("encodes");
        let stage: Stage = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(stage, Stage::Other);
    }
}

mod usage {
    use beep_proto::AgentSlug;
    use beep_proto::node::usage::*;
    use serde::Serialize;

    #[test]
    fn session_spend_round_trips_and_an_older_turn_reads() {
        let spend = SessionSpend {
            agent: "claude".into(),
            session: "0f3c2a9e-1111-4222-8333-944455556666".into(),
            cwd: "/srv/project".into(),
            since: 1_789_000_000,
            hours: vec![HourSpend {
                hour: 1_789_995_600,
                model: "claude-opus-4-1".into(),
                input_tokens: 10,
                output_tokens: 20,
                cache_read_tokens: 300,
                cache_write_tokens: 40,
                cost_usd: 0.5,
                unpriced_tokens: 0,
                billing: Billing::Metered,
            }],
        };
        let body = rmp_serde::to_vec_named(&spend).unwrap();
        assert_eq!(rmp_serde::from_slice::<SessionSpend>(&body).unwrap(), spend);
        assert_eq!(spend.hours[0].tokens(), 370);

        #[derive(Serialize)]
        struct OlderTurn {
            session: &'static str,
            tokens: u64,
        }
        let body = rmp_serde::to_vec_named(&OlderTurn {
            session: "a3f9c1",
            tokens: 5,
        })
        .unwrap();
        let read: TurnUsage = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(read.tokens, 5);
        assert_eq!(read.agent_session, None);
    }

    #[test]
    fn an_older_hour_and_a_newer_billing_read_as_unknown() {
        #[derive(Serialize)]
        struct OlderHour {
            hour: u64,
            cost_usd: f64,
        }
        let body = rmp_serde::to_vec_named(&OlderHour {
            hour: 3_600,
            cost_usd: 1.0,
        })
        .unwrap();
        let read: HourSpend = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(read.billing, Billing::Unknown);

        #[derive(Serialize)]
        struct NewerHour {
            hour: u64,
            billing: &'static str,
        }
        let body = rmp_serde::to_vec_named(&NewerHour {
            hour: 3_600,
            billing: "prepaid",
        })
        .unwrap();
        let read: HourSpend = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(read.billing, Billing::Unknown);
        assert_eq!(
            Billing::from_name(Billing::Metered.as_str()),
            Billing::Metered
        );
    }

    #[test]
    fn plan_usage_round_trips_and_reads_without_fields() {
        let plan = PlanUsage {
            agent: AgentSlug::CLAUDE,
            plan: Some("max".into()),
            windows: vec![LimitWindow {
                minutes: 300,
                used_percent: 38.0,
                resets_at: Some(1_790_000_000),
                scope: None,
            }],
            at: 1_789_990_000,
        };
        let body = rmp_serde::to_vec_named(&plan).unwrap();
        assert_eq!(rmp_serde::from_slice::<PlanUsage>(&body).unwrap(), plan);

        #[derive(Serialize)]
        struct Older {
            agent: &'static str,
        }
        let body = rmp_serde::to_vec_named(&Older { agent: "codex" }).unwrap();
        let read: PlanUsage = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(read.agent, "codex");
        assert!(read.windows.is_empty());
    }
}

mod agent {
    mod checkout {
        use beep_proto::AgentCheckoutPlan;
        use beep_proto::node::agent::*;

        #[test]
        fn checkout_plans_cross_both_wires() {
            let plans = [
                CheckoutPlan::NewBranch { base: None },
                CheckoutPlan::NewWorktree {
                    base: Some("origin/main".into()),
                },
            ];
            for plan in plans {
                assert_eq!(
                    CheckoutPlan::from(AgentCheckoutPlan::from(plan.clone())),
                    plan
                );
                let body = rmp_serde::to_vec_named(&plan).unwrap();
                assert_eq!(rmp_serde::from_slice::<CheckoutPlan>(&body).unwrap(), plan);
            }
        }

        #[test]
        fn a_prompt_names_its_branch_and_worktree() {
            assert_eq!(slug("Fix the Android build!"), "fix-the-android-build");
            assert_eq!(slug("  Ünïcode -- and   spaces  "), "n-code-and-spaces");
            assert_eq!(slug("!!!"), "");
            let long = slug(&format!("{} {}", "a".repeat(39), "b".repeat(20)));
            assert_eq!(long, "a".repeat(39), "cut at 40, then the hyphen goes");
            assert_eq!(
                prompt_branch("Add a dark mode\nto settings"),
                "beep/add-a-dark-mode-to-settings"
            );
            assert_eq!(prompt_branch("?"), "beep/agent");
            assert_eq!(worktree_path("beep/fix-it-2"), ".worktrees/beep-fix-it-2");
        }

        #[test]
        fn base_refs_are_plain_names() {
            for good in ["main", "origin/main", "v1.2.0", "HEAD~2", "a1b2c3d"] {
                assert!(valid_ref(good), "{good}");
            }
            let long = "x".repeat(MAX_REF + 1);
            for bad in [
                "",
                "-x",
                "--upload-pack=x",
                "a..b",
                "a b",
                "a:b",
                "a\nb",
                "@{-1}",
                &long,
            ] {
                assert!(!valid_ref(bad), "{bad:?}");
            }
        }
    }

    mod external {
        use beep_proto::node::agent::*;
        use serde::{Deserialize, Serialize};

        /// A reader from before `place`: it reads with `lenient`, as the
        /// node's streams and mesh frames do, and skips the field it lacks.
        #[test]
        fn a_session_with_a_place_still_reads_for_an_older_reader() {
            #[derive(Deserialize)]
            struct Before {
                id: String,
                agent: String,
                cwd: String,
                title: Option<String>,
                started_at: u64,
                last_activity: u64,
                presence: Presence,
                pid: Option<u32>,
                last_prompt: String,
                last_message: String,
                taken_over: Option<String>,
            }
            let sent = ExternalSession {
                id: "s1".into(),
                agent: "codex".into(),
                cwd: "/srv/project".into(),
                taken_over: Some("a3f9c1".into()),
                place: Some(WorkPlace {
                    folder: "/srv/project/.worktrees/fix".into(),
                    main: "/srv/project".into(),
                    linked: true,
                    branch: Some("fix".into()),
                    origin: String::new(),
                }),
                ..ExternalSession::default()
            };
            let bytes = rmp_serde::to_vec(&sent).expect("encodes");
            let read: Before = beep_proto::lenient::from_slice(&bytes).expect("reads");
            assert_eq!(read.id, "s1");
            assert_eq!(read.agent, "codex");
            assert_eq!(read.cwd, "/srv/project");
            assert_eq!(read.title, None);
            assert_eq!((read.started_at, read.last_activity), (0, 0));
            assert_eq!(read.presence, Presence::default());
            assert_eq!(read.pid, None);
            assert!(read.last_prompt.is_empty() && read.last_message.is_empty());
            assert_eq!(read.taken_over.as_deref(), Some("a3f9c1"));
            // And one without it reads for this build.
            let old = rmp_serde::to_vec(&(
                "s2",
                "claude",
                "/srv/project",
                None::<String>,
                1u64,
                2u64,
                "Working",
                None::<u32>,
                "",
                "",
                None::<String>,
            ))
            .expect("encodes");
            let now: ExternalSession = beep_proto::lenient::from_slice(&old).expect("reads");
            assert_eq!(now.place, None);
        }

        #[test]
        fn an_external_session_from_a_newer_node_still_reads() {
            #[derive(Serialize)]
            struct Newer {
                id: &'static str,
                presence: &'static str,
                branch: &'static str,
            }
            let body = rmp_serde::to_vec_named(&Newer {
                id: "s1",
                presence: "Compacting",
                branch: "main",
            })
            .unwrap();
            let session: ExternalSession = rmp_serde::from_slice(&body).unwrap();
            assert_eq!(session.id, "s1");
            assert_eq!(session.presence, Presence::Unknown);
        }
    }

    mod models {
        use beep_proto::AgentEffort;
        use beep_proto::node::agent::*;
        use serde::Serialize;

        #[test]
        fn each_agent_takes_the_thinking_levels_its_cli_does() {
            use Effort::*;
            assert_eq!(efforts("claude"), [Low, Medium, High, Xhigh, Max]);
            assert_eq!(efforts("codex"), [Low, Medium, High]);
            assert_eq!(efforts("pi"), [Off, Minimal, Low, Medium, High, Xhigh, Max]);
            for agent in ["gemini", "flsh", "opencode", "other"] {
                assert!(efforts(agent).is_empty(), "{agent}");
            }
            let names: Vec<&str> = efforts("pi").iter().map(|e| e.as_str()).collect();
            assert_eq!(
                names,
                ["off", "minimal", "low", "medium", "high", "xhigh", "max"]
            );
            let body = rmp_serde::to_vec_named(&Xhigh).unwrap();
            assert_eq!(rmp_serde::from_slice::<String>(&body).unwrap(), "xhigh");
            for effort in efforts("pi") {
                assert_eq!(Effort::from(AgentEffort::from(*effort)), *effort);
            }
        }

        #[test]
        fn a_newer_model_pick_reads_as_fixed() {
            #[derive(Serialize)]
            struct Newer {
                pick: &'static str,
                current: &'static str,
            }
            let body = rmp_serde::to_vec_named(&Newer {
                pick: "Searched",
                current: "m",
            })
            .unwrap();
            let list: ModelList = rmp_serde::from_slice(&body).unwrap();
            assert_eq!(list.pick, ModelPick::Fixed);
            assert_eq!(list.current.as_deref(), Some("m"));
        }
    }

    mod session {
        use beep_proto::AgentContext;
        use beep_proto::node::agent::*;
        use serde::{Deserialize, Serialize};

        #[test]
        fn a_newer_status_still_reads() {
            #[derive(Serialize)]
            struct Newer {
                status: &'static str,
                summary: &'static str,
                mood: u8,
            }
            let body = rmp_serde::to_vec_named(&Newer {
                status: "Compacting",
                summary: "Thinking",
                mood: 3,
            })
            .unwrap();
            let state: State = rmp_serde::from_slice(&body).unwrap();
            assert_eq!(state.status, Status::Unknown);
            assert_eq!(state.summary, "Thinking");
        }

        #[test]
        fn a_start_with_a_model_reads_on_an_older_node_and_an_older_start_here() {
            #[derive(Debug, PartialEq, Serialize, Deserialize)]
            enum Older {
                Start { cwd: Option<String>, prompt: String },
            }
            let newer = Request::Start {
                cwd: Some("/srv/project".into()),
                prompt: "go".into(),
                model: Some("m".into()),
                effort: Some(Effort::High),
                checkout: Some(CheckoutPlan::NewBranch { base: None }),
            };
            let body = rmp_serde::to_vec_named(&newer).unwrap();
            assert_eq!(
                rmp_serde::from_slice::<Older>(&body).unwrap(),
                Older::Start {
                    cwd: Some("/srv/project".into()),
                    prompt: "go".into()
                }
            );
            let body = rmp_serde::to_vec_named(&Older::Start {
                cwd: None,
                prompt: "go".into(),
            })
            .unwrap();
            assert_eq!(
                rmp_serde::from_slice::<Request>(&body).unwrap(),
                Request::Start {
                    cwd: None,
                    prompt: "go".into(),
                    model: None,
                    effort: None,
                    checkout: None,
                }
            );
        }

        #[test]
        fn a_session_with_a_parent_reads_on_an_older_reader_and_back() {
            #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
            #[serde(default)]
            struct Older {
                id: String,
                agent: String,
                turns: u32,
            }
            let newer = SessionInfo {
                id: "c1".into(),
                agent: "codex".into(),
                turns: 2,
                parent: Some(ParentLink {
                    machine: None,
                    session: "p1".into(),
                    key: "p1".into(),
                    root_machine: None,
                    root: "p1".into(),
                    depth: 1,
                    others: 0,
                    agent: "claude".into(),
                    mode: None,
                }),
                ..SessionInfo::default()
            };
            let named = rmp_serde::to_vec_named(&newer).unwrap();
            assert_eq!(
                rmp_serde::from_slice::<Older>(&named).unwrap(),
                Older {
                    id: "c1".into(),
                    agent: "codex".into(),
                    turns: 2
                }
            );
            // A session from a node before the field has no parent.
            let old = rmp_serde::to_vec_named(&Older::default()).unwrap();
            let read = rmp_serde::from_slice::<SessionInfo>(&old).unwrap();
            assert_eq!(read.parent, None);
            assert_eq!(rmp_serde::from_slice::<SessionInfo>(&named).unwrap(), newer);
        }

        #[test]
        fn a_session_with_a_parent_reads_compact_on_a_lenient_older_reader() {
            // What a node from before `parent` reads: the fields up to `place`.
            #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
            #[serde(default)]
            struct Older {
                id: String,
                agent: String,
                cwd: String,
                title: Option<String>,
                started_at: u64,
                state: State,
                question: Option<Question>,
                last_message: String,
                turns: u32,
                context: Option<AgentContext>,
                tokens: u64,
                cost_usd: Option<f64>,
                modes: Vec<ModeOption>,
                mode: Option<String>,
                plan_ready: Option<String>,
                place: Option<WorkPlace>,
            }
            let newer = SessionInfo {
                id: "c1".into(),
                agent: "codex".into(),
                turns: 3,
                parent: Some(ParentLink {
                    machine: None,
                    session: "p1".into(),
                    key: "p1".into(),
                    root_machine: None,
                    root: "p1".into(),
                    depth: 2,
                    others: 1,
                    agent: "claude".into(),
                    mode: Some("plan".into()),
                }),
                ..SessionInfo::default()
            };
            let compact = rmp_serde::to_vec(&newer).unwrap();
            let read: Older = beep_proto::lenient::from_slice(&compact).unwrap();
            assert_eq!((read.id.as_str(), read.turns), ("c1", 3));
            // Plain rmp refuses the longer array, which is why readers are lenient.
            assert!(rmp_serde::from_slice::<Older>(&compact).is_err());
            // A node from before the field sends the short array.
            let short = rmp_serde::to_vec(&Older::default()).unwrap();
            let read: SessionInfo = beep_proto::lenient::from_slice(&short).unwrap();
            assert_eq!(read.parent, None);
            assert_eq!(
                beep_proto::lenient::from_slice::<SessionInfo>(&compact).unwrap(),
                newer
            );
        }

        #[test]
        fn events_round_trip() {
            let event = Event::Question {
                question: Question {
                    id: 1,
                    text: "Run cargo test".into(),
                    detail: None,
                    options: vec![AnswerOption {
                        id: "allow".into(),
                        name: "Allow".into(),
                        kind: OptionKind::AllowOnce,
                    }],
                },
            };
            let body = rmp_serde::to_vec_named(&event).unwrap();
            assert_eq!(rmp_serde::from_slice::<Event>(&body).unwrap(), event);
        }
    }

    mod sessions {
        use beep_proto::node::agent::*;
        use serde::{Deserialize, Serialize};

        /// What a node from before moves reads on `agents/1`.
        #[derive(Debug, PartialEq, Deserialize)]
        enum BeforeMoves {
            List {},
        }

        #[test]
        fn the_move_request_is_new_and_a_node_before_it_refuses_it() {
            let asked = SessionsRequest::MoveProgress {
                session: "s1".into(),
            };
            let bytes = rmp_serde::to_vec(&asked).expect("encodes");
            // Fixture: a map of one entry, the variant's name, its fields in a
            // list. It never changes once released.
            let mut fixture = vec![0x81, 0xac];
            fixture.extend_from_slice(b"MoveProgress");
            fixture.extend_from_slice(&[0x91, 0xa2, b's', b'1']);
            assert_eq!(bytes, fixture);
            assert_eq!(
                beep_proto::lenient::from_slice::<SessionsRequest>(&fixture).expect("decodes"),
                asked
            );
            // The older node cannot read it: its stream closes.
            assert!(rmp_serde::from_slice::<BeforeMoves>(&bytes).is_err());
            // What it did read still reads.
            let list = rmp_serde::to_vec(&SessionsRequest::List {}).expect("encodes");
            assert_eq!(
                rmp_serde::from_slice::<BeforeMoves>(&list).expect("decodes"),
                BeforeMoves::List {}
            );
        }

        #[test]
        fn the_move_reply_round_trips() {
            let reply = SessionsReply::Moving {
                progress: Some(MoveProgress {
                    phase: MovePhase::Files,
                    copied: 6,
                    total: 6,
                    files_left: 12,
                    source_away: false,
                    reason: None,
                }),
            };
            let bytes = rmp_serde::to_vec(&reply).expect("encodes");
            assert_eq!(
                beep_proto::lenient::from_slice::<SessionsReply>(&bytes).expect("decodes"),
                reply
            );
        }

        #[test]
        fn only_one_shot_reads_may_be_asked_again() {
            let folder = SessionsRequest::Folder {
                root: "/srv/project".into(),
                path: "src".into(),
                ignored: false,
                depth: 3,
            };
            let read = SessionsRequest::ReadFile {
                root: "/srv/project".into(),
                path: "notes.txt".into(),
                offset: 0,
                len: 10,
            };
            assert!(folder.is_read());
            assert!(read.is_read());
            assert!(SessionsRequest::List {}.is_read());
            let write = SessionsRequest::WriteFile {
                root: "/srv/project".into(),
                path: "notes.txt".into(),
                offset: 0,
                size: 2,
                data: b"hi".to_vec(),
            };
            assert!(!write.is_read());
            assert!(
                !SessionsRequest::UpdateHarness {
                    agent: "claude".into()
                }
                .is_read()
            );
            assert!(!SessionsRequest::FollowAlerts {}.is_read());
            assert!(!SessionsRequest::Watch { sessions: vec![] }.is_read());
        }

        #[test]
        fn worktrees_round_trip_and_may_be_asked_again() {
            let ask = SessionsRequest::Worktrees {
                folder: "/srv/project".into(),
            };
            assert!(ask.is_read());
            let body = rmp_serde::to_vec(&ask).unwrap();
            assert_eq!(
                rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
                ask
            );
            let reply = SessionsReply::Worktrees {
                worktrees: vec![Worktree {
                    path: "/srv/project/.worktrees/fix".into(),
                    branch: None,
                    head: "abc1234".into(),
                    own: true,
                }],
            };
            let body = rmp_serde::to_vec(&reply).unwrap();
            assert_eq!(
                rmp_serde::from_slice::<SessionsReply>(&body).unwrap(),
                reply
            );
        }

        #[test]
        fn commands_round_trip_and_may_be_asked_again() {
            let ask = SessionsRequest::Commands {
                session: "a3f9c1".into(),
            };
            assert!(ask.is_read());
            let body = rmp_serde::to_vec(&ask).unwrap();
            assert_eq!(
                rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
                ask
            );
            let reply = SessionsReply::Commands {
                commands: vec![AgentCommand {
                    name: "review".into(),
                    description: "Review the changes".into(),
                    hint: "[number]".into(),
                }],
            };
            let body = rmp_serde::to_vec(&reply).unwrap();
            assert_eq!(
                rmp_serde::from_slice::<SessionsReply>(&body).unwrap(),
                reply
            );
        }

        #[test]
        fn a_command_without_a_hint_reads_with_an_empty_one() {
            #[derive(Serialize)]
            struct Older {
                name: String,
                description: String,
            }
            let body = rmp_serde::to_vec(&Older {
                name: "compact".into(),
                description: "Free up context".into(),
            })
            .unwrap();
            let read: AgentCommand = rmp_serde::from_slice(&body).unwrap();
            assert_eq!(read.name, "compact");
            assert_eq!(read.hint, "");
        }

        #[test]
        fn follow_usage_reads_across_versions() {
            #[derive(Serialize, Deserialize)]
            enum Older {
                FollowUsage {},
            }
            let body = rmp_serde::to_vec_named(&Older::FollowUsage {}).unwrap();
            assert_eq!(
                rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
                SessionsRequest::FollowUsage { history: false }
            );
            let body =
                rmp_serde::to_vec_named(&SessionsRequest::FollowUsage { history: true }).unwrap();
            assert!(matches!(
                rmp_serde::from_slice::<Older>(&body).unwrap(),
                Older::FollowUsage {}
            ));
        }

        #[test]
        fn external_requests_and_replies_round_trip() {
            let request = SessionsRequest::External {
                agent: "claude".into(),
                limit: 20,
                session: None,
            };
            let body = rmp_serde::to_vec_named(&request).unwrap();
            assert_eq!(
                rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
                request
            );
            let reply = SessionsReply::Activity {
                activity: Activity {
                    session: "0f3c2a9e-1111-4222-8333-944455556666".into(),
                    kind: ActivityKind::NeedsInput,
                    message: "Claude needs your permission to use Bash".into(),
                    from_hook: true,
                },
            };
            let body = rmp_serde::to_vec_named(&reply).unwrap();
            assert_eq!(
                rmp_serde::from_slice::<SessionsReply>(&body).unwrap(),
                reply
            );
        }

        #[test]
        fn model_requests_and_replies_round_trip() {
            let request = SessionsRequest::Models {
                agent: "flsh".into(),
            };
            let body = rmp_serde::to_vec_named(&request).unwrap();
            assert_eq!(
                rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
                request
            );
            let reply = SessionsReply::Models {
                models: ModelList {
                    pick: ModelPick::Listed,
                    current: Some("openai:gpt-test".into()),
                    models: vec![ModelOption {
                        id: "openai:gpt-test".into(),
                        name: "GPT Test".into(),
                        description: None,
                    }],
                },
            };
            let body = rmp_serde::to_vec_named(&reply).unwrap();
            assert_eq!(
                rmp_serde::from_slice::<SessionsReply>(&body).unwrap(),
                reply
            );
            let start = Request::Start {
                cwd: None,
                prompt: "go".into(),
                model: Some("openai:gpt-test".into()),
                effort: Some(Effort::Xhigh),
                checkout: Some(CheckoutPlan::NewWorktree {
                    base: Some("main".into()),
                }),
            };
            let body = rmp_serde::to_vec_named(&start).unwrap();
            assert_eq!(rmp_serde::from_slice::<Request>(&body).unwrap(), start);
        }

        #[test]
        fn a_transcript_ask_before_an_entry_reads_on_a_node_that_does_not_know_before() {
            let ask = SessionsRequest::Transcript {
                session: "s".into(),
                last: 50,
                thoughts: true,
                before: Some(120),
            };
            // A node from before `before`, in both encodings its link uses.
            #[derive(Debug, PartialEq, Serialize, Deserialize)]
            enum Older {
                Transcript {
                    session: String,
                    last: u32,
                    thoughts: bool,
                },
            }
            let older = Older::Transcript {
                session: "s".into(),
                last: 50,
                thoughts: true,
            };
            let named = rmp_serde::to_vec_named(&ask).unwrap();
            assert_eq!(rmp_serde::from_slice::<Older>(&named).unwrap(), older);
            let compact = rmp_serde::to_vec(&ask).unwrap();
            assert_eq!(
                beep_proto::lenient::from_slice::<Older>(&compact).unwrap(),
                older
            );
            assert_eq!(
                rmp_serde::from_slice::<SessionsRequest>(&named).unwrap(),
                ask
            );
            assert_eq!(
                rmp_serde::from_slice::<SessionsRequest>(&compact).unwrap(),
                ask
            );
        }

        #[test]
        fn a_transcript_ask_from_before_before_reads_here_and_a_field_after_it_is_skipped() {
            #[derive(Debug, PartialEq, Serialize, Deserialize)]
            enum Older {
                Transcript {
                    session: String,
                    last: u32,
                    thoughts: bool,
                },
            }
            let old = rmp_serde::to_vec(&Older::Transcript {
                session: "s".into(),
                last: 5,
                thoughts: true,
            })
            .unwrap();
            assert_eq!(
                beep_proto::lenient::from_slice::<SessionsRequest>(&old).unwrap(),
                SessionsRequest::Transcript {
                    session: "s".into(),
                    last: 5,
                    thoughts: true,
                    before: None,
                }
            );

            // A newer app's field after `before` is skipped by the lenient reader.
            #[derive(Serialize)]
            enum Newer {
                Transcript {
                    session: String,
                    last: u32,
                    thoughts: bool,
                    before: Option<u64>,
                    later: u8,
                },
            }
            let newer = rmp_serde::to_vec(&Newer::Transcript {
                session: "s".into(),
                last: 5,
                thoughts: false,
                before: Some(9),
                later: 1,
            })
            .unwrap();
            assert_eq!(
                beep_proto::lenient::from_slice::<SessionsRequest>(&newer).unwrap(),
                SessionsRequest::Transcript {
                    session: "s".into(),
                    last: 5,
                    thoughts: false,
                    before: Some(9),
                }
            );
        }

        #[test]
        fn a_transcript_ask_without_thoughts_reads_here_and_a_thought_reads_as_other_there() {
            #[derive(Debug, PartialEq, Serialize, Deserialize)]
            enum Older {
                Transcript { session: String, last: u32 },
            }
            let body = rmp_serde::to_vec_named(&Older::Transcript {
                session: "s".into(),
                last: 5,
            })
            .unwrap();
            assert_eq!(
                rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
                SessionsRequest::Transcript {
                    session: "s".into(),
                    last: 5,
                    thoughts: false,
                    before: None,
                }
            );

            #[derive(Debug, PartialEq, Serialize, Deserialize)]
            enum OlderKind {
                User,
                #[serde(other)]
                Other,
            }
            #[derive(Debug, PartialEq, Serialize, Deserialize)]
            struct OlderEntry {
                kind: OlderKind,
                text: String,
            }
            let thought = Entry {
                kind: EntryKind::Thought,
                text: "hmm".into(),
                ms: Some(1200),
            };
            let body = rmp_serde::to_vec_named(&thought).unwrap();
            assert_eq!(rmp_serde::from_slice::<Entry>(&body).unwrap(), thought);
            assert_eq!(
                rmp_serde::from_slice::<OlderEntry>(&body).unwrap(),
                OlderEntry {
                    kind: OlderKind::Other,
                    text: "hmm".into()
                }
            );
            let body = rmp_serde::to_vec_named(&OlderEntry {
                kind: OlderKind::User,
                text: "hi".into(),
            })
            .unwrap();
            assert_eq!(
                rmp_serde::from_slice::<Entry>(&body).unwrap(),
                Entry::new(EntryKind::User, "hi".into())
            );
        }

        #[test]
        fn protocols_name_their_agent() {
            assert_eq!(protocol("claude"), "claude/1");
            assert_eq!(agent_of("codex/1"), Some("codex"));
            assert_eq!(agent_of("shell/1"), None);
            assert_eq!(agent_of("files/1"), None);
            assert_eq!(agent_of("process/1"), None);
            assert_eq!(agent_of(beep_proto::node::sync::PROTOCOL), None);
            assert_eq!(agent_of(beep_proto::node::terminal::PROTOCOL), None);
            assert_eq!(agent_of(beep_proto::node::transfer::PROTOCOL), None);
            assert_eq!(agent_of(beep_proto::node::tool_update::PROTOCOL), None);
            assert_eq!(agent_of(beep_proto::node::update::PROTOCOL), None);
            assert_eq!(agent_of(SESSIONS_PROTOCOL), None);
            assert_eq!(agent_of("claude/2"), None);
        }
    }
}
