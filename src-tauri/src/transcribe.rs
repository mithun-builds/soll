use anyhow::{anyhow, Context, Result};
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct Transcriber {
    ctx: WhisperContext,
}

impl Transcriber {
    pub fn load(model_path: &Path) -> Result<Self> {
        let ctx = WhisperContext::new_with_params(
            model_path
                .to_str()
                .ok_or_else(|| anyhow!("model path not utf-8"))?,
            WhisperContextParameters::default(),
        )
        .context("load whisper model")?;
        Ok(Self { ctx })
    }

    /// Run a throwaway inference on 1 second of silence to force Metal kernel
    /// compilation. Without this, the user's first real dictation pays a
    /// 2–5 s one-time cost while shaders compile.
    pub fn warm(&self) -> Result<()> {
        let silence = vec![0.0f32; 16_000];
        let _ = self.transcribe(&silence)?;
        Ok(())
    }

    /// Simple transcribe with no initial prompt. Kept for the benchmark harness.
    pub fn transcribe(&self, samples: &[f32]) -> Result<String> {
        self.transcribe_with_prompt(samples, None)
    }

    /// Transcribe with an optional `initial_prompt` that biases Whisper's
    /// decoding toward specific vocabulary. Pass a short comma-separated
    /// list of names/jargon (≤224 tokens). Useful for personal dictionary.
    pub fn transcribe_with_prompt(
        &self,
        samples: &[f32],
        initial_prompt: Option<&str>,
    ) -> Result<String> {
        let mut state = self.ctx.create_state().context("create whisper state")?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(num_threads());
        params.set_translate(false);
        params.set_language(Some("en"));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_single_segment(false);
        if let Some(prompt) = initial_prompt {
            if !prompt.trim().is_empty() {
                params.set_initial_prompt(prompt);
            }
        }

        state.full(params, samples).context("whisper full")?;
        let n = state.full_n_segments().context("segment count")?;
        let mut text = String::new();
        for i in 0..n {
            let seg = state.full_get_segment_text(i).context("segment text")?;
            text.push_str(&seg);
        }
        Ok(text.trim().to_string())
    }
}

fn num_threads() -> i32 {
    let n = std::thread::available_parallelism()
        .map(|v| v.get())
        .unwrap_or(4);
    // Leave some headroom for audio + UI threads
    (n.saturating_sub(1).max(2)) as i32
}
