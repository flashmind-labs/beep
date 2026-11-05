//! Types dictated words into the focused field of any app, as key events carrying the text. It
//! needs the Accessibility permission, which `dictation` checks before it records.

use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventTapLocation};
use std::{ffi::c_ulong, thread::sleep, time::Duration};

/// UTF-16 units one key event carries; macOS drops more than about twenty.
const UNITS_PER_EVENT: usize = 16;
/// Pause between key events, so the focused app keeps them in order.
const GAP: Duration = Duration::from_millis(3);

#[derive(Debug, thiserror::Error)]
#[error("macOS did not make a key event")]
pub(super) struct NoEvent;

/// Types `text`, a few characters at a time. Blocks while it does, so call it off the async
/// workers.
pub(super) fn type_text(text: &str) -> Result<(), NoEvent> {
    let units: Vec<u16> = text.encode_utf16().collect();
    for chunk in units.chunks(UNITS_PER_EVENT) {
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(None, 0, down).ok_or(NoEvent)?;
            // Option may still be coming up: the words are typed with no modifier.
            CGEvent::set_flags(Some(&*event), CGEventFlags::empty());
            let len = c_ulong::try_from(chunk.len()).map_err(|_| NoEvent)?;
            // SAFETY: `chunk` is a live slice of `len` UTF-16 units, which the call copies.
            unsafe { CGEvent::keyboard_set_unicode_string(Some(&*event), len, chunk.as_ptr()) };
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&*event));
        }
        sleep(GAP);
    }
    Ok(())
}
