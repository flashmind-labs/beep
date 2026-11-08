use super::*;

#[tokio::test]
async fn a_long_reply_is_heard_whole_while_the_reader_is_behind() {
    let (teller, mut heard) = channel();
    let mut said = String::new();
    // Far more deltas than any bounded queue held, none heard yet.
    for n in 0..10_000 {
        let delta = format!("word{n} ");
        said.push_str(&delta);
        teller.tell(RunNews::Text(delta));
    }
    teller.tell(RunNews::Tool("Searching".into()));
    teller.tell(RunNews::Text("after.".into()));
    drop(teller);
    assert_eq!(heard.recv().await, Some(RunNews::Text(said)));
    assert_eq!(heard.recv().await, Some(RunNews::Tool("Searching".into())));
    assert_eq!(heard.recv().await, Some(RunNews::Text("after.".into())));
    assert_eq!(heard.recv().await, None);
}

#[tokio::test]
async fn a_waiting_reader_wakes_for_news() {
    let (teller, mut heard) = channel();
    let reader = tokio::spawn(async move {
        let mut got = Vec::new();
        while let Some(news) = heard.recv().await {
            got.push(news);
        }
        got
    });
    tokio::task::yield_now().await;
    teller.tell(RunNews::Text("Hi.".into()));
    drop(teller);
    let got = reader.await.expect("the reader does not panic");
    assert_eq!(got, vec![RunNews::Text("Hi.".into())]);
}

#[test]
fn telling_after_the_reader_left_keeps_nothing() {
    let (teller, heard) = channel();
    let shared = heard.0.clone();
    drop(heard);
    teller.tell(RunNews::Text("lost on purpose".into()));
    assert!(shared.queue().news.is_empty());
}
