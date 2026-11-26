//! Pauses active media playback while voice dictation uses the microphone, and
//! resumes it once microphone capture ends and the audio output device leaves
//! call mode (such as Bluetooth HFP/SCO returning to stereo A2DP).

use async_trait::async_trait;
use beep_proto::watched;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tauri::{State, Window};
use tokio::{
    sync::{Mutex as AsyncMutex, watch},
    time::timeout,
};
use uuid::Uuid;

/// Stereo playback codecs (SBC, AAC, aptX, LDAC) run at 44.1 kHz or above,
/// whereas Bluetooth HFP/HSP/BLE voice call modes run at 8, 16, 24 or 32 kHz.
pub(crate) const NON_CALL_MIN_RATE: u32 = 44_100;
const NON_CALL_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputKind {
    BuiltInSpeaker,
    HeadphonesOrExternal,
    Bluetooth,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OutputState {
    pub device_id: String,
    pub kind: OutputKind,
    pub sample_rate: u32,
    pub channels: u16,
    pub mic_input_active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResumeCheck {
    /// Output is back in non-call mode and ready for media playback.
    Ready,
    /// Microphone teardown or Bluetooth call-to-stereo profile switch is still in progress.
    WaitForNonCall,
    /// Headphones disconnected onto built-in speakers; leave playback paused.
    AbortDisconnected,
}

/// Evaluates whether `current` has returned from call mode relative to `baseline`.
pub(crate) fn check_output_ready(
    baseline: &OutputState,
    current: &OutputState,
    baseline_present: bool,
) -> ResumeCheck {
    if matches!(
        baseline.kind,
        OutputKind::Bluetooth | OutputKind::HeadphonesOrExternal
    ) && current.kind == OutputKind::BuiltInSpeaker
        && !baseline_present
    {
        return ResumeCheck::AbortDisconnected;
    }

    if !baseline.mic_input_active && current.mic_input_active {
        return ResumeCheck::WaitForNonCall;
    }

    if baseline.kind == OutputKind::Bluetooth
        && !baseline.device_id.is_empty()
        && current.device_id != baseline.device_id
        && baseline_present
    {
        return ResumeCheck::WaitForNonCall;
    }

    let min_rate = baseline.sample_rate.min(NON_CALL_MIN_RATE);
    if min_rate > 0 && current.sample_rate > 0 && current.sample_rate < min_rate {
        return ResumeCheck::WaitForNonCall;
    }

    if baseline.channels > 1 && current.channels > 0 && current.channels < baseline.channels {
        return ResumeCheck::WaitForNonCall;
    }

    ResumeCheck::Ready
}

/// Fallback decision if the OS never reports the original sample rate before the deadline.
pub(crate) fn should_resume_after_deadline(
    baseline: &OutputState,
    current: &OutputState,
    baseline_present: bool,
) -> bool {
    if check_output_ready(baseline, current, baseline_present) == ResumeCheck::AbortDisconnected {
        return false;
    }

    baseline.mic_input_active || !current.mic_input_active
}

#[async_trait]
pub(crate) trait OutputWatch: Send {
    /// Resolves with `true` when an audio device/route property changed, or `false` if closed.
    async fn changed(&mut self) -> bool;
}

#[async_trait]
pub(crate) trait MediaBackend: Send + Sync + 'static {
    type Paused: Send + 'static;

    async fn output_state(&self) -> Option<OutputState>;
    async fn device_present(&self, device_id: &str) -> bool;
    async fn pause_if_playing(&self) -> Option<Self::Paused>;
    async fn resume(&self, paused: Self::Paused);
    async fn watch_output(&self) -> Option<Box<dyn OutputWatch>>;

    fn non_call_deadline(&self) -> Duration {
        NON_CALL_DEADLINE
    }
}

async fn wait_for_non_call<B: MediaBackend>(
    backend: &B,
    baseline: &OutputState,
    mut cancel: watch::Receiver<u64>,
    expected_gen: u64,
) -> Option<bool> {
    if watched::read(&cancel, |&cur| cur != expected_gen) {
        return None;
    }

    let Some(initial) = backend.output_state().await else {
        return Some(true);
    };
    let initial_present = backend.device_present(&baseline.device_id).await;
    match check_output_ready(baseline, &initial, initial_present) {
        ResumeCheck::Ready => return Some(true),
        ResumeCheck::AbortDisconnected => return Some(false),
        ResumeCheck::WaitForNonCall => {}
    }

    let Some(mut watcher) = backend.watch_output().await else {
        return Some(should_resume_after_deadline(
            baseline,
            &initial,
            initial_present,
        ));
    };

    // Re-read after subscribing so a transition between the initial check and
    // listener registration is never missed.
    let Some(rechecked) = backend.output_state().await else {
        return Some(true);
    };
    let rechecked_present = backend.device_present(&baseline.device_id).await;
    match check_output_ready(baseline, &rechecked, rechecked_present) {
        ResumeCheck::Ready => return Some(true),
        ResumeCheck::AbortDisconnected => return Some(false),
        ResumeCheck::WaitForNonCall => {}
    }

    let waited = timeout(backend.non_call_deadline(), async {
        loop {
            tokio::select! {
                _ = watched::wait_for(&mut cancel, |&cur| cur != expected_gen) => {
                    return Ok(None);
                }
                open = watcher.changed() => {
                    if !open {
                        return Err(());
                    }
                    let Some(current) = backend.output_state().await else {
                        return Ok(Some(true));
                    };
                    let present = backend.device_present(&baseline.device_id).await;
                    match check_output_ready(baseline, &current, present) {
                        ResumeCheck::Ready => return Ok(Some(true)),
                        ResumeCheck::AbortDisconnected => return Ok(Some(false)),
                        ResumeCheck::WaitForNonCall => {}
                    }
                }
            }
        }
    })
    .await;

    if watched::read(&cancel, |&cur| cur != expected_gen) {
        return None;
    }

    if let Ok(Ok(decision)) = waited {
        return decision;
    }

    let Some(current) = backend.output_state().await else {
        return Some(true);
    };
    let present = backend.device_present(&baseline.device_id).await;
    Some(should_resume_after_deadline(baseline, &current, present))
}

struct PausedSession<P> {
    baseline: OutputState,
    paused_system: Option<P>,
    paused_page: bool,
}

struct Inner<P> {
    leases: HashMap<Uuid, String>,
    active: Option<PausedSession<P>>,
    generation: u64,
    generation_tx: watch::Sender<u64>,
    _generation_rx: watch::Receiver<u64>,
}

pub(crate) struct Coordinator<B: MediaBackend> {
    backend: B,
    inner: AsyncMutex<Inner<B::Paused>>,
}

impl<B: MediaBackend> Coordinator<B> {
    pub(crate) fn new(backend: B) -> Self {
        let (generation_tx, generation_rx) = watch::channel(0);
        Self {
            backend,
            inner: AsyncMutex::new(Inner {
                leases: HashMap::new(),
                active: None,
                generation: 0,
                generation_tx,
                _generation_rx: generation_rx,
            }),
        }
    }

    pub(crate) async fn enter(&self, window: String, page_paused: bool) -> Uuid {
        let id = Uuid::new_v4();
        let mut inner = self.inner.lock().await;
        inner.generation = inner.generation.wrapping_add(1);
        let _ = inner.generation_tx.send(inner.generation);
        inner.leases.insert(id, window);

        if let Some(existing) = inner.active.as_mut() {
            existing.paused_page |= page_paused;
            return id;
        }

        let baseline = self
            .backend
            .output_state()
            .await
            .unwrap_or_else(unknown_output_state);
        let paused_system = self.backend.pause_if_playing().await;
        if paused_system.is_some() || page_paused {
            inner.active = Some(PausedSession {
                baseline,
                paused_system,
                paused_page: page_paused,
            });
        }
        id
    }

    pub(crate) async fn leave(&self, id: Uuid) -> bool {
        let (baseline, cancel_rx, expected_gen) = {
            let mut inner = self.inner.lock().await;
            if inner.leases.remove(&id).is_none() {
                return false;
            }
            if !inner.leases.is_empty() {
                return false;
            }
            let Some(baseline) = inner.active.as_ref().map(|active| active.baseline.clone()) else {
                return false;
            };
            inner.generation = inner.generation.wrapping_add(1);
            let expected_gen = inner.generation;
            let _ = inner.generation_tx.send(expected_gen);
            (baseline, inner.generation_tx.subscribe(), expected_gen)
        };

        self.finish_when_ready(baseline, cancel_rx, expected_gen)
            .await
    }

    pub(crate) async fn clear_window(&self, window: &str) {
        let pending = {
            let mut inner = self.inner.lock().await;
            let before = inner.leases.len();
            inner.leases.retain(|_, owner| owner != window);
            if inner.leases.len() == before || !inner.leases.is_empty() {
                return;
            }
            let Some(baseline) = inner.active.as_ref().map(|active| active.baseline.clone()) else {
                return;
            };
            inner.generation = inner.generation.wrapping_add(1);
            let expected_gen = inner.generation;
            let _ = inner.generation_tx.send(expected_gen);
            (baseline, inner.generation_tx.subscribe(), expected_gen)
        };

        let _ = self
            .finish_when_ready(pending.0, pending.1, pending.2)
            .await;
    }

    async fn finish_when_ready(
        &self,
        baseline: OutputState,
        cancel_rx: watch::Receiver<u64>,
        expected_gen: u64,
    ) -> bool {
        let Some(allow_resume) =
            wait_for_non_call(&self.backend, &baseline, cancel_rx, expected_gen).await
        else {
            return false;
        };

        let session = {
            let mut inner = self.inner.lock().await;
            if inner.generation != expected_gen || !inner.leases.is_empty() {
                return false;
            }
            inner.active.take()
        };

        let Some(session) = session else {
            return false;
        };
        if !allow_resume {
            return false;
        }
        if let Some(paused) = session.paused_system {
            self.backend.resume(paused).await;
        }
        session.paused_page
    }
}

fn unknown_output_state() -> OutputState {
    OutputState {
        device_id: String::new(),
        kind: OutputKind::Unknown,
        sample_rate: 0,
        channels: 0,
        mic_input_active: false,
    }
}

/// Parses `pactl get-default-sink`, `pactl list sinks short`, and
/// `pactl list source-outputs short` into a normalized `OutputState` and the
/// list of present sink names.
#[cfg(any(
    test,
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
pub(crate) fn parse_pactl_state(
    default_sink: &str,
    sinks_short: &str,
    source_outputs_short: &str,
) -> Option<(OutputState, Vec<String>)> {
    let target = default_sink.trim();
    let mut sinks = Vec::new();
    let mut matched: Option<(String, u32, u16)> = None;

    for line in sinks_short.lines() {
        let mut cols = line.split('\t');
        let _id = cols.next()?;
        let name = cols.next()?.trim();
        if name.is_empty() {
            continue;
        }
        let _driver = cols.next();
        let spec = cols.next().unwrap_or("");
        let (mut rate, mut channels) = parse_sample_spec(spec);
        if is_bluetooth_call_sink_name(name) {
            rate = if rate == 0 { 16_000 } else { rate.min(32_000) };
            channels = if channels == 0 { 1 } else { channels.min(1) };
        }
        sinks.push(name.to_owned());
        if name == target || (target.is_empty() && matched.is_none()) {
            matched = Some((name.to_owned(), rate, channels));
        }
    }

    let (device_id, sample_rate, channels) = matched?;
    let kind = classify_sink_name(&device_id);
    let mic_input_active = source_outputs_short
        .lines()
        .any(|line| !line.trim().is_empty());

    Some((
        OutputState {
            device_id,
            kind,
            sample_rate,
            channels,
            mic_input_active,
        },
        sinks,
    ))
}

#[cfg(any(
    test,
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
pub(crate) fn pactl_device_present(baseline_id: &str, sinks: &[String]) -> bool {
    if baseline_id.is_empty() {
        return false;
    }
    if sinks.iter().any(|sink| sink == baseline_id) {
        return true;
    }
    // PipeWire/PulseAudio renames the suffix of a `bluez_output.<addr>.*` sink
    // when switching a connected headset between A2DP and HFP profiles.
    if let Some(prefix) = bluetooth_card_prefix(baseline_id) {
        return sinks
            .iter()
            .any(|sink| bluetooth_card_prefix(sink) == Some(prefix));
    }
    false
}

#[cfg(any(
    test,
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn bluetooth_card_prefix(sink_name: &str) -> Option<&str> {
    let rest = sink_name.strip_prefix("bluez_output.")?;
    let end = rest.find('.')?;
    Some(&sink_name[..("bluez_output.".len() + end)])
}

#[cfg(any(
    test,
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn parse_sample_spec(spec: &str) -> (u32, u16) {
    let mut rate = 0u32;
    let mut channels = 0u16;
    for part in spec.split_whitespace() {
        if let Some(ch) = part.strip_suffix("ch")
            && let Ok(parsed) = ch.parse::<u16>()
        {
            channels = parsed;
        } else if let Some(hz) = part.strip_suffix("Hz")
            && let Ok(parsed) = hz.parse::<u32>()
        {
            rate = parsed;
        }
    }
    (rate, channels)
}

#[cfg(any(
    test,
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn is_bluetooth_call_sink_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("headset-head-unit")
        || lower.contains("handsfree_head_unit")
        || lower.contains("hfp_hf")
        || lower.contains("hsp_hs")
}

#[cfg(any(
    test,
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn classify_sink_name(name: &str) -> OutputKind {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("bluez_") || lower.contains("bluetooth") {
        OutputKind::Bluetooth
    } else if lower.contains("usb") || lower.contains("headphone") || lower.contains("hdmi") {
        OutputKind::HeadphonesOrExternal
    } else if lower.starts_with("alsa_output.") {
        OutputKind::BuiltInSpeaker
    } else {
        OutputKind::Unknown
    }
}

// ----- macOS backend -----

#[cfg(target_os = "macos")]
mod platform {
    use super::{MediaBackend, OutputKind, OutputState, OutputWatch};
    use async_trait::async_trait;
    use std::{
        ffi::{CStr, CString, c_char, c_void},
        ptr::NonNull,
        sync::Arc,
    };
    use tokio::sync::Notify;

    const MAX_TRACKED_PROCS: usize = 32;

    #[repr(C)]
    struct BeepMacOutputState {
        uid: [c_char; 256],
        kind: u32,
        sample_rate: u32,
        channels: u16,
        mic_input_active: bool,
    }

    unsafe extern "C" {
        fn beep_macos_output_state(out: *mut BeepMacOutputState) -> bool;
        fn beep_macos_device_present(uid: *const c_char) -> bool;
        fn beep_macos_pause_media(out_procs: *mut u32, cap: usize, out_len: *mut usize) -> bool;
        fn beep_macos_resume_media(procs: *const u32, len: usize) -> bool;
        fn beep_macos_watch_start(
            callback: extern "C" fn(*mut c_void),
            ctx: *mut c_void,
        ) -> *mut c_void;
        fn beep_macos_watch_stop(watcher: *mut c_void);
    }

    extern "C" fn notify_coreaudio_change(ctx: *mut c_void) {
        if ctx.is_null() {
            return;
        }
        // SAFETY: `ctx` points to an `Arc<Notify>` kept alive until `beep_macos_watch_stop`
        // drains the serial listener queue in `MacOutputWatch::drop`.
        let notify = unsafe { &*(ctx as *const Notify) };
        notify.notify_one();
    }

    struct MacOutputWatch {
        notify: Arc<Notify>,
        raw_ctx: *const Notify,
        handle: NonNull<c_void>,
    }

    // SAFETY: `handle` is only touched on drop via `beep_macos_watch_stop`, which is
    // thread-safe, and `notify` is `Send + Sync`.
    unsafe impl Send for MacOutputWatch {}

    impl Drop for MacOutputWatch {
        fn drop(&mut self) {
            let ptr = self.handle.as_ptr();
            // SAFETY: `ptr` was returned by `beep_macos_watch_start` and has not been stopped;
            // `beep_macos_watch_stop` drains its serial queue before returning, so `raw_ctx`
            // is never dereferenced again afterwards.
            unsafe {
                beep_macos_watch_stop(ptr);
                drop(Arc::from_raw(self.raw_ctx));
            }
        }
    }

    #[async_trait]
    impl OutputWatch for MacOutputWatch {
        async fn changed(&mut self) -> bool {
            self.notify.notified().await;
            true
        }
    }

    #[derive(Default)]
    pub(crate) struct SystemBackend;

    #[async_trait]
    impl MediaBackend for SystemBackend {
        type Paused = Vec<u32>;

        async fn output_state(&self) -> Option<OutputState> {
            tokio::task::spawn_blocking(|| {
                let mut raw = BeepMacOutputState {
                    uid: [0; 256],
                    kind: 3,
                    sample_rate: 0,
                    channels: 0,
                    mic_input_active: false,
                };
                // SAFETY: `raw` is a valid writable `BeepMacOutputState` on the stack.
                if !unsafe { beep_macos_output_state(&mut raw) } {
                    return None;
                }
                // SAFETY: `beep_macos_output_state` zero-initializes and NUL-terminates `uid`.
                let device_id = unsafe { CStr::from_ptr(raw.uid.as_ptr()) }
                    .to_string_lossy()
                    .into_owned();
                let kind = match raw.kind {
                    0 => OutputKind::BuiltInSpeaker,
                    1 => OutputKind::HeadphonesOrExternal,
                    2 => OutputKind::Bluetooth,
                    _ => OutputKind::Unknown,
                };
                Some(OutputState {
                    device_id,
                    kind,
                    sample_rate: raw.sample_rate,
                    channels: raw.channels,
                    mic_input_active: raw.mic_input_active,
                })
            })
            .await
            .ok()
            .flatten()
        }

        async fn device_present(&self, device_id: &str) -> bool {
            let Ok(c_uid) = CString::new(device_id) else {
                return false;
            };
            tokio::task::spawn_blocking(move || {
                // SAFETY: `c_uid` is a valid NUL-terminated string for this call.
                unsafe { beep_macos_device_present(c_uid.as_ptr()) }
            })
            .await
            .unwrap_or(false)
        }

        async fn pause_if_playing(&self) -> Option<Self::Paused> {
            tokio::task::spawn_blocking(|| {
                let mut procs = [0u32; MAX_TRACKED_PROCS];
                let mut len = 0usize;
                // SAFETY: `procs` holds `MAX_TRACKED_PROCS` writable `u32` slots and `len` is valid.
                let paused = unsafe {
                    beep_macos_pause_media(procs.as_mut_ptr(), MAX_TRACKED_PROCS, &mut len)
                };
                paused.then(|| procs[..len.min(MAX_TRACKED_PROCS)].to_vec())
            })
            .await
            .ok()
            .flatten()
        }

        async fn resume(&self, paused: Self::Paused) {
            let _ = tokio::task::spawn_blocking(move || {
                // SAFETY: `paused` buffer is valid for `paused.len()` elements during this call.
                unsafe { beep_macos_resume_media(paused.as_ptr(), paused.len()) };
            })
            .await;
        }

        async fn watch_output(&self) -> Option<Box<dyn OutputWatch>> {
            tokio::task::spawn_blocking(|| {
                let notify = Arc::new(Notify::new());
                let raw_ctx = Arc::into_raw(Arc::clone(&notify));
                // SAFETY: `notify_coreaudio_change` only reads `raw_ctx` as `&Notify` until
                // `beep_macos_watch_stop` drains its queue.
                let ptr =
                    unsafe { beep_macos_watch_start(notify_coreaudio_change, raw_ctx as *mut _) };
                let Some(handle) = NonNull::new(ptr) else {
                    // SAFETY: `beep_macos_watch_start` failed and kept no reference to `raw_ctx`.
                    unsafe { drop(Arc::from_raw(raw_ctx)) };
                    return None;
                };
                Some(Box::new(MacOutputWatch {
                    notify,
                    raw_ctx,
                    handle,
                }) as Box<dyn OutputWatch>)
            })
            .await
            .ok()
            .flatten()
        }
    }
}

// ----- Linux / BSD backend (MPRIS over D-Bus + PipeWire/PulseAudio) -----

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
mod platform {
    use super::{MediaBackend, OutputState, OutputWatch, pactl_device_present, parse_pactl_state};
    use async_trait::async_trait;
    use std::{process::Stdio, time::Duration};
    use tokio::{
        io::{AsyncBufReadExt as _, BufReader},
        process::{Child, ChildStdout, Command},
        time::timeout,
    };
    use zbus::OwnedVariant;

    const DBUS_CALL_TIMEOUT: Duration = Duration::from_millis(400);
    const PACTL_TIMEOUT: Duration = Duration::from_millis(600);
    const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
    const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
    const MPRIS_PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

    struct PactlWatch {
        _child: Child,
        lines: tokio::io::Lines<BufReader<ChildStdout>>,
    }

    #[async_trait]
    impl OutputWatch for PactlWatch {
        async fn changed(&mut self) -> bool {
            matches!(self.lines.next_line().await, Ok(Some(_)))
        }
    }

    #[derive(Default)]
    pub(crate) struct SystemBackend;

    async fn run_pactl(args: &[&str]) -> Option<String> {
        let output = timeout(
            PACTL_TIMEOUT,
            Command::new("pactl")
                .args(args)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output(),
        )
        .await
        .ok()?
        .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    async fn read_pactl_snapshot() -> Option<(OutputState, Vec<String>)> {
        let (default_sink, sinks_short, source_outputs_short) = tokio::join!(
            run_pactl(&["get-default-sink"]),
            run_pactl(&["list", "sinks", "short"]),
            run_pactl(&["list", "source-outputs", "short"]),
        );
        parse_pactl_state(
            default_sink.as_deref()?,
            sinks_short.as_deref()?,
            source_outputs_short.as_deref().unwrap_or(""),
        )
    }

    async fn mpris_playback_status(conn: &zbus::Connection, bus_name: &str) -> Option<String> {
        let reply = timeout(
            DBUS_CALL_TIMEOUT,
            conn.call_method(
                Some(bus_name),
                MPRIS_PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(MPRIS_PLAYER_IFACE, "PlaybackStatus"),
            ),
        )
        .await
        .ok()?
        .ok()?;
        let value: OwnedVariant = reply.body().deserialize().ok()?;
        <&str>::try_from(&value).ok().map(str::to_owned)
    }

    #[async_trait]
    impl MediaBackend for SystemBackend {
        type Paused = Vec<String>;

        async fn output_state(&self) -> Option<OutputState> {
            read_pactl_snapshot().await.map(|(state, _)| state)
        }

        async fn device_present(&self, device_id: &str) -> bool {
            let Some((_, sinks)) = read_pactl_snapshot().await else {
                return true;
            };
            pactl_device_present(device_id, &sinks)
        }

        async fn pause_if_playing(&self) -> Option<Self::Paused> {
            let conn = timeout(DBUS_CALL_TIMEOUT, zbus::Connection::session())
                .await
                .ok()?
                .ok()?;
            let reply = timeout(
                DBUS_CALL_TIMEOUT,
                conn.call_method(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    Some("org.freedesktop.DBus"),
                    "ListNames",
                    &(),
                ),
            )
            .await
            .ok()?
            .ok()?;
            let names: Vec<String> = reply.body().deserialize().ok()?;
            let mut paused = Vec::new();

            for name in names
                .into_iter()
                .filter(|candidate| candidate.starts_with(MPRIS_PREFIX))
            {
                if mpris_playback_status(&conn, &name).await.as_deref() != Some("Playing") {
                    continue;
                }
                let ok = timeout(
                    DBUS_CALL_TIMEOUT,
                    conn.call_method(
                        Some(name.as_str()),
                        MPRIS_PATH,
                        Some(MPRIS_PLAYER_IFACE),
                        "Pause",
                        &(),
                    ),
                )
                .await
                .is_ok_and(|res| res.is_ok());
                if ok {
                    paused.push(name);
                }
            }

            (!paused.is_empty()).then_some(paused)
        }

        async fn resume(&self, paused: Self::Paused) {
            let Ok(Ok(conn)) = timeout(DBUS_CALL_TIMEOUT, zbus::Connection::session()).await else {
                return;
            };
            for name in paused {
                if mpris_playback_status(&conn, &name).await.as_deref() != Some("Paused") {
                    continue;
                }
                let _ = timeout(
                    DBUS_CALL_TIMEOUT,
                    conn.call_method(
                        Some(name.as_str()),
                        MPRIS_PATH,
                        Some(MPRIS_PLAYER_IFACE),
                        "Play",
                        &(),
                    ),
                )
                .await;
            }
        }

        async fn watch_output(&self) -> Option<Box<dyn OutputWatch>> {
            let mut child = Command::new("pactl")
                .arg("subscribe")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .ok()?;
            let stdout = child.stdout.take()?;
            Some(Box::new(PactlWatch {
                _child: child,
                lines: BufReader::new(stdout).lines(),
            }))
        }
    }
}

// ----- Windows backend (GSMTC + MMDeviceEnumerator) -----

#[cfg(target_os = "windows")]
mod platform {
    use super::{MediaBackend, OutputKind, OutputState, OutputWatch};
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Notify;
    use windows::{
        Media::Control::{
            GlobalSystemMediaTransportControlsSessionManager,
            GlobalSystemMediaTransportControlsSessionPlaybackStatus,
        },
        Win32::{
            Foundation::PROPERTYKEY,
            Media::Audio::{
                AudioSessionStateActive, DEVICE_STATE, DEVICE_STATE_ACTIVE, EDataFlow, ERole,
                IAudioClient, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator,
                IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator, eCapture,
                eMultimedia, eRender,
            },
            System::Com::{
                CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
                STGM_READ, StructuredStorage::PropVariantClear,
            },
            System::Variant::VT_LPWSTR,
        },
        core::{GUID, HSTRING, PCWSTR},
    };
    use windows_core::implement;

    const DEVPKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };

    #[implement(IMMNotificationClient)]
    struct EndpointListener {
        notify: Arc<Notify>,
    }

    impl IMMNotificationClient_Impl for EndpointListener_Impl {
        fn OnDeviceStateChanged(
            &self,
            _pwstrdeviceid: &PCWSTR,
            _dwnewstate: DEVICE_STATE,
        ) -> windows::core::Result<()> {
            self.notify.notify_one();
            Ok(())
        }

        fn OnDeviceAdded(&self, _pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
            self.notify.notify_one();
            Ok(())
        }

        fn OnDeviceRemoved(&self, _pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
            self.notify.notify_one();
            Ok(())
        }

        fn OnDefaultDeviceChanged(
            &self,
            _flow: EDataFlow,
            _role: ERole,
            _pwstrdefaultdeviceid: &PCWSTR,
        ) -> windows::core::Result<()> {
            self.notify.notify_one();
            Ok(())
        }

        fn OnPropertyValueChanged(
            &self,
            _pwstrdeviceid: &PCWSTR,
            _key: &PROPERTYKEY,
        ) -> windows::core::Result<()> {
            self.notify.notify_one();
            Ok(())
        }
    }

    struct WinOutputWatch {
        enumerator: IMMDeviceEnumerator,
        client: IMMNotificationClient,
        notify: Arc<Notify>,
    }

    // SAFETY: `MMDeviceEnumerator` is registered in the multithreaded apartment (`COINIT_MULTITHREADED`),
    // so unregistering the callback on drop from another worker thread is supported by COM.
    unsafe impl Send for WinOutputWatch {}

    impl Drop for WinOutputWatch {
        fn drop(&mut self) {
            // SAFETY: `client` was registered on `enumerator` in `watch_output`.
            unsafe {
                let _ = self
                    .enumerator
                    .UnregisterEndpointNotificationCallback(&self.client);
            }
        }
    }

    #[async_trait]
    impl OutputWatch for WinOutputWatch {
        async fn changed(&mut self) -> bool {
            self.notify.notified().await;
            true
        }
    }

    #[derive(Default)]
    pub(crate) struct SystemBackend;

    fn init_mta() {
        // SAFETY: safe to call per thread; returns `S_FALSE` or `RPC_E_CHANGED_MODE` if already initialized.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
    }

    fn device_id_string(device: &IMMDevice) -> Option<String> {
        // SAFETY: `GetId` allocates a CoTaskMem wide string that we copy and free immediately.
        unsafe {
            let raw = device.GetId().ok()?;
            let text = raw.to_string().ok();
            CoTaskMemFree(Some(raw.0 as *const _));
            text
        }
    }

    fn classify_windows_device(device: &IMMDevice) -> OutputKind {
        // SAFETY: reading `DEVPKEY_DEVICE_FRIENDLY_NAME` from the read-only property store
        // and freeing the returned `PROPVARIANT` via `PropVariantClear`.
        let friendly = unsafe {
            device
                .OpenPropertyStore(STGM_READ)
                .ok()
                .and_then(|store| store.GetValue(&DEVPKEY_DEVICE_FRIENDLY_NAME).ok())
                .map(|mut prop| {
                    let text = if prop.Anonymous.Anonymous.vt == VT_LPWSTR {
                        prop.Anonymous
                            .Anonymous
                            .Anonymous
                            .pwszVal
                            .to_string()
                            .unwrap_or_default()
                            .to_ascii_lowercase()
                    } else {
                        String::new()
                    };
                    let _ = PropVariantClear(&mut prop);
                    text
                })
                .unwrap_or_default()
        };
        if friendly.contains("bluetooth")
            || friendly.contains("hands-free")
            || friendly.contains("airpods")
        {
            OutputKind::Bluetooth
        } else if friendly.contains("headphone") || friendly.contains("headset") {
            OutputKind::HeadphonesOrExternal
        } else if friendly.contains("speaker") {
            OutputKind::BuiltInSpeaker
        } else {
            OutputKind::Unknown
        }
    }

    fn any_capture_session_active(enumerator: &IMMDeviceEnumerator) -> bool {
        // SAFETY: COM calls on an initialized MTA thread with owned interface pointers.
        unsafe {
            let Ok(capture) = enumerator.GetDefaultAudioEndpoint(eCapture, eMultimedia) else {
                return false;
            };
            let Ok(manager) = capture.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                return false;
            };
            let Ok(sessions) = manager.GetSessionEnumerator() else {
                return false;
            };
            let count = sessions.GetCount().unwrap_or(0);
            for idx in 0..count {
                if let Ok(session) = sessions.GetSession(idx)
                    && session.GetState() == Ok(AudioSessionStateActive)
                {
                    return true;
                }
            }
            false
        }
    }

    #[async_trait]
    impl MediaBackend for SystemBackend {
        type Paused = Vec<String>;

        async fn output_state(&self) -> Option<OutputState> {
            tokio::task::spawn_blocking(|| {
                init_mta();
                // SAFETY: COM calls on an initialized MTA thread; `GetMixFormat` buffer freed via `CoTaskMemFree`.
                unsafe {
                    let enumerator: IMMDeviceEnumerator =
                        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
                    let render = enumerator
                        .GetDefaultAudioEndpoint(eRender, eMultimedia)
                        .ok()?;
                    let device_id = device_id_string(&render)?;
                    let kind = classify_windows_device(&render);
                    let mut sample_rate = 0u32;
                    let mut channels = 0u16;
                    if let Ok(client) = render.Activate::<IAudioClient>(CLSCTX_ALL, None)
                        && let Ok(fmt) = client.GetMixFormat()
                        && !fmt.is_null()
                    {
                        sample_rate = (*fmt).nSamplesPerSec;
                        channels = (*fmt).nChannels;
                        CoTaskMemFree(Some(fmt as *const _));
                    }
                    let mic_input_active = any_capture_session_active(&enumerator);
                    Some(OutputState {
                        device_id,
                        kind,
                        sample_rate,
                        channels,
                        mic_input_active,
                    })
                }
            })
            .await
            .ok()
            .flatten()
        }

        async fn device_present(&self, device_id: &str) -> bool {
            let id = HSTRING::from(device_id);
            tokio::task::spawn_blocking(move || {
                init_mta();
                // SAFETY: COM query for `id` on an initialized MTA thread.
                unsafe {
                    let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(
                        &MMDeviceEnumerator,
                        None,
                        CLSCTX_ALL,
                    ) else {
                        return false;
                    };
                    let Ok(dev) = enumerator.GetDevice(&id) else {
                        return false;
                    };
                    dev.GetState() == Ok(DEVICE_STATE_ACTIVE)
                }
            })
            .await
            .unwrap_or(false)
        }

        async fn pause_if_playing(&self) -> Option<Self::Paused> {
            tokio::task::spawn_blocking(|| {
                init_mta();
                let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                    .ok()?
                    .join()
                    .ok()?;
                let sessions = manager.GetSessions().ok()?;
                let mut paused = Vec::new();
                for session in sessions {
                    let status = session
                        .GetPlaybackInfo()
                        .ok()
                        .and_then(|info| info.PlaybackStatus().ok());
                    if status
                        != Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
                    {
                        continue;
                    }
                    let Ok(app_id) = session.SourceAppUserModelId() else {
                        continue;
                    };
                    let ok = session
                        .TryPauseAsync()
                        .ok()
                        .and_then(|op| op.join().ok())
                        .unwrap_or(false);
                    if ok {
                        paused.push(app_id.to_string_lossy());
                    }
                }
                (!paused.is_empty()).then_some(paused)
            })
            .await
            .ok()
            .flatten()
        }

        async fn resume(&self, paused: Self::Paused) {
            let _ = tokio::task::spawn_blocking(move || {
                init_mta();
                let Ok(manager) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                    .and_then(|op| op.join())
                else {
                    return;
                };
                let Ok(sessions) = manager.GetSessions() else {
                    return;
                };
                for session in sessions {
                    let Ok(app_id) = session.SourceAppUserModelId() else {
                        continue;
                    };
                    if !paused
                        .iter()
                        .any(|saved| saved == &app_id.to_string_lossy())
                    {
                        continue;
                    }
                    let status = session
                        .GetPlaybackInfo()
                        .ok()
                        .and_then(|info| info.PlaybackStatus().ok());
                    if status
                        == Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused)
                    {
                        let _ = session.TryPlayAsync().and_then(|op| op.join());
                    }
                }
            })
            .await;
        }

        async fn watch_output(&self) -> Option<Box<dyn OutputWatch>> {
            tokio::task::spawn_blocking(|| {
                init_mta();
                let notify = Arc::new(Notify::new());
                // SAFETY: COM object creation and callback registration on an MTA thread.
                unsafe {
                    let enumerator: IMMDeviceEnumerator =
                        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
                    let client: IMMNotificationClient = EndpointListener {
                        notify: Arc::clone(&notify),
                    }
                    .into();
                    enumerator
                        .RegisterEndpointNotificationCallback(&client)
                        .ok()?;
                    Some(Box::new(WinOutputWatch {
                        enumerator,
                        client,
                        notify,
                    }) as Box<dyn OutputWatch>)
                }
            })
            .await
            .ok()
            .flatten()
        }
    }
}

/// Process-wide media pause/resume coordinator across open desktop windows.
#[derive(Clone)]
pub struct MicMedia {
    inner: Arc<Coordinator<platform::SystemBackend>>,
}

impl Default for MicMedia {
    fn default() -> Self {
        Self {
            inner: Arc::new(Coordinator::new(platform::SystemBackend)),
        }
    }
}

impl MicMedia {
    /// Releases any active microphone leases owned by a destroyed window.
    pub fn clear_window(&self, window: &str) {
        let coord = Arc::clone(&self.inner);
        let owner = window.to_owned();
        tauri::async_runtime::spawn(async move {
            coord.clear_window(&owner).await;
        });
    }
}

#[tauri::command]
pub async fn dictation_mic_enter(
    window: Window,
    mic_media: State<'_, MicMedia>,
    page_paused: bool,
) -> std::result::Result<Uuid, String> {
    Ok(mic_media
        .inner
        .enter(window.label().to_owned(), page_paused)
        .await)
}

#[tauri::command]
pub async fn dictation_mic_leave(
    mic_media: State<'_, MicMedia>,
    id: Uuid,
) -> std::result::Result<bool, String> {
    Ok(mic_media.inner.leave(id).await)
}

#[cfg(test)]
#[path = "../tests/chat_mic_media.rs"]
mod tests;
