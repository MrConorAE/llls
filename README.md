# llls — LLM Language Server

An editor-driven code-review loop between an LLM agent and you. The agent
produces an artifact and requests a review; you annotate it in your editor with
ordinary LSP code actions; your verdict and comments are routed back to the
agent. The inverse of [glls](../glls).

## Components

- `llls lsp` — the language server (point your editor at it).
- `llls await-review --for <files> --message <why>` — the agent side: requests a
  review and blocks until you submit one, then prints it.

They share nothing but files under a repo-local `.llls/` directory.

## Install

`cargo install --path .`

## Editor (Helix)

```toml
[language-server.llls]
command = "llls"
args = ["lsp"]
```
Attach it to the languages you review. In a file, run code actions to add
comments, then "Submit review" and pick a verdict (Approve / Request changes /
Comment).

## Design

See `docs/superpowers/specs/2026-06-30-llls-design.md`.
