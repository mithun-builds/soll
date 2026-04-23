//! User-extensible skills system. Each skill is a single markdown file
//! describing:
//!
//!   1. Frontmatter metadata
//!      - name: stable id
//!      - description: one-liner for the UI
//!      - trigger: regex (anchored ^) that matches the start of raw speech
//!      - capture: comma-separated names for the regex groups
//!      - (optional) native: hook into a hardcoded Rust path by this key
//!
//!   2. ## System Prompt — sent to Ollama with `{{var}}` substitutions
//!   3. ## Output Template — wraps Ollama's response, also supports `{{var}}`
//!
//! Built-in variables available in both sections:
//!   - captured regex group names (recipient, body, …)
//!   - `{{llm_output}}` in the output template
//!   - `{{user_name}}` and `{{sign_off}}` from the user's settings
//!
//! When a user dictation arrives we try each skill's trigger in order.
//! First match wins. No match → fall through to the default pipeline
//! (corrections / Ollama polish / format detection).

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use std::collections::HashMap;

/// Parsed skill definition loaded from a `.md` file.
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub trigger: Regex,
    pub capture_names: Vec<String>,
    /// If set, dispatch via a hardcoded implementation (e.g. "email" maps
    /// to the rich email template + capitalization pass). The system
    /// prompt and output template are still used for non-native handling
    /// if the native hook is unavailable.
    pub native: Option<String>,
    pub system_prompt: String,
    pub output_template: String,
}

/// Result of invoking a skill — either a ready-to-paste string (skills
/// that own their whole output) or instructions for the pipeline.
#[derive(Debug, Clone)]
pub struct SkillMatch {
    pub skill_name: String,
    pub vars: HashMap<String, String>,
}

impl Skill {
    pub fn from_markdown(md: &str) -> Result<Self> {
        let (fm, body) = split_frontmatter(md)?;
        let meta = parse_frontmatter(fm);

        let name = meta
            .get("name")
            .cloned()
            .ok_or_else(|| anyhow!("skill missing `name`"))?;
        let description = meta.get("description").cloned().unwrap_or_default();
        let trigger_str = meta
            .get("trigger")
            .cloned()
            .ok_or_else(|| anyhow!("skill `{name}` missing `trigger`"))?;
        let trigger = Regex::new(&format!("(?i){}", trigger_str))
            .with_context(|| format!("skill `{name}` invalid trigger regex: {trigger_str}"))?;
        let capture_names = meta
            .get("capture")
            .map(|s| {
                s.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let native = meta.get("native").cloned();

        let system_prompt = extract_section(body, "System Prompt")
            .with_context(|| format!("skill `{name}` missing `## System Prompt` section"))?;
        let output_template = extract_section(body, "Output Template")
            .unwrap_or_else(|_| "{{llm_output}}".to_string());

        Ok(Skill {
            name,
            description,
            trigger,
            capture_names,
            native,
            system_prompt,
            output_template,
        })
    }

    /// If the skill's trigger regex matches, return the variable bindings
    /// (capture names → values). Returns None otherwise.
    pub fn matches(&self, raw: &str) -> Option<HashMap<String, String>> {
        let caps = self.trigger.captures(raw.trim())?;
        let mut vars = HashMap::new();
        for (i, name) in self.capture_names.iter().enumerate() {
            if let Some(m) = caps.get(i + 1) {
                vars.insert(name.clone(), m.as_str().trim().to_string());
            }
        }
        // Reject empty captures — "email to John" with no body shouldn't
        // match a skill whose prompt needs a body.
        for name in &self.capture_names {
            if vars.get(name).map(|v| v.is_empty()).unwrap_or(true) {
                return None;
            }
        }
        Some(vars)
    }

    pub fn interpolate(&self, template: &str, vars: &HashMap<String, String>) -> String {
        let mut out = template.to_string();
        for (k, v) in vars {
            let needle = format!("{{{{{}}}}}", k);
            out = out.replace(&needle, v);
        }
        out
    }
}

/// Load all built-in skills embedded in the binary. User-supplied skills
/// from the app data directory are merged by `load_all` below.
pub fn builtin() -> Vec<Skill> {
    let mut out = Vec::new();
    for md in [
        include_str!("../skills/email.md"),
        include_str!("../skills/prompt_better.md"),
    ] {
        match Skill::from_markdown(md) {
            Ok(s) => out.push(s),
            Err(e) => log::error!("built-in skill parse failed: {e:?}"),
        }
    }
    out
}

/// Load built-in skills plus any `*.md` files the user has dropped in
/// `$APP_DATA/skills/`. Errors on user files are logged, not fatal.
pub fn load_all(user_dir: Option<&std::path::Path>) -> Vec<Skill> {
    let mut out = builtin();
    if let Some(dir) = user_dir {
        if dir.exists() {
            match std::fs::read_dir(dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().map(|s| s == "md").unwrap_or(false) {
                            match std::fs::read_to_string(&path) {
                                Ok(md) => match Skill::from_markdown(&md) {
                                    Ok(s) => {
                                        log::info!("loaded user skill: {}", s.name);
                                        out.push(s);
                                    }
                                    Err(e) => {
                                        log::error!("user skill {:?}: {e:?}", path.display())
                                    }
                                },
                                Err(e) => log::error!("read {:?}: {e:?}", path.display()),
                            }
                        }
                    }
                }
                Err(e) => log::error!("read_dir({:?}): {e:?}", dir.display()),
            }
        }
    }
    out
}

/// First-match-wins skill lookup.
pub fn match_skill<'a>(skills: &'a [Skill], raw: &str) -> Option<(&'a Skill, HashMap<String, String>)> {
    for s in skills {
        if let Some(vars) = s.matches(raw) {
            return Some((s, vars));
        }
    }
    None
}

// ── markdown helpers ───────────────────────────────────────────────────────

fn split_frontmatter(md: &str) -> Result<(&str, &str)> {
    let md = md.trim_start();
    let md = md.strip_prefix("---").ok_or_else(|| {
        anyhow!("skill markdown must start with YAML frontmatter (---)")
    })?;
    let (fm, rest) = md
        .split_once("\n---")
        .ok_or_else(|| anyhow!("unterminated frontmatter (missing closing ---)"))?;
    Ok((fm, rest.trim_start_matches('\n')))
}

fn parse_frontmatter(fm: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in fm.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

/// Extract the content under a `## {name}` heading up to the next `## `
/// heading or end of file. Returns the body trimmed.
fn extract_section(body: &str, name: &str) -> Result<String> {
    let heading = format!("## {name}");
    let start = body
        .find(&heading)
        .ok_or_else(|| anyhow!("section `{name}` not found"))?;
    let after = &body[start + heading.len()..];
    // Drop to next newline to skip any trailing heading text
    let after = after.trim_start_matches(|c: char| c != '\n').trim_start_matches('\n');
    let end = after.find("\n## ").unwrap_or(after.len());
    Ok(after[..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"---
name: test-skill
description: A toy skill
trigger: ^\s*debug\s+(.+)$
capture: body
---

## System Prompt
Debug this: {{body}}

## Output Template
DEBUG: {{llm_output}}
"#;

    #[test]
    fn parses_basic_skill() {
        let s = Skill::from_markdown(SAMPLE).unwrap();
        assert_eq!(s.name, "test-skill");
        assert_eq!(s.description, "A toy skill");
        assert_eq!(s.capture_names, vec!["body"]);
        assert!(s.system_prompt.contains("Debug this: {{body}}"));
        assert!(s.output_template.contains("DEBUG: {{llm_output}}"));
    }

    #[test]
    fn matches_with_captures() {
        let s = Skill::from_markdown(SAMPLE).unwrap();
        let vars = s.matches("debug segfault in foo").unwrap();
        assert_eq!(vars.get("body").unwrap(), "segfault in foo");
    }

    #[test]
    fn no_match_on_unrelated_input() {
        let s = Skill::from_markdown(SAMPLE).unwrap();
        assert!(s.matches("hello world").is_none());
    }

    #[test]
    fn interpolation_replaces_placeholders() {
        let s = Skill::from_markdown(SAMPLE).unwrap();
        let mut vars = HashMap::new();
        vars.insert("body".into(), "hello".into());
        vars.insert("llm_output".into(), "reply".into());
        assert_eq!(
            s.interpolate(&s.system_prompt, &vars),
            "Debug this: hello"
        );
        assert_eq!(
            s.interpolate(&s.output_template, &vars),
            "DEBUG: reply"
        );
    }

    #[test]
    fn empty_body_rejected() {
        // Trigger matches but body capture is empty — match() returns None.
        let md = r#"---
name: x
trigger: ^prefix\s*(.*)$
capture: body
---

## System Prompt
{{body}}
"#;
        let s = Skill::from_markdown(md).unwrap();
        assert!(s.matches("prefix").is_none());
        assert!(s.matches("prefix something").is_some());
    }

    #[test]
    fn builtin_skills_all_parse() {
        let skills = builtin();
        assert!(!skills.is_empty());
        assert!(skills.iter().any(|s| s.name == "email"));
        assert!(skills.iter().any(|s| s.name == "prompt-better"));
    }

    #[test]
    fn first_match_wins() {
        let skills = builtin();
        // "email to Jane hello" hits email, not prompt-better.
        let (hit, vars) = match_skill(&skills, "email to Jane hello there").unwrap();
        assert_eq!(hit.name, "email");
        assert_eq!(vars.get("recipient").unwrap(), "Jane");
    }

    #[test]
    fn prompt_better_triggers() {
        let skills = builtin();
        let (hit, vars) =
            match_skill(&skills, "prompt write a python script to parse CSV").unwrap();
        assert_eq!(hit.name, "prompt-better");
        assert!(vars.get("body").unwrap().contains("python"));
    }
}
