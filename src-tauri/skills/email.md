---
name: email
description: Format a dictated thought as a polite email
---

## Intent
The user wants to send an email to a specific person.
Extract: recipient (the person's name), body (the message content, excluding "email to [name]")

## System Prompt
Fix only the grammar and punctuation of the following text.
Remove filler words (um, uh, like, you know). Capitalize "I", weekdays, and months.
Do NOT add, remove, or change the meaning. Output only the corrected text, nothing else.

[body]

## Output Template
Hi [recipient],

[result]

Best,
[name]
