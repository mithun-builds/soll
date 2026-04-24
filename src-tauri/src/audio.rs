use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use parking_lot::Mutex;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Owns the cpal input stream on a dedicated thread.
/// CoreAudio's `cpal::Stream` is `!Send`, so we never let it leave this thread.
pub struct AudioRecorder {
    shutdown_tx: mpsc::SyncSender<()>,
    samples: Arc<Mutex<Vec<f32>>>,
    source_rate: u32,
    channels: u16,
    thread: Option<JoinHandle<()>>,
}

impl AudioRecorder {
    pub fn start() -> Result<Self> {
        let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(16_000 * 60)));
        let samples_for_thread = samples.clone();

        let (shutdown_tx, shutdown_rx) = mpsc::sync_channel::<()>(1);
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(u32, u16)>>(1);

        let handle = std::thread::Builder::new()
            .name("soll-audio".into())
            .spawn(move || {
                let outcome = Self::run_stream(samples_for_thread, shutdown_rx, &ready_tx);
                if let Err(e) = outcome {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .context("spawn audio thread")?;

        let (source_rate, channels) = ready_rx
            .recv()
            .map_err(|_| anyhow!("audio thread died before ready"))??;

        Ok(Self {
            shutdown_tx,
            samples,
            source_rate,
            channels,
            thread: Some(handle),
        })
    }

    fn run_stream(
        samples: Arc<Mutex<Vec<f32>>>,
        shutdown_rx: mpsc::Receiver<()>,
        ready_tx: &mpsc::SyncSender<Result<(u32, u16)>>,
    ) -> Result<()> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow!("no default input device"))?;
        let config = device
            .default_input_config()
            .context("default input config")?;

        let source_rate = config.sample_rate().0;
        let channels = config.channels();
        let sample_format = config.sample_format();
        let stream_config = config.into();

        let err_fn = |err| log::error!("audio stream error: {err}");
        let s1 = samples.clone();
        let s2 = samples.clone();
        let s3 = samples.clone();

        let stream = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    s1.lock().extend_from_slice(data);
                },
                err_fn,
                None,
            )?,
            SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    let mut b = s2.lock();
                    b.extend(data.iter().map(|&v| v as f32 / i16::MAX as f32));
                },
                err_fn,
                None,
            )?,
            SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                move |data: &[u16], _| {
                    let mut b = s3.lock();
                    b.extend(data.iter().map(|&v| {
                        (v as f32 - u16::MAX as f32 / 2.0) / (u16::MAX as f32 / 2.0)
                    }));
                },
                err_fn,
                None,
            )?,
            fmt => return Err(anyhow!("unsupported sample format: {fmt:?}")),
        };
        stream.play().context("play stream")?;

        ready_tx
            .send(Ok((source_rate, channels)))
            .map_err(|_| anyhow!("ready channel dropped"))?;

        // Block until stop() is called. Dropping `stream` here is safe — same thread it was built on.
        let _ = shutdown_rx.recv();
        drop(stream);
        Ok(())
    }

    /// Stop capture and return 16kHz mono f32 samples.
    pub fn stop(mut self) -> Result<Vec<f32>> {
        let _ = self.shutdown_tx.send(());
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
        let raw = self.samples.lock().clone();
        let mono = to_mono(&raw, self.channels);
        Ok(resample_linear(&mono, self.source_rate, TARGET_SAMPLE_RATE))
    }
}

fn to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let ch = channels as usize;
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Cheap linear-interp resampler. Good enough for speech; upgrade to rubato later.
fn resample_linear(input: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = src_rate as f64 / dst_rate as f64;
    let out_len = ((input.len() as f64) / ratio).ceil() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = (i as f64) * ratio;
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;
        let a = input[idx.min(input.len() - 1)];
        let b = input[(idx + 1).min(input.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}
