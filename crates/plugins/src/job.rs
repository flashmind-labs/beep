use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Instant;

use log::Level;

use crate::asset::Asset;
use crate::console::emit;
use crate::event::Mailbox;
use crate::event::UiEvent;
use crate::ident::PluginId;
use crate::instance::InstanceBatch;
use crate::meter::Meter;
use crate::sandbox::{Sandbox, SandboxError};
use crate::tool::ToolCall;
use crate::topic::{Delivery, Inbox, Topic};
use crate::ui::Placement;

pub(crate) enum Event {
    /// A path changed under the plugins folder.
    Fs(PathBuf),
    /// A plugin's record or folder changed: read both again now, then answer.
    Refresh {
        id: PluginId,
        done: Sender<()>,
    },
    /// A plugin's render result.
    Shown {
        id: PluginId,
        generation: u64,
        placements: Vec<Placement>,
        /// The images the placements use.
        assets: Vec<Asset>,
    },
    /// The topics a plugin follows now.
    Wanted {
        id: PluginId,
        generation: u64,
        topics: Vec<Topic>,
    },
    /// A plugin failed or went over its budget, and its thread is done.
    Stopped {
        id: PluginId,
        generation: u64,
        reason: String,
    },
    /// An update from the page.
    Publish(Delivery),
    /// What changed about the slots on the page.
    Instances(InstanceBatch),
    /// A window showed or hid.
    Visible {
        window: String,
        visible: bool,
    },
    /// The user used a control a plugin drew.
    Ui(UiEvent),
    /// A coding agent called a tool of a plugin.
    Tool {
        id: PluginId,
        call: ToolCall,
    },
    Stop,
}

/// One plugin's thread: render, wait for the next timer or a wake, fire.
/// While no window is visible it renders nothing, fires no timers and takes
/// no updates, and blocks until it is woken.
pub(crate) struct PluginJob {
    pub(crate) dir: PathBuf,
    pub(crate) id: PluginId,
    pub(crate) generation: u64,
    pub(crate) events: Sender<Event>,
    pub(crate) inbox: Arc<Inbox>,
    pub(crate) mailbox: Arc<Mailbox>,
    pub(crate) meter: Arc<Meter>,
    /// Set by the supervisor while no window is visible.
    pub(crate) hidden: Arc<AtomicBool>,
}

impl PluginJob {
    pub(crate) fn run(self, woken: &Receiver<()>) {
        let mut sandbox = match Sandbox::load_metered(&self.dir, Instant::now(), self.meter.clone())
        {
            Ok(sandbox) => sandbox,
            Err(error) => return self.fail(&error),
        };

        loop {
            match self.draw(&mut sandbox) {
                Ok(true) => {}
                Ok(false) => return,
                Err(error) => return self.fail(&error),
            }

            if let Some(topics) = sandbox.take_wanted()
                && !self.report(topics)
            {
                return;
            }

            let woke = if self.is_hidden() {
                woken.recv().map_err(|_| RecvTimeoutError::Disconnected)
            } else {
                match sandbox.next_timer() {
                    Some(due) => woken.recv_timeout(due.saturating_duration_since(Instant::now())),
                    None => woken.recv().map_err(|_| RecvTimeoutError::Disconnected),
                }
            };
            if matches!(woke, Err(RecvTimeoutError::Disconnected)) {
                return;
            }
            if let Err(error) = self.on_wake(&mut sandbox, Instant::now()) {
                return self.fail(&error);
            }
        }
    }

    fn is_hidden(&self) -> bool {
        self.hidden.load(Ordering::Acquire)
    }

    /// Draws the plugin's changes, unless it is hidden. Returns false when the
    /// supervisor is gone.
    pub(crate) fn draw(&self, sandbox: &mut Sandbox) -> Result<bool, SandboxError> {
        if self.is_hidden() {
            return Ok(true);
        }

        // Instances come before the first render.
        self.take_mail(sandbox, Instant::now())?;
        let Some(placements) = sandbox.render()? else {
            return Ok(true);
        };

        Ok(self.show(placements, sandbox.assets().to_vec()))
    }

    /// Runs what a wake is for: the timers that are due at `now`, then the
    /// updates that waited. A hidden plugin does neither, so they wait for it
    /// to show, and the timers that are overdue then fire once each.
    pub(crate) fn on_wake(&self, sandbox: &mut Sandbox, now: Instant) -> Result<(), SandboxError> {
        if self.is_hidden() {
            return Ok(());
        }

        self.take_mail(sandbox, now)?;
        sandbox.fire_due(now)?;
        for delivery in self.inbox.take() {
            sandbox.deliver(&delivery)?;
        }

        Ok(())
    }

    /// Runs the events, applies the instances and answers the tool calls that
    /// waited for the plugin.
    fn take_mail(&self, sandbox: &mut Sandbox, now: Instant) -> Result<(), SandboxError> {
        let mail = self.mailbox.take();

        for event in &mail.events {
            sandbox.handle(event)?;
        }
        if let Some(instances) = mail.instances {
            sandbox.set_instances(&instances)?;
        }
        for call in mail.calls {
            sandbox.call_tool(call, now)?;
        }

        Ok(())
    }

    /// Logs the error once and tells the supervisor, which shows nothing for this plugin.
    fn fail(&self, error: &SandboxError) {
        emit(&self.id, Level::Error, &format!("stopped: {error}"));
        let _ = self.events.send(Event::Stopped {
            id: self.id.clone(),
            generation: self.generation,
            reason: error.to_string(),
        });
    }

    /// Returns false when the supervisor is gone.
    fn report(&self, topics: Vec<Topic>) -> bool {
        self.events
            .send(Event::Wanted {
                id: self.id.clone(),
                generation: self.generation,
                topics,
            })
            .is_ok()
    }

    /// Returns false when the supervisor is gone.
    fn show(&self, placements: Vec<Placement>, assets: Vec<Asset>) -> bool {
        self.events
            .send(Event::Shown {
                id: self.id.clone(),
                generation: self.generation,
                placements,
                assets,
            })
            .is_ok()
    }
}

#[cfg(test)]
#[path = "tests/job.rs"]
mod tests;
