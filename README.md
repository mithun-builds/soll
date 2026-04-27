# Soll — Voice to text for your Mac

> Hold a shortcut, speak, release. Your words appear wherever your cursor is — in any app, instantly, privately.

A free, open-source alternative to [Wispr Flow](https://wisprflow.ai). Everything runs on your Mac — no cloud, no account, no subscription.

---

## Install

### Option A — Homebrew *(easiest)*

```bash
brew tap mithun-builds/soll
brew install --cask soll
```

Homebrew handles everything — downloads, installs, and opens Soll with no warnings.

### Option B — Build from source

See [Build from source](#build-from-source) below if you prefer to build it yourself.

---

## Update

If you installed via Homebrew, get the latest release with:

```bash
brew update && brew upgrade --cask soll
```

You can run this while Soll is open — brew quits the running app, swaps in the new `.app`, and you can relaunch from the menu bar / Spotlight.

If you downloaded the DMG manually, [grab the newer DMG from the Releases page](https://github.com/mithun-builds/soll/releases), quit Soll, and drag the new app over your existing `/Applications/Soll.app`.

Soll itself doesn't auto-update — you choose when to upgrade.

---

## Uninstall

If you installed via Homebrew:

```bash
brew uninstall --cask soll
brew untap mithun-builds/soll        # optional, removes the tap
```

If you installed manually (DMG):

```bash
rm -rf /Applications/Soll.app
```

To wipe **everything** Soll left behind — settings, downloaded models, dictionary, skills, granted permissions — run these too:

```bash
rm -rf "$HOME/Library/Application Support/com.soll.app"
rm -rf "$HOME/Library/Logs/com.soll.app"
tccutil reset Microphone com.soll.app
tccutil reset Accessibility com.soll.app
```

Ollama and its models are independent — Soll never installs them — so they aren't touched. Remove them separately if you want.

---

## What it does

- **Hold `⌃⇧Space`** anywhere on your Mac → speak → release → text is pasted at your cursor
- Works in **any app** — Slack, Notion, Gmail, Terminal, VS Code, anywhere
- **AI cleanup** removes filler words, fixes punctuation and casing
- **Skills** — voice-triggered AI actions (e.g. say *"git commit fixed the login bug"* → formatted commit message)
- **Phrases** — instant text snippets (e.g. say *"my email"* → pastes your email address)
- Everything runs **100% on-device** — your audio never leaves your Mac

---

## Before you start

You need four things installed. Click each link for the official install page:

| Tool | What it's for | How to install |
|------|--------------|----------------|
| **Homebrew** | Mac package manager — installs everything else | [brew.sh](https://brew.sh) |
| **Rust** | Builds the Soll app | [rustup.rs](https://rustup.rs) |
| **Node.js** | Builds the UI | [nodejs.org](https://nodejs.org) — download the LTS version |
| **pnpm** | Node package manager | After Node: run `npm install -g pnpm` |
| **cmake** | Required by the audio engine | After Homebrew: run `brew install cmake` |

**Optional but recommended:**

| Tool | What it's for | How to install |
|------|--------------|----------------|
| **Ollama** | Powers AI cleanup and Skills | [ollama.com](https://ollama.com) — download and install, then run `ollama pull llama3.2:3b` |

---

## Build from source

Open **Terminal** (press `⌘Space`, type "Terminal", press Enter) and run these one at a time:

### Step 1 — Install Homebrew
```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```
*Skip if you already have it — type `brew --version` to check.*

### Step 2 — Install Rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
When it finishes, **close Terminal and open a new one** before continuing.

### Step 3 — Install Node.js and pnpm
```bash
brew install node
npm install -g pnpm
```

### Step 4 — Install cmake
```bash
brew install cmake
```

### Step 5 — Install Ollama and download the AI model *(optional)*
```bash
brew install ollama
ollama pull llama3.2:3b
```

### Step 6 — Clone Soll
```bash
git clone https://github.com/mithun-builds/soll.git
cd soll
```

### Step 7 — Install dependencies
```bash
pnpm install
```

### Step 8 — Run Soll
```bash
pnpm tauri dev
```

A small Soll icon appears in your **menu bar** — you're ready.

> **First launch:** macOS will ask for **Microphone** and **Accessibility** permissions. Grant both — Soll needs them to hear you and paste text into other apps.

---

## How to use it

| What you want to do | How |
|---------------------|-----|
| Dictate into any app | Hold `⌃⇧Space`, speak, release |
| Format as a bullet list | Say *"bullet list …"* |
| Format as a numbered list | Say *"numbered list …"* |
| Self-correct mid-sentence | Say *"actually"*, *"I mean"*, or *"no wait"* |
| Use a Skill (AI action) | Say the trigger phrase, e.g. *"git commit fixed the login bug"* |
| Use a Phrase (instant snippet) | Say the trigger, e.g. *"my calendly link"* |
| Open Settings | Click the Soll icon in the menu bar |

---

## Status indicator

A floating pill appears at the bottom of your screen while dictating:

| Pill | Meaning |
|------|---------|
| Wave animating · yellow cursor | Listening — keep speaking |
| Yellow wave · white cursor | Processing your speech |
| ✓ done | Text pasted successfully |
| ✓ skill: *name* | A Skill or Phrase fired |

---

## Tips

- Hold the shortcut **until you finish your full sentence** — quick taps under ¼ second are ignored
- Add unusual words, brand names, and acronyms in **Settings → Dictionary** so Whisper gets them right
- Build your own Skills and Phrases in **Settings → Skills / Phrases**
- Set your name once in **Settings → General** so Skills can personalise output

---

## Requirements

- macOS 13 Ventura or later
- Apple Silicon (M1/M2/M3/M4) strongly recommended — Whisper runs significantly faster

---

## License

MIT — free to use, modify, and distribute.
