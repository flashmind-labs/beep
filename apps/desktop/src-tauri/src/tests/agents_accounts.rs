use super::*;

#[tokio::test]
async fn a_cancelled_attempt_owns_sign_in_until_its_process_ends() {
    let sign_in = ClaudeSignIn::default();
    let first = sign_in.start().expect("nothing under way");
    assert!(sign_in.start().is_err());

    let stopping = sign_in.stop().expect("one sign-in under way");
    let wait = sign_in.stop_and_wait();
    tokio::pin!(wait);
    assert!(watched::read(&first.cancelled, |cancelled| *cancelled));
    assert!(sign_in.start().is_err());
    assert!(futures::poll!(&mut wait).is_pending());

    drop(first);
    let second = sign_in.start().expect("the first process has ended");
    stopping.closed().await;
    wait.await;
    assert!(!watched::read(&second.cancelled, |cancelled| *cancelled));
}

#[test]
fn an_attempt_that_ends_on_its_own_frees_sign_in() {
    let sign_in = ClaudeSignIn::default();
    let first = sign_in.start().expect("nothing under way");
    drop(first);

    assert!(sign_in.stop().is_none());
    assert!(sign_in.start().is_ok());
}
