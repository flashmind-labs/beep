use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;

struct FakeWatch {
    notify: Arc<Notify>,
}

#[async_trait]
impl OutputWatch for FakeWatch {
    async fn changed(&mut self) -> bool {
        self.notify.notified().await;
        true
    }
}

struct FakeState {
    output: Option<OutputState>,
    present: bool,
    playing_player: Option<String>,
    resumed: Vec<String>,
}

struct FakeBackend {
    state: Arc<Mutex<FakeState>>,
    notify: Arc<Notify>,
    pause_calls: AtomicUsize,
    deadline: Duration,
}

impl FakeBackend {
    fn new(output: OutputState, playing_player: Option<&str>, deadline: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                output: Some(output),
                present: true,
                playing_player: playing_player.map(str::to_owned),
                resumed: Vec::new(),
            })),
            notify: Arc::new(Notify::new()),
            pause_calls: AtomicUsize::new(0),
            deadline,
        }
    }

    fn set_output(&self, output: OutputState, present: bool) {
        {
            let mut guard = self.state.lock().unwrap();
            guard.output = Some(output);
            guard.present = present;
        }
        self.notify.notify_waiters();
    }

    fn resumed(&self) -> Vec<String> {
        self.state.lock().unwrap().resumed.clone()
    }
}

#[async_trait]
impl MediaBackend for Arc<FakeBackend> {
    type Paused = String;

    async fn output_state(&self) -> Option<OutputState> {
        self.state.lock().unwrap().output.clone()
    }

    async fn device_present(&self, _device_id: &str) -> bool {
        self.state.lock().unwrap().present
    }

    async fn pause_if_playing(&self) -> Option<Self::Paused> {
        self.pause_calls.fetch_add(1, Ordering::SeqCst);
        self.state.lock().unwrap().playing_player.take()
    }

    async fn resume(&self, paused: Self::Paused) {
        let mut guard = self.state.lock().unwrap();
        guard.playing_player = Some(paused.clone());
        guard.resumed.push(paused);
    }

    async fn watch_output(&self) -> Option<Box<dyn OutputWatch>> {
        Some(Box::new(FakeWatch {
            notify: Arc::clone(&self.notify),
        }))
    }

    fn non_call_deadline(&self) -> Duration {
        self.deadline
    }
}

fn bt_stereo(rate: u32) -> OutputState {
    OutputState {
        device_id: "bt-headphones".into(),
        kind: OutputKind::Bluetooth,
        sample_rate: rate,
        channels: 2,
        mic_input_active: false,
    }
}

fn bt_call(rate: u32, mic_input_active: bool) -> OutputState {
    OutputState {
        device_id: "bt-headphones".into(),
        kind: OutputKind::Bluetooth,
        sample_rate: rate,
        channels: 1,
        mic_input_active,
    }
}

#[tokio::test]
async fn pauses_music_on_mic_enter_and_waits_for_bluetooth_to_leave_call_mode() {
    let backend = Arc::new(FakeBackend::new(
        bt_stereo(48_000),
        Some("spotify"),
        Duration::from_secs(2),
    ));
    let coord = Arc::new(Coordinator::new(Arc::clone(&backend)));

    let lease = coord.enter("center".into(), true).await;
    assert_eq!(backend.pause_calls.load(Ordering::SeqCst), 1);

    // Microphone opens and headphones drop into 24 kHz mono call mode.
    backend.set_output(bt_call(24_000, true), true);

    let leaving = {
        let coord = Arc::clone(&coord);
        tokio::spawn(async move { coord.leave(lease).await })
    };
    tokio::task::yield_now().await;
    assert!(
        backend.resumed().is_empty(),
        "must not resume while mic input and call-mode sample rate are still active"
    );

    // Mic closes first, but headphones are still in 24 kHz mono call mode.
    backend.set_output(bt_call(24_000, false), true);
    tokio::task::yield_now().await;
    assert!(
        backend.resumed().is_empty(),
        "must wait until output switches back from mono call mode to stereo"
    );

    // Headphones renegotiate stereo A2DP (44.1 kHz is valid stereo even when baseline was 48 kHz).
    backend.set_output(bt_stereo(44_100), true);
    let resume_page = leaving.await.unwrap();
    assert!(resume_page);
    assert_eq!(backend.resumed(), vec!["spotify"]);
}

#[tokio::test]
async fn silent_session_never_starts_playback_on_leave() {
    let backend = Arc::new(FakeBackend::new(
        bt_stereo(48_000),
        None,
        Duration::from_secs(1),
    ));
    let coord = Coordinator::new(Arc::clone(&backend));

    let lease = coord.enter("center".into(), false).await;
    let resume_page = coord.leave(lease).await;
    assert!(!resume_page);
    assert!(backend.resumed().is_empty());
}

#[tokio::test]
async fn back_to_back_dictations_keep_music_paused_until_last_one_leaves_call_mode() {
    let backend = Arc::new(FakeBackend::new(
        bt_stereo(48_000),
        Some("music-app"),
        Duration::from_secs(2),
    ));
    let coord = Arc::new(Coordinator::new(Arc::clone(&backend)));

    let first = coord.enter("center".into(), false).await;
    backend.set_output(bt_call(16_000, false), true);

    let first_leave = {
        let coord = Arc::clone(&coord);
        tokio::spawn(async move { coord.leave(first).await })
    };
    tokio::task::yield_now().await;

    // User holds push-to-talk again before Bluetooth returned to stereo.
    let second = coord.enter("center".into(), true).await;
    assert!(!first_leave.await.unwrap());
    assert!(backend.resumed().is_empty());
    assert_eq!(
        backend.pause_calls.load(Ordering::SeqCst),
        1,
        "keeps the original paused player and pre-call baseline instead of re-querying in call mode"
    );

    let second_leave = {
        let coord = Arc::clone(&coord);
        tokio::spawn(async move { coord.leave(second).await })
    };
    tokio::task::yield_now().await;
    assert!(backend.resumed().is_empty());

    backend.set_output(bt_stereo(48_000), true);
    assert!(second_leave.await.unwrap());
    assert_eq!(backend.resumed(), vec!["music-app"]);
}

#[tokio::test]
async fn disconnecting_headphones_onto_speakers_leaves_music_paused() {
    let backend = Arc::new(FakeBackend::new(
        bt_stereo(48_000),
        Some("spotify"),
        Duration::from_secs(1),
    ));
    let coord = Coordinator::new(Arc::clone(&backend));

    let lease = coord.enter("center".into(), true).await;
    backend.set_output(
        OutputState {
            device_id: "built-in-speakers".into(),
            kind: OutputKind::BuiltInSpeaker,
            sample_rate: 48_000,
            channels: 2,
            mic_input_active: false,
        },
        false,
    );

    let resume_page = coord.leave(lease).await;
    assert!(!resume_page);
    assert!(backend.resumed().is_empty());
}

#[tokio::test]
async fn deadline_keeps_music_paused_during_external_call_but_resumes_when_mic_is_idle() {
    let in_call_backend = Arc::new(FakeBackend::new(
        bt_stereo(48_000),
        Some("spotify"),
        Duration::from_millis(30),
    ));
    let in_call_coord = Coordinator::new(Arc::clone(&in_call_backend));
    let lease = in_call_coord.enter("center".into(), true).await;
    // Another app (e.g. voice call) keeps the microphone open past the deadline.
    in_call_backend.set_output(bt_call(16_000, true), true);
    assert!(!in_call_coord.leave(lease).await);
    assert!(in_call_backend.resumed().is_empty());

    let unusual_rate_backend = Arc::new(FakeBackend::new(
        bt_stereo(48_000),
        Some("spotify"),
        Duration::from_millis(30),
    ));
    let unusual_coord = Coordinator::new(Arc::clone(&unusual_rate_backend));
    let lease2 = unusual_coord.enter("center".into(), true).await;
    // Microphone is closed and headphones are still connected, even if sample rate stayed low.
    unusual_rate_backend.set_output(bt_call(32_000, false), true);
    assert!(unusual_coord.leave(lease2).await);
    assert_eq!(unusual_rate_backend.resumed(), vec!["spotify"]);
}

#[test]
fn parses_pactl_bluetooth_profile_switches_and_disconnects() {
    let a2dp_sinks = "51\talsa_output.pci-0000_00_1f.3.analog-stereo\tPipeWire\ts32le 2ch 48000Hz\tSUSPENDED\n\
                      64\tbluez_output.AA_BB_CC_DD_EE_FF.a2dp-sink\tPipeWire\ts32le 2ch 48000Hz\tRUNNING\n";
    let (baseline, baseline_sinks) =
        parse_pactl_state("bluez_output.AA_BB_CC_DD_EE_FF.a2dp-sink\n", a2dp_sinks, "")
            .expect("parses A2DP sink");
    assert_eq!(baseline.kind, OutputKind::Bluetooth);
    assert_eq!(baseline.sample_rate, 48_000);
    assert_eq!(baseline.channels, 2);
    assert!(!baseline.mic_input_active);
    assert!(pactl_device_present(&baseline.device_id, &baseline_sinks));

    let hfp_sinks = "51\talsa_output.pci-0000_00_1f.3.analog-stereo\tPipeWire\ts32le 2ch 48000Hz\tSUSPENDED\n\
                     79\tbluez_output.AA_BB_CC_DD_EE_FF.headset-head-unit\tPipeWire\ts16le 1ch 16000Hz\tRUNNING\n";
    let (hfp_state, hfp_list) = parse_pactl_state(
        "bluez_output.AA_BB_CC_DD_EE_FF.headset-head-unit\n",
        hfp_sinks,
        "",
    )
    .expect("parses HFP sink");
    assert!(
        pactl_device_present(&baseline.device_id, &hfp_list),
        "same Bluetooth card in HFP profile still counts as connected"
    );
    assert_eq!(
        check_output_ready(&baseline, &hfp_state, true),
        ResumeCheck::WaitForNonCall
    );

    let (back_to_a2dp, _) =
        parse_pactl_state("bluez_output.AA_BB_CC_DD_EE_FF.a2dp-sink\n", a2dp_sinks, "").unwrap();
    assert_eq!(
        check_output_ready(&baseline, &back_to_a2dp, true),
        ResumeCheck::Ready
    );

    let unplugged_sinks =
        "51\talsa_output.pci-0000_00_1f.3.analog-stereo\tPipeWire\ts32le 2ch 48000Hz\tIDLE\n";
    let (speaker_state, speaker_list) = parse_pactl_state(
        "alsa_output.pci-0000_00_1f.3.analog-stereo\n",
        unplugged_sinks,
        "",
    )
    .unwrap();
    let present = pactl_device_present(&baseline.device_id, &speaker_list);
    assert!(!present);
    assert_eq!(
        check_output_ready(&baseline, &speaker_state, present),
        ResumeCheck::AbortDisconnected
    );
}
