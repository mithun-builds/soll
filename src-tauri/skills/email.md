---
name: email
description: Format a dictated thought as a polite email
native: email
---

## Triggers
- email to {recipient} {body...}
- email for {recipient} {body...}
- draft email to {recipient} {body...}
- compose email for {recipient} {body...}
- compose email to {recipient} {body...}
- send email to {recipient} {body...}
- write email to {recipient} {body...}

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
