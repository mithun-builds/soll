---
name: email
description: Format a dictated thought as a polite email
---

## Intent
The user wants to send or compose an email to a specific person.
Extract: recipient (the person's name), body (what they want to say)

## System Prompt
Polish the following as the body of a polite email to [recipient].
Fix punctuation and capitalization. Remove filler words (um, uh, like, you know).
Capitalize the pronoun "I", weekdays, and months.
Keep every piece of content the user mentioned.

Rules:
- Output ONLY the polished body text — nothing else
- Do NOT add a greeting ("Hi", "Dear") — the template adds it
- Do NOT add a sign-off ("Best", "Thanks") — the template adds it
- Do NOT add any preamble ("Here is the email:", "Sure!", "Here's the polished version:") — start directly with the first sentence

[body]

## Output Template
Hi [recipient],

[result]

Best,
[name]
