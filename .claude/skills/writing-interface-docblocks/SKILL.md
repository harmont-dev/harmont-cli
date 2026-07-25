---
name: writing-interface-docblocks
description: Use when writing or editing any docblock, doc comment, docstring, function/method/class header, or API doc — in any language, including when documenting existing code or code you just changed.
---

# Writing Interface Docblocks

## Overview

**A docblock is a contract, not a transcript and not a changelog.** It tells a caller — who has never seen the body, and never saw your prompt — what the thing IS and how to use it. Terse. Present tense. Timeless.

If the reader could learn it by reading the body, the docblock does not repeat it. If the reader only knows it because they saw your conversation, the docblock never mentions it.

**Violating the letter of these rules is violating the spirit.**

## The one test

Before writing any line, ask: *does a caller need this to use the function correctly, without reading the body?*

- Yes → it belongs (purpose, params, return, errors, invariants, caller constraints).
- No → delete it.

## Belongs vs excluded

| Belongs (the interface) | Excluded (the implementation / the transcript) |
|---|---|
| What it does, in one line | Step-by-step of how it does it ("proceeds as follows: 1, 2, 3") |
| What params/return *mean* | Restating the signature or types already visible |
| When it errors/panics (the *when*) | Why you chose errors over panics; internal mechanism |
| Caller-visible invariants & constraints | Private fields, helpers, backing arrays, Big-O of the body |
| Non-obvious usage caveats | Analogies to what the code "is like" |

## Never leak the prompt

The doc describes current behavior as if it always existed. It has no memory of your task.

Ban these — they mean you are documenting the conversation, not the contract:
- `as requested`, `as required`, `to satisfy`, `per the requirement`
- `rather than X`, `instead of X`, `now returns`, `changed to` — no comparison to old/alternative behavior
- Parroting a constraint you were given: asked "must never panic" → do **not** write "this never panics." State the contract (`# Errors: returns ... when ...`); absence of panic is implied by the return type.

## Terse by default

Most functions need 1–3 lines. Reach for more only when a caller-facing caveat genuinely requires it. A docblock longer than the body of a simple function is a smell.

Never guess at behavior to pad length — a wrong doc is worse than a short one.

## Inline `//` comments — heavily avoided

Inline comments are the **only** sanctioned place for implementation talk, and you avoid them anyway.

- Default: none. Clear code with good names needs no narration.
- Add one only when the *why* is non-obvious and cannot be made obvious by better naming/structure — a subtle invariant, a workaround, a non-obvious ordering constraint.
- Never narrate the *what* (`// increment counter`). Never restate the line below it.

## Core pattern

```rust
// BAD — transcript, leaks internals, parrots the prompt
/// Parses a port. Proceeds by parsing the string to a u32, then checking the
/// range. Returns an error rather than panicking so the no-panic requirement
/// is upheld. This function never panics.

// GOOD — contract only
/// Parses a TCP port from `s`.
///
/// # Errors
/// [`ParsePortError`] if `s` is not an integer in `1..=65535`.
```

## Red flags — STOP and rewrite

- Numbered/ordered steps, "proceeds as follows", "first… then…"
- A private field, helper, or algorithm named in the doc
- The words `rather than`, `instead of`, `as requested`, `so that <a requirement>`, `now returns`, `never panics`
- Doc restates the signature, or is longer than a simple body
- You wrote a fact you inferred from the body but did not verify → delete it
- An inline `//` that describes *what* the next line does

## Rationalizations

| Excuse | Reality |
|---|---|
| "Caller should know why it returns errors" | Caller needs *when* it errors, not why you chose errors. |
| "Thorough docs are better docs" | Thorough = complete contract, not narrated implementation. |
| "The design rationale is useful" | Rationale → commit message / PR / design doc, never the docblock. |
| "I'm explaining the tradeoff" | Tradeoff vs what? The caller has no 'what'. Present tense only. |
| "It documents the change I made" | Docblocks are timeless. The diff is the changelog. |
| "More detail can't hurt" | Wrong-because-guessed detail hurts; noise buries the contract. |
