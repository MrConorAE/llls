---
name: llls
description: Use when you've produced a design/spec doc, a plan, or a body of generated code and want the developer's review before building further; when a change is risky or hard to reverse and you are unsure it matches their intent; or when the developer says they have left you review notes in their editor.
---

# Editor reviews with llls

`llls` runs a review loop through the developer's editor. Two directions: you
request a review and wait, or the developer pushes one to you. Requires the
`llls` binary on `PATH` and a git repo (state lives in `<repo>/.llls/`).

**llls is not a reviewer.** It relays a *human developer's* review to you — the
verdict and notes are the developer's decisions. Attribute them to the developer
("the developer approved", "Conor requested changes"), never to llls or yourself.

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
  (working tree, or this branch's diff vs `<base>`).
- **Rich — prefer this whenever there's more than one file, or the files differ:**
  a JSON request on stdin where **each file carries its own line/range and its own
  message**. This is the most useful form — every marker points the reviewer at the
  exact lines with the exact question:
  ```
  llls await-review --request - <<'EOF'
  { "message": "optional overall context",
    "files": [
      {"path": "src/a.rs",    "range": [40, 80], "message": "race between refresh and read?"},
      {"path": "src/b.rs",    "line": 12,        "message": "off-by-one at the boundary?"},
      {"path": "docs/plan.md",                   "message": "does the migration section still match?"}
    ] }
  EOF
  ```
  (`--request` is exclusive with `--for`/`--changed`; per-file `line`/`range`/`message` each optional.)
- `--round N` on follow-up rounds.

**Always prefer a line/range plus a specific question over handing over a whole
file** — that's the difference between "glance at this" and an actionable review.

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
