use std::time::{Duration, Instant};

use log::Level;

use crate::ident::PluginId;

/// The log target of every line a plugin or the host writes about a plugin.
/// A line reads `plugin: <id>: <text>`, so one plugin's lines are easy to find.
pub(crate) const TARGET: &str = "plugin";

/// A line is cut to this many bytes.
const MAX_LINE: usize = 1024;
/// Lines one plugin may write in a second; the rest are counted and dropped.
const LINES_PER_SECOND: u32 = 20;
const SECOND: Duration = Duration::from_secs(1);

/// Writes one line to the app's log, on one line and cut to `MAX_LINE`.
pub(crate) fn emit(id: &PluginId, level: Level, text: &str) {
    let mut line = text.replace(['\r', '\n'], "\\n");
    if line.len() > MAX_LINE {
        let mut end = MAX_LINE;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        line.truncate(end);
        line.push_str("...");
    }

    log::log!(target: TARGET, level, "{id}: {line}");
}

/// A plugin's `console` and `beep.log`: its lines in the app log, at most
/// `LINES_PER_SECOND` a second.
pub(crate) struct Console {
    id: PluginId,
    window: Option<Instant>,
    lines: u32,
    dropped: u32,
}

impl Console {
    pub(crate) fn new(id: PluginId) -> Self {
        Self {
            id,
            window: None,
            lines: 0,
            dropped: 0,
        }
    }

    pub(crate) fn line(&mut self, now: Instant, level: Level, text: &str) {
        let fresh = self
            .window
            .is_none_or(|start| now.saturating_duration_since(start) >= SECOND);
        if fresh {
            self.report_dropped();
            self.window = Some(now);
            self.lines = 0;
        }

        if self.lines < LINES_PER_SECOND {
            self.lines += 1;
            emit(&self.id, level, text);
        } else {
            self.dropped += 1;
        }
    }

    fn report_dropped(&mut self) {
        if self.dropped > 0 {
            emit(
                &self.id,
                Level::Warn,
                &format!("dropped {} lines", self.dropped),
            );
            self.dropped = 0;
        }
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        self.report_dropped();
    }
}

#[cfg(test)]
#[path = "tests/console.rs"]
pub(crate) mod tests;
