---
name: validate-ci
description: Validate CI locally with `hm run` before pushing. Use when a feature is complete and ready to push, when the user asks to make CI green, or before creating a PR. Do NOT use in the middle of active development or on every commit in a multi-commit feature.
---

Validate that the project's CI pipeline passes. Run the pipeline locally with `hm run`, diagnose and fix any failures, and only push to remote once local CI is green.

## When to use

- The user asks you to "make CI green", "run CI", "validate the pipeline", or similar
- A feature or bugfix is complete and ready to push
- Before creating a pull request
- The user explicitly requests local validation before pushing

## When NOT to use

- In the middle of active development across multiple commits — wait until the logical unit of work is done
- On every individual commit in a multi-commit feature — run once at the end
- When the user only asks you to commit (committing does not imply CI validation)
- When the user explicitly says to skip local validation

## What this does

`hm run` executes the project's CI pipeline locally inside Docker containers. It runs the same steps that would run on Harmont Cloud, but on your machine. This catches failures before they reach remote CI.

## Procedure

1. Run `hm run -k --logs` from the project root (`-k` continues past failures so you see every broken step in one run; `--logs` streams full build logs for easier diagnosis)
2. If it exits 0 — pipeline passed. Inform the user and proceed with push if requested.
3. If it exits non-zero — pipeline failed. Read the output, diagnose ALL failures (not just the first), fix every issue, and re-run `hm run -k --logs` until it passes.
4. Only after local CI is green: push to remote. If the user wants cloud validation too, they can run `hm run --backend cloud --logs` or wait for the remote CI triggered by the push.

## Important

- `hm run` is the ONLY correct way to run this project's CI locally. Do NOT try to reverse-engineer pipeline steps by reading `.hm/` files and running commands manually.
- Do NOT commit, push, and wait for remote CI results as a way to "test" — always validate locally first.
- Exit code 0 = pass, non-zero = fail. Exit code 130 = user cancelled (Ctrl-C).
- If Docker is not running, `hm run` will fail. Tell the user to start Docker.
