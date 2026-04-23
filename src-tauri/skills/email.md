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
Keep every piece of content the user mentioned. Do not add a greeting or sign-off — those are added by the template.

[body]

## Output Template
Hi [recipient],

[result]

Best,
[name]
