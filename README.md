# Svara

> Open-source, local-first voice dictation for macOS. A free alternative to Wispr Flow.

Hold **Ctrl + Shift + Space**, speak, release — polished text pastes into the focused app.
Everything runs on-device: [whisper.cpp](https://github.com/ggerganov/whisper.cpp) for transcription, [Ollama](https://ollama.com) for optional AI cleanup. No cloud. No accounts. Free forever.

## Phase 1 feature set

- [x] Global push-to-talk hotkey (`Ctrl+Shift+Space`)
- [x] Local speech-to-text (whisper.cpp, Metal-accelerated on Apple Silicon)
- [x] AI cleanup layer (Ollama `llama3.2:3b`) with graceful fallback to raw transcript
- [x] Pastes into any focused app (Slack, Gmail, Notion, VS Code, Cursor, browser)
- [x] Minimal floating pill UI (Listening / Polishing / Done)
- [x] Auto-downloads Whisper model on first run
- [ ] Personal dictionary (Day 4)
- [ ] Auto-formatting (lists, emails) (Day 5)
- [ ] Multi-language (Day 6)
- [ ] Smart mid-sentence corrections (Day 8)
- [ ] Edit Mode (Day 9)

## Requirements

- macOS 12+ (Apple Silicon strongly recommended)
- [Rust](https://rustup.rs) (auto-installed if missing)
- [Node.js 20+](https://nodejs.org) + [pnpm](https://pnpm.io)
- [cmake](https://cmake.org) (`brew install cmake`) — needed by whisper.cpp
- [Ollama](https://ollama.com) **(optional — for AI cleanup)**

## Install

```bash
# 1. Clone and enter
git clone https://github.com/your-org/svara.git && cd svara

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

All three are **one-time** prompts. Svara never sends audio anywhere.

## Architecture

```
┌────────────────── Svara ───────────────────┐
│                                            │
│  React pill UI ◀─── events ──── Rust core  │
│  (Tauri webview)                 │         │
│                                  ▼         │
│                          ┌───────────────┐ │
│                          │  hotkey (v2)  │ │
│                          └───────┬───────┘ │
│                                  ▼         │
│                          ┌───────────────┐ │
│                          │ cpal audio    │ │
│                          │ 16 kHz mono   │ │
│                          └───────┬───────┘ │
│                                  ▼         │
│                          ┌───────────────┐ │
│                          │ whisper-rs    │ │
│                          │ (Metal)       │ │
│                          └───────┬───────┘ │
│                                  ▼         │
│                          ┌───────────────┐ │
│                          │ Ollama HTTP   │──► 127.0.0.1:11434
│                          │ (fallback ok) │  │
│                          └───────┬───────┘ │
│                                  ▼         │
│                          ┌───────────────┐ │
│                          │ clipboard +   │ │
│                          │ osascript ⌘V  │ │
│                          └───────────────┘ │
│                                            │
└────────────────────────────────────────────┘
```

## License

MIT — see [LICENSE](LICENSE).
