use super::*;
use crate::{
    Shell, ShellOutput,
    issues::{ImportedPlace, MachineName},
};
use async_trait::async_trait;
use iroh::{EndpointId, SecretKey};
use std::{str::FromStr, sync::Arc, sync::Mutex, time::Duration};

fn repo() -> RepoName {
    RepoName::from_str("acme/widget").expect("a repo")
}

fn github() -> IssueHost {
    IssueHost::Github
}

fn forgejo() -> IssueHost {
    IssueHost::of_hostname("forge.example").expect("a Forgejo host")
}

fn login(name: &str) -> ForgeLogin {
    name.parse().expect("a login")
}

fn words(call: &ForgeCall) -> Vec<&str> {
    call.args.iter().map(String::as_str).collect()
}

#[test]
fn forgejo_assign_keeps_existing() {
    let merged = merged_assignees(
        &["kim".to_owned(), "sam".to_owned()],
        &[login("Kim"), login("alice")],
    );

    assert_eq!(merged, ["kim", "sam", "alice"]);
}

#[test]
fn calls_use_the_gh_and_teacli_api_shapes() {
    let who = login("alice");
    let gh = collaborator_call(&github(), &repo(), &who);
    assert_eq!(gh.program, "gh");
    assert_eq!(words(&gh), ["api", "repos/acme/widget/collaborators/alice"]);

    let tea = collaborator_call(&forgejo(), &repo(), &who);
    assert_eq!(tea.program, "teacli");
    assert_eq!(
        words(&tea),
        [
            "api",
            "repos/acme/widget/collaborators/alice",
            "--host",
            "forge.example"
        ]
    );

    assert_eq!(words(&user_call(&github())), ["api", "user"]);
    assert_eq!(
        words(&issue_call(&forgejo(), &repo(), 7)),
        [
            "api",
            "repos/acme/widget/issues/7",
            "--host",
            "forge.example"
        ]
    );
}

#[test]
fn assign_posts_on_github_and_patches_on_forgejo() {
    let logins = ["kim".to_owned(), "alice".to_owned()];

    assert_eq!(
        words(&assign_call(&github(), &repo(), 7, &logins)),
        [
            "api",
            "repos/acme/widget/issues/7/assignees",
            "-X",
            "POST",
            "-f",
            "assignees[]=kim",
            "-f",
            "assignees[]=alice"
        ]
    );
    assert_eq!(
        words(&assign_call(&forgejo(), &repo(), 7, &logins)),
        [
            "api",
            "repos/acme/widget/issues/7",
            "--host",
            "forge.example",
            "-X",
            "PATCH",
            "--raw-field",
            r#"assignees=["kim","alice"]"#
        ]
    );
}

#[test]
fn assigned_issues_ask_each_host_its_own_filter() {
    let who = login("alice");

    assert!(words(&assigned_issues_call(&github(), &repo(), &who))[1].ends_with("&assignee=alice"));
    assert!(
        words(&assigned_issues_call(&forgejo(), &repo(), &who))[1].ends_with("&assigned_by=alice")
    );
}

#[test]
fn a_404_says_no_and_other_failures_are_errors() {
    assert!(collaborator_answer(true, "", "").expect("204"));
    assert!(!collaborator_answer(false, "", "gh: Not Found (HTTP 404)").expect("404"));
    assert!(collaborator_answer(false, "", "sh: 1: gh: not found").is_err());
    assert!(collaborator_answer(false, "", "teacli: not found").is_err());
    assert!(collaborator_answer(false, "", "gh: Bad credentials (HTTP 401)").is_err());
}

#[test]
fn logins_and_assignees_are_read() {
    assert_eq!(parse_user_login("alice\n").expect("bare"), login("alice"));
    assert_eq!(
        parse_user_login(r#"{"login":"kim","id":3}"#).expect("json"),
        login("kim")
    );
    assert!(parse_user_login("").is_err());
    assert_eq!(
        parse_assignees(r#"{"assignees":[{"login":"kim"},{"login":"sam"}]}"#).expect("two"),
        ["kim", "sam"]
    );
    assert!(
        parse_assignees(r#"{"assignees":null}"#)
            .expect("none")
            .is_empty()
    );
}

#[test]
fn rendering_quotes_every_word() {
    let call = assign_call(&forgejo(), &repo(), 7, &["kim".to_owned()]);

    assert_eq!(
        call.render(&repo()),
        "teacli api repos/acme/widget/issues/7 --host forge.example -X PATCH --raw-field 'assignees=[\"kim\"]'"
    );
}

struct Store(Vec<ImportedPlace>);

#[async_trait]
impl ProjectStore for Store {
    fn name(&self) -> &RepoName {
        static NAME: std::sync::OnceLock<RepoName> = std::sync::OnceLock::new();
        NAME.get_or_init(repo)
    }

    async fn imports(&self) -> Result<Vec<ImportedPlace>> {
        Ok(self.0.clone())
    }
}

/// Answers by the first rule whose text the command holds, and records the commands.
struct Rules {
    rules: Vec<(&'static str, i32, &'static str)>,
    ran: Mutex<Vec<String>>,
}

#[async_trait]
impl Shell for Rules {
    async fn run(&self, script: &str, _: Duration) -> Result<ShellOutput> {
        self.ran
            .lock()
            .expect("not poisoned")
            .push(script.to_owned());
        let (_, code, out) = self
            .rules
            .iter()
            .find(|(holds, ..)| script.contains(holds))
            .copied()
            .unwrap_or(("", 1, "no rule"));

        Ok(ShellOutput::exited(code, out))
    }
}

struct Fleet(Arc<Rules>);

#[async_trait]
impl Machines for Fleet {
    fn shell(&self, _: &EndpointId) -> Option<Arc<dyn Shell>> {
        Some(self.0.clone())
    }

    async fn names(&self) -> Vec<MachineName> {
        Vec::new()
    }
}

fn setup(url: &str, rules: Vec<(&'static str, i32, &'static str)>) -> (Store, Fleet, Arc<Rules>) {
    let shell = Arc::new(Rules {
        rules,
        ran: Mutex::new(Vec::new()),
    });
    let store = Store(vec![ImportedPlace {
        url: url.into(),
        machine: Some(SecretKey::from_bytes(&[1; 32]).public()),
    }]);

    (store, Fleet(shell.clone()), shell)
}

#[tokio::test]
async fn is_collaborator_maps_204_and_404() {
    let url = "https://github.com/acme/widget";
    let (store, fleet, _) = setup(url, vec![("collaborators/alice", 0, "")]);
    assert!(
        is_collaborator(&store, &fleet, &login("alice"))
            .await
            .expect("yes")
    );

    let (store, fleet, _) = setup(
        url,
        vec![("collaborators/bob", 1, "gh: Not Found (HTTP 404)")],
    );
    assert!(
        !is_collaborator(&store, &fleet, &login("bob"))
            .await
            .expect("no")
    );

    let (store, fleet, _) = setup(
        url,
        vec![("collaborators", 1, "gh: Bad credentials (HTTP 401)")],
    );
    assert!(
        is_collaborator(&store, &fleet, &login("bob"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn forgejo_assign_reads_current_then_patches_the_union() {
    let (store, fleet, shell) = setup(
        "https://forge.example/acme/widget",
        vec![
            ("PATCH", 0, "{}"),
            ("issues/7", 0, r#"{"assignees":[{"login":"sam"}]}"#),
        ],
    );
    assign(&store, &fleet, 7, &[login("kim")])
        .await
        .expect("assigned");

    let ran = shell.ran.lock().expect("not poisoned");
    assert_eq!(ran.len(), 2);
    assert!(
        ran[1].contains(r#"'assignees=["sam","kim"]'"#),
        "{}",
        ran[1]
    );
}

#[tokio::test]
async fn my_login_reads_the_user() {
    let (store, fleet, _) = setup(
        "https://github.com/acme/widget",
        vec![("api user", 0, r#"{"login":"kim"}"#)],
    );

    assert_eq!(my_login(&store, &fleet).await.expect("login"), login("kim"));
}
