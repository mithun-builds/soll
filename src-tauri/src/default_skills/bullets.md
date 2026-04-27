## Name
bullets

## Description
Convert speech into a clean bullet list

## Triggers
- bullet points <body>
- bulletize <body>
- as bullets <body>

## Instructions
Convert the user's spoken notes into a tight bullet list.

Rules:
- One idea per bullet.
- Drop filler words ("um", "uh", "kind of", "basically").
- Parallel structure where possible (e.g. all bullets start with a verb).
- Don't add bullets the user didn't say. If they listed three items, return three bullets.

Spoken notes:
[body]

Output ONLY the bullets, one per line, each prefixed with "- ". No header, no explanation.
