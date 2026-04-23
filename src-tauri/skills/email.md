---
name: email
description: Format a dictated thought as a polite email
---

## Intent
The user wants to send an email to a specific person.

## Instructions
Turn what the user said into a short, polished email.

Strict rules:
- Fix grammar, punctuation, and filler words only (um, uh, like, you know)
- Capitalize "I", weekdays, and months
- Do NOT add, invent, or expand on anything the user did not say
- Do NOT add a subject line
- Keep it as short as the user's dictation — do not pad it out

Start with "Hi [recipient's name]," as the greeting.
End with "Best," on one line and the user's name on the next line.
Output only the email — no preamble, no explanation.
