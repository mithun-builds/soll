# Soll

> Open-source, local-first voice dictation. A free alternative to Wispr Flow.

Hold **Ctrl + Shift + Space**, speak, release — polished text pastes into the focused app.
Everything runs on-device: [whisper.cpp](https://github.com/ggerganov/whisper.cpp) for transcription, [Ollama](https://ollama.com) for optional AI cleanup. No cloud. No accounts. Free forever.

## Features

- **Push-to-talk** — hold `Ctrl+Shift+Space` anywhere, release to transcribe and paste
- **Fully local** — whisper.cpp (Metal-accelerated on Apple Silicon) + Ollama on `127.0.0.1`; no audio leaves the device
- **AI cleanup** — filler words stripped, casing fixed, LLM preambles removed; graceful fallback to raw transcript if Ollama is unavailable
- **Skills** — AI-powered voice macros that reshape dictation into a specific format (commit messages, Slack messages, bug reports, …). Say `skill [trigger]` or speak the trigger phrase directly
- **Phrases** — instant text snippets pasted verbatim with no AI and no latency. Say `phrase [trigger]` or speak the trigger phrase directly
- **Personal dictionary** — teach Soll brand names, jargon, and acronyms so Whisper gets them right
- **Smart list formatting** — start with `bullet list …` or `numbered list …` to get formatted output
- **Self-corrections** — say "actually", "I mean", "no wait", etc. mid-sentence to fix a word without re-recording
- **Floating status pill** — a non-intrusive pill at the bottom of your screen shows the current state; disappears when idle
- **Settings UI** — model picker, dictionary editor, skill/phrase builder, and Tips & Tricks pane, all in a native window

## Status indicator

A floating pill appears at the bottom of your screen during each dictation:

| Pill | Meaning |
|------|---------|
| White wave · yellow cursor (animated) — **listening** | Microphone is live, speak now |
| Yellow wave · white cursor (animated) — **processing…** | Transcribing and running AI cleanup |
| Static logo · ✓ — **done** | Text pasted, clears in ~1 s |
| Static logo · ✓ · **skill: name** | A skill or phrase fired — verify the name |

**Menu bar icon** — always a static white logo. A small red dot badge appears in the corner while the model is loading or initializing. Once it disappears, Soll is ready.

## Requirements

- macOS 12+ (Apple Silicon strongly recommended)
- [Rust](https://rustup.rs)
- [Node.js 20+](https://nodejs.org) + [pnpm](https://pnpm.io)
- [cmake](https://cmake.org) (`brew install cmake`) — needed by whisper.cpp
- [Ollama](https://ollama.com) *(optional — for AI cleanup and Skills)*

## Install

```bash
# 1. Clone and enter
git clone https://github.com/mithun-builds/soll.git && cd soll

# 2. Install deps
pnpm install

# 3. (Optional) Install Ollama + pull cleanup model
brew install ollama
ollama pull llama3.2:3b
ollama serve &   # runs in background

# 4. Run in dev mode
pnpm tauri:dev

# 5. Build a release .dmg
pnpm tauri:build
```

On first launch macOS will ask for:
1. **Microphone** — to capture audio
2. **Accessibility** — so AppleScript can send Cmd+V
3. **Input Monitoring** — for the global hotkey

All three are one-time prompts. Soll never sends audio anywhere.

## Architecture

Tauri v2 app — Rust core, React + TypeScript UI. The menu-bar tray icon is always visible; a transparent frameless overlay window hosts the status pill. A separate settings window opens on demand.

```
┌──────────────────── Soll (menu-bar app) ─────────────────────┐
│                                                               │
│  Tray icon (white logo)  ◀── state updates ── Rust core      │
│  Overlay pill (bottom)   ◀──────────────────┘  │             │
│                                                 ▼             │
│                                        ┌────────────────┐    │
│                                        │  global hotkey │    │
│                                        │  ⌃⇧Space hold  │    │
│                                        └───────┬────────┘    │
│                                                ▼             │
│                                        ┌────────────────┐    │
│                                        │  cpal audio    │    │
│                                        │  16 kHz mono   │    │
│                                        └───────┬────────┘    │
│                                                ▼  on release │
│                                        ┌────────────────┐    │
│                                        │  whisper-rs    │    │
│                                        │  (Metal)       │    │
│                                        └───────┬────────┘    │
│                                                ▼             │
│                                        ┌────────────────┐    │
│                     skill / phrase? ──▶│  skills engine │    │
│                                        └───────┬────────┘    │
│                                                ▼             │
│                                        ┌────────────────┐    │
│                                        │  Ollama HTTP   │──▶ 127.0.0.1:11434
│                                        │  (fallback ok) │    │
│                                        └───────┬────────┘    │
│                                                ▼             │
│                                        ┌────────────────┐    │
│                                        │  clipboard +   │    │
│                                        │  osascript ⌘V  │    │
│                                        └────────────────┘    │
└───────────────────────────────────────────────────────────────┘
```

## License

MIT — see [LICENSE](LICENSE).
