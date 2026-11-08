use super::*;
use iroh::SecretKey;

#[tokio::test]
async fn a_silent_node_gives_this_machines_share_marked_silent() {
    let silent = anyhow::Error::from(ReportError::Silent(30));
    let now = Local::now().timestamp();
    let share = own_share(
        SecretKey::generate().public(),
        "build-box",
        now,
        fallback_answer(&silent),
        &format!("{silent:#}"),
    )
    .await;
    assert_eq!(
        share.status,
        "the running node did not answer in 30 seconds"
    );
    assert_eq!(share.answer, Answer::Silent);
    assert_eq!(share.answer.label(), "did not answer");
    assert_eq!(share.version, VERSION);
}

#[test]
fn other_collect_failures_read_as_failed_with_why() {
    let older = anyhow::Error::from(ReportError::Older);
    assert_eq!(fallback_answer(&older), Answer::OlderNode);
    let broken = anyhow::anyhow!("reading the node's report");
    assert_eq!(
        fallback_answer(&broken),
        Answer::Failed {
            reason: "reading the node's report".to_owned()
        }
    );
}

#[test]
fn the_issue_shows_its_title_and_body() {
    let issue = Issue {
        title: "Problem report R-1".to_owned(),
        body: "> it broke\n\nReference: R-1\n".to_owned(),
    };
    assert_eq!(
        issue_text(&issue),
        "\nProblem report R-1\n\n> it broke\n\nReference: R-1\n"
    );
}
