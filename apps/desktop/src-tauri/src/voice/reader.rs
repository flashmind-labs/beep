//! Reading lines aloud: each line's audio fetched while the one before
//! plays, fillers in quiet stretches, and captions for lines without voice.

use super::{Voice, audio::Audio, news::Heard, reply::Outcome, session::Playing};
use crate::beep::RunNews;
use anyhow::Context;
use beep_live::{Filler, Fillers, Input, Phase, Reading};
use beep_proto::{Filler as FillerLine, Say};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, PoisonError, atomic::Ordering},
    time::{Duration, Instant},
};
use tauri::Manager;
use tokio::{
    sync::{mpsc, watch},
    task::AbortHandle,
    time::{interval, sleep},
};

/// How often a run is checked for a filler.
const FILLER_CHECK: Duration = Duration::from_millis(250);
/// Sentences waiting for their audio.
const LINES: usize = 64;
/// Spoken sentences waiting to play.
const READY: usize = 2;
const NOT_ALOUD: &str = "Could not read the reply aloud";

/// The task reading aloud. It is started and stopped only through the
/// session, under the state lock, so a hush cannot fall between spawning a
/// reader and keeping its handle.
#[derive(Default)]
pub(super) struct Reader(Option<AbortHandle>);

impl Reader {
    /// Stops the reader there is and spawns `task` in its place.
    pub(super) fn start<F>(&mut self, task: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.stop();
        // Tests spawn on their own single-threaded runtime, so a reader
        // cannot run on another thread while it is being replaced.
        #[cfg(not(test))]
        let reader = tauri::async_runtime::spawn(task).inner().abort_handle();
        #[cfg(test)]
        let reader = tokio::spawn(task).abort_handle();
        self.0 = Some(reader);
    }

    pub(super) fn stop(&mut self) {
        if let Some(reader) = self.0.take() {
            reader.abort();
        }
    }
}

/// A sentence for the reader, and whether it is a filler.
enum Line {
    Answer(String),
    Filler(Filler),
}

/// A sentence with its audio, ready to play.
pub(super) struct Ready {
    pub(super) text: String,
    pub(super) audio: Vec<u8>,
    pub(super) filler: bool,
}

/// Fillers and how many lines wait to be spoken, shared by the reader's
/// parts.
pub(super) struct Voiced {
    fillers: Fillers,
    queued: u32,
    /// The user was told the reply cannot be read aloud.
    warned: bool,
}

impl Voiced {
    pub(super) fn new(queued: u32) -> Mutex<Self> {
        Mutex::new(Self {
            fillers: Fillers::new(Instant::now()),
            queued,
            warned: false,
        })
    }
}

/// What the reader speaks: a reply as it streams, or lines known already.
pub(super) enum Feed {
    Reply {
        heard: Heard,
        said: String,
        /// The reply answers a message just sent.
        announce: bool,
    },
    Lines(Vec<String>),
}

fn lock(voiced: &Mutex<Voiced>) -> MutexGuard<'_, Voiced> {
    voiced.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Voice {
    /// Speaks `feed` line by line: getting each line's audio while the one
    /// before plays. Returns how the reply's stream ended.
    pub(super) async fn read(
        self: &Arc<Self>,
        id: u64,
        feed: Feed,
        voiced: &Mutex<Voiced>,
    ) -> Outcome {
        let (lines, todo) = mpsc::channel(LINES);
        let (ready, playable) = mpsc::channel(READY);
        let (answers, ticks) = watch::channel(0u64);
        let said = match &feed {
            Feed::Reply { said, .. } => said.clone(),
            Feed::Lines(_) => String::new(),
        };
        let fed = self.feed(id, feed, lines, voiced);
        let voiced_up = self.synthesize(todo, ready, answers, said, voiced);
        let played = self.play(id, playable, ticks, voiced, true);
        let (outcome, (), _) = tokio::join!(fed, voiced_up, played);
        outcome
    }

    /// Turns the reply into lines as it streams, with fillers in its
    /// quiet stretches.
    async fn feed(
        &self,
        id: u64,
        feed: Feed,
        lines: mpsc::Sender<Line>,
        voiced: &Mutex<Voiced>,
    ) -> Outcome {
        let queue = |line: Line| {
            let mut v = lock(voiced);
            v.queued += 1;
            v.fillers.speaking(true, Instant::now());
            line
        };
        let mut outcome = Outcome::default();
        let (mut heard, said, announce) = match feed {
            Feed::Lines(all) => {
                for line in all {
                    if lines.send(queue(Line::Answer(line))).await.is_err() {
                        break;
                    }
                }
                return outcome;
            }
            Feed::Reply {
                heard,
                said,
                announce,
            } => (heard, said, announce),
        };
        let mut reading = Reading::default();
        let mut check = interval(FILLER_CHECK);
        loop {
            tokio::select! {
                news = heard.recv() => match news {
                    Some(RunNews::Started(run)) => {
                        outcome.started = true;
                        self.started(id, run, announce.then_some(said.as_str()));
                    }
                    Some(RunNews::Text(delta)) => {
                        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
                            s.reply_text.push_str(&delta);
                        }
                        for line in reading.push(&delta) {
                            if lines.send(queue(Line::Answer(line))).await.is_err() {
                                return outcome;
                            }
                        }
                    }
                    Some(RunNews::Tool(step)) => {
                        lock(voiced).fillers.tool(&step);
                        self.set_step(id, Some(step));
                    }
                    Some(RunNews::Approval(approval)) => {
                        outcome.asked = true;
                        self.asking(id, approval);
                    }
                    Some(RunNews::Failed(why)) => outcome.failed = Some(why),
                    Some(RunNews::Finished(status)) => outcome.status = Some(status),
                    None => break,
                },
                _ = check.tick() => {
                    let due = lock(voiced).fillers.due(Instant::now());
                    if let Some(filler) = due
                        && lines.send(queue(Line::Filler(filler))).await.is_err()
                    {
                        return outcome;
                    }
                }
            }
        }
        for line in reading.finish() {
            if lines.send(queue(Line::Answer(line))).await.is_err() {
                break;
            }
        }
        outcome
    }

    /// Tells the user once per reply that it cannot be read aloud.
    fn warn_not_aloud(&self, voiced: &Mutex<Voiced>) {
        let first = !std::mem::replace(&mut lock(voiced).warned, true);
        if first {
            self.toast(NOT_ALOUD);
        }
    }

    /// Gets each line's audio from the server, in order.
    async fn synthesize(
        &self,
        mut todo: mpsc::Receiver<Line>,
        ready: mpsc::Sender<Ready>,
        answers: watch::Sender<u64>,
        said: String,
        voiced: &Mutex<Voiced>,
    ) {
        while let Some(line) = todo.recv().await {
            let (text, filler, note) = match line {
                Line::Answer(text) => {
                    let note = self.beep.call(Say { text: text.clone() }).await;
                    (text, false, note)
                }
                Line::Filler(Filler { step, still }) => {
                    let note = self
                        .beep
                        .call(FillerLine {
                            step,
                            said: said.clone(),
                            still,
                        })
                        .await;
                    let text = note.as_ref().map(|n| n.text.clone()).unwrap_or_default();
                    (text, true, note)
                }
            };
            let ready_line = match note {
                Ok(note) => Ready {
                    text,
                    audio: note.audio.data,
                    filler,
                },
                Err(e) => {
                    log::warn!("speaking a line: {e}");
                    if !filler {
                        self.warn_not_aloud(voiced);
                    }
                    // Still counted, so the queue empties; an answer line is
                    // shown without its voice.
                    Ready {
                        text,
                        audio: Vec::new(),
                        filler,
                    }
                }
            };
            if !filler {
                answers.send_modify(|n| *n += 1);
            }
            if ready.send(ready_line).await.is_err() {
                return;
            }
        }
    }

    /// Plays lines as they are ready. A filler stops when an answer line
    /// is ready. `reply` says the lines are an answer: the first one moves
    /// live mode to speaking. An answer line that cannot play shows as a
    /// caption for about as long as it takes to read.
    pub(super) async fn play(
        self: &Arc<Self>,
        id: u64,
        mut playable: mpsc::Receiver<Ready>,
        mut answers: watch::Receiver<u64>,
        voiced: &Mutex<Voiced>,
        reply: bool,
    ) -> bool {
        let Some(audio) = self.audio(id).await else {
            return false;
        };
        let mut spoke = false;
        while let Some(line) = playable.recv().await {
            // An answer readied since the last line started makes a filler
            // moot.
            let fresh = answers.has_changed().unwrap_or(false);
            answers.borrow_and_update();
            let cut = line.filler && fresh;
            let shown = !line.filler && !line.text.trim().is_empty();
            if (!line.audio.is_empty() || shown) && !cut {
                if !line.filler && reply && !spoke {
                    self.input_reply_ready(id);
                }
                let played = if line.audio.is_empty() {
                    Err(anyhow::anyhow!("no audio for the line"))
                } else {
                    self.play_line(id, &audio, &line, &mut answers).await
                };
                if let Err(e) = played {
                    if shown {
                        if !line.audio.is_empty() {
                            log::warn!("playing a line: {e:#}");
                            self.warn_not_aloud(voiced);
                        }
                        self.caption(id, &line.text).await;
                    } else {
                        log::warn!("playing a filler: {e:#}");
                    }
                }
                if !line.filler {
                    spoke = true;
                    if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
                        s.last_reply.push(line.text.clone());
                    }
                }
            }
            let mut v = lock(voiced);
            v.queued = v.queued.saturating_sub(1);
            if v.queued == 0 {
                v.fillers.speaking(false, Instant::now());
            }
        }
        spoke
    }

    /// Shows `text` as Beep's line without its voice, for as long as it
    /// takes to read.
    async fn caption(&self, id: u64, text: &str) {
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.prev = s.line.take();
            s.line = Some(text.to_owned());
        }
        self.show(id);
        let words = u64::try_from(text.split_whitespace().count()).unwrap_or(u64::MAX);
        let read = Duration::from_millis(words.saturating_mul(250))
            .clamp(Duration::from_millis(1200), Duration::from_secs(6));
        sleep(read).await;
    }

    fn input_reply_ready(self: &Arc<Self>, id: u64) {
        let thinking = self
            .state()
            .live
            .as_ref()
            .is_some_and(|s| s.id == id && s.live.phase() == Phase::Thinking);
        if thinking {
            self.input(id, Input::ReplyReady);
        }
    }

    async fn play_line(
        &self,
        id: u64,
        audio: &Audio,
        line: &Ready,
        answers: &mut watch::Receiver<u64>,
    ) -> anyhow::Result<()> {
        let path = self.line_path()?;
        tokio::fs::write(&path, &line.audio)
            .await
            .with_context(|| format!("writing {}", path.display()))?;
        let played = audio.play(&path).await;
        let removed = tokio::fs::remove_file(&path).await;
        if let Err(e) = removed {
            log::debug!("removing {}: {e}", path.display());
        }
        let (played, done) = played?;
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.prev = s.line.take();
            s.line = Some(line.text.clone());
            s.playing = Some(Playing {
                since: Instant::now(),
                played,
            });
        }
        self.show(id);
        if line.filler {
            tokio::select! {
                _ = done => {}
                // An answer is ready: the filler gives way.
                _ = answers.changed() => audio.stop_playing(),
            }
        } else {
            // Stopped playback ends it as well.
            let _ = done.await;
        }
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.playing = None;
        }
        self.show(id);
        Ok(())
    }

    pub(super) fn line_path(&self) -> anyhow::Result<PathBuf> {
        let dir = self.app.path().app_cache_dir()?;
        std::fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
        let n = self.files.fetch_add(1, Ordering::Relaxed);
        Ok(dir.join(format!("live-{n}.mp3")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    /// A reader that says a line once `go` fires, and the receiver that
    /// hears it, or an error when the reader was stopped first.
    fn gated_reader(reader: &mut Reader) -> (oneshot::Sender<()>, oneshot::Receiver<()>) {
        let (go, gate) = oneshot::channel();
        let (said, heard) = oneshot::channel();
        reader.start(async move {
            if gate.await.is_ok() {
                let _ = said.send(());
            }
        });
        (go, heard)
    }

    #[tokio::test]
    async fn a_hush_right_after_a_reader_starts_stops_it() {
        let mut reader = Reader::default();
        let (go, heard) = gated_reader(&mut reader);
        // The hush comes before the task has had a chance to run.
        reader.stop();
        // The gate may already be gone with the aborted task.
        let _ = go.send(());
        assert!(heard.await.is_err(), "a hushed reader still spoke");
    }

    #[tokio::test]
    async fn a_new_reader_stops_the_one_before() {
        let mut reader = Reader::default();
        let (first_go, first) = gated_reader(&mut reader);
        let (second_go, second) = gated_reader(&mut reader);
        let _ = first_go.send(());
        assert!(first.await.is_err(), "the replaced reader still spoke");
        second_go
            .send(())
            .expect("the running reader waits on its gate");
        assert!(second.await.is_ok());
    }
}
