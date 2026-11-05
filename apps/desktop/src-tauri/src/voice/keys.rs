//! The Option key in live mode: two taps, a hold to talk and a single tap,
//! and the microphone opened early as Option goes down.

use super::{
    LIVE_ENABLED, Voice, audio,
    hotkey::{self, Act, HOLD, HoldStep, Input as Key, TapStep, WINDOW},
    session::{Start, open_audio},
};
use crate::bar;
use beep_live::Input;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::sleep;

/// How long the microphone opened when Option went down stays open for a
/// hold or a tap that never came.
const WARM_FOR: Duration = Duration::from_millis(1500);

impl Voice {
    /// Every Option change, from the hotkey monitor on the main thread.
    pub fn on_input(self: &Arc<Self>, key: Key) {
        let now = Instant::now();
        let (taps, hold) = {
            let mut state = self.state();
            let hold = state.hold.on_event(key, now);
            let taps = state.taps.on_event(key, now);
            if taps != TapStep::Nothing {
                state.hold.forget_tap();
            }
            (taps, hold)
        };
        if taps == TapStep::Two {
            self.on_two_taps();
        }
        // Two taps always open the bar. Holding Option says nothing unless live mode is on and
        // there is a server: talking needs it.
        if !(LIVE_ENABLED && self.beep.linked()) {
            return;
        }
        match hold {
            HoldStep::Wait(press) => {
                if self.voice() == hotkey::Voice::Off {
                    self.warm_up(press);
                }
                let me = self.clone();
                tauri::async_runtime::spawn(async move {
                    sleep(HOLD).await;
                    me.on_hold_timer(press);
                });
            }
            HoldStep::Send => self.release(),
            HoldStep::Cancel => self.input_now(Input::HoldCancel),
            HoldStep::Tap(press) => {
                let me = self.clone();
                tauri::async_runtime::spawn(async move {
                    sleep(WINDOW).await;
                    me.on_tap_timer(press);
                });
            }
            HoldStep::Start | HoldStep::Single | HoldStep::Nothing => {}
        }
    }

    /// Two taps: open or hide the bar. Over the orb they bring the bar back
    /// with live mode still running; with the bar up they leave live mode,
    /// and the bar opens on the chat. With a notice card up they open its
    /// chat.
    fn on_two_taps(self: &Arc<Self>) {
        self.cool(None);
        match hotkey::on_two(self.voice(), bar::is_open(&self.app)) {
            // A notice card up: two taps open its chat, as Open does.
            hotkey::Two::Toggle if !bar::is_open(&self.app) && self.open_any_card() => {}
            hotkey::Two::Toggle => bar::toggle(&self.app),
            hotkey::Two::ShowBar => bar::show(&self.app, None),
            hotkey::Two::Leave => {
                self.leave_live();
                if !self.is_live() {
                    bar::show(&self.app, None);
                }
            }
        }
    }

    /// `HOLD` after Option went down: talk while it stays down.
    fn on_hold_timer(self: &Arc<Self>, press: u64) {
        let act = hotkey::on_held(self.voice(), bar::is_open(&self.app));
        let (step, tapped) = {
            let mut state = self.state();
            let step = state
                .hold
                .on_timer(press, hotkey::since_press(), Instant::now());
            (step, state.hold.tapped())
        };
        if step != HoldStep::Start {
            // Option went with another key: nothing listens. A tap keeps
            // the microphone for the live mode it may start.
            if !tapped {
                self.cool(Some(press));
            }
            return;
        }
        match act {
            Act::Live => self.input_now(Input::HoldStart),
            Act::Start => {
                let chat = self.open_chat();
                self.start(Start::Hold, chat);
            }
            Act::EndNote | Act::Nothing => self.cool(Some(press)),
        }
    }

    /// A single tap stood alone: interrupt or leave live mode, stop a voice
    /// note, or start live mode in a bar the user opened.
    fn on_tap_timer(self: &Arc<Self>, press: u64) {
        let step = self
            .state()
            .hold
            .on_tap_timer(press, hotkey::since_press(), Instant::now());
        if step != HoldStep::Single {
            return self.cool(Some(press));
        }
        match hotkey::on_single(self.voice(), bar::is_open(&self.app)) {
            Act::Live => self.input_now(Input::Tap),
            Act::EndNote => self.leave_live(),
            Act::Start => {
                let chat = self.open_chat();
                self.start(Start::Tap, chat);
            }
            Act::Nothing => self.cool(Some(press)),
        }
    }

    /// Opens the microphone as Option goes down in the open bar, so a hold
    /// or a tap that follows hears from the start. Only once the user has
    /// allowed the microphone: asking belongs to starting live mode.
    fn warm_up(self: &Arc<Self>, press: u64) {
        if !bar::is_open(&self.app) || !audio::mic_allowed() {
            return;
        }
        {
            let mut state = self.state();
            if state.live.is_some() || state.warm.is_some() {
                return;
            }
            state.warm = Some((press, open_audio(true)));
        }
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            sleep(WARM_FOR).await;
            me.cool(Some(press));
        });
    }

    /// Closes the microphone opened for press `press`, or for any press.
    fn cool(&self, press: Option<u64>) {
        self.state()
            .warm
            .take_if(|(p, _)| press.is_none_or(|press| *p == press));
    }
}
