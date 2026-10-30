//! Sound for live mode: the microphone, cleaned of Beep's own voice by
//! macOS voice processing, and the player for Beep's answers. Both run in
//! one `AVAudioEngine`, which echo cancellation needs, on a thread of its
//! own since the engine stays where it was made.

use std::time::Duration;

/// The rate the server's live transcription takes.
pub const RATE: u32 = beep_proto::TRANSCRIBE_RATE;
/// How finely the loudness of an answer is kept, for the orb.
pub const ENVELOPE_STEP: Duration = Duration::from_millis(50);

/// A stretch of microphone sound: 16 kHz mono 16-bit little-endian PCM,
/// and how loud it was from 0 to 1.
#[derive(Debug, Clone)]
pub struct MicChunk {
    pub pcm: Vec<u8>,
    pub level: f32,
}

/// The user has not let Beep use the microphone.
#[derive(Debug, thiserror::Error)]
#[error("Beep is not allowed to use the microphone")]
pub struct MicDenied;

/// An answer that started playing.
#[derive(Debug, Clone, Default)]
pub struct Played {
    /// Loudness from 0 to 1 for each `ENVELOPE_STEP`.
    pub envelope: Vec<f32>,
}

impl Played {
    /// How loud the answer is `at` into it.
    pub fn level_at(&self, at: Duration) -> f32 {
        let step = ENVELOPE_STEP.as_millis().max(1);
        usize::try_from(at.as_millis() / step)
            .ok()
            .and_then(|i| self.envelope.get(i))
            .copied()
            .unwrap_or(0.0)
    }
}

/// Loudness of `samples` from 0 to 1. Speech peaks near 0.3 RMS, so it is
/// scaled up to fill the range.
pub fn level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    let rms = (sum / samples.len() as f32).sqrt();
    (rms * 4.0).min(1.0)
}

/// Resamples by linear interpolation. Plenty for speech going to a model.
pub fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() || from == 0 || to == 0 {
        return samples.to_vec();
    }
    let step = f64::from(from) / f64::from(to);
    let len = (samples.len() as f64 / step).floor() as usize;
    (0..len)
        .map(|i| {
            let at = i as f64 * step;
            let low = at.floor() as usize;
            let high = (low + 1).min(samples.len() - 1);
            let frac = (at - low as f64) as f32;
            samples[low] * (1.0 - frac) + samples[high] * frac
        })
        .collect()
}

/// `samples` as 16-bit little-endian PCM.
pub fn pcm16(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Loudness of each `ENVELOPE_STEP` of `samples` at `rate`.
pub fn envelope(samples: &[f32], rate: f64) -> Vec<f32> {
    let per = ((rate * ENVELOPE_STEP.as_secs_f64()) as usize).max(1);
    samples.chunks(per).map(level).collect()
}

#[cfg(target_os = "macos")]
mod engine {
    use super::{MicChunk, MicDenied, Played, RATE, envelope, level, pcm16, resample};
    use anyhow::{Context, anyhow};
    use block2::RcBlock;
    use objc2::{AllocAnyThread, rc::Retained, runtime::Bool};
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
    use objc2_avf_audio::{
        AVAudioEngine, AVAudioFile, AVAudioFormat, AVAudioPCMBuffer, AVAudioPlayerNode,
        AVAudioPlayerNodeCompletionCallbackType, AVAudioTime,
    };
    use objc2_foundation::{NSString, NSURL};
    use std::{
        path::{Path, PathBuf},
        ptr::NonNull,
        sync::{
            Mutex, PoisonError,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };
    use tokio::sync::{mpsc::Sender, oneshot};

    /// Frames the microphone tap asks for at a time: about 100 ms.
    const TAP_FRAMES: u32 = 4096;
    /// How long Bluetooth headphones get to finish switching modes.
    const DEVICE_SETTLE: Duration = Duration::from_millis(500);

    enum Command {
        Play {
            path: PathBuf,
            started: oneshot::Sender<anyhow::Result<Played>>,
            done: oneshot::Sender<()>,
        },
        StopPlaying,
    }

    /// The engine, running until dropped.
    pub struct Audio {
        commands: mpsc::Sender<Command>,
    }

    impl Audio {
        /// Opens the microphone with echo cancellation and starts sending
        /// it to `mic`. The first time, macOS asks the user. Blocks until
        /// the engine runs, or fails with `MicDenied` when not allowed.
        pub fn open(mic: Sender<MicChunk>) -> anyhow::Result<Self> {
            allowed()?;
            let (commands, inbox) = mpsc::channel();
            let (ready, opened) = mpsc::channel();
            thread::Builder::new()
                .name("beep-audio".into())
                .spawn(move || run(mic, inbox, ready))
                .context("starting the audio thread")?;
            opened
                .recv()
                .map_err(|_| anyhow!("the audio thread stopped"))??;
            Ok(Self { commands })
        }

        /// Plays the MP3 at `path`. Returns once it started, with its
        /// loudness; `done` fires when it ends or is stopped.
        pub async fn play(&self, path: &Path) -> anyhow::Result<(Played, oneshot::Receiver<()>)> {
            let (started, start) = oneshot::channel();
            let (done, finished) = oneshot::channel();
            self.commands
                .send(Command::Play {
                    path: path.to_owned(),
                    started,
                    done,
                })
                .map_err(|_| anyhow!("the audio thread stopped"))?;
            let played = start
                .await
                .map_err(|_| anyhow!("the audio thread stopped"))??;
            Ok((played, finished))
        }

        pub fn stop_playing(&self) {
            // A stopped thread plays nothing already.
            let _ = self.commands.send(Command::StopPlaying);
        }
    }

    /// Asks the user for the microphone when macOS has not yet, and waits
    /// for the answer. An engine started before that hears only silence, or
    /// does not start at all.
    fn allowed() -> anyhow::Result<()> {
        // SAFETY: AVMediaTypeAudio is a constant AVFoundation defines.
        let audio = unsafe { AVMediaTypeAudio }.context("AVFoundation has no audio media type")?;
        // SAFETY: a class method safe to call from any thread.
        let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) };
        match status {
            AVAuthorizationStatus::Authorized => Ok(()),
            AVAuthorizationStatus::NotDetermined => {
                let (answer, answered) = mpsc::channel();
                let asked = RcBlock::new(move |granted: Bool| {
                    // `allowed` waits for this, so it is there to hear it.
                    let _ = answer.send(granted.as_bool());
                });
                // SAFETY: the handler may run on any thread, which the
                // channel allows.
                unsafe {
                    AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &asked)
                };
                match answered.recv() {
                    Ok(true) => Ok(()),
                    Ok(false) => Err(MicDenied.into()),
                    Err(_) => Err(anyhow!("macOS did not answer the microphone request")),
                }
            }
            _ => Err(MicDenied.into()),
        }
    }

    /// The audio thread: builds the engine, then plays what it is asked
    /// until `Audio` drops.
    fn run(
        mic: Sender<MicChunk>,
        inbox: mpsc::Receiver<Command>,
        ready: mpsc::Sender<anyhow::Result<()>>,
    ) {
        // Voice processing refuses to start when the microphone and the
        // output run at different rates. Bluetooth headphones switch to
        // their call mode when it starts, so the first try can read the
        // microphone's rate from before the switch; the second sees the
        // new one. Failing that, headphones need no echo cancellation.
        let built = build(mic.clone(), true)
            .or_else(|e| {
                log::warn!("starting voice processing again: {e:#}");
                thread::sleep(DEVICE_SETTLE);
                build(mic.clone(), true)
            })
            .or_else(|e| {
                log::warn!("starting without voice processing: {e:#}");
                build(mic, false)
            });
        let (engine, player) = match built {
            Ok(built) => {
                log::info!("microphone running");
                // `open` waits for this, so it is there to hear it.
                let _ = ready.send(Ok(()));
                built
            }
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        };
        while let Ok(command) = inbox.recv() {
            match command {
                Command::Play {
                    path,
                    started,
                    done,
                } => {
                    let played = play(&engine, &player, &path, done);
                    // Nobody waiting means nobody wants to know.
                    let _ = started.send(played);
                }
                // SAFETY: the player belongs to this thread's engine.
                Command::StopPlaying => unsafe { player.stop() },
            }
        }
        // SAFETY: the player belongs to this thread's engine.
        unsafe { player.stop() };
        tear_down(&engine);
    }

    /// Stops `engine`, removes its microphone tap and turns voice processing
    /// off. Left on, voice processing holds the devices and the next engine,
    /// even one without it, hears nothing.
    fn tear_down(engine: &AVAudioEngine) {
        // SAFETY: the engine was made on this thread, and the tap, when
        // there is one, is on bus 0 of its input node.
        unsafe {
            let input = engine.inputNode();
            input.removeTapOnBus(0);
            engine.stop();
            if input.isVoiceProcessingEnabled()
                && let Err(e) = input.setVoiceProcessingEnabled_error(false)
            {
                log::warn!("voice processing stayed on: {}", e.localizedDescription());
            }
        }
    }

    fn build(
        mic: Sender<MicChunk>,
        voice_processing: bool,
    ) -> anyhow::Result<(Retained<AVAudioEngine>, Retained<AVAudioPlayerNode>)> {
        // SAFETY: everything below runs on this one thread, on objects it
        // made, with the arguments AVFoundation documents.
        unsafe {
            let engine = AVAudioEngine::new();
            let input = engine.inputNode();
            if voice_processing && let Err(e) = input.setVoiceProcessingEnabled_error(true) {
                log::warn!(
                    "voice processing is off, so Beep may hear itself: {}",
                    e.localizedDescription()
                );
            }
            let format = input.outputFormatForBus(0);
            let rate = format.sampleRate();
            log::info!(
                "microphone at {rate} Hz, {} channel(s), voice processing {}",
                format.channelCount(),
                if voice_processing { "on" } else { "off" }
            );
            let heard = AtomicBool::new(false);
            let tap = RcBlock::new(
                move |buffer: NonNull<AVAudioPCMBuffer>, _: NonNull<AVAudioTime>| {
                    if !heard.swap(true, Ordering::Relaxed) {
                        log::info!("first sound from the microphone");
                    }
                    // SAFETY: AVFoundation passes a live buffer for the length
                    // of the call.
                    let buffer = buffer.as_ref();
                    let Some(samples) = channel_zero(buffer) else {
                        return;
                    };
                    let level = level(&samples);
                    let speech = resample(&samples, rate as u32, RATE);
                    // A full queue drops a stretch; the next one goes through.
                    let _ = mic.try_send(MicChunk {
                        pcm: pcm16(&speech),
                        level,
                    });
                },
            );
            input.installTapOnBus_bufferSize_format_block(
                0,
                super::engine::TAP_FRAMES,
                Some(&format),
                &*tap as *const _ as *mut _,
            );
            let player = AVAudioPlayerNode::new();
            engine.attachNode(&player);
            engine.connect_to_format(&player, &engine.mainMixerNode(), None);
            if voice_processing {
                // Voice processing wants the output at the microphone's
                // rate; left alone it takes the speakers' rate, and when
                // the two differ the engine does not start.
                let Some(output) = AVAudioFormat::initStandardFormatWithSampleRate_channels(
                    AVAudioFormat::alloc(),
                    rate,
                    2,
                ) else {
                    tear_down(&engine);
                    return Err(anyhow!("no output format at {rate} Hz"));
                };
                engine.connect_to_format(
                    &engine.mainMixerNode(),
                    &engine.outputNode(),
                    Some(&output),
                );
            }
            engine.prepare();
            if let Err(e) = engine.startAndReturnError() {
                tear_down(&engine);
                return Err(anyhow!(
                    "the microphone could not start: {}",
                    e.localizedDescription()
                ));
            }
            Ok((engine, player))
        }
    }

    /// The first channel of `buffer`, as floats.
    ///
    /// # Safety
    ///
    /// `buffer` must hold float samples, as the engine's taps and files
    /// opened for reading do.
    unsafe fn channel_zero(buffer: &AVAudioPCMBuffer) -> Option<Vec<f32>> {
        // SAFETY: per the caller, the data is float, `frameLength` frames
        // long, and the first channel pointer is valid while `buffer` is.
        unsafe {
            let channels = buffer.floatChannelData();
            if channels.is_null() {
                return None;
            }
            let first = *channels;
            let frames = buffer.frameLength() as usize;
            Some(std::slice::from_raw_parts(first.as_ptr(), frames).to_vec())
        }
    }

    fn play(
        engine: &AVAudioEngine,
        player: &AVAudioPlayerNode,
        path: &Path,
        done: oneshot::Sender<()>,
    ) -> anyhow::Result<Played> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        // SAFETY: the engine and player belong to this thread; the file
        // is read into a buffer of its own processing format and length.
        unsafe {
            let file = AVAudioFile::initForReading_error(AVAudioFile::alloc(), &url)
                .map_err(|e| anyhow!("reading the answer: {}", e.localizedDescription()))?;
            let format = file.processingFormat();
            let frames = u32::try_from(file.length()).context("the answer is too long")?;
            let buffer = AVAudioPCMBuffer::initWithPCMFormat_frameCapacity(
                AVAudioPCMBuffer::alloc(),
                &format,
                frames,
            )
            .context("no room for the answer")?;
            file.readIntoBuffer_error(&buffer)
                .map_err(|e| anyhow!("reading the answer: {}", e.localizedDescription()))?;
            let rate = format.sampleRate();
            let samples = channel_zero(&buffer).unwrap_or_default();
            player.stop();
            engine.connect_to_format(player, &engine.mainMixerNode(), Some(&format));
            let done = Mutex::new(Some(done));
            let finished = RcBlock::new(move || {
                let done = done.lock().unwrap_or_else(PoisonError::into_inner).take();
                if let Some(done) = done {
                    // Gone when live mode ended meanwhile.
                    let _ = done.send(());
                }
            });
            player.scheduleBuffer_completionCallbackType_completionHandler(
                &buffer,
                AVAudioPlayerNodeCompletionCallbackType::DataPlayedBack,
                &*finished as *const _ as *mut _,
            );
            player.play();
            Ok(Played {
                envelope: envelope(&samples, rate),
            })
        }
    }
}

#[cfg(target_os = "macos")]
pub use engine::Audio;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_level_zero_and_loud_speech_tops_out() {
        assert_eq!(level(&[]), 0.0);
        assert_eq!(level(&[0.0; 100]), 0.0);
        assert_eq!(level(&[0.5, -0.5, 0.5, -0.5]), 1.0);
        let quiet = level(&[0.05, -0.05]);
        assert!(quiet > 0.1 && quiet < 0.3);
    }

    #[test]
    fn a_second_at_48k_becomes_a_second_at_16k() {
        let out = resample(&vec![0.1; 48_000], 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|s| (s - 0.1).abs() < 1e-6));
    }

    #[test]
    fn pcm_is_little_endian_and_clamped() {
        assert_eq!(pcm16(&[0.0, 1.0, -2.0]), [0, 0, 0xff, 0x7f, 0x01, 0x80]);
    }

    #[test]
    fn the_envelope_follows_the_answer() {
        let mut samples = vec![0.0; 2400];
        samples.extend(vec![0.5; 2400]);
        let played = Played {
            envelope: envelope(&samples, 24_000.0),
        };
        assert_eq!(played.envelope.len(), 4);
        assert_eq!(played.level_at(Duration::from_millis(10)), 0.0);
        assert_eq!(played.level_at(Duration::from_millis(160)), 1.0);
        assert_eq!(played.level_at(Duration::from_secs(5)), 0.0);
    }
}
