---
name: email
description: Format a dictated thought as a polite email
---

## Intent
The user wants to send an email to a specific person.
Extract: recipient (the person's name), body (the message content, not including "email to [name]")

## Instructions
Write a short, polished email to the recipient.
Fix grammar, punctuation, filler words (um, uh, like). Capitalize "I", weekdays, months.
Do NOT invent or add anything beyond what the user said.
Do NOT add a subject line.

Start with "Hi [recipient],"
End with "Best," on one line and the user's name on the next.
Output only the email — no preamble, no explanation.
