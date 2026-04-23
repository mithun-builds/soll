//! User-extensible skills system. Each skill is a markdown file.
//!
//! ## Authoring a skill
//!
//! ### `## Intent` — plain English, preferred
//!
//!   Describe in one or two sentences when the skill should activate.
//!   Add an optional `Extract:` line to name the variables the LLM should
//!   pull from the utterance (example):
//!
//!   `Extract: recipient (the person's name), body (what they want to say)`
//!
//!   At runtime a fast LLM call reads all skill intents and decides which
//!   one (if any) matches what the user said.
//!
//! ### `## System Prompt` — sent to Ollama
//!
//!   Use `[var]` to insert extracted values:
//!     [body]      — the utterance (whole or extracted)
//!     [recipient] — any variable named in the Extract line
//!
//! ### `## Output Template` — wraps the AI response
//!
//!   Special variables always available:
//!     [result]    — what Ollama returned
//!     [name]      — the user's name from Settings
//!     [recipient] — (or any extracted variable)
//!
//!   Defaults to `[result]` if the section is absent.
//!
//! ### `## Triggers` — legacy fast-path (optional)
//!
//!   Bulleted list of plain-English phrases compiled to regex. Skills
//!   with triggers are matched instantly without an LLM call. Useful for
//!   power users who always say exactly the same phrase.

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Legacy regex-backed trigger patterns from `## Triggers`. Empty for
    /// intent-based skills.
    pub triggers: Vec<TriggerPattern>,
    /// Plain-English activation description from `## Intent`. The LLM
    /// classifier reads this to decide which skill to route to.
    pub intent: Option<String>,
    /// Variable names the LLM should extract, parsed from the `Extract:`
    /// line in `## Intent`. E.g. `["recipient", "body"]`.
    pub extract_vars: Vec<String>,
    pub native: Option<String>,
    pub system_prompt: String,
    pub output_template: String,
    pub source: SkillSource,
    /// Raw markdown this skill was parsed from. Served to the editor UI.
    pub markdown_source: String,
}

/// Built-in skill markdown, embedded at compile time.
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
    /// Ships with the app. Rendered as "default" in the UI.
    Builtin,
    /// Saved by the user into the skills directory. Rendered as "custom".
    User,
}

impl SkillSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Builtin => "default",
            Self::User => "custom",
        }
    }
}

impl TriggerPattern {
    /// Compile a trigger template into an executable pattern.
    pub fn compile(template: &str) -> Result<Self> {
        let t = template.trim();
        if t.is_empty() {
            return Err(anyhow!("trigger template is empty"));
        }

        // Raw regex escape hatch for advanced users
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

        // Plain-English template. Placeholders: `<name>` or `{name}` (legacy).
        // The LAST placeholder is automatically greedy.
        let mut tokens: Vec<Token> = Vec::new();
        for tok in t.split_whitespace() {
            if let Some(name) = placeholder_name(tok) {
                if name.is_empty() {
                    return Err(anyhow!("empty placeholder in trigger `{t}`"));
                }
                if !name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                {
                    return Err(anyhow!(
                        "placeholder name must be alphanumeric (got `<{name}>`)"
                    ));
                }
                tokens.push(Token::Placeholder(name.to_string()));
            } else {
                let bare = tok.trim_matches(|c: char| !c.is_alphanumeric());
                if bare.is_empty() {
                    continue;
                }
                tokens.push(Token::Literal(bare.to_string()));
            }
        }
        if tokens.is_empty() {
            return Err(anyhow!("trigger `{t}` has no content"));
        }
        let last_placeholder_idx = tokens
            .iter()
            .rposition(|t| matches!(t, Token::Placeholder(_)));

        let mut parts = Vec::new();
        let mut capture_names = Vec::new();
        for (i, tok) in tokens.iter().enumerate() {
            match tok {
                Token::Literal(w) => parts.push(regex::escape(w)),
                Token::Placeholder(name) => {
                    let is_last = Some(i) == last_placeholder_idx;
                    if is_last {
                        parts.push(r"(.+)".to_string());
                    } else {
                        parts.push(r"([A-Za-z][A-Za-z0-9\-'_]*)".to_string());
                    }
                    capture_names.push(name.clone());
                }
            }
        }

        let body = parts.join(r"[\s,.?!;:]+");
        let pat = format!(r"(?i)^\s*{body}\s*[.?!]?\s*$");
        let regex = Regex::new(&pat).with_context(|| format!("compiling `{t}`"))?;
        Ok(TriggerPattern {
            template: t.to_string(),
            regex,
            capture_names,
        })
    }

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

        // `## Triggers` — optional, legacy pattern-matching
        let triggers: Vec<TriggerPattern> = match extract_section(body, "Triggers") {
            Ok(section) => {
                let templates: Vec<String> = section
                    .lines()
                    .filter_map(|l| {
                        let line = l.trim();
                        let line = line
                            .strip_prefix('-')
                            .or_else(|| line.strip_prefix('*'))
                            .unwrap_or(line)
                            .trim();
                        if line.is_empty() { None } else { Some(line.to_string()) }
                    })
                    .collect();
                if templates.is_empty() {
                    return Err(anyhow!(
                        "skill `{name}` has an empty `## Triggers` section — \
                         add trigger lines or remove the section"
                    ));
                }
                templates
                    .iter()
                    .map(|t| TriggerPattern::compile(t))
                    .collect::<Result<Vec<_>>>()
                    .with_context(|| format!("skill `{name}` trigger compile failed"))?
            }
            Err(_) => Vec::new(), // absent is fine
        };

        // `## Intent` — plain-English description for LLM classification
        let (intent, extract_vars) = match extract_section(body, "Intent") {
            Ok(section) => parse_intent_section(&section),
            Err(_) => (None, Vec::new()),
        };

        let system_prompt = extract_section(body, "System Prompt")
            .with_context(|| format!("skill `{name}` missing `## System Prompt` section"))?;

        // Default output template is just [result] (bare LLM output)
        let output_template =
            extract_section(body, "Output Template").unwrap_or_else(|_| "[result]".into());

        Ok(Skill {
            name,
            description,
            triggers,
            intent,
            extract_vars,
            native,
            system_prompt,
            output_template,
            source: SkillSource::Builtin,
            markdown_source: md.to_string(),
        })
    }

    /// Try each legacy trigger in order; return captures from the first match.
    pub fn matches(&self, raw: &str) -> Option<HashMap<String, String>> {
        for t in &self.triggers {
            if let Some(vars) = t.match_vars(raw) {
                return Some(vars);
            }
        }
        None
    }

    pub fn interpolate(&self, template: &str, vars: &HashMap<String, String>) -> String {
        interpolate(template, vars)
    }

    /// Human-readable trigger phrases for the UI (legacy skills only).
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
    let builtin_names: Vec<String> = builtin().into_iter().map(|s| s.name).collect();
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

/// Replace `[var]`, `<var>`, and `{{var}}` placeholders with values from `vars`.
/// Unknown placeholders are left untouched.
pub fn interpolate(template: &str, vars: &HashMap<String, String>) -> String {
    // [var] — preferred square-bracket syntax
    let square = Regex::new(r"\[([A-Za-z][A-Za-z0-9_]*)\]").unwrap();
    let out = square
        .replace_all(template, |caps: &regex::Captures| match vars.get(&caps[1]) {
            Some(v) => v.clone(),
            None => caps[0].to_string(),
        })
        .into_owned();

    // <var> — angle bracket (back-compat)
    let angle = Regex::new(r"<([A-Za-z][A-Za-z0-9_\-]*)>").unwrap();
    let out = angle
        .replace_all(&out, |caps: &regex::Captures| match vars.get(&caps[1]) {
            Some(v) => v.clone(),
            None => caps[0].to_string(),
        })
        .into_owned();

    // {{var}} — double curly (back-compat)
    let curly = Regex::new(r"\{\{([A-Za-z][A-Za-z0-9_\-]*)\}\}").unwrap();
    curly
        .replace_all(&out, |caps: &regex::Captures| match vars.get(&caps[1]) {
            Some(v) => v.clone(),
            None => caps[0].to_string(),
        })
        .into_owned()
}

/// Try each skill's legacy trigger patterns; return the first match.
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

// ── intent parsing ─────────────────────────────────────────────────────────

/// Parse the `## Intent` section body into (description, extract_vars).
///
/// ```text
/// The user wants to send an email to a specific person.
/// Extract: recipient (the person's name), body (what they want to say)
/// ```
///
/// Returns the description lines joined as a single string, and the list of
/// variable names from the `Extract:` line.
fn parse_intent_section(section: &str) -> (Option<String>, Vec<String>) {
    let mut desc_lines: Vec<&str> = Vec::new();
    let mut extract_vars: Vec<String> = Vec::new();

    for line in section.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Extract:") {
            // "recipient (the person's name), body (what they want to say)"
            for part in rest.split(',') {
                let var_name = part
                    .trim()
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !var_name.is_empty() {
                    extract_vars.push(var_name);
                }
            }
        } else if !trimmed.is_empty() {
            desc_lines.push(trimmed);
        }
    }

    let desc = if desc_lines.is_empty() {
        None
    } else {
        Some(desc_lines.join(" "))
    };
    (desc, extract_vars)
}

// ── template internals ─────────────────────────────────────────────────────

enum Token {
    Literal(String),
    Placeholder(String),
}

fn placeholder_name(tok: &str) -> Option<&str> {
    let inner = if let Some(s) = tok.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        s
    } else if let Some(s) = tok.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        s
    } else {
        return None;
    };
    Some(inner.strip_suffix("...").unwrap_or(inner).trim())
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
    let after_line = match after_heading.find('\n') {
        Some(nl) => &after_heading[nl + 1..],
        None => "",
    };
    let end = after_line.find("\n## ").unwrap_or(after_line.len());
    Ok(after_line[..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TriggerPattern compiler ────────────────────────────────

    #[test]
    fn compiles_angle_bracket_template() {
        let p = TriggerPattern::compile("email to <recipient> <body>").unwrap();
        let vars = p
            .match_vars("email to Jane can we push the launch")
            .unwrap();
        assert_eq!(vars.get("recipient").unwrap(), "Jane");
        assert_eq!(vars.get("body").unwrap(), "can we push the launch");
    }

    #[test]
    fn legacy_curly_template_still_works() {
        let p = TriggerPattern::compile("email to {recipient} {body...}").unwrap();
        let vars = p.match_vars("email to Jane hello world").unwrap();
        assert_eq!(vars.get("recipient").unwrap(), "Jane");
        assert_eq!(vars.get("body").unwrap(), "hello world");
    }

    #[test]
    fn last_placeholder_is_automatically_greedy() {
        let p = TriggerPattern::compile("prompt <thought>").unwrap();
        let vars = p
            .match_vars("prompt write a python script that parses csv")
            .unwrap();
        assert_eq!(
            vars.get("thought").unwrap(),
            "write a python script that parses csv"
        );
    }

    #[test]
    fn non_last_placeholder_is_one_word() {
        let p = TriggerPattern::compile("email to <recipient> <body>").unwrap();
        let vars = p.match_vars("email to Mary-Jane the plan is").unwrap();
        assert_eq!(vars.get("recipient").unwrap(), "Mary-Jane");
    }

    #[test]
    fn handles_apostrophes() {
        let p = TriggerPattern::compile("email to <recipient> <body>").unwrap();
        let vars = p.match_vars("email to O'Brien meeting today").unwrap();
        assert_eq!(vars.get("recipient").unwrap(), "O'Brien");
    }

    #[test]
    fn flexible_punctuation_between_literals() {
        let p = TriggerPattern::compile("email to <recipient> <body>").unwrap();
        assert!(p.match_vars("email, to Jane hello").is_some());
        assert!(p.match_vars("email. to Jane hello").is_some());
    }

    #[test]
    fn case_insensitive_match() {
        let p = TriggerPattern::compile("email to <recipient> <body>").unwrap();
        assert!(p.match_vars("EMAIL TO jane HELLO WORLD").is_some());
    }

    #[test]
    fn rejects_empty_placeholder_name() {
        assert!(TriggerPattern::compile("email <> body").is_err());
    }

    #[test]
    fn rejects_empty_template() {
        assert!(TriggerPattern::compile("   ").is_err());
    }

    #[test]
    fn no_match_without_trigger_words() {
        let p = TriggerPattern::compile("email to <recipient> <body>").unwrap();
        assert!(p.match_vars("hello world").is_none());
    }

    #[test]
    fn raw_regex_passthrough() {
        let p = TriggerPattern::compile(r"^\s*debug\s+(\w+)").unwrap();
        let vars = p.match_vars("debug segfault here").unwrap();
        assert_eq!(vars.get("g1").unwrap(), "segfault");
    }

    // ── interpolation ──────────────────────────────────────────

    #[test]
    fn interpolation_square_brackets() {
        let mut vars = HashMap::new();
        vars.insert("recipient".into(), "John".into());
        vars.insert("result".into(), "Body text.".into());
        vars.insert("name".into(), "Mithun".into());
        let tmpl = "Hi [recipient],\n\n[result]\n\nBest,\n[name]";
        assert_eq!(
            interpolate(tmpl, &vars),
            "Hi John,\n\nBody text.\n\nBest,\nMithun"
        );
    }

    #[test]
    fn interpolation_angle_brackets_backcompat() {
        let mut vars = HashMap::new();
        vars.insert("a".into(), "hello".into());
        vars.insert("b".into(), "world".into());
        assert_eq!(
            interpolate("Greet <a>, say <b>!", &vars),
            "Greet hello, say world!"
        );
    }

    #[test]
    fn interpolation_leaves_unknown_placeholders() {
        let vars = HashMap::new();
        // Unknown square-bracket token left intact
        assert_eq!(
            interpolate("Hello [unknown]!", &vars),
            "Hello [unknown]!"
        );
    }

    // ── full skill parsing ─────────────────────────────────────

    const TRIGGER_SKILL: &str = r#"---
name: test-skill
description: A toy
---

## Triggers
- debug <body>
- fix <body>

## System Prompt
Debug: [body]

## Output Template
DEBUG: [result]
"#;

    #[test]
    fn parses_trigger_skill() {
        let s = Skill::from_markdown(TRIGGER_SKILL).unwrap();
        assert_eq!(s.triggers.len(), 2);
        assert!(s.matches("debug memory leak").is_some());
        assert!(s.matches("fix the button color").is_some());
        assert!(s.matches("hello world").is_none());
    }

    #[test]
    fn first_matching_trigger_wins() {
        let s = Skill::from_markdown(TRIGGER_SKILL).unwrap();
        let vars = s.matches("debug foo").unwrap();
        assert_eq!(vars.get("body").unwrap(), "foo");
    }

    const INTENT_SKILL: &str = r#"---
name: intent-skill
description: Intent-based skill
---

## Intent
The user wants to do something specific.
Extract: topic (the subject), body (what they said)

## System Prompt
Handle [topic]: [body]

## Output Template
[result]
"#;

    #[test]
    fn parses_intent_skill() {
        let s = Skill::from_markdown(INTENT_SKILL).unwrap();
        assert!(s.intent.is_some());
        assert!(s.intent.as_ref().unwrap().contains("something specific"));
        assert_eq!(s.extract_vars, vec!["topic", "body"]);
        assert!(s.triggers.is_empty());
    }

    #[test]
    fn no_triggers_section_is_ok() {
        let md = "---\nname: x\n---\n\n## System Prompt\nhi";
        assert!(Skill::from_markdown(md).is_ok());
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
    fn builtin_email_has_intent() {
        let skills = builtin();
        let email = skills.iter().find(|s| s.name == "email").unwrap();
        assert!(email.intent.is_some(), "email skill should have ## Intent");
        assert!(
            email.extract_vars.contains(&"recipient".to_string()),
            "email should extract `recipient`"
        );
        assert!(
            email.extract_vars.contains(&"body".to_string()),
            "email should extract `body`"
        );
    }

    #[test]
    fn builtin_prompt_better_has_intent() {
        let skills = builtin();
        let pb = skills.iter().find(|s| s.name == "prompt-better").unwrap();
        assert!(pb.intent.is_some(), "prompt-better should have ## Intent");
    }

    #[test]
    fn builtin_email_template_uses_square_brackets() {
        let skills = builtin();
        let email = skills.iter().find(|s| s.name == "email").unwrap();
        assert!(email.output_template.contains("[recipient]"));
        assert!(email.output_template.contains("[result]"));
        assert!(email.output_template.contains("[name]"));
    }
}
