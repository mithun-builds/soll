//! User-extensible skills system. Each skill is a markdown file with
//! four sections — no hidden features, no surprises.
//!
//! ## Authoring a skill
//!
//! ### `## Name` — required, the skill id
//!
//!   A single line. Becomes the filename and the key used to look up the
//!   skill internally. Short and URL-safe (lowercase, digits, hyphens).
//!
//! ### `## Description` — one-line UI label
//!
//!   Shown in Settings under the skill's row.
//!
//! ### `## Triggers` — required, activation phrases
//!
//!   Bulleted list of plain-English phrases compiled to regex. The first
//!   trigger that matches the utterance wins. Use `<name>` placeholders
//!   to capture variables the instructions/snippet can reference (the
//!   last placeholder in a trigger is automatically greedy).
//!
//! ### `## Instructions` *or* `## Phrase` — required, pick exactly one
//!
//!   **`## Instructions`** — AI skill. The whole section text is sent to
//!   Ollama; whatever it returns is the final pasted output.
//!
//!   **`## Phrase`** — literal paste. The section text is pasted verbatim
//!   after variable interpolation. No LLM call, ~0 ms. Great for canned
//!   replies like meeting links, signatures, boilerplate responses.
//!
//!   Either section can reference:
//!     [body]      — the utterance (or the captured `<body>` variable)
//!     [name]      — the user's name from Settings
//!     [recipient] — any variable named in a trigger's `<placeholder>`

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Regex-backed trigger patterns from `## Triggers`. At least one is
    /// required — triggers are the only activation path.
    pub triggers: Vec<TriggerPattern>,
    pub native: Option<String>,
    /// What the skill does when it fires — either send text to Ollama
    /// (`## Instructions`) or paste a literal phrase (`## Phrase`).
    pub kind: SkillKind,
    /// Raw markdown this skill was parsed from. Served to the editor UI.
    pub markdown_source: String,
}

#[derive(Debug, Clone)]
pub enum SkillKind {
    /// AI skill: contents of `## Instructions` sent to Ollama; model output
    /// is pasted as-is.
    Ai { instructions: String },
    /// Phrase: contents of `## Phrase` are pasted verbatim after variable
    /// interpolation. No LLM call — ideal for canned replies.
    Phrase { text: String },
}

impl SkillKind {
    /// Machine-readable label — "ai" or "phrase" — for the UI.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ai { .. } => "ai",
            Self::Phrase { .. } => "phrase",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TriggerPattern {
    pub template: String,
    regex: Regex,
    capture_names: Vec<String>,
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
        let (fm, body) = split_frontmatter(md);
        let meta = parse_frontmatter(fm);

        // `## Name` is the preferred place for the skill id. YAML frontmatter
        // (`name:`) is still accepted for back-compat with older skills.
        let name = section_first_line(body, "Name")
            .or_else(|| meta.get("name").cloned())
            .ok_or_else(|| anyhow!("skill missing `## Name` section"))?;
        validate_name(&name)?;

        // `## Description` is preferred; frontmatter `description:` still works.
        let description = section_first_line(body, "Description")
            .or_else(|| meta.get("description").cloned())
            .unwrap_or_default();

        let native = meta.get("native").cloned();

        // `## Triggers` — required. Bulleted list compiled to regex.
        let section = extract_section(body, "Triggers")
            .with_context(|| format!("skill `{name}` needs `## Triggers`"))?;
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
                 add at least one trigger phrase"
            ));
        }
        let triggers: Vec<TriggerPattern> = templates
            .iter()
            .map(|t| TriggerPattern::compile(t))
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("skill `{name}` trigger compile failed"))?;

        // Exactly one of `## Instructions` (AI skill) or `## Phrase`
        // (literal paste) — not both, not neither.
        let instructions = extract_section(body, "Instructions").ok();
        let phrase = extract_section(body, "Phrase").ok();
        let kind = match (instructions, phrase) {
            (Some(i), None) => {
                if i.trim().is_empty() {
                    return Err(anyhow!(
                        "skill `{name}` has an empty `## Instructions` section"
                    ));
                }
                SkillKind::Ai { instructions: i }
            }
            (None, Some(p)) => {
                if p.trim().is_empty() {
                    return Err(anyhow!(
                        "skill `{name}` has an empty `## Phrase` section"
                    ));
                }
                SkillKind::Phrase { text: p }
            }
            (Some(_), Some(_)) => {
                return Err(anyhow!(
                    "skill `{name}` has both `## Instructions` and `## Phrase` — \
                     pick one (AI skill or phrase, not both)"
                ));
            }
            (None, None) => {
                return Err(anyhow!(
                    "skill `{name}` needs `## Instructions` (AI skill) or \
                     `## Phrase` (literal paste)"
                ));
            }
        };

        Ok(Skill {
            name,
            description,
            triggers,
            native,
            kind,
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

/// Load every skill found in `user_dir`. Returns skills in stable
/// alphabetical order (so the Settings UI has a predictable listing).
/// Individual file failures are logged and skipped — one broken skill
/// never takes down the others.
pub fn load_all(user_dir: Option<&std::path::Path>) -> Vec<Skill> {
    let Some(dir) = user_dir else { return Vec::new() };
    if !dir.exists() {
        return Vec::new();
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            log::error!("read_dir({:?}): {e:?}", dir.display());
            return Vec::new();
        }
    };

    let mut by_name: std::collections::BTreeMap<String, Skill> = std::collections::BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().map(|s| s == "md").unwrap_or(false) {
            continue;
        }
        let md = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                log::error!("read {:?}: {e:?}", path.display());
                continue;
            }
        };
        match Skill::from_markdown(&md) {
            Ok(s) => {
                log::info!("loaded skill: {} (from {})", s.name, path.display());
                by_name.insert(s.name.clone(), s);
            }
            Err(e) => log::error!("skill {:?}: {e:?}", path.display()),
        }
    }
    by_name.into_values().collect()
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

/// Try each skill's trigger patterns; return the first match.
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

/// Explicit "use [skill-name] [body]" invocation — the voice equivalent of a
/// slash command. Matches utterances like:
///
///   "use commit fixed the null pointer bug"
///   "Use email, John tomorrow at 5pm"
///   "USE COMMIT Fixed it."
///
/// Case-insensitive on "use" and the skill name. Body preserves original
/// capitalisation so commit messages, email bodies etc. look right.
/// Falls back to `None` if the prefix is absent or the name is unknown.
pub fn direct_invoke<'a>(
    skills: &'a [Skill],
    raw: &str,
) -> Option<(&'a Skill, HashMap<String, String>)> {
    // Skip any leading non-alphabetic characters (Whisper sometimes adds them).
    let trimmed = raw.trim();
    let content_start = trimmed
        .char_indices()
        .find(|(_, c)| c.is_alphabetic())
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    let content = &trimmed[content_start..];

    // Require case-insensitive "use" at the front.
    if !content.to_lowercase().starts_with("use") {
        return None;
    }
    let after_use = &content[3..]; // skip "use"

    // Skip the separator between "use" and the skill name (space, comma, etc.).
    let name_start = after_use
        .char_indices()
        .find(|(_, c)| c.is_alphabetic())
        .map(|(i, _)| i)
        .unwrap_or(after_use.len());
    let after_sep = &after_use[name_start..];
    if after_sep.is_empty() {
        return None;
    }

    // Skill name: run of lowercase letters, digits, and hyphens.
    let name_len = after_sep
        .char_indices()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '-')
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    if name_len == 0 {
        return None;
    }
    let skill_name = after_sep[..name_len].to_lowercase();

    // Body: everything after the skill name, trimmed of leading separators
    // and trailing sentence-ending punctuation.
    let after_name = &after_sep[name_len..];
    let body_start = after_name
        .char_indices()
        .find(|(_, c)| c.is_alphanumeric())
        .map(|(i, _)| i)
        .unwrap_or(after_name.len());
    let body_raw = &after_name[body_start..];
    let body = body_raw
        .trim_end_matches(|c: char| matches!(c, '.' | '!' | '?' | ','))
        .trim()
        .to_string();

    let skill = skills.iter().find(|s| s.name == skill_name)?;
    let mut vars = HashMap::new();
    vars.insert("body".into(), body);
    Some((skill, vars))
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

/// Split optional YAML frontmatter from the body. Skills written purely with
/// `## Name`/`## Description` sections have no frontmatter — in that case
/// this returns an empty meta slice and the whole document as the body.
fn split_frontmatter(md: &str) -> (&str, &str) {
    let trimmed = md.trim_start();
    let Some(after_open) = trimmed.strip_prefix("---") else {
        return ("", md);
    };
    match after_open.split_once("\n---") {
        Some((fm, rest)) => (fm, rest.trim_start_matches('\n')),
        None => {
            log::warn!("skill markdown has unterminated frontmatter; treating as body");
            ("", md)
        }
    }
}

/// Validate a skill's name. A name is used as a filename and as a key, so
/// it must be short and URL/filesystem-safe. Rules:
///   - non-empty, at most 40 chars
///   - starts with a lowercase ASCII letter
///   - remaining chars are lowercase letters, digits, or `-`
///   - does not end with a hyphen
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("name cannot be empty"));
    }
    if name.len() > 40 {
        return Err(anyhow!("name too long (max 40 characters)"));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() {
        return Err(anyhow!(
            "name must start with a lowercase letter (got `{first}`)"
        ));
    }
    if name.ends_with('-') {
        return Err(anyhow!("name cannot end with a hyphen"));
    }
    for c in name.chars() {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(anyhow!(
                "name may only contain lowercase letters, digits, and hyphens (got `{c}`)"
            ));
        }
    }
    Ok(())
}

/// Read a section and return its first non-empty, non-comment line, trimmed.
/// Used for `## Name` and `## Description` where a single-line value is
/// expected. Returns `None` when the section is absent or entirely empty.
fn section_first_line(body: &str, name: &str) -> Option<String> {
    extract_section(body, name).ok().and_then(|s| {
        s.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_string)
    })
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
    let raw = &after_line[..end];
    // Strip comment lines (lines whose first non-whitespace character is `#`).
    // This allows skills to embed authoring notes inside sections without
    // affecting runtime behaviour.
    let filtered: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(filtered.trim().to_string())
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

## Instructions
Debug: [body]
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

    #[test]
    fn skill_without_triggers_errors() {
        let md = "---\nname: x\n---\n\n## Instructions\nhi";
        assert!(Skill::from_markdown(md).is_err());
    }

    #[test]
    fn empty_triggers_section_errors() {
        let md = "---\nname: x\n---\n\n## Triggers\n\n## Instructions\nhi";
        assert!(Skill::from_markdown(md).is_err());
    }

    #[test]
    fn empty_instructions_section_errors() {
        let md = "---\nname: x\n---\n\n## Triggers\n- debug <body>\n\n## Instructions\n";
        assert!(Skill::from_markdown(md).is_err());
    }

    #[test]
    fn missing_instructions_errors() {
        let md = "---\nname: x\n---\n\n## Triggers\n- debug <body>";
        assert!(Skill::from_markdown(md).is_err());
    }

    // ── Name / Description sections ────────────────────────────

    const MINIMAL_VALID_TAIL: &str =
        "\n\n## Triggers\n- debug <body>\n\n## Instructions\nhi\n";

    #[test]
    fn reads_name_from_section() {
        let md = format!("## Name\nsummarize{MINIMAL_VALID_TAIL}");
        let s = Skill::from_markdown(&md).unwrap();
        assert_eq!(s.name, "summarize");
    }

    #[test]
    fn reads_description_from_section() {
        let md = format!(
            "## Name\nx\n\n## Description\nTurn rambling into a summary{MINIMAL_VALID_TAIL}"
        );
        let s = Skill::from_markdown(&md).unwrap();
        assert_eq!(s.description, "Turn rambling into a summary");
    }

    #[test]
    fn section_name_beats_frontmatter_name() {
        let md = format!(
            "---\nname: old-name\n---\n\n## Name\nnew-name{MINIMAL_VALID_TAIL}"
        );
        let s = Skill::from_markdown(&md).unwrap();
        assert_eq!(s.name, "new-name");
    }

    #[test]
    fn frontmatter_name_still_works() {
        let md = format!("---\nname: legacy\n---\n{MINIMAL_VALID_TAIL}");
        let s = Skill::from_markdown(&md).unwrap();
        assert_eq!(s.name, "legacy");
    }

    #[test]
    fn errors_when_neither_name_source_present() {
        let md = format!("## Instructions\nhi{MINIMAL_VALID_TAIL}");
        assert!(Skill::from_markdown(&md).is_err());
    }

    #[test]
    fn name_section_strips_comment_lines() {
        let md = format!(
            "## Name\n# this is a comment\nreal-name{MINIMAL_VALID_TAIL}"
        );
        let s = Skill::from_markdown(&md).unwrap();
        assert_eq!(s.name, "real-name");
    }

    #[test]
    fn no_frontmatter_document_parses() {
        let md = format!(
            "## Name\nx\n\n## Description\ny{MINIMAL_VALID_TAIL}"
        );
        let s = Skill::from_markdown(&md).unwrap();
        assert_eq!(s.name, "x");
        assert_eq!(s.description, "y");
    }

    // ── Name validation ────────────────────────────────────────

    #[test]
    fn name_accepts_valid_ids() {
        assert!(validate_name("email").is_ok());
        assert!(validate_name("email-me").is_ok());
        assert!(validate_name("a1b2").is_ok());
        assert!(validate_name("x").is_ok());
    }

    #[test]
    fn name_rejects_empty() {
        assert!(validate_name("").is_err());
    }

    #[test]
    fn name_rejects_uppercase() {
        assert!(validate_name("Email").is_err());
        assert!(validate_name("my-SKILL").is_err());
    }

    #[test]
    fn name_rejects_spaces_and_symbols() {
        assert!(validate_name("my skill").is_err());
        assert!(validate_name("my_skill").is_err());
        assert!(validate_name("my/skill").is_err());
        assert!(validate_name("my.skill").is_err());
    }

    #[test]
    fn name_rejects_leading_digit_or_hyphen() {
        assert!(validate_name("1skill").is_err());
        assert!(validate_name("-skill").is_err());
    }

    #[test]
    fn name_rejects_trailing_hyphen() {
        assert!(validate_name("skill-").is_err());
    }

    #[test]
    fn name_rejects_too_long() {
        assert!(validate_name(&"a".repeat(41)).is_err());
        assert!(validate_name(&"a".repeat(40)).is_ok());
    }

    #[test]
    fn from_markdown_applies_name_validation() {
        let md = format!("## Name\nMy Skill{MINIMAL_VALID_TAIL}");
        assert!(
            Skill::from_markdown(&md).is_err(),
            "space in name should reject"
        );
    }

    // ── direct_invoke ──────────────────────────────────────────

    fn make_skill(name: &str) -> Skill {
        Skill::from_markdown(&format!(
            "## Name\n{name}\n\n## Description\ntest\n\n## Triggers\n- {name} <body>\n\n## Instructions\n[body]\n"
        ))
        .unwrap()
    }

    #[test]
    fn direct_invoke_basic() {
        let skills = vec![make_skill("commit")];
        let (s, vars) = direct_invoke(&skills, "use commit fixed the auth bug").unwrap();
        assert_eq!(s.name, "commit");
        assert_eq!(vars["body"], "fixed the auth bug");
    }

    #[test]
    fn direct_invoke_case_insensitive() {
        let skills = vec![make_skill("commit")];
        let (s, vars) = direct_invoke(&skills, "USE COMMIT Fixed the auth bug").unwrap();
        assert_eq!(s.name, "commit");
        // Body preserves original case.
        assert_eq!(vars["body"], "Fixed the auth bug");
    }

    #[test]
    fn direct_invoke_strips_punctuation() {
        let skills = vec![make_skill("commit")];
        let (_, vars) = direct_invoke(&skills, "Use commit, fixed the auth bug.").unwrap();
        assert_eq!(vars["body"], "fixed the auth bug");
    }

    #[test]
    fn direct_invoke_unknown_skill_returns_none() {
        let skills = vec![make_skill("commit")];
        assert!(direct_invoke(&skills, "use email hello world").is_none());
    }

    #[test]
    fn direct_invoke_requires_use_prefix() {
        let skills = vec![make_skill("commit")];
        // "commit fixed the bug" should NOT match direct_invoke (it's a trigger match).
        assert!(direct_invoke(&skills, "commit fixed the bug").is_none());
    }

    #[test]
    fn direct_invoke_with_empty_body() {
        let skills = vec![make_skill("calendly")];
        let (s, vars) = direct_invoke(&skills, "use calendly").unwrap();
        assert_eq!(s.name, "calendly");
        assert_eq!(vars["body"], "");
    }

    // ── load_all ───────────────────────────────────────────────

    #[test]
    fn load_all_returns_empty_when_dir_absent() {
        assert!(load_all(None).is_empty());
        let missing = std::path::PathBuf::from("/tmp/soll-test-missing-dir-xyz");
        assert!(load_all(Some(&missing)).is_empty());
    }

    #[test]
    fn load_all_reads_md_files_and_skips_broken_ones() {
        let tmp = std::env::temp_dir().join(format!(
            "soll-skills-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("good.md"),
            "## Name\ngood\n\n## Description\nd\n\n## Triggers\n- do <body>\n\n## Instructions\nhi [body]\n",
        )
        .unwrap();
        std::fs::write(tmp.join("broken.md"), "not a skill at all").unwrap();
        std::fs::write(tmp.join("ignored.txt"), "## Name\nx").unwrap();

        let skills = load_all(Some(&tmp));
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "good");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
