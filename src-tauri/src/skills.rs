//! User-extensible skills system. Each skill is a single markdown file
//! describing:
//!
//!   1. YAML-lite frontmatter:
//!      name        — stable id
//!      description — one-liner for the UI
//!      native      — optional hook key (e.g. "email") into hardcoded logic
//!
//!   2. `## Triggers` section — bulleted list of plain-English phrases.
//!      Each phrase may contain:
//!        {name}        single word/name (letters, digits, hyphen, apostrophe)
//!        {name...}     everything until end-of-utterance
//!      Whitespace + common punctuation between literal words is flexible.
//!      First trigger that matches wins.
//!
//!   3. `## System Prompt` — sent to Ollama with `{{var}}` substitutions
//!      for captures + built-ins ({{body}}, {{user_name}}, {{sign_off}}).
//!
//!   4. `## Output Template` — wraps Ollama's response ({{llm_output}} + caps).
//!      Optional; defaults to `{{llm_output}}`.

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub triggers: Vec<TriggerPattern>,
    pub native: Option<String>,
    pub system_prompt: String,
    pub output_template: String,
    pub source: SkillSource,
    /// Raw markdown this skill was parsed from. Served to the editor UI
    /// so users can see and modify the exact text.
    pub markdown_source: String,
}

/// Built-in skill markdown, embedded at compile time. Keeping a named
/// lookup lets us:
///   - populate the "New skill" form with a starter template
///   - reset an overridden built-in back to its factory version
///   - show the default markdown if the user deletes their override
pub const BUILTIN_SOURCES: &[(&str, &str)] = &[
    ("email", include_str!("../skills/email.md")),
    ("prompt-better", include_str!("../skills/prompt_better.md")),
];

pub fn builtin_source(name: &str) -> Option<&'static str> {
    BUILTIN_SOURCES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| *s)
}

pub fn builtin_names() -> impl Iterator<Item = &'static str> {
    BUILTIN_SOURCES.iter().map(|(n, _)| *n)
}

#[derive(Debug, Clone)]
pub struct TriggerPattern {
    pub template: String,
    regex: Regex,
    capture_names: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    Builtin,
    User,
}

impl SkillSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::User => "user",
        }
    }
}

impl TriggerPattern {
    /// Compile a trigger template (or a raw regex if the template starts
    /// with `^`) into an executable pattern. Returns an error on malformed
    /// placeholders.
    pub fn compile(template: &str) -> Result<Self> {
        let t = template.trim();
        if t.is_empty() {
            return Err(anyhow!("trigger template is empty"));
        }

        // Escape hatch for advanced users — raw regex starting with `^`
        // bypasses the template compiler. Captures get auto-named $1, $2, …
        if t.starts_with('^') {
            let re = Regex::new(&format!("(?i){}", t))
                .with_context(|| format!("invalid regex trigger: {t}"))?;
            let n = re.captures_len().saturating_sub(1);
            let capture_names: Vec<String> = (1..=n).map(|i| format!("g{i}")).collect();
            return Ok(TriggerPattern {
                template: t.to_string(),
                regex: re,
                capture_names,
            });
        }

        // Simple-English template: split on whitespace, render each token
        // as either a literal or a capture group. Join tokens with a
        // flexible whitespace-or-punctuation separator so speech with
        // different punctuation still matches.
        let mut parts = Vec::new();
        let mut capture_names = Vec::new();

        for tok in t.split_whitespace() {
            if let Some(inner) = tok.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                let (raw_name, greedy) = match inner.strip_suffix("...") {
                    Some(n) => (n, true),
                    None => (inner, false),
                };
                let name = raw_name.trim();
                if name.is_empty() {
                    return Err(anyhow!("empty placeholder `{{...}}` in `{t}`"));
                }
                if !name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                {
                    return Err(anyhow!(
                        "placeholder name must be alphanumeric (got `{name}`)"
                    ));
                }
                if greedy {
                    parts.push(r"(.+)".to_string());
                } else {
                    // Single-word: letters + digits + hyphen + apostrophe
                    parts.push(r"([A-Za-z][A-Za-z0-9\-'_]*)".to_string());
                }
                capture_names.push(name.to_string());
            } else {
                // Literal: strip surrounding punctuation, escape the rest.
                // That way "email," and "email" in a template both match
                // "email" with optional trailing punctuation in speech.
                let bare = tok.trim_matches(|c: char| !c.is_alphanumeric());
                if bare.is_empty() {
                    continue;
                }
                parts.push(regex::escape(bare));
            }
        }

        if parts.is_empty() {
            return Err(anyhow!("trigger `{t}` has no content"));
        }

        // Between every pair of parts, allow whitespace and/or one piece
        // of punctuation. At both ends, allow leading/trailing padding.
        let body = parts.join(r"[\s,.?!;:]+");
        let pat = format!(r"(?i)^\s*{body}\s*[.?!]?\s*$");
        let regex = Regex::new(&pat).with_context(|| format!("compiling `{t}`"))?;
        Ok(TriggerPattern {
            template: t.to_string(),
            regex,
            capture_names,
        })
    }

    /// Try to match the raw text. Returns variable bindings on success.
    pub fn match_vars(&self, raw: &str) -> Option<HashMap<String, String>> {
        let caps = self.regex.captures(raw.trim())?;
        let mut vars = HashMap::new();
        for (i, name) in self.capture_names.iter().enumerate() {
            if let Some(m) = caps.get(i + 1) {
                vars.insert(name.clone(), m.as_str().trim().to_string());
            }
        }
        for name in &self.capture_names {
            if vars.get(name).map(|v| v.is_empty()).unwrap_or(true) {
                return None;
            }
        }
        Some(vars)
    }

    pub fn capture_names(&self) -> &[String] {
        &self.capture_names
    }
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
        let native = meta.get("native").cloned();

        // Triggers come from the `## Triggers` bulleted-list section.
        let triggers_section = extract_section(body, "Triggers")
            .with_context(|| format!("skill `{name}` missing `## Triggers` section"))?;
        let trigger_templates: Vec<String> = triggers_section
            .lines()
            .filter_map(|l| {
                let line = l.trim();
                let line = line
                    .strip_prefix('-')
                    .or_else(|| line.strip_prefix('*'))
                    .unwrap_or(line)
                    .trim();
                if line.is_empty() {
                    None
                } else {
                    Some(line.to_string())
                }
            })
            .collect();
        if trigger_templates.is_empty() {
            return Err(anyhow!(
                "skill `{name}` has no triggers (add `- phrase` lines under ## Triggers)"
            ));
        }
        let triggers: Vec<TriggerPattern> = trigger_templates
            .iter()
            .map(|t| TriggerPattern::compile(t))
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("skill `{name}` trigger compile failed"))?;

        let system_prompt = extract_section(body, "System Prompt")
            .with_context(|| format!("skill `{name}` missing `## System Prompt` section"))?;
        let output_template =
            extract_section(body, "Output Template").unwrap_or_else(|_| "{{llm_output}}".into());

        Ok(Skill {
            name,
            description,
            triggers,
            native,
            system_prompt,
            output_template,
            source: SkillSource::Builtin,
            markdown_source: md.to_string(),
        })
    }

    /// Try each trigger in order; return captures from the first match.
    pub fn matches(&self, raw: &str) -> Option<HashMap<String, String>> {
        for t in &self.triggers {
            if let Some(vars) = t.match_vars(raw) {
                return Some(vars);
            }
        }
        None
    }

    pub fn interpolate(&self, template: &str, vars: &HashMap<String, String>) -> String {
        let mut out = template.to_string();
        for (k, v) in vars {
            let needle = format!("{{{{{}}}}}", k);
            out = out.replace(&needle, v);
        }
        out
    }

    /// Human-readable trigger phrases for the UI.
    pub fn trigger_templates(&self) -> Vec<String> {
        self.triggers.iter().map(|t| t.template.clone()).collect()
    }
}

pub fn builtin() -> Vec<Skill> {
    let mut out = Vec::new();
    for (_, md) in BUILTIN_SOURCES {
        match Skill::from_markdown(md) {
            Ok(s) => out.push(s),
            Err(e) => log::error!("built-in skill parse failed: {e:?}"),
        }
    }
    out
}

pub fn load_all(user_dir: Option<&std::path::Path>) -> Vec<Skill> {
    // Index by name so user-provided files can OVERRIDE same-named built-ins.
    // Preserve insertion order for built-ins so the UI list is stable.
    let mut by_name: std::collections::BTreeMap<String, Skill> = std::collections::BTreeMap::new();
    for s in builtin() {
        by_name.insert(s.name.clone(), s);
    }
    if let Some(dir) = user_dir {
        if dir.exists() {
            match std::fs::read_dir(dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().map(|s| s == "md").unwrap_or(false) {
                            match std::fs::read_to_string(&path) {
                                Ok(md) => match Skill::from_markdown(&md) {
                                    Ok(mut s) => {
                                        s.source = SkillSource::User;
                                        log::info!(
                                            "loaded user skill: {} (from {})",
                                            s.name,
                                            path.display()
                                        );
                                        by_name.insert(s.name.clone(), s);
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
    // Emit built-ins first (stable order), then user-only skills alphabetically.
    let builtin_names: Vec<String> = builtin()
        .into_iter()
        .map(|s| s.name)
        .collect();
    let mut out: Vec<Skill> = Vec::with_capacity(by_name.len());
    for n in &builtin_names {
        if let Some(s) = by_name.remove(n) {
            out.push(s);
        }
    }
    for (_, s) in by_name {
        out.push(s);
    }
    out
}

pub fn match_skill<'a>(
    skills: &'a [Skill],
    raw: &str,
) -> Option<(&'a Skill, HashMap<String, String>)> {
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

fn extract_section(body: &str, name: &str) -> Result<String> {
    let heading = format!("## {name}");
    let start = body
        .find(&heading)
        .ok_or_else(|| anyhow!("section `{name}` not found"))?;
    let after_heading = &body[start + heading.len()..];
    // Consume the rest of the heading line up to and including its newline
    // so the section body proper starts after it.
    let after_line = match after_heading.find('\n') {
        Some(nl) => &after_heading[nl + 1..],
        None => "",
    };
    // The section ends at the next "## " heading or end-of-body. We leave
    // the preceding "\n" in the match so sections separated by a blank
    // line still terminate cleanly.
    let end = after_line.find("\n## ").unwrap_or(after_line.len());
    Ok(after_line[..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── template compiler ──────────────────────────────────────

    #[test]
    fn compiles_simple_template() {
        let p = TriggerPattern::compile("email to {recipient} {body...}").unwrap();
        let vars = p
            .match_vars("email to Jane can we push the launch")
            .unwrap();
        assert_eq!(vars.get("recipient").unwrap(), "Jane");
        assert_eq!(
            vars.get("body").unwrap(),
            "can we push the launch"
        );
    }

    #[test]
    fn handles_hyphenated_names() {
        let p = TriggerPattern::compile("email to {recipient} {body...}").unwrap();
        let vars = p.match_vars("email to Mary-Jane hello").unwrap();
        assert_eq!(vars.get("recipient").unwrap(), "Mary-Jane");
    }

    #[test]
    fn handles_apostrophes() {
        let p = TriggerPattern::compile("email to {recipient} {body...}").unwrap();
        let vars = p.match_vars("email to O'Brien meeting today").unwrap();
        assert_eq!(vars.get("recipient").unwrap(), "O'Brien");
    }

    #[test]
    fn flexible_punctuation_between_literals() {
        let p = TriggerPattern::compile("email to {recipient} {body...}").unwrap();
        assert!(p.match_vars("email, to Jane hello").is_some());
        assert!(p.match_vars("email. to Jane hello").is_some());
        assert!(p.match_vars("email to, Jane, hello").is_some());
    }

    #[test]
    fn greedy_placeholder_captures_rest() {
        let p = TriggerPattern::compile("prompt {body...}").unwrap();
        let vars = p
            .match_vars("prompt write a python script that parses csv")
            .unwrap();
        assert_eq!(
            vars.get("body").unwrap(),
            "write a python script that parses csv"
        );
    }

    #[test]
    fn case_insensitive_match() {
        let p = TriggerPattern::compile("email to {recipient} {body...}").unwrap();
        assert!(p
            .match_vars("EMAIL TO jane HELLO WORLD")
            .is_some());
    }

    #[test]
    fn rejects_empty_placeholder_name() {
        assert!(TriggerPattern::compile("email {} body").is_err());
    }

    #[test]
    fn rejects_empty_template() {
        assert!(TriggerPattern::compile("   ").is_err());
    }

    #[test]
    fn no_match_without_trigger_words() {
        let p = TriggerPattern::compile("email to {recipient} {body...}").unwrap();
        assert!(p.match_vars("hello world").is_none());
    }

    #[test]
    fn raw_regex_passthrough() {
        let p = TriggerPattern::compile(r"^\s*debug\s+(\w+)").unwrap();
        let vars = p.match_vars("debug segfault here").unwrap();
        assert_eq!(vars.get("g1").unwrap(), "segfault");
    }

    // ── full skill parsing ─────────────────────────────────────

    const SAMPLE: &str = r#"---
name: test-skill
description: A toy
---

## Triggers
- debug {body...}
- fix {body...}

## System Prompt
Debug: {{body}}

## Output Template
DEBUG: {{llm_output}}
"#;

    #[test]
    fn parses_multi_trigger_skill() {
        let s = Skill::from_markdown(SAMPLE).unwrap();
        assert_eq!(s.triggers.len(), 2);
        assert!(s.matches("debug memory leak").is_some());
        assert!(s.matches("fix the button color").is_some());
        assert!(s.matches("hello world").is_none());
    }

    #[test]
    fn first_matching_trigger_wins() {
        let s = Skill::from_markdown(SAMPLE).unwrap();
        let vars = s.matches("debug foo").unwrap();
        assert_eq!(vars.get("body").unwrap(), "foo");
    }

    #[test]
    fn missing_triggers_section_errors() {
        let md = "---\nname: x\n---\n\n## System Prompt\nhi";
        assert!(Skill::from_markdown(md).is_err());
    }

    #[test]
    fn empty_triggers_section_errors() {
        let md = "---\nname: x\n---\n\n## Triggers\n\n## System Prompt\nhi";
        assert!(Skill::from_markdown(md).is_err());
    }

    // ── built-ins ──────────────────────────────────────────────

    #[test]
    fn builtin_skills_all_parse() {
        let skills = builtin();
        assert!(!skills.is_empty());
        assert!(skills.iter().any(|s| s.name == "email"));
        assert!(skills.iter().any(|s| s.name == "prompt-better"));
    }

    #[test]
    fn builtin_email_matches_all_variants() {
        let skills = builtin();
        for raw in [
            "email to Jane hello world",
            "draft email to Jane hello world",
            "compose email for Jane hello world",
            "send email to Jane hello world",
        ] {
            let (hit, vars) = match_skill(&skills, raw).unwrap_or_else(|| {
                panic!("no match for: {raw}");
            });
            assert_eq!(hit.name, "email");
            assert_eq!(vars.get("recipient").unwrap(), "Jane");
        }
    }

    #[test]
    fn builtin_prompt_better_matches() {
        let skills = builtin();
        for raw in [
            "prompt write a python script",
            "prompt about fixing a flaky test",
            "make a prompt for my pr description",
        ] {
            let (hit, _) = match_skill(&skills, raw).unwrap_or_else(|| {
                panic!("no match for: {raw}");
            });
            assert_eq!(hit.name, "prompt-better");
        }
    }

    #[test]
    fn plain_prose_matches_no_skill() {
        let skills = builtin();
        assert!(match_skill(&skills, "hello world how are you today").is_none());
    }
}
