# llls — llm language server

Editor-driven code review where the reviewee is your AI agent. It produces
something (spec, plan, code), asks for review, and waits; you annotate in your
editor with LSP code actions; your verdict + notes go back to it. The inverse of
[glls](../glls) — you push reviews *to* the agent instead of pulling them from a forge.

## Install

- `cargo build --release`, then symlink `target/release/llls` onto your `PATH`
  (e.g. `~/.local/bin/llls`) — rebuilds refresh it automatically. Or `cargo install --path .`.
- Needs a git repo: state lives in a gitignored `.llls/` at the repo root.

## Editor (Helix)

```toml
[language-server.llls]
command = "llls"
args = ["lsp"]
```
- Attach it to the languages you review. Helix **replaces** a language's server
  list, so re-list the defaults too: `language-servers = ["rust-analyzer", "llls"]`.
- Runs alongside your real language servers; `:lsp-restart` after a rebuild.

Optional keybinds — a `space v a` review submenu (`%{buffer_name}` is normalized
to repo-relative server-side, so cursor-line commands work):

```toml
[keys.normal.space.v.a]
c = ":lsp-workspace-command llls.addComment {\"file\": \"%{buffer_name}\", \"line\": %{cursor_line}}"
e = ":lsp-workspace-command llls.editComment {\"file\": \"%{buffer_name}\", \"line\": %{cursor_line}}"
x = ":lsp-workspace-command llls.deleteComment {\"file\": \"%{buffer_name}\", \"line\": %{cursor_line}}"
m = ":lsp-workspace-command llls.markReviewed {\"file\": \"%{buffer_name}\"}"
n = "@:lsp-workspace-command llls.markReviewed {\"file\": \"%{buffer_name}\"}<ret>:lsp-workspace-command llls.nextFile<ret>"
s = ":lsp-workspace-command llls.submitReview"
D = ":lsp-workspace-command llls.dismissReview"
f = "@<space>DClaude requests %source llls %sev INFO %p " # unseen files
u = "@<space>Dreview %source llls %m "                    # all requested files
U = "@<space>D@ %source llls %sev HINT %m "               # my notes
```
(`n` marks the file reviewed before advancing, else "next" reopens the same
buffer. Diagnostic-picker filters mirror glls's syntax — tweak to taste.)

## Reviewing (your side)

- Requested files show a line-1 diagnostic: `Claude requests review — <message>`.
- Code actions on a line:
  - **Leave an agent note** → write in the scratch buffer, `:wbc` to submit (empty = cancel).
  - **Edit / Delete agent note** on a noted line (hover shows it).
  - **Mark file reviewed** / **Next file to review** to work through the set.
  - **Send review to Claude** → pick **Approve / Request changes / Comment**; or **Discard review**.
- Push a review *unprompted*: leave notes on any file and **Send review to Claude**
  with no pending request — the agent collects them with `llls take-review`.

## Agent side

- `llls await-review --for <files> --message <why>` — request a review, block until
  you submit, print it. Target regions with `path:LINE` or `path:START-END`.
- `llls await-review --changed [<ref>]` — review git-changed files (working tree, or
  this branch's diff vs `<ref>` for pre-merge) instead of listing them.
- `llls take-review` — drain any review you pushed unprompted.

## When the agent asks

Driven by the `llls` skill, gated by a `review-cadence:` line in your CLAUDE.md:
`off` / `end-only` / `spec+plan` / `all` (default `spec+plan`). Install the personal
skill so the agent knows to use any of this. `llls` is global — works in any git repo.

## Gotchas

- No code actions on a file? It's outside the repo root — llls only acts on files
  inside the git repo (so files under `/tmp`, a scratchpad, etc. get nothing).
  `RUST_LOG=llls=debug` logs the reason and the root it's scoped to.

## Design

`docs/superpowers/specs/2026-06-30-llls-design.md` (+ `…-v2-design.md`).
