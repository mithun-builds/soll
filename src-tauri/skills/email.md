---
name: email
description: Format a dictated thought as a polite email
---

## Intent
The user wants to send an email to a specific person.

## Instructions
The user said something like "email to [name] [their message]" or "email [name] [their message]".

Do this:
1. Find the recipient's name — the first name after "email to", "email for", or just "email"
2. Find the message — everything the user said after the recipient's name
3. Turn the message into one or two natural, polished sentences

Rules:
- Do NOT include "email to", "email for", or any trigger phrase in the body
- Fix grammar, punctuation, filler words (um, uh, like) — do NOT invent anything the user didn't say
- Capitalize "I", weekdays, and months
- Do NOT add a subject line
- Keep it short — match the length of what was dictated

Start with "Hi [recipient's name],"
End with "Best," on one line and the user's name on the next.
Output only the email — no preamble, no explanation.
