---
name: email
description: Format a dictated thought as a polite email
trigger: ^\s*(?:draft|compose|write|send)?\s*email\s+(?:to|for)?\s*([A-Za-z][a-zA-Z\-']{0,40})\s*[,.]?\s+(.+)$
capture: recipient, body
native: email
---

## System Prompt
Polish the following spoken thought as the body of a polite email to {{recipient}}.
Return only the body paragraph(s) — greeting and sign-off are added by the template.

Rules:
- Preserve every piece of content the user said
- Fix punctuation and capitalization
- Remove filler words (um, uh, like, you know)
- Do NOT paraphrase or add new information
- Weekdays, months, and the pronoun "I" must be capitalized

User's dictation:
{{body}}

## Output Template
Hi {{recipient}},

{{llm_output}}

{{sign_off}},
{{user_name}}
