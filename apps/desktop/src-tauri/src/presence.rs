//! Whether the user is at this Mac and free to hear a voice note: they used
//! the keyboard or mouse lately, the screen is unlocked, and no other app is
//! using the microphone, as one does during a call.

use std::time::Duration;

/// Longest since the last input for the user to count as here.
const AWAY: Duration = Duration::from_secs(3 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presence {
    /// Time since the last key, click or mouse move.
    pub idle: Duration,
    pub locked: bool,
    /// The default microphone is in use by some app.
    pub mic_busy: bool,
}

impl Presence {
    pub fn can_speak(&self) -> bool {
        !self.locked && !self.mic_busy && self.idle <= AWAY
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::Presence;
    use objc2_core_audio::{
        AudioObjectGetPropertyData, AudioObjectID, AudioObjectPropertyAddress,
        kAudioDevicePropertyDeviceIsRunningSomewhere, kAudioHardwarePropertyDefaultInputDevice,
        kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    };
    use objc2_core_foundation::{CFBoolean, CFDictionary, CFRetained, CFString, CFType};
    use objc2_core_graphics::{
        CGEventSource, CGEventSourceStateID, CGEventType, CGSessionCopyCurrentDictionary,
    };
    use std::{ffi::c_void, mem::size_of, ptr::NonNull, time::Duration};

    /// `kCGAnyInputEventType`, which the bindings leave out.
    const ANY_INPUT: CGEventType = CGEventType(u32::MAX);

    impl Presence {
        pub fn now() -> Self {
            Self {
                idle: idle(),
                locked: locked(),
                mic_busy: mic_busy(),
            }
        }
    }

    fn idle() -> Duration {
        let secs = CGEventSource::seconds_since_last_event_type(
            CGEventSourceStateID::CombinedSessionState,
            ANY_INPUT,
        );
        Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX)
    }

    /// True while the lock screen is up. No session at all, as over SSH,
    /// counts as locked: nobody is there to hear.
    fn locked() -> bool {
        let Some(session) = CGSessionCopyCurrentDictionary() else {
            return true;
        };
        // SAFETY: the session dictionary maps CFString keys to CF objects.
        let session: CFRetained<CFDictionary<CFString, CFType>> =
            unsafe { CFRetained::cast_unchecked(session) };
        let key = CFString::from_static_str("CGSSessionScreenIsLocked");
        session
            .get(&key)
            .and_then(|value| value.downcast::<CFBoolean>().ok())
            .is_some_and(|value| value.as_bool())
    }

    /// Whether any app is recording from the default input device.
    fn mic_busy() -> bool {
        let Some(device) = property::<AudioObjectID>(
            kAudioObjectSystemObject as AudioObjectID,
            kAudioHardwarePropertyDefaultInputDevice,
        ) else {
            return false;
        };
        if device == 0 {
            return false;
        }
        property::<u32>(device, kAudioDevicePropertyDeviceIsRunningSomewhere)
            .is_some_and(|running| running != 0)
    }

    /// Reads a global property that holds one plain value.
    fn property<T: Default>(object: AudioObjectID, selector: u32) -> Option<T> {
        let mut address = AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut value = T::default();
        let mut size = u32::try_from(size_of::<T>()).ok()?;
        // SAFETY: the address, size and value pointers are valid for the
        // call, and `size` is the size of `value`, which CoreAudio does not
        // write past.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&mut address),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::from(&mut value).cast::<c_void>(),
            )
        };
        (status == 0).then_some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERE: Presence = Presence {
        idle: Duration::from_secs(5),
        locked: false,
        mic_busy: false,
    };

    #[test]
    fn speaks_only_to_someone_here_and_free() {
        assert!(HERE.can_speak());
        assert!(
            !Presence {
                idle: Duration::from_secs(600),
                ..HERE
            }
            .can_speak()
        );
        assert!(
            !Presence {
                locked: true,
                ..HERE
            }
            .can_speak()
        );
        assert!(
            !Presence {
                mic_busy: true,
                ..HERE
            }
            .can_speak()
        );
    }
}
