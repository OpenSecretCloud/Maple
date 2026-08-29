//! Microphone capture and speech playback.
//!
//! One thread owns the audio devices. `cpal` and `rodio` streams are not
//! `Send` on every platform, so the UI and backend never touch them; they
//! send commands here and await the replies. Recordings come back as 16 kHz
//! mono 16-bit WAV, the format the Maple web app sends to Whisper.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::Cursor;
use std::sync::mpsc::{RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

/// Sample rate of the WAV handed to transcription.
pub const RECORDING_SAMPLE_RATE: u32 = 16_000;

/// Longest recording kept, so a forgotten microphone cannot grow without
/// bound. Whisper requests carry the whole file in one body.
const MAX_RECORDING: Duration = Duration::from_secs(10 * 60);

enum Command {
    StartRecording(oneshot::Sender<Result<(), String>>),
    /// Stop and return the recording as WAV bytes.
    StopRecording(oneshot::Sender<Result<Vec<u8>, String>>),
    /// Stop and discard the recording.
    CancelRecording,
    /// Queue one WAV clip. A new generation replaces what is playing.
    Play {
        generation: u64,
        wav: Vec<u8>,
        reply: oneshot::Sender<Result<(), String>>,
    },
    /// Resolve when every clip of `generation` has played, or at once
    /// when that generation is no longer current.
    AwaitIdle {
        generation: u64,
        reply: oneshot::Sender<()>,
    },
    /// Stop playback and refuse clips older than `generation`.
    StopPlayback {
        generation: u64,
    },
}

/// Handle to the audio thread. Cheap to clone through an `Arc`.
pub struct AudioEngine {
    tx: Mutex<Sender<Command>>,
}

impl AudioEngine {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("maple-audio".into())
            .spawn(move || AudioThread::default().run(rx))
            .expect("spawn audio thread");
        Self { tx: Mutex::new(tx) }
    }

    fn send(&self, command: Command) {
        let tx = self
            .tx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if tx.send(command).is_err() {
            log::error!("audio thread is gone");
        }
    }

    pub async fn start_recording(&self) -> Result<(), String> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::StartRecording(reply));
        rx.await
            .unwrap_or_else(|_| Err("The audio thread stopped".to_string()))
    }

    pub async fn stop_recording(&self) -> Result<Vec<u8>, String> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::StopRecording(reply));
        rx.await
            .unwrap_or_else(|_| Err("The audio thread stopped".to_string()))
    }

    pub fn cancel_recording(&self) {
        self.send(Command::CancelRecording);
    }

    pub async fn play(&self, generation: u64, wav: Vec<u8>) -> Result<(), String> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Play {
            generation,
            wav,
            reply,
        });
        rx.await
            .unwrap_or_else(|_| Err("The audio thread stopped".to_string()))
    }

    pub async fn await_idle(&self, generation: u64) {
        let (reply, rx) = oneshot::channel();
        self.send(Command::AwaitIdle { generation, reply });
        rx.await.ok();
    }

    pub fn stop_playback(&self, generation: u64) {
        self.send(Command::StopPlayback { generation });
    }
}

/// Samples captured so far, interleaved as the device delivers them.
struct Recording {
    _stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
}

struct Playback {
    sink: rodio::MixerDeviceSink,
    /// `None` after a stop: a stopped player leaves the mixer, so the next
    /// clip needs a fresh one.
    player: Option<rodio::Player>,
    /// Newest generation seen; older clips are refused.
    generation: u64,
}

#[derive(Default)]
struct AudioThread {
    recording: Option<Recording>,
    playback: Option<Playback>,
    /// Callers waiting for a generation to finish playing.
    idle_waiters: Vec<(u64, oneshot::Sender<()>)>,
}

impl AudioThread {
    fn run(mut self, rx: std::sync::mpsc::Receiver<Command>) {
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(command) => self.handle(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.notify_idle();
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::StartRecording(reply) => {
                let result = if self.recording.is_some() {
                    Err("A recording is already running".to_string())
                } else {
                    start_recording().map(|recording| {
                        self.recording = Some(recording);
                    })
                };
                reply.send(result).ok();
            }
            Command::StopRecording(reply) => {
                let result = match self.recording.take() {
                    Some(recording) => {
                        drop(recording._stream);
                        let samples = std::mem::take(
                            &mut *recording
                                .samples
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()),
                        );
                        let mono = downmix(&samples, recording.channels);
                        let resampled =
                            resample(&mono, recording.sample_rate, RECORDING_SAMPLE_RATE);
                        if resampled.len() < RECORDING_SAMPLE_RATE as usize / 4 {
                            Err("The recording is too short".to_string())
                        } else {
                            Ok(encode_wav(&resampled, RECORDING_SAMPLE_RATE))
                        }
                    }
                    None => Err("No recording is running".to_string()),
                };
                reply.send(result).ok();
            }
            Command::CancelRecording => {
                self.recording = None;
            }
            Command::Play {
                generation,
                wav,
                reply,
            } => {
                reply.send(self.play(generation, wav)).ok();
            }
            Command::AwaitIdle { generation, reply } => {
                self.idle_waiters.push((generation, reply));
            }
            Command::StopPlayback { generation } => {
                if let Some(playback) = self.playback.as_mut() {
                    if let Some(player) = playback.player.take() {
                        player.stop();
                    }
                    playback.generation = playback.generation.max(generation);
                }
            }
        }
    }

    fn play(&mut self, generation: u64, wav: Vec<u8>) -> Result<(), String> {
        let source = rodio::Decoder::new_wav(Cursor::new(wav)).map_err(|error| {
            log::warn!("audio: speech clip could not be decoded: {error}");
            format!("The speech audio could not be decoded: {error}")
        })?;
        let playback = match self.playback.as_mut() {
            Some(playback) => playback,
            None => {
                log::info!("audio: opening the default output device");
                let started = std::time::Instant::now();
                let mut sink = rodio::DeviceSinkBuilder::open_default_sink().map_err(|error| {
                    log::warn!(
                        "audio: output device failed after {:?}: {error}",
                        started.elapsed()
                    );
                    format!("No audio output is available: {error}")
                })?;
                log::info!("audio: output device ready after {:?}", started.elapsed());
                sink.log_on_drop(false);
                self.playback.insert(Playback {
                    sink,
                    player: None,
                    generation,
                })
            }
        };
        if generation < playback.generation {
            // A chunk from a speak request that was stopped or replaced.
            return Ok(());
        }
        if generation > playback.generation {
            if let Some(player) = playback.player.take() {
                player.stop();
            }
            playback.generation = generation;
        }
        let player = playback
            .player
            .get_or_insert_with(|| rodio::Player::connect_new(playback.sink.mixer()));
        player.append(source);
        Ok(())
    }

    /// Resolve waiters whose generation is done or superseded.
    fn notify_idle(&mut self) {
        if self.idle_waiters.is_empty() {
            return;
        }
        let (current, empty) = match self.playback.as_ref() {
            Some(playback) => (
                Some(playback.generation),
                playback.player.as_ref().is_none_or(|player| player.empty()),
            ),
            None => (None, true),
        };
        let mut waiting = Vec::new();
        for (generation, reply) in self.idle_waiters.drain(..) {
            let done = match current {
                Some(current) if current == generation => empty,
                _ => true,
            };
            if done {
                reply.send(()).ok();
            } else {
                waiting.push((generation, reply));
            }
        }
        self.idle_waiters = waiting;
    }
}

fn start_recording() -> Result<Recording, String> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or_else(|| "No microphone found. Check your audio devices.".to_string())?;
    let supported = device
        .default_input_config()
        .map_err(|error| format!("The microphone could not be opened: {error}"))?;
    let config: cpal::StreamConfig = supported.config();
    let sample_rate = config.sample_rate;
    let channels = config.channels;
    let samples = Arc::new(Mutex::new(Vec::new()));
    let max_samples = MAX_RECORDING.as_secs() as usize * sample_rate as usize * channels as usize;
    let error_callback = |error| log::warn!("microphone stream error: {error}");
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            build_input::<f32>(&device, &config, &samples, max_samples, error_callback)
        }
        cpal::SampleFormat::I16 => {
            build_input::<i16>(&device, &config, &samples, max_samples, error_callback)
        }
        cpal::SampleFormat::U16 => {
            build_input::<u16>(&device, &config, &samples, max_samples, error_callback)
        }
        cpal::SampleFormat::I32 => {
            build_input::<i32>(&device, &config, &samples, max_samples, error_callback)
        }
        cpal::SampleFormat::U8 => {
            build_input::<u8>(&device, &config, &samples, max_samples, error_callback)
        }
        cpal::SampleFormat::I8 => {
            build_input::<i8>(&device, &config, &samples, max_samples, error_callback)
        }
        other => return Err(format!("Unsupported microphone sample format: {other}")),
    }
    .map_err(|error| format!("The microphone could not be opened: {error}"))?;
    stream
        .play()
        .map_err(|error| format!("The microphone could not start: {error}"))?;
    Ok(Recording {
        _stream: stream,
        samples,
        sample_rate,
        channels,
    })
}

fn build_input<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: &Arc<Mutex<Vec<f32>>>,
    max_samples: usize,
    error_callback: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let samples = Arc::clone(samples);
    device.build_input_stream(
        config,
        move |data: &[T], _info| {
            let mut samples = samples
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let room = max_samples.saturating_sub(samples.len());
            samples.extend(
                data.iter()
                    .take(room)
                    .map(|sample| <f32 as cpal::FromSample<T>>::from_sample_(*sample)),
            );
        },
        error_callback,
        None,
    )
}

/// Average interleaved channels into one.
fn downmix(samples: &[f32], channels: u16) -> Vec<f32> {
    let channels = usize::from(channels.max(1));
    if channels == 1 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Linear-interpolation resample. Speech to Whisper does not need better.
fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() || from == 0 {
        return samples.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let len = ((samples.len() as f64) / ratio).floor() as usize;
    (0..len)
        .map(|ix| {
            let position = ix as f64 * ratio;
            let base = position.floor() as usize;
            let fraction = (position - base as f64) as f32;
            let current = samples[base.min(samples.len() - 1)];
            let next = samples[(base + 1).min(samples.len() - 1)];
            current + (next - current) * fraction
        })
        .collect()
}

/// 16-bit PCM mono WAV.
fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1., 1.) * i16::MAX as f32) as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(downmix(&[1., 0., 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix(&[0.25, 0.75], 1), vec![0.25, 0.75]);
    }

    #[test]
    fn resample_halves_a_double_rate_signal() {
        let input: Vec<f32> = (0..8).map(|ix| ix as f32).collect();
        let output = resample(&input, 32_000, 16_000);
        assert_eq!(output, vec![0., 2., 4., 6.]);
    }

    #[test]
    fn resample_keeps_the_same_rate() {
        assert_eq!(resample(&[0.1, 0.2], 16_000, 16_000), vec![0.1, 0.2]);
    }

    #[test]
    fn wav_header_describes_mono_pcm() {
        let wav = encode_wav(&[0., 1., -1.], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            16_000
        );
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 6);
        assert_eq!(wav.len(), 50);
        assert_eq!(i16::from_le_bytes([wav[46], wav[47]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([wav[48], wav[49]]), -i16::MAX);
    }
}
