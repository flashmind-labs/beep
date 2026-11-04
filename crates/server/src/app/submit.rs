//! Submitting a message to a user's chat, live runs and shutdown.

use super::App;
use super::Message;
use super::STOPPING_WAIT;
use super::UserNotice;
use crate::{
    account::user::User,
    agent::live::LiveRun,
    agent::metering,
    agent::titles,
    protocol::RunEvent,
    store::{CreateRun, RunStatus, SessionRef},
};
use anyhow::Result;
use flashmind::core::CancellationToken;
use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

impl App {
    pub async fn submit(self: &Arc<Self>, user: &Arc<User>, message: Message) -> Result<CreateRun> {
        // Serialize registration with cancellation so a newly created run can always be stopped.
        let runs = self.runs.lock().await;
        self.submit_locked(runs, user, message).await
    }

    pub(super) async fn submit_locked(
        self: &Arc<Self>,
        mut runs: tokio::sync::MutexGuard<'_, HashMap<String, Arc<LiveRun>>>,
        user: &Arc<User>,
        message: Message,
    ) -> Result<CreateRun> {
        let result = user
            .create_run(message.session_id, &message.request(), &message.message)
            .await?;
        if let CreateRun::Created(run) = &result {
            let live = Arc::new(LiveRun::new(CancellationToken::new()));
            live.push(RunEvent::RunStarted {
                run_id: run.id.clone(),
                session_id: run.session_id.clone(),
            });
            if message.voice.is_some() {
                live.push(RunEvent::Transcript {
                    text: message.message.clone(),
                });
            }
            runs.insert(run.id.clone(), live.clone());
            drop(runs);

            let (app, user, run) = (self.clone(), user.clone(), run.clone());
            let session = SessionRef {
                user: user.id().to_owned(),
                id: message.session_id,
            };
            // A woken chat is named after its event, and only if it is kept.
            match self.store.needs_first_title(&session).await {
                Ok(true) if message.woken.is_none() => {
                    let (app, text) = (self.clone(), message.message.clone());
                    tokio::spawn(async move {
                        let generator = app.generator(&session.user);
                        if titles::name_first(&app.store, generator, &session, &text).await {
                            // Nobody listening just means no phone is online.
                            let _ = app
                                .notifications
                                .send(UserNotice::changed(&session.user, ""));
                        }
                    });
                }
                Ok(_) => {}
                Err(error) => log::warn!("unable to check title: {error}; run_id={}", run.id),
            }
            tokio::spawn(async move {
                // Saved before the run can finish, which replaces it.
                if message.is_prompt()
                    && let Err(error) = user.save_prompt(&run, &message.message).await
                {
                    log::warn!("unable to save the prompt: {error:#}; run_id={}", run.id);
                }
                let turn = metering::in_turn(app.execute(&user, &run, &message, &live));
                if let Err(error) = turn.await {
                    log::error!("run failed: {error}; run_id={}", run.id);
                    if let Err(error) = user.finish(&run, None, RunStatus::Failed).await {
                        log::error!("unable to persist failure: {error}; run_id={}", run.id);
                    }
                    live.push(RunEvent::RunFinished {
                        status: RunStatus::Failed.as_str().to_owned(),
                        updated_at: None,
                    });
                }
                app.runs.lock().await.remove(&run.id);
                live.done.cancel();
                // Node reports wait for the chat to be free.
                app.subagents.turn_ended();
            });
        }
        Ok(result)
    }

    /// The event feed of a run in progress, `None` once it has finished.
    pub async fn live(&self, id: &str) -> Option<Arc<LiveRun>> {
        self.runs.lock().await.get(id).cloned()
    }

    /// Waits up to `limit` for `chat`'s run to be saved; `false` when one is still going.
    pub(crate) async fn run_ended(&self, user: &User, chat: Uuid, limit: Duration) -> Result<bool> {
        let Some(run) = user.active_run(chat).await? else {
            return Ok(true);
        };
        // Gone from the live runs means it is saved or being saved now.
        if let Some(live) = self.live(&run.id).await
            && timeout(limit, live.done.cancelled()).await.is_err()
        {
            return Ok(false);
        }
        Ok(true)
    }

    /// Stops the run and returns once it is saved, so the next message can follow at once.
    pub async fn cancel(&self, id: &str) {
        if let Some(live) = self.live(id).await {
            live.cancel.cancel();
            let _ = timeout(STOPPING_WAIT, live.done.cancelled()).await;
        }
    }

    pub async fn shutdown(&self) {
        // Read by runs as they stop, so they end as interrupted, not cancelled.
        self.shutting_down.store(true, Ordering::Relaxed);
        self.events.stop();
        self.watchers.stop();
        self.subagents.stop();
        self.stop_teams().await;
        for live in self.runs.lock().await.values() {
            live.cancel.cancel();
        }
        let _ = timeout(Duration::from_secs(10), async {
            loop {
                if self.runs.lock().await.is_empty() {
                    break;
                }
                sleep(Duration::from_millis(25)).await;
            }
        })
        .await;
    }
}
