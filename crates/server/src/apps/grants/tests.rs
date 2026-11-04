use super::*;
use crate::{
    apps::permissions::Approved,
    protocol::PermissionLevel,
    routines::cron::BeepCronStore,
    store::{AppPermissionRow, Store, TestDb},
};
use flashmind::{
    core::CancellationToken,
    cron::{CronRegistry, JobSchedule},
};
use std::{
    collections::HashSet,
    sync::atomic::{AtomicUsize, Ordering},
};

const REPLY: &str = "GMAIL_REPLY_TO_THREAD";
const THREAD: &str = "18c2a9f0d4e5b6a7";

fn reply_spec() -> &'static Grantable {
    grantable(&ToolSlug::new(REPLY)).expect("Gmail replies can be granted")
}

fn ana_in_thread() -> Constraints {
    let exact = json!({"thread_id": THREAD});
    Constraints::new(
        reply_spec(),
        &["Ana@Example.com".to_owned()],
        exact.as_object().expect("an object"),
    )
    .expect("a grant Gmail replies can have")
}

fn reply(to: &str) -> Value {
    json!({"thread_id": THREAD, "recipient_email": to, "message_body": "Thanks, see you then."})
}

#[test]
fn a_call_within_the_grant_matches() {
    let spec = reply_spec();
    let grant = ana_in_thread();
    assert!(grant.allow(spec, &reply("ana@example.com")));
    // Addresses compare without case, and a display name is fine.
    assert!(grant.allow(spec, &reply("ANA@example.COM")));
    assert!(grant.allow(spec, &reply("Ana <ana@example.com>")));
    // Formatting and the user's own mailbox are not widening.
    let mut args = reply("ana@example.com");
    args["is_html"] = json!(false);
    args["user_id"] = json!("me");
    args["cc"] = json!([]);
    assert!(grant.allow(spec, &args));
}

#[test]
fn a_call_outside_the_grant_does_not_match() {
    let spec = reply_spec();
    let grant = ana_in_thread();
    let outside = |change: &dyn Fn(&mut Value)| {
        let mut args = reply("ana@example.com");
        change(&mut args);
        !grant.allow(spec, &args)
    };
    assert!(outside(
        &|args| args["recipient_email"] = json!("sam@example.com")
    ));
    assert!(outside(&|args| args["thread_id"] = json!("other-thread")));
    assert!(outside(&|args| {
        args.as_object_mut().expect("an object").remove("thread_id");
    }));
    assert!(outside(&|args| {
        args.as_object_mut()
            .expect("an object")
            .remove("recipient_email");
    }));
    assert!(outside(&|args| args["user_id"] = json!("boss@example.com")));
    assert!(!grant.allow(spec, &json!("not an object")));
}

#[test]
fn arguments_cannot_widen_a_grant() {
    let spec = reply_spec();
    let grant = ana_in_thread();
    let widened = |key: &str, value: Value| {
        let mut args = reply("ana@example.com");
        args[key] = value;
        !grant.allow(spec, &args)
    };
    // More recipients, in any field or form.
    assert!(widened("bcc", json!(["sam@example.com"])));
    assert!(widened("cc", json!(["ana@example.com", "sam@example.com"])));
    assert!(widened("extra_recipients", json!(["sam@example.com"])));
    assert!(widened(
        "recipient_email",
        json!("ana@example.com, sam@example.com")
    ));
    assert!(widened(
        "recipient_email",
        json!("ana@example.com;sam@example.com")
    ));
    assert!(widened(
        "recipient_email",
        json!("ana@example.com <sam@example.com>")
    ));
    assert!(widened(
        "recipient_email",
        json!("Ana <ana@example.com> sam@example.com")
    ));
    assert!(widened("recipient_email", json!(["sam@example.com"])));
    // Arguments the grant says nothing about.
    assert!(widened(
        "attachment",
        json!({"name": "notes.pdf", "s3key": "x"})
    ));
    assert!(widened("from_email", json!("other@example.com")));
    assert!(widened("Thread_Id", json!(THREAD)));
    // The same recipient again is still only Ana.
    let mut args = reply("ana@example.com");
    args["extra_recipients"] = json!(["ANA@example.com"]);
    assert!(grant.allow(spec, &args));
}

#[test]
fn a_grant_can_only_hold_what_its_tool_allows() {
    let spec = reply_spec();
    let ana = ["ana@example.com".to_owned()];
    let pin =
        |exact: Value| Constraints::new(spec, &ana, exact.as_object().expect("an object")).is_err();
    // A reply grant names its thread.
    assert!(pin(json!({})));
    // Recipients and text are not pinned, and only text pins.
    assert!(pin(json!({"thread_id": THREAD, "bcc": "sam@example.com"})));
    assert!(pin(json!({"thread_id": THREAD, "message_body": "hi"})));
    assert!(pin(json!({"thread_id": ""})));
    assert!(pin(json!({"thread_id": 7})));
    let thread = json!({"thread_id": THREAD});
    let thread = thread.as_object().expect("an object");
    assert!(Constraints::new(spec, &[], thread).is_err());
    assert!(Constraints::new(spec, &["Ana <ana@example.com>".to_owned()], thread).is_err());
    assert!(Constraints::new(spec, &["ana@example".to_owned()], thread).is_err());
    let saved = serde_json::to_value(ana_in_thread()).expect("constraints save");
    assert_eq!(
        saved,
        json!({"recipients": ["ana@example.com"], "pinned": {"thread_id": THREAD}})
    );
}

#[test]
fn fastmail_sends_new_mail_unless_a_reply_is_pinned() {
    let spec = grantable(&ToolSlug::new("FASTMAIL_SEND_EMAIL")).expect("grantable");
    let ana = ["ana@example.com".to_owned()];
    let new_mail = Constraints::new(spec, &ana, &Map::new()).expect("a grant");
    let to_ana = json!([{"name": "Ana", "email": "ana@example.com"}]);
    let send = json!({"to": to_ana, "subject": "Hi", "body": "Hello"});
    assert!(new_mail.allow(spec, &send));
    let mut forward = send.clone();
    forward["mode"] = json!("forward");
    forward["emailId"] = json!("M1");
    assert!(!new_mail.allow(spec, &forward));
    let mut reply_all = send.clone();
    reply_all["replyAll"] = json!(true);
    assert!(!new_mail.allow(spec, &reply_all));
    let tricky =
        json!({"to": [{"name": "sam@example.com", "email": "ana@example.com"}], "body": "x"});
    assert!(!new_mail.allow(spec, &tricky));
    // Without `to`, a reply goes to whoever wrote, which no grant names.
    assert!(!new_mail.allow(spec, &json!({"body": "Hello"})));
}

#[test]
fn every_grantable_tool_acts() {
    for spec in GRANTABLE {
        let (app, acts) = permissions::lookup(spec.tool).expect("a known tool");
        assert_ne!(acts, permissions::Acts::Never, "{} does not act", spec.tool);
        assert!(!app.name().is_empty());
        for key in spec.required {
            assert!(spec.pinnable.contains(key), "{} requires {key}", spec.tool);
        }
        for key in spec.primary {
            assert!(spec.recipients.contains(key), "{} names {key}", spec.tool);
        }
    }
}

/// Stands in for Gmail's reply tool, counting what it sends.
struct Reply(Arc<AtomicUsize>);

#[async_trait]
impl Tool for Reply {
    fn name(&self) -> &str {
        REPLY
    }
    fn description(&self) -> &str {
        "Reply in a thread."
    }
    fn parameters(&self) -> Value {
        json!({"type": "object"})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::success(
            ctx.tool_call_id,
            r#"{"successful": true}"#,
        ))
    }
}

struct Fixture {
    _db: TestDb,
    store: Store,
    user: Arc<User>,
    routine: Uuid,
    chat: Uuid,
    sent: Arc<AtomicUsize>,
}

async fn fixture() -> Fixture {
    let db = TestDb::new().await.expect("a test database");
    let store = db.store().await.expect("a store");
    let user = User::for_test(store.clone(), "alice");
    user.save_app_permissions(&AppPermissionRow {
        app: "gmail".to_owned(),
        level: PermissionLevel::Ask {},
        allowed: Vec::new(),
    })
    .await
    .expect("saved rules");
    let routines = CronRegistry::new(Arc::new(BeepCronStore::for_user(store.clone(), "alice")));
    let routine = routines
        .create_with_metadata(
            JobSchedule::Cron("*/15 * * * *".to_owned()),
            "When Ana replies in the lunch thread, reply back".to_owned(),
            false,
            json!({"user_id": "alice", "label": "Reply to Ana"}),
        )
        .await
        .expect("routine saved");
    Fixture {
        _db: db,
        store,
        user,
        routine: routine.id,
        chat: Uuid::new_v4(),
        sent: Arc::new(AtomicUsize::new(0)),
    }
}

impl Fixture {
    /// Asks for the grant, checks the card, and allows it.
    async fn grant(&self) -> Uuid {
        let cancel = CancellationToken::new();
        let args = json!({
            "routine": self.routine.to_string(),
            "tool": REPLY,
            "recipients": ["ana@example.com"],
            "exact": {"thread_id": THREAD},
        });
        let before = self.user.live_grants().await.expect("grants").len();
        let asking = RequestGrantTool {
            user: self.user.clone(),
            approved: None,
        };
        let asked = asking
            .execute(ToolContext::new("ask", args.clone(), None, &cancel))
            .await
            .expect("the tool runs");
        let payload = asked.payload().expect("the turn stops on a card");
        let card = &payload
            .as_any()
            .downcast_ref::<ApprovalPayload>()
            .expect("an approval card")
            .approval;
        let shown: Vec<(&str, &str)> = card
            .fields
            .iter()
            .map(|field| (field.label.as_str(), field.value.as_str()))
            .collect();
        assert_eq!(card.app, "Gmail");
        assert!(shown.contains(&("Routine", "Reply to Ana")), "{shown:?}");
        assert!(shown.contains(&("To", "ana@example.com only")), "{shown:?}");
        assert!(shown.contains(&("Only in", "this thread")), "{shown:?}");
        assert!(
            shown.contains(&("Allows", "1 reply each time it runs, without asking")),
            "{shown:?}"
        );
        assert!(
            shown.contains(&("Ends", "When the routine ends, or in 8 hours")),
            "{shown:?}"
        );
        // Asking saves nothing.
        let after = self.user.live_grants().await.expect("grants").len();
        assert_eq!(after, before);

        let allowed = RequestGrantTool {
            user: self.user.clone(),
            approved: Some("ask".to_owned()),
        };
        let saved = allowed
            .execute(ToolContext::new("ask", args, None, &cancel))
            .await
            .expect("the tool runs");
        assert!(saved.is_success(), "{}", saved.output());
        let grants = self.user.live_grants().await.expect("grants");
        assert_eq!(grants.len(), 1);
        let lasts = grants[0].expires_at - Utc::now();
        assert!(
            lasts > Duration::hours(7) && lasts <= Duration::hours(8),
            "a grant lasts 8 hours by default: {lasts}"
        );
        grants[0].id
    }

    fn run(&self, run: &str) -> Arc<RoutineGrants> {
        Arc::new(RoutineGrants::new(
            self.user.clone(),
            self.routine,
            run,
            self.chat,
        ))
    }

    /// Calls the guarded reply tool; true when it ran without asking.
    async fn call(&self, grants: &Arc<RoutineGrants>, call_id: &str, args: Value) -> bool {
        let rules = self.user.app_rules().await.expect("rules");
        let tool = permissions::guard(
            vec![Arc::new(Reply(self.sent.clone()))],
            rules,
            Approved(None),
            &HashSet::new(),
            Some(grants.clone()),
        )
        .pop()
        .expect("the reply tool");
        let before = self.sent.load(Ordering::SeqCst);
        let cancel = CancellationToken::new();
        let result = tool
            .execute(ToolContext::new(call_id, args, None, &cancel))
            .await
            .expect("the tool runs");
        let ran = self.sent.load(Ordering::SeqCst) > before;
        assert_eq!(ran, !result.is_interrupt(), "{}", result.output());
        ran
    }
}

#[tokio::test]
async fn a_grant_lets_one_call_through_per_run() {
    let fixture = fixture().await;
    fixture.grant().await;
    let mut notices = fixture.user.notifications.subscribe();

    let first = fixture.run("run-1");
    assert!(fixture.call(&first, "c1", reply("ana@example.com")).await);
    let notice = notices.try_recv().expect("the user is told").notice;
    assert_eq!(notice.session_id, fixture.chat.to_string());
    assert!(
        notice.body.contains("Thanks, see you then."),
        "{}",
        notice.body
    );
    assert!(notice.body.contains("ana@example.com"), "{}", notice.body);
    let used = first.take_used();
    assert_eq!(used.len(), 1);
    assert!(used[0].text.contains("Thanks, see you then."));

    // A second call in the same run asks.
    assert!(!fixture.call(&first, "c2", reply("ana@example.com")).await);
    assert!(first.take_used().is_empty());

    // The next run gets its own call.
    let second = fixture.run("run-2");
    assert!(fixture.call(&second, "c3", reply("ana@example.com")).await);
    let listed = listed(fixture.user.live_grants().await.expect("grants").remove(0));
    assert!(listed.last_used_at.is_some());
    assert_eq!(listed.app, "Gmail");
}

#[tokio::test]
async fn a_call_outside_the_grant_asks() {
    let fixture = fixture().await;
    fixture.grant().await;
    let run = fixture.run("run-1");
    assert!(!fixture.call(&run, "c1", reply("sam@example.com")).await);
    let mut other_thread = reply("ana@example.com");
    other_thread["thread_id"] = json!("other-thread");
    assert!(!fixture.call(&run, "c2", other_thread).await);
    let mut bcc = reply("ana@example.com");
    bcc["bcc"] = json!(["sam@example.com"]);
    assert!(!fixture.call(&run, "c3", bcc).await);
    // None of those used the run's call.
    assert!(fixture.call(&run, "c4", reply("ana@example.com")).await);
}

#[tokio::test]
async fn another_routine_or_user_has_no_grant() {
    let fixture = fixture().await;
    fixture.grant().await;
    let other_routine = Arc::new(RoutineGrants::new(
        fixture.user.clone(),
        Uuid::new_v4(),
        "run-1",
        fixture.chat,
    ));
    assert!(
        !fixture
            .call(&other_routine, "c1", reply("ana@example.com"))
            .await
    );
    let bob = User::for_test(fixture.store.clone(), "bob");
    let bobs = RoutineGrants::new(bob, fixture.routine, "run-1", fixture.chat);
    assert!(
        bobs.claim(REPLY, &reply("ana@example.com"), "c2")
            .await
            .is_none()
    );
}

#[tokio::test]
async fn an_expired_grant_asks() {
    let fixture = fixture().await;
    let id = fixture.grant().await;
    fixture.store.expire_grant(id).await.expect("expired");
    assert!(
        !fixture
            .call(&fixture.run("run-1"), "c1", reply("ana@example.com"))
            .await
    );
    assert!(fixture.user.live_grants().await.expect("grants").is_empty());
}

#[tokio::test]
async fn a_grant_long_ended_is_pruned_with_its_uses() {
    let fixture = fixture().await;
    let id = fixture.grant().await;
    assert!(
        fixture
            .call(&fixture.run("run-1"), "c1", reply("ana@example.com"))
            .await
    );
    // A live grant keeps its last use.
    fixture.store.prune_grants().await.expect("pruned");
    assert_eq!(fixture.store.grant_rows(id).await.expect("rows"), (true, 1));
    fixture.store.age_grant(id).await.expect("aged");
    fixture.store.prune_grants().await.expect("pruned");
    assert_eq!(
        fixture.store.grant_rows(id).await.expect("rows"),
        (false, 0)
    );
}

#[tokio::test]
async fn a_revoked_grant_asks() {
    let fixture = fixture().await;
    let id = fixture.grant().await;
    assert!(fixture.user.revoke_grant(id).await.expect("revoked"));
    assert!(!fixture.user.revoke_grant(id).await.expect("runs"));
    assert!(
        !fixture
            .call(&fixture.run("run-1"), "c1", reply("ana@example.com"))
            .await
    );
    assert!(fixture.user.live_grants().await.expect("grants").is_empty());
}

#[tokio::test]
async fn a_grant_ends_with_its_routine() {
    let fixture = fixture().await;
    fixture.grant().await;
    let ended =
        crate::routines::cron::EndRoutineTool::new(fixture.store.clone(), "alice", fixture.routine);
    let cancel = CancellationToken::new();
    ended
        .execute(ToolContext::new("end", json!({}), None, &cancel))
        .await
        .expect("the routine ends");
    assert!(
        !fixture
            .call(&fixture.run("run-1"), "c1", reply("ana@example.com"))
            .await
    );
    assert!(fixture.user.live_grants().await.expect("grants").is_empty());
}

#[tokio::test]
async fn answering_twice_saves_one_grant() {
    let fixture = fixture().await;
    fixture.grant().await;
    fixture.grant().await;
}

#[tokio::test]
async fn a_request_outside_what_can_be_granted_is_refused() {
    let fixture = fixture().await;
    let cancel = CancellationToken::new();
    let tool = RequestGrantTool {
        user: fixture.user.clone(),
        approved: Some("ask".to_owned()),
    };
    let refused = |args: Value| {
        let tool = &tool;
        let cancel = &cancel;
        async move {
            let result = tool
                .execute(ToolContext::new("ask", args, None, cancel))
                .await
                .expect("the tool runs");
            !result.is_success() && !result.is_interrupt()
        }
    };
    let routine = fixture.routine.to_string();
    assert!(refused(json!({"routine": routine, "tool": "GMAIL_MOVE_TO_TRASH", "recipients": ["ana@example.com"]})).await);
    assert!(
        refused(json!({"routine": Uuid::new_v4().to_string(), "tool": REPLY,
        "recipients": ["ana@example.com"], "exact": {"thread_id": THREAD}}))
        .await
    );
    assert!(
        refused(json!({"routine": routine, "tool": REPLY, "recipients": ["ana@example.com"]}))
            .await
    );
    assert!(
        refused(
            json!({"routine": routine, "tool": REPLY, "recipients": ["ana@example.com"],
        "exact": {"thread_id": THREAD}, "per_run": 50})
        )
        .await
    );
    assert!(
        refused(
            json!({"routine": routine, "tool": REPLY, "recipients": ["ana@example.com"],
        "exact": {"thread_id": THREAD}, "hours": MAX_HOURS + 1})
        )
        .await
    );
    assert!(fixture.user.live_grants().await.expect("grants").is_empty());
}
