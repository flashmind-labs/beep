use super::*;
use std::sync::Arc;

/// What codex-cli 0.166 prints, colours and all.
const DEVICE_AUTH: &str = "\n\u{1b}[1mWelcome to Codex\u{1b}[0m [v\u{1b}[90m0.166.0\u{1b}[0m]\n\
    \u{1b}[90mOpenAI's command-line coding agent\u{1b}[0m\n\n\
    Follow these steps to sign in with ChatGPT using device code authorization:\n\n\
    1. Open this link in your browser and sign in to your account\n   \
    \u{1b}[94mhttps://auth.openai.com/codex/device\u{1b}[0m\n\n\
    2. Enter this one-time code \u{1b}[90m(expires in 15 minutes)\u{1b}[0m\n   \
    \u{1b}[94mUZIW-O7IAI\u{1b}[0m\n\n\
    \u{1b}[90mContinue only if you started this login in Codex. If a website or another \
    person gave you this code, cancel.\u{1b}[0m\n";

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

#[test]
fn the_page_and_code_are_read_from_codex() {
    assert_eq!(
        Prompt::read(&lines(DEVICE_AUTH)),
        Some(Prompt {
            url: "https://auth.openai.com/codex/device".into(),
            code: "UZIW-O7IAI".into(),
            lifetime: Duration::from_secs(15 * 60),
        })
    );
    // Not before the code is printed.
    let half: Vec<String> = lines(DEVICE_AUTH).into_iter().take(9).collect();
    assert_eq!(Prompt::read(&half), None);
}

#[test]
fn only_openai_pages_are_passed_on() {
    assert!(device_page("https://auth.openai.com/codex/device").is_some());
    assert!(device_page("https://chatgpt.com/device.").is_some());
    assert_eq!(device_page("http://auth.openai.com/codex/device"), None);
    assert_eq!(device_page("https://openai.com.example.net/device"), None);
    assert_eq!(device_page("https://example.com"), None);
}

/// A node that pushes the sign-in task's status on a watch and answers nothing else: a direct
/// ask would open a stream and panic.
struct PushedStatus(tokio::sync::watch::Sender<crate::Followed>);

impl crate::SnapshotReader for PushedStatus {
    fn snapshot(&self, _read: Read) -> crate::SnapshotFuture<'_> {
        Box::pin(async { None })
    }

    fn follow(&self, _read: Read) -> Option<tokio::sync::watch::Receiver<crate::Followed>> {
        Some(self.0.subscribe())
    }
}

fn status(state: TaskState, text: &str) -> crate::Followed {
    Some(Ok(Value::Process(process::Reply::Status {
        task: process::Task {
            id: TaskId(1),
            command: CODEX_LOGIN.into(),
            state,
        },
        lines: lines(text),
        earlier: false,
    })))
}

#[tokio::test]
async fn the_code_is_taken_from_the_output_the_node_pushes() {
    let (push, _) = tokio::sync::watch::channel(None);
    let unasked: crate::SessionFn = Arc::new(|_, _| panic!("the output is pushed, not asked"));
    let client =
        PeerClient::local(unasked).with_snapshot_reader(Arc::new(PushedStatus(push.clone())));
    let half: String = DEVICE_AUTH.lines().take(9).collect::<Vec<_>>().join("\n");

    let (prompt, ()) = tokio::join!(wait_for_code(&client, TaskId(1)), async {
        push.send_replace(status(TaskState::Running, &half));
        push.send_replace(status(TaskState::Running, DEVICE_AUTH));
    });

    assert_eq!(prompt.unwrap().code, "UZIW-O7IAI");
}

#[tokio::test]
async fn a_sign_in_that_ends_before_its_code_says_why() {
    let (push, _) = tokio::sync::watch::channel(None);
    let unasked: crate::SessionFn = Arc::new(|_, _| panic!("the output is pushed, not asked"));
    let client =
        PeerClient::local(unasked).with_snapshot_reader(Arc::new(PushedStatus(push.clone())));

    let (prompt, ()) = tokio::join!(wait_for_code(&client, TaskId(1)), async {
        push.send_replace(status(TaskState::Exited { code: Some(1) }, "network down"));
    });

    assert_eq!(prompt.unwrap_err(), "Codex could not sign in: network down");
}
