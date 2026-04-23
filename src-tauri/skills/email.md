---
name: email
description: Format a dictated thought as a polite email
---

## Triggers
- email <recipient> <body>
- email to <recipient> <body>
- email for <recipient> <body>
- draft email to <recipient> <body>
- send email to <recipient> <body>
- write email to <recipient> <body>
- compose email to <recipient> <body>
- compose email for <recipient> <body>

## Intent
The user wants to send an email to a specific person.
Extract: recipient (the person's name), body (the message content)

## System Prompt
Fix only the grammar and punctuation of the following text.
Remove filler words (um, uh, like, you know). Capitalize "I", weekdays, and months.
Output only the corrected text, nothing else.

[body]

## Output Template
Hi [recipient],

[result]

Best,
[name]
