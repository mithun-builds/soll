//! Benchmark harness for the Soll dictation pipeline.
//!
//! Reads a WAV file (any sample rate / channels — we resample to 16 kHz mono),
//! runs the production `Transcriber` and `OllamaClient` N iterations, and
//! emits JSON stats for latency regression tracking.
//!
//! Usage:
//!   cargo run --release --example bench_pipeline -- \
//!       --wav fixtures/hello.wav --n 10
//!
//! Flags:
//!   --wav PATH        WAV file to bench (required)
//!   --n N             Iterations (default 5)
//!   --no-ollama       Skip cleanup stage
//!   --model PATH      Override Whisper model path
//!
//! Output: one JSON object per iteration to stdout, then a `summary`
//! object with p50/p95/min/max at the end.

use anyhow::{anyhow, Context, Result};
use hound::{SampleFormat as HoundSampleFormat, WavReader};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Instant;

use soll_lib::cleanup::OllamaClient;
use soll_lib::metal::ensure_metal_resources;
use soll_lib::model::default_model_path_standalone;
use soll_lib::transcribe::Transcriber;

// NOTE: the bench deliberately uses the standalone path resolver (base.en)
// rather than the AppHandle-based per-model downloader used by the live
// app, so benches run fully offline once any model has been downloaded
// once by the app. Override with --model to benchmark other sizes.

#[derive(Debug)]
struct Args {
    wav: PathBuf,
    n: usize,
    no_ollama: bool,
    model: Option<PathBuf>,
}

fn parse_args() -> Result<Args> {
    let mut it = std::env::args().skip(1);
    let mut wav: Option<PathBuf> = None;
    let mut n: usize = 5;
    let mut no_ollama = false;
    let mut model: Option<PathBuf> = None;

    while let Some(a) = it.next() {
        match a.as_str() {
            "--wav" => wav = it.next().map(PathBuf::from),
            "--n" => {
                n = it
                    .next()
                    .ok_or_else(|| anyhow!("--n takes a value"))?
                    .parse()
                    .context("--n must be an integer")?;
            }
            "--no-ollama" => no_ollama = true,
            "--model" => model = it.next().map(PathBuf::from),
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(anyhow!("unknown argument: {other}")),
        }
    }
    let wav = wav.ok_or_else(|| anyhow!("--wav PATH is required"))?;
    Ok(Args {
        wav,
        n,
        no_ollama,
        model,
    })
}

fn print_usage() {
    eprintln!(
        "bench_pipeline --wav PATH [--n N] [--no-ollama] [--model PATH]

Benchmark the Svara transcription + cleanup pipeline on a fixture WAV.
Output: NDJSON to stdout.
"
    );
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "iteration")]
    Iteration {
        i: usize,
        audio_ms: u64,
        whisper_ms: u64,
        ollama_ms: Option<u64>,
        ollama_used: bool,
        text: String,
    },
    #[serde(rename = "summary")]
    Summary {
        samples: usize,
        audio_ms: u64,
        whisper_ms: Stats,
        ollama_ms: Option<Stats>,
        total_ms: Stats,
    },
}

#[derive(Serialize, Debug)]
struct Stats {
    p50: u64,
    p95: u64,
    min: u64,
    max: u64,
    mean: u64,
}

impl Stats {
    fn from(values: &[u64]) -> Self {
        let mut v = values.to_vec();
        v.sort_unstable();
        let n = v.len();
        let p50 = v[n / 2];
        let p95_idx = ((n as f32 * 0.95).ceil() as usize).saturating_sub(1).min(n - 1);
        let p95 = v[p95_idx];
        let min = v[0];
        let max = v[n - 1];
        let mean = v.iter().sum::<u64>() / n as u64;
        Self { p50, p95, min, max, mean }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Point whisper.cpp at the bundled Metal shader BEFORE loading the model.
    ensure_metal_resources();
    let args = parse_args().inspect_err(|_| print_usage())?;

    // NOTE: the benchmark does not exercise the per-model downloader (that
    // is Tauri-bound via AppHandle). It expects `--model` or the default
    // base.en path to already exist on disk.

    // 1. Load and normalize the WAV to 16 kHz mono f32
    let (samples, audio_ms) = load_wav_to_16k_mono(&args.wav)
        .with_context(|| format!("loading {}", args.wav.display()))?;
    eprintln!(
        "bench: loaded {} ({} samples = {} ms @ 16 kHz mono)",
        args.wav.display(),
        samples.len(),
        audio_ms
    );

    // 2. Resolve model path
    let model_path = args
        .model
        .clone()
        .or_else(default_model_path_standalone)
        .ok_or_else(|| anyhow!("no model path; pass --model"))?;
    if !model_path.exists() {
        return Err(anyhow!(
            "model not found at {}. Run the app once to auto-download, or pass --model.",
            model_path.display()
        ));
    }

    // 3. Load whisper (and warm Metal)
    eprintln!("bench: loading whisper from {}", model_path.display());
    let t0 = Instant::now();
    let transcriber = Transcriber::load(&model_path)?;
    transcriber.warm()?;
    eprintln!("bench: whisper ready in {:?}", t0.elapsed());

    // 4. Warm Ollama (if needed)
    let ollama = OllamaClient::new();
    if !args.no_ollama {
        eprintln!("bench: warming ollama…");
        ollama.warm_up().await;
    }

    // 5. Iterate
    let mut whisper_ms_all: Vec<u64> = Vec::with_capacity(args.n);
    let mut ollama_ms_all: Vec<u64> = Vec::with_capacity(args.n);
    let mut total_ms_all: Vec<u64> = Vec::with_capacity(args.n);
    let mut ollama_used_count = 0;

    for i in 0..args.n {
        let iter_t0 = Instant::now();

        let whisper_t0 = Instant::now();
        let raw = transcriber.transcribe(&samples)?;
        let whisper_ms = whisper_t0.elapsed().as_millis() as u64;

        let (ollama_ms, ollama_used) = if args.no_ollama {
            (None, false)
        } else {
            let ot0 = Instant::now();
            let used = ollama.polish(&raw).await.is_ok();
            let ms = ot0.elapsed().as_millis() as u64;
            if used {
                ollama_used_count += 1;
            }
            (Some(ms), used)
        };

        let total_ms = iter_t0.elapsed().as_millis() as u64;

        let ev = Event::Iteration {
            i,
            audio_ms,
            whisper_ms,
            ollama_ms,
            ollama_used,
            text: raw,
        };
        println!("{}", serde_json::to_string(&ev)?);

        whisper_ms_all.push(whisper_ms);
        if let Some(ms) = ollama_ms {
            ollama_ms_all.push(ms);
        }
        total_ms_all.push(total_ms);
    }

    // 6. Summary
    let summary = Event::Summary {
        samples: args.n,
        audio_ms,
        whisper_ms: Stats::from(&whisper_ms_all),
        ollama_ms: if ollama_ms_all.is_empty() {
            None
        } else {
            Some(Stats::from(&ollama_ms_all))
        },
        total_ms: Stats::from(&total_ms_all),
    };
    println!("{}", serde_json::to_string(&summary)?);
    eprintln!(
        "bench: done. ollama_used={}/{}",
        ollama_used_count, args.n
    );
    Ok(())
}

fn load_wav_to_16k_mono(path: &Path) -> Result<(Vec<f32>, u64)> {
    let mut reader = WavReader::open(path).context("open WAV")?;
    let spec = reader.spec();
    let src_rate = spec.sample_rate;
    let channels = spec.channels as usize;

    let interleaved: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (HoundSampleFormat::Float, 32) => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()?,
        (HoundSampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
            .collect::<Result<Vec<_>, _>>()?,
        (HoundSampleFormat::Int, 32) => reader
            .samples::<i32>()
            .map(|s| s.map(|v| v as f32 / i32::MAX as f32))
            .collect::<Result<Vec<_>, _>>()?,
        other => {
            return Err(anyhow!(
                "unsupported WAV format: {:?} @ {} bit",
                other.0,
                other.1
            ))
        }
    };

    let mono: Vec<f32> = if channels == 1 {
        interleaved
    } else {
        interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    };

    let resampled = resample_linear(&mono, src_rate, 16_000);
    let audio_ms = (resampled.len() as f64 / 16_000.0 * 1000.0) as u64;
    Ok((resampled, audio_ms))
}

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
