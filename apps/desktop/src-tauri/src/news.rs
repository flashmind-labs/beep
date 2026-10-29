//! What live mode hears of a reply as it streams. Telling never waits, so
//! the chat stream is never held up by the reader, and nothing is dropped:
//! text that arrives while the reader is behind joins the text still
//! waiting, so the queue holds no more than the reply itself.

use crate::beep::RunNews;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use tokio::sync::Notify;

#[derive(Default)]
struct Queue {
    news: VecDeque<RunNews>,
    teller_gone: bool,
    hearer_gone: bool,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    ready: Notify,
}

impl Shared {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A teller and the hearer it tells.
pub fn channel() -> (Teller, Heard) {
    let shared = Arc::new(Shared::default());
    (Teller(shared.clone()), Heard(shared))
}

/// The chat stream's end.
pub struct Teller(Arc<Shared>);

impl Teller {
    /// Queues `news` without waiting. Text right after text still waiting
    /// is added to it.
    pub fn tell(&self, news: RunNews) {
        {
            let mut queue = self.0.queue();
            if queue.hearer_gone {
                return;
            }
            if let RunNews::Text(more) = &news
                && let Some(RunNews::Text(waiting)) = queue.news.back_mut()
            {
                waiting.push_str(more);
            } else {
                queue.news.push_back(news);
            }
        }
        self.0.ready.notify_one();
    }
}

impl Drop for Teller {
    fn drop(&mut self) {
        self.0.queue().teller_gone = true;
        self.0.ready.notify_one();
    }
}

/// The reader's end.
pub struct Heard(Arc<Shared>);

impl Heard {
    /// The next news, or `None` once the teller is gone and everything it
    /// told was heard. Cancel safe: news is taken only when returned.
    pub async fn recv(&mut self) -> Option<RunNews> {
        loop {
            {
                let mut queue = self.0.queue();
                if let Some(news) = queue.news.pop_front() {
                    return Some(news);
                }
                if queue.teller_gone {
                    return None;
                }
            }
            // A notify sent since the check above left a permit, so this
            // cannot miss it.
            self.0.ready.notified().await;
        }
    }
}

impl Drop for Heard {
    fn drop(&mut self) {
        let mut queue = self.0.queue();
        queue.hearer_gone = true;
        queue.news.clear();
    }
}

#[cfg(test)]
mod tests {
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
}
