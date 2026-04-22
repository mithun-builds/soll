# Svara benchmarks

Latency measurement harness for the dictation pipeline. Reproduces the
exact stages used in production (Whisper + Ollama) against fixture WAVs,
so any change can be checked against a baseline before shipping.

## Fixtures

| File | Content | Purpose |
|---|---|---|
| `fixtures/silence_3s.wav` | 3 s of silence @ 16 kHz mono | Smoke test — exercises full pipeline, Whisper should emit empty/blank |
| `fixtures/tone_440_2s.wav` | 2 s of 440 Hz sine @ 20% amplitude | Smoke test — Whisper will hallucinate music-like nothing; useful for timing only |

For real accuracy benchmarks, record your own utterances via:
- macOS Voice Memos → export to WAV (convert with `ffmpeg -i in.m4a -ac 1 -ar 16000 out.wav`)
- Or any 16 kHz / mono WAV file (the harness resamples otherwise)

## Running

```bash
# Dev build — for quick iteration
cargo run --example bench_pipeline -- --wav benchmarks/fixtures/silence_3s.wav

# Release build — for real numbers (use this when comparing)
cargo run --release --example bench_pipeline -- \
  --wav benchmarks/fixtures/tone_440_2s.wav --n 10

# Skip Ollama (measure Whisper in isolation)
cargo run --release --example bench_pipeline -- \
  --wav benchmarks/fixtures/silence_3s.wav --no-ollama
```

## Output

NDJSON on stdout. One `iteration` object per run, then a `summary` at the end:

```jsonc
{"type":"iteration","i":0,"audio_ms":3000,"whisper_ms":420,"ollama_ms":0,"ollama_used":false,"text":""}
{"type":"iteration","i":1,"audio_ms":3000,"whisper_ms":410, ... }
// ...
{"type":"summary","samples":5,"audio_ms":3000,
 "whisper_ms":{"p50":415,"p95":445,"min":405,"max":450,"mean":420},
 "ollama_ms":null,
 "total_ms":{"p50":415,"p95":445,"min":405,"max":450,"mean":420}}
```

## Regression detection

Store run output in `benchmarks/runs/YYYY-MM-DD-HHMM.json` and diff
against the previous run. Summary `p50` and `p95` are the headline
metrics. A regression is anything where `whisper_ms.p50` or
`total_ms.p50` increases by more than 10%.

## Known caveats

- First iteration always includes Metal-kernel compile (~3 ms with pre-warm).
- Ollama first-request cold-load can be 10–30 s. The harness warms the
  model once before the timed loop, so iteration #0 is representative.
- macOS CPU frequency scaling adds ±5 % noise. Plug in, close Chrome, don't
  try to read too much into single-digit-percent deltas.
