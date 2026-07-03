---
name: llls
description: Use when you've produced a design/spec doc, a plan, or a body of generated code and want the developer's review before building further; when a change is risky or hard to reverse and you are unsure it matches their intent; or when the developer says they have left you review notes in their editor.
---

# Editor reviews with llls

`llls` runs a review loop through the developer's editor. Two directions: you
request a review and wait, or the developer pushes one to you. Requires the
`llls` binary on `PATH` and a git repo (state lives in `<repo>/.llls/`).

**llls is not a reviewer.** It relays *the user's own* review to you — the verdict
and notes are the user's decisions, and the user is the person you're talking to.
Refer to them in the **second person** ("you approved", "you flagged a race on
line 40"), not the third person or by name, and never as llls or yourself.

## When to request — honor the developer's cadence

Read `review-cadence` from CLAUDE.md (default: `spec+plan`, plus a pre-merge
review). It sets which checkpoints warrant a review:

| cadence | request review at |
|---|---|
| `off` | never proactively — only when the developer asks |
| `end-only` | once, before merging |
| `spec+plan` | after a design/spec doc, and after a plan |
| `all` | spec, plan, and each batch of code before committing |

Checkpoints are by **artifact kind**: a design/spec doc written, a plan written,
code about to be committed/merged. (If a structured workflow is in use, its
design / plan / pre-merge steps are the natural markers.) With no clear artifact
boundaries, honor explicit requests plus `end-only` only — do not invent checkpoints.

**Do NOT request** for trivial or easily-reversed edits (renames, typos,
formatting) or mid-exploration. When genuinely unsure, ask the developer.

## How to request

Launch as a **background task** so the conversation continues. Two input styles:

- **Simple** — one blanket message: `llls await-review --for <file>[:LINE|:START-END] --message "..."`
  (comma-separated files), or `--changed [<base>]` to pull changed files from git
  (working tree, or this branch's diff vs `<base>`). `--for`/`--changed` allow
  **one target per file** (a file listed twice is de-duplicated) and share the
  single `--message` — for several ranges or notes within one file, use Rich.
- **Rich — prefer this whenever there's more than one file, or the files differ:**
  a JSON request on stdin where **each entry carries its own line/range and its own
  message**. A file can appear multiple times — one entry per question. The most
  useful form combines a whole-file entry for context with targeted range entries
  for specific concerns:
  ```
  llls await-review --request - <<'EOF'
  { "message": "optional overall context",
    "files": [
      {"path": "src/a.rs",    "message": "overall: does the cache invalidation logic look right?"},
      {"path": "src/a.rs",    "range": [40, 80],  "message": "especially here: race between refresh and read?"},
      {"path": "src/a.rs",    "range": [110, 130], "message": "is this recovery path reachable?"},
      {"path": "src/b.rs",    "line": 12,          "message": "off-by-one at the boundary?"},
      {"path": "docs/plan.md",                     "message": "does the migration section still match?"}
    ] }
  EOF
  ```
  (`--request` is exclusive with `--for`/`--changed`; per-entry `line`/`range`/`message` each optional.)
- `--round N` on follow-up rounds.

**Always prefer ranges with specific questions over whole-file entries.** For a
file you want broadly reviewed *and* have a specific concern about, use both: a
whole-file entry for the general question, plus a range entry for each targeted
question. That's the difference between "glance at this" and an actionable review.

When it returns, act on the **verdict**: `approve` → proceed (comments optional
polish); `request_changes` → address every note, then consider another round;
`comment` → weigh it; `dismissed` → proceed, noting no review was given.

## When the developer pushes a review

If the developer says they have left notes (or asks you to check), run
`llls take-review` — it prints any pending notes (verdict `comment`, advisory)
and clears them. Address them, then continue.

## Notes
- llls only sees files **inside the git repo**. Write reviewable artifacts into
  the repo (never a scratchpad / `/tmp`) and request in-repo paths.
- One review in flight at a time. If a CLI call exits non-zero, report it; do
  not silently proceed.
- Address notes with rigor: verify each point on its merits, don't perform
  agreement, and reply referencing `path:line`.
