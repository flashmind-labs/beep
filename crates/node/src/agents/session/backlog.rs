//! The prompts a session holds for its agent.

use crate::agents::acp::Prompt;
use beep_proto::{MAX_AGENT_IMAGES, MAX_AGENT_IMAGES_TOTAL};
use std::collections::VecDeque;

/// The prompts a session holds for its agent: the ones queued behind the turn, the
/// steers that go first, and the one steer the adapter is answering now. The actor
/// is the one writer; every reader of "what waits" asks here.
#[derive(Default)]
pub(super) struct Backlog {
    /// Prompts waiting for the agent to be free, the first prompt among them.
    queued: VecDeque<Prompt>,
    /// Messages for the running turn, sent once its tool call finishes.
    steers: VecDeque<Prompt>,
    /// The steer sent with `_session/steering`, until the adapter answers.
    steering: Option<PendingSteer>,
}

pub(super) struct PendingSteer {
    pub(super) prompt: Prompt,
    pub(super) shown: bool,
    /// The user took it back while the adapter still held it: it is not
    /// waiting, and goes nowhere if the adapter hands it back.
    pub(super) dropped: bool,
}

impl Backlog {
    pub(super) fn new(queued: impl IntoIterator<Item = Prompt>) -> Self {
        Self {
            queued: queued.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Adds a prompt behind the others, or a steer behind the steers.
    pub(super) fn push(&mut self, prompt: Prompt, steer: bool) {
        if steer {
            self.steers.push_back(prompt);
        } else {
            self.queued.push_back(prompt);
        }
    }

    /// Puts a prompt first among the queued ones.
    pub(super) fn push_front(&mut self, prompt: Prompt) {
        self.queued.push_front(prompt);
    }

    pub(super) fn push_front_steer(&mut self, prompt: Prompt) {
        self.steers.push_front(prompt);
    }

    pub(super) fn pop_steer(&mut self) -> Option<Prompt> {
        self.steers.pop_front()
    }

    /// Records the steer the adapter is now answering.
    pub(super) fn begin_steer(&mut self, prompt: Prompt) {
        self.steering = Some(PendingSteer {
            prompt,
            shown: false,
            dropped: false,
        });
    }

    pub(super) fn take_steering(&mut self) -> Option<PendingSteer> {
        self.steering.take()
    }

    pub(super) fn steering(&self) -> Option<&PendingSteer> {
        self.steering.as_ref()
    }

    pub(super) fn steering_mut(&mut self) -> Option<&mut PendingSteer> {
        self.steering.as_mut()
    }

    pub(super) fn is_steering(&self) -> bool {
        self.steering.is_some()
    }

    pub(super) fn steers_empty(&self) -> bool {
        self.steers.is_empty()
    }

    pub(super) fn has_prompts(&self) -> bool {
        !(self.queued.is_empty() && self.steers.is_empty())
    }

    /// What waits, in the order the state shows it: the steer the adapter
    /// holds (unless the user took it back), the steers, then the queue.
    pub(super) fn prompts(&self) -> impl Iterator<Item = &Prompt> {
        self.steering
            .iter()
            .filter(|steer| !steer.dropped)
            .map(|steer| &steer.prompt)
            .chain(&self.steers)
            .chain(&self.queued)
    }

    /// Prompts and steers waiting, as the state counts them.
    pub(super) fn waiting(&self) -> u32 {
        u32::try_from(self.prompts().count()).unwrap_or(u32::MAX)
    }

    pub(super) fn messages(&self) -> Vec<String> {
        self.prompts()
            .map(|prompt| {
                let mut text = prompt.text.clone();

                for image in &prompt.images {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&format!("[Image: {}]", image.name));
                }

                text
            })
            .collect()
    }

    /// The next prompt to send: the steers waiting, as one, ahead of the queue.
    pub(super) fn next(&mut self) -> Option<Prompt> {
        take_steers(&mut self.steers).or_else(|| self.queued.pop_front())
    }

    /// Takes the steers back, the one the adapter holds too.
    pub(super) fn drop_steers(&mut self) {
        self.steers.clear();

        if let Some(steer) = &mut self.steering {
            steer.dropped = true;
        }
    }

    /// A user stop keeps the steers: they go as the next prompt. Any other stop
    /// drops them, in flight or not. A cancel already sent for a steer
    /// (`Stopping::Steer`) cannot be taken back; the turn ends.
    pub(super) fn cancel(&mut self, by_user: bool) {
        self.queued.clear();

        if !by_user {
            self.drop_steers();
        }
    }

    /// Everything that waits, steers first, for the session to carry on with.
    pub(super) fn drain(&mut self) -> impl Iterator<Item = Prompt> {
        self.steers.drain(..).chain(self.queued.drain(..))
    }
}

/// The steers waiting, as one prompt: their words in order and their
/// images, as many as one prompt takes. The rest wait for the next.
fn take_steers(steers: &mut VecDeque<Prompt>) -> Option<Prompt> {
    let mut merged = steers.pop_front()?;

    while let Some(next) = steers.front() {
        let images = merged.images.len() + next.images.len();
        let bytes = merged
            .images
            .iter()
            .chain(&next.images)
            .fold(0usize, |n, image| n.saturating_add(image.data.len()));
        if images > MAX_AGENT_IMAGES || bytes > MAX_AGENT_IMAGES_TOTAL {
            break;
        }

        let Some(next) = steers.pop_front() else {
            break;
        };
        if merged.text.is_empty() {
            merged.text = next.text;
        } else if !next.text.is_empty() {
            merged.text = format!("{}\n\n{}", merged.text, next.text);
        }
        merged.images.extend(next.images);
        merged.context = match (merged.context.take(), next.context) {
            (Some(first), Some(second)) => Some(format!("{first}\n{second}")),
            (first, second) => first.or(second),
        };
    }

    Some(merged)
}

#[cfg(test)]
#[path = "../../tests/agents_session_backlog.rs"]
mod tests;
