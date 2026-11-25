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
    let gh = permission_call(&github(), &repo(), &who);
    assert_eq!(gh.program, "gh");
    assert_eq!(
        words(&gh),
        ["api", "repos/acme/widget/collaborators/alice/permission"]
    );

    let tea = permission_call(&forgejo(), &repo(), &who);
    assert_eq!(tea.program, "teacli");
    assert_eq!(
        words(&tea),
        [
            "api",
            "repos/acme/widget/collaborators/alice/permission",
            "--host",
            "forge.example"
        ]
    );
    assert_eq!(
        words(&collaborator_call(&github(), &repo(), &who)),
        ["api", "repos/acme/widget/collaborators/alice"]
    );

    assert_eq!(words(&user_call(&github())), ["api", "user"]);
    assert_eq!(
        words(&repo_call(&github(), &repo())),
        ["api", "repos/acme/widget"]
    );
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

    assert_eq!(
        words(&assigned_issues_call(&github(), &repo(), &who))[1],
        "repos/acme/widget/issues?state=open&per_page=100&assignee=alice"
    );
    assert!(
        words(&assigned_issues_call(&forgejo(), &repo(), &who))[1].ends_with("&assigned_by=alice")
    );
    assert_eq!(
        words(&assigned_issues_call(&github(), &repo(), &who))[2],
        "--paginate"
    );
    assert_eq!(
        words(&assigned_issues_call(&forgejo(), &repo(), &who))[4],
        "--paginate"
    );
}

#[test]
fn open_issues_leave_out_pull_requests_and_read_past_the_first_page() {
    let github = open_issues_call(&github(), &repo());
    assert_eq!(github.program, "gh");
    assert_eq!(
        words(&github)[..8],
        [
            "issue",
            "list",
            "-R",
            "acme/widget",
            "--state",
            "open",
            "--limit",
            "1000"
        ]
    );

    let forgejo = open_issues_call(&forgejo(), &repo());
    assert_eq!(
        words(&forgejo)[1],
        "repos/acme/widget/issues?state=open&type=issues&limit=50"
    );
    assert!(words(&forgejo).contains(&"--paginate"));
}

#[test]
fn assignment_events_are_read_from_each_hosts_own_list() {
    assert_eq!(
        words(&assignment_events_call(&github(), &repo(), 7))[1],
        "repos/acme/widget/issues/7/events?per_page=100"
    );
    assert_eq!(
        words(&assignment_events_call(&forgejo(), &repo(), 7))[1],
        "repos/acme/widget/issues/7/timeline?limit=50"
    );
}

#[test]
fn the_latest_assignment_says_whether_it_was_self_assigned() {
    let me = login("alice");
    let github = |actor: &str, assignee: &str| {
        format!(
            r#"{{"event":"assigned","actor":{{"login":"{actor}"}},"assignee":{{"login":"{assignee}"}}}}"#
        )
    };
    let by_me = format!("[{}]", github("Alice", "alice"));
    let by_bob = format!("[{}]", github("bob", "alice"));
    let other = format!("[{}]", github("alice", "bob"));
    let later = format!(
        "[{}]\n[{}]",
        github("alice", "alice"),
        github("bob", "alice")
    );

    assert!(parse_self_assigned(&by_me, &me).unwrap());
    assert!(!parse_self_assigned(&by_bob, &me).unwrap());
    assert!(!parse_self_assigned(&other, &me).unwrap());
    assert!(!parse_self_assigned(&later, &me).unwrap());
    assert!(!parse_self_assigned(r#"[{"event":"labeled"}]"#, &me).unwrap());
    assert!(parse_self_assigned("not json", &me).is_err());

    let forgejo = |user: &str, removed: bool| {
        format!(
            r#"{{"type":"assignees","user":{{"login":"{user}"}},"assignee":{{"login":"alice"}},"removed_assignee":{removed}}}"#
        )
    };
    let added = format!("[{}]", forgejo("alice", false));
    let removed_after = format!("[{},{}]", forgejo("bob", false), forgejo("alice", true));

    assert!(parse_self_assigned(&added, &me).unwrap());
    assert!(!parse_self_assigned(&removed_after, &me).unwrap());
    assert!(!parse_self_assigned("null", &me).unwrap());
}

#[test]
fn assigned_issues_read_every_page() {
    let page = |number: u64| {
        format!(
            r#"[{{"number":{number},"title":"t","created_at":"2026-10-30T08:00:00Z","assignees":[{{"login":"alice"}}]}}]"#
        )
    };
    let two = format!("{}\n{}", page(1), page(2));
    let issues = crate::commands::parse_open_issues(&two).expect("two pages");

    assert_eq!(
        issues.iter().map(|issue| issue.number).collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(
        crate::commands::parse_open_issues("[]null")
            .expect("empty")
            .is_empty()
    );
    assert!(crate::commands::parse_open_issues("").is_err());
}

/// However many others were updated since, a branch's pull request is listed.
#[test]
fn a_branchs_pulls_are_read_open_and_closed_from_any_fork() {
    let github = branch_pulls_call(&github(), &repo(), "issue-52-fix");
    assert_eq!(github.program, "gh");
    assert_eq!(
        words(&github)[..8],
        [
            "pr",
            "list",
            "-R",
            "acme/widget",
            "--head",
            "issue-52-fix",
            "--state",
            "all"
        ]
    );
    let forgejo = branch_pulls_call(&forgejo(), &repo(), "issue-52-fix");
    assert_eq!(
        words(&forgejo)[1],
        "repos/acme/widget/pulls?state=all&sort=recentupdate&limit=50"
    );
    assert!(words(&forgejo).contains(&"--paginate"));

    let pull = |n: u64, head: &str| {
        format!(
            r#"[{{"number":{n},"created_at":"2026-11-20T10:00:00Z","head":{{"ref":"{head}"}}}}]"#
        )
    };
    let pages = format!("{}\n{}null", pull(1, "other"), pull(2, "issue-52-fix"));
    let heads: Vec<String> = crate::commands::parse_open_prs(&pages)
        .expect("pages")
        .into_iter()
        .map(|pull| pull.head)
        .collect();
    assert_eq!(heads, ["other", "issue-52-fix"]);
}

#[test]
fn a_404_says_no_and_other_failures_are_errors() {
    // A read-only collaborator gets a 204 too: it does not say they may push.
    assert!(collaborator_answer(true, "").is_err());
    assert!(!collaborator_answer(false, "gh: Not Found (HTTP 404)").expect("404"));
    assert!(collaborator_answer(false, "sh: 1: gh: not found").is_err());
    assert!(collaborator_answer(false, "teacli: not found").is_err());
    assert!(collaborator_answer(false, "gh: Bad credentials (HTTP 401)").is_err());
}

#[test]
fn only_write_access_and_above_may_push() {
    let said = |reply: &str| permission_answer(true, reply, "").expect("read");

    assert_eq!(
        said(r#"{"permission":"admin","role_name":"admin"}"#),
        Some(true)
    );
    assert_eq!(
        said(r#"{"permission":"write","role_name":"maintain"}"#),
        Some(true)
    );
    assert_eq!(
        said(r#"{"permission":"write","role_name":"write"}"#),
        Some(true)
    );
    assert_eq!(
        said(r#"{"permission":"owner","role_name":"owner"}"#),
        Some(true)
    );
    // A collaborator downgraded to read, or triage, may no longer push.
    assert_eq!(
        said(r#"{"permission":"read","role_name":"read"}"#),
        Some(false)
    );
    assert_eq!(
        said(r#"{"permission":"read","role_name":"triage"}"#),
        Some(false)
    );
    assert_eq!(said(r#"{"permission":"none","role_name":""}"#), Some(false));

    assert_eq!(
        permission_answer(false, "", "gh: Not Found (HTTP 404)").expect("404"),
        Some(false)
    );
    // Forgejo tells only admins: the collaborator check says who may not.
    assert_eq!(
        permission_answer(false, "", "teacli: Forbidden (HTTP 403)").expect("403"),
        None
    );
    assert!(permission_answer(false, "", "sh: 1: gh: not found").is_err());
    assert!(permission_answer(false, "", "gh: Bad credentials (HTTP 401)").is_err());
    assert!(permission_answer(true, "not json", "").is_err());
}

#[test]
fn a_proof_is_stamped_and_its_stampers_read() {
    assert_eq!(
        words(&stamp_call(&github(), &repo(), "abc1", "beep/proof/n1")),
        [
            "api",
            "repos/acme/widget/statuses/abc1",
            "-X",
            "POST",
            "-f",
            "state=success",
            "-f",
            "context=beep/proof/n1"
        ]
    );
    assert_eq!(
        words(&stamps_call(&forgejo(), &repo(), "abc1")),
        [
            "api",
            "repos/acme/widget/commits/abc1/statuses",
            "--host",
            "forge.example"
        ]
    );

    let statuses = r#"[
        {"context":"beep/proof/n1","creator":{"login":"alice"}},
        {"context":"Beep/Proof/N1","creator":{"login":"bob"}},
        {"context":"beep/proof/n2","creator":{"login":"carol"}},
        {"context":"ci","creator":{"login":"dave"}},
        {"context":"beep/proof/n1","creator":null}
    ]"#;
    assert_eq!(
        parse_stampers(statuses, "beep/proof/n1").expect("read"),
        ["alice", "bob"]
    );
    assert_eq!(
        parse_stampers(statuses, "beep/proof/").expect("read"),
        ["alice", "bob", "carol"]
    );
    assert!(
        parse_stampers("null", "beep/proof/n1")
            .expect("none")
            .is_empty()
    );
    assert!(parse_stampers("{}", "beep/proof/n1").is_err());
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

type Rule = (&'static str, i32, &'static str);

/// Answers by the first rule whose text the command holds, used up when a later rule holds the
/// same text, and records the commands.
struct Rules {
    rules: Mutex<Vec<Rule>>,
    ran: Mutex<Vec<String>>,
}

#[async_trait]
impl Shell for Rules {
    async fn run(&self, script: &str, _: Duration) -> Result<ShellOutput> {
        self.ran
            .lock()
            .expect("not poisoned")
            .push(script.to_owned());
        let mut rules = self.rules.lock().expect("not poisoned");
        let first = rules.iter().position(|(holds, ..)| script.contains(holds));
        let (holds, code, out) = first.map_or(("", 1, "no rule"), |at| rules[at]);
        let again = rules.iter().filter(|(other, ..)| *other == holds).count() > 1;

        if let Some(at) = first
            && again
        {
            rules.remove(at);
        }

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

fn setup(url: &str, rules: Vec<Rule>) -> (Store, Fleet, Arc<Rules>) {
    let shell = Arc::new(Rules {
        rules: Mutex::new(rules),
        ran: Mutex::new(Vec::new()),
    });
    let store = Store(vec![ImportedPlace {
        url: url.into(),
        machine: Some(SecretKey::from_bytes(&[1; 32]).public()),
    }]);

    (store, Fleet(shell.clone()), shell)
}

#[tokio::test]
async fn forgejo_assign_reads_current_then_patches_the_union() {
    let (store, fleet, shell) = setup(
        "https://forge.example/acme/widget",
        vec![
            ("PATCH", 0, "{}"),
            ("issues/7", 0, r#"{"assignees":[{"login":"sam"}]}"#),
            (
                "issues/7",
                0,
                r#"{"assignees":[{"login":"sam"},{"login":"kim"}]}"#,
            ),
        ],
    );
    assign(&store, &fleet, 7, &[login("kim")])
        .await
        .expect("assigned");

    let ran = shell.ran.lock().expect("not poisoned");
    assert_eq!(ran.len(), 3);
    assert!(
        ran[1].contains(r#"'assignees=["sam","kim"]'"#),
        "{}",
        ran[1]
    );
}

#[tokio::test]
async fn forgejo_assign_lost_to_another_at_once_is_made_again() {
    // Bob's PATCH, read before Kim's went in, replaced it.
    let (store, fleet, shell) = setup(
        "https://forge.example/acme/widget",
        vec![
            ("PATCH", 0, "{}"),
            ("issues/7", 0, r#"{"assignees":[{"login":"sam"}]}"#),
            (
                "issues/7",
                0,
                r#"{"assignees":[{"login":"sam"},{"login":"bob"}]}"#,
            ),
            (
                "issues/7",
                0,
                r#"{"assignees":[{"login":"sam"},{"login":"bob"},{"login":"kim"}]}"#,
            ),
        ],
    );
    assign(&store, &fleet, 7, &[login("kim")])
        .await
        .expect("assigned");

    let ran = shell.ran.lock().expect("not poisoned");
    assert_eq!(ran.len(), 5);
    assert!(
        ran[3].contains(r#"'assignees=["sam","bob","kim"]'"#),
        "{}",
        ran[3]
    );
}

#[tokio::test]
async fn forgejo_assign_lost_twice_says_so() {
    let (store, fleet, _) = setup(
        "https://forge.example/acme/widget",
        vec![
            ("PATCH", 0, "{}"),
            ("issues/7", 0, r#"{"assignees":[{"login":"sam"}]}"#),
        ],
    );

    assert!(assign(&store, &fleet, 7, &[login("kim")]).await.is_err());
}

#[tokio::test]
async fn my_login_reads_the_user() {
    let (store, fleet, _) = setup(
        "https://github.com/acme/widget",
        vec![("api user", 0, r#"{"login":"kim"}"#)],
    );

    assert_eq!(my_login(&store, &fleet).await.expect("login"), login("kim"));
}
