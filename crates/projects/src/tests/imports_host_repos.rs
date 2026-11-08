use super::*;
use std::time::{Duration, UNIX_EPOCH};

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

/// A shell whose command always fails with `why`.
struct Broken(&'static str);

#[async_trait::async_trait]
impl Shell for Broken {
    async fn run(&self, _: &str, _: Duration) -> Result<crate::ShellOutput> {
        Err(anyhow!(self.0))
    }
}

#[tokio::test]
async fn a_list_no_machine_could_read_says_why_for_each() {
    let shells: Vec<Arc<dyn Shell>> = vec![
        Arc::new(Broken("gh is not signed in")),
        Arc::new(Broken("gh is not installed")),
    ];
    let hosts = read_hosts(&shells, &[], &[]).await;
    let said = hosts[0].error.clone().expect("both machines fail");
    assert!(
        said.contains("not signed in") && said.contains("not installed"),
        "{said}"
    );
}

/// A machine whose teacli is set up for the servers in `config`, and lists one repository
/// for any of them. What it ran is kept.
struct Tea {
    config: &'static str,
    ran: std::sync::Mutex<Vec<String>>,
}

impl Tea {
    fn new(config: &'static str) -> Arc<Self> {
        Arc::new(Self {
            config,
            ran: Default::default(),
        })
    }

    fn ran(&self) -> Vec<String> {
        self.ran.lock().expect("not poisoned").clone()
    }
}

#[async_trait::async_trait]
impl Shell for Tea {
    async fn run(&self, script: &str, _: Duration) -> Result<crate::ShellOutput> {
        self.ran
            .lock()
            .expect("not poisoned")
            .push(script.to_owned());
        let listed = "    \"full_name\": \"team/infra\",\n    \"archived\": false,\n    \"updated_at\": \"2026-01-01T00:00:00Z\",";
        Ok(crate::ShellOutput::exited(
            0,
            if script == TEA_SERVERS {
                self.config
            } else {
                listed
            },
        ))
    }
}

#[tokio::test]
async fn a_forgejo_list_runs_only_where_teacli_has_that_server_by_its_name() {
    let other = Tea::new(r#"[{"name":"work","url":"https://git.other.example","default":true}]"#);
    let has = Tea::new(r#"[{"name":"home","url":"https://GIT.example.com/","default":false}]"#);
    let tea: Vec<Arc<dyn Shell>> = vec![other.clone(), has.clone()];
    let server: ForgejoUrl = "https://git.example.com".parse().expect("a server URL");
    let hosts = read_hosts(&[], &tea, std::slice::from_ref(&server)).await;
    assert_eq!(hosts[0].error, None);
    assert_eq!(hosts[0].repos.len(), 1);
    assert_eq!(other.ran(), vec![TEA_SERVERS.to_owned()]);
    let ran = has.ran();
    assert_eq!(ran.len(), 2, "{ran:?}");
    assert!(ran[1].contains("--host home "), "{}", ran[1]);

    let none: Vec<Arc<dyn Shell>> = vec![Tea::new("[]")];
    let hosts = read_hosts(&[], &none, &[server]).await;
    assert_eq!(
        hosts[0].error.as_deref(),
        Some("No machine's teacli is signed in to git.example.com.")
    );
}

#[test]
fn gh_and_teacli_lists_parse_newest_first_without_archived_repos() {
    let gh = "me/tool\t2026-01-01T00:00:00Z\nacme/app\t2026-01-01T00:00:10Z\n\n";
    assert_eq!(
        parse_gh(gh).expect("gh output parses"),
        vec![
            Listed {
                name: "acme/app".into(),
                updated_at: at(1_767_225_610)
            },
            Listed {
                name: "me/tool".into(),
                updated_at: at(1_767_225_600)
            }
        ]
    );
    assert!(parse_gh("not a repo line").is_err());
    let many: String = (0..GH_MOST + 5)
        .map(|n| format!("acme/r{n}\t2026-01-01T00:00:00Z\n"))
        .collect();
    assert_eq!(parse_gh(&many).expect("a long list parses").len(), GH_MOST);
    let tea = r#"    "full_name": "team/infra",
    "archived": false,
    "updated_at": "2026-01-01T01:00:10+01:00",
    "full_name": "team/old",
    "archived": true,
    "updated_at": "2026-01-01T00:00:00Z","#;
    assert_eq!(
        parse_teacli(tea).expect("teacli output parses"),
        vec![Listed {
            name: "team/infra".into(),
            updated_at: at(1_767_225_610)
        }]
    );
    assert!(parse_teacli("").expect("nothing parses").is_empty());
}
