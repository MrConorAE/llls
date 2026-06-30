use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc, RwLock};
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{async_trait, Client, LanguageServer, LspService, Server};

use crate::lsp::backend::{comment_from_buffer, BackendState, PendingInput, Shared};
use crate::lsp::convert;
use crate::store::Store;
use crate::types::{Comment, Draft, Verdict};

pub struct Backend {
    pub client: Client,
    pub state: Shared,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self { client, state: Arc::new(RwLock::new(BackendState::default())) }
    }

    fn store(dir: &Path) -> Store {
        Store { dir: dir.to_path_buf() }
    }
}

pub async fn serve() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

// ── helpers ────────────────────────────────────────────────────────────────

fn uri_to_repo_path(uri: &Url, repo_root: &Path) -> Option<String> {
    uri.to_file_path().ok()?.strip_prefix(repo_root).ok().map(|p| p.to_string_lossy().into_owned())
}

fn read_line_context(path: &Path, line1: u32) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.lines().nth(line1.saturating_sub(1) as usize).map(|l| l.to_string()))
        .unwrap_or_default()
}

/// Re-read request.json from disk (the external source) and, if its id changed,
/// reset per-request state — loading any persisted draft for that id. Then
/// republish diagnostics.
async fn reload_and_publish(client: &Client, state: &Shared) {
    let dir = state.read().await.llls_dir.clone();
    let store = Backend::store(&dir);
    let disk_request = store.read_request();
    {
        let mut s = state.write().await;
        let new_id = disk_request.as_ref().map(|r| r.id.clone());
        let cur_id = s.request.as_ref().map(|r| r.id.clone());
        if new_id != cur_id {
            // Defer adopting a new request while an ad-hoc draft is open.
            if s.request.is_none() && !s.draft.comments.is_empty() && disk_request.is_some() {
                if !s.warned_pending_request {
                    s.warned_pending_request = true;
                    drop(s);
                    client.show_message(MessageType::WARNING,
                        "A request arrived while your ad-hoc notes are open — send or discard them first.").await;
                    return;
                }
                return;
            }
            s.warned_pending_request = false;
            s.request = disk_request.clone();
            let id = new_id.clone().unwrap_or_default();
            s.draft = Draft { id: id.clone(), comments: vec![] };
            s.reviewed.clear();
            if let Some(d) = store.read_draft() {
                if Some(&d.id) == new_id.as_ref() { s.draft = d; }
            }
        }
    }
    refresh_diagnostics(client, state).await;
}

/// Publish diagnostics for every file referenced by the request or the draft,
/// and clear diagnostics for files no longer referenced.
async fn refresh_diagnostics(client: &Client, state: &Shared) {
    let (repo_root, request, draft, reviewed, prev) = {
        let s = state.read().await;
        (s.repo_root.clone(), s.request.clone(), s.draft.clone(), s.reviewed.clone(), s.published_files.clone())
    };

    let mut current: HashSet<String> = HashSet::new();
    if let Some(r) = &request {
        for t in &r.files {
            current.insert(t.path.clone());
        }
    }
    for c in &draft.comments {
        current.insert(c.file.clone());
    }

    for file in &current {
        let comments: Vec<&Comment> = draft.comments.iter().filter(|c| &c.file == file).collect();
        let reviewed_flag = reviewed.contains(file);
        let diags = convert::file_diagnostics(request.as_ref(), file, reviewed_flag, &comments);
        if let Ok(uri) = Url::from_file_path(repo_root.join(file)) {
            client.publish_diagnostics(uri, diags, None).await;
        }
    }
    for file in prev.difference(&current) {
        if let Ok(uri) = Url::from_file_path(repo_root.join(file)) {
            client.publish_diagnostics(uri, vec![], None).await;
        }
    }
    state.write().await.published_files = current;
}

async fn persist_draft(state: &Shared) {
    let (dir, draft) = {
        let s = state.read().await;
        (s.llls_dir.clone(), s.draft.clone())
    };
    let _ = Backend::store(&dir).write_draft(&draft);
}

// ── command handlers ─────────────────────────────────────────────────────────

impl Backend {
    fn arg_str(args: &[Value], key: &str) -> String {
        args.first().and_then(|v| v.get(key)).and_then(|v| v.as_str()).unwrap_or("").to_string()
    }
    fn arg_u32(args: &[Value], key: &str) -> u32 {
        args.first().and_then(|v| v.get(key)).and_then(|v| v.as_u64()).unwrap_or(0) as u32
    }

    async fn open_input(&self, hint: &str, prefill: &str, pending: PendingInput) {
        let already_pending = self.state.read().await.pending_input.is_some();
        if already_pending {
            self.client.show_message(MessageType::WARNING,
                "A comment buffer is already open. Finish it (save & close) first.").await;
            return;
        }
        // Typing area on the first line (cursor lands here), hint pushed below —
        // so the buffer opens ready for `i` + type without navigating past it.
        let body = if prefill.is_empty() {
            format!("\n{hint}")
        } else {
            format!("{prefill}\n\n{hint}")
        };
        if std::fs::write(&pending.buffer, body).is_err() {
            self.client.show_message(MessageType::ERROR, "Could not open the note buffer.").await;
            return;
        }
        self.state.write().await.pending_input = Some(pending.clone());
        if let Ok(uri) = Url::from_file_path(&pending.buffer) {
            let top = Range {
                start: Position { line: 0, character: 0 },
                end: Position { line: 0, character: 0 },
            };
            let _ = self.client.show_document(ShowDocumentParams {
                uri, external: Some(false), take_focus: Some(true), selection: Some(top),
            }).await;
        }
        self.client.show_message(MessageType::INFO,
            "Write your note, then save & close the buffer (:wbc). Empty = cancel.").await;
    }

    async fn add_comment(&self, args: Vec<Value>) {
        let file = Self::arg_str(&args, "file");
        let line = Self::arg_u32(&args, "line");
        let (repo_root, buffer) = {
            let s = self.state.read().await;
            (s.repo_root.clone(), s.llls_dir.join("comment.md"))
        };
        let context = read_line_context(&repo_root.join(&file), line);
        self.open_input(
            "# Leave an agent note — save & close (:wbc) to submit, empty to cancel.\n",
            "",
            PendingInput { buffer, file, line, context, edit_index: None },
        ).await;
    }

    async fn edit_comment(&self, args: Vec<Value>) {
        let file = Self::arg_str(&args, "file");
        let line = Self::arg_u32(&args, "line");
        let (buffer, prefill, idx, context) = {
            let s = self.state.read().await;
            let idx = s.comment_index_at(&file, line);
            let (prefill, context) = idx
                .map(|i| (s.draft.comments[i].body.clone(), s.draft.comments[i].context.clone()))
                .unwrap_or_default();
            (s.llls_dir.join("comment.md"), prefill, idx, context)
        };
        self.open_input(
            "# Edit agent note — save & close (:wbc) to submit, empty to delete.\n",
            &prefill,
            PendingInput { buffer, file, line, context, edit_index: idx },
        ).await;
    }

    async fn delete_comment(&self, args: Vec<Value>) {
        let file = Self::arg_str(&args, "file");
        let line = Self::arg_u32(&args, "line");
        self.state.write().await.delete_comment(&file, line);
        persist_draft(&self.state).await;
        refresh_diagnostics(&self.client, &self.state).await;
    }

    async fn mark_reviewed(&self, args: Vec<Value>) {
        let file = Self::arg_str(&args, "file");
        self.state.write().await.toggle_reviewed(&file);
        refresh_diagnostics(&self.client, &self.state).await;
    }

    async fn submit_review(&self) {
        let (has_request, n) = {
            let s = self.state.read().await;
            (s.request.is_some(), s.draft.comments.len())
        };
        if !has_request {
            if n == 0 {
                self.client.show_message(MessageType::WARNING, "No notes to send.").await;
                return;
            }
            self.finalize_adhoc().await;
            return;
        }
        let prompt = match n {
            0 => "Review has no notes. Which verdict?".to_string(),
            1 => "Review has 1 note. Which verdict?".to_string(),
            _ => format!("Review has {n} notes. Which verdict?"),
        };
        let verdict = self.client.show_message_request(
            MessageType::INFO, prompt,
            Some(vec![
                MessageActionItem { title: "Approve".into(), properties: Default::default() },
                MessageActionItem { title: "Request changes".into(), properties: Default::default() },
                MessageActionItem { title: "Comment".into(), properties: Default::default() },
            ]),
        ).await.ok().flatten();
        let verdict = match verdict.as_ref().map(|a| a.title.as_str()) {
            Some("Approve") => Verdict::Approve,
            Some("Request changes") => Verdict::RequestChanges,
            Some("Comment") => Verdict::Comment,
            _ => return,
        };
        self.finalize(verdict).await;
    }

    /// Ad-hoc (user-initiated): no agent request. Write the notes to inbox.json
    /// as a `comment` verdict, clear the draft, and tell the developer.
    async fn finalize_adhoc(&self) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let (dir, comments) = {
            let s = self.state.read().await;
            (s.llls_dir.clone(), s.draft.comments.clone())
        };
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let review = crate::types::Review {
            id: format!("adhoc-{nanos}"),
            verdict: Verdict::Comment,
            summary: None,
            comments,
        };
        let store = Backend::store(&dir);
        if store.read_request().is_some() {
            self.client.show_message(MessageType::WARNING,
                "A review request just arrived — its markers will appear shortly; \
                 run 'Send review to Claude' again to respond with a verdict.").await;
            return; // leave the draft intact; the request will activate on the next reload
        }
        if store.write_inbox(&review).is_err() {
            self.client.show_message(MessageType::ERROR, "Could not write inbox.json.").await;
            return;
        }
        store.clear_draft(); // removes draft.json only — never touches request.json
        self.state.write().await.draft = Draft::default();
        refresh_diagnostics(&self.client, &self.state).await;
        self.client.show_message(MessageType::INFO,
            "Review sent to Claude. Tell Claude to run `llls take-review`.").await;
    }

    async fn dismiss_review(&self) {
        let (has_request, dir) = {
            let s = self.state.read().await;
            (s.request.is_some(), s.llls_dir.clone())
        };
        if !has_request {
            // Ad-hoc draft: discard the notes locally (no inbox write).
            Backend::store(&dir).clear_draft();
            self.state.write().await.draft = Draft::default();
            refresh_diagnostics(&self.client, &self.state).await;
            self.client.show_message(MessageType::INFO, "Review notes discarded.").await;
            return;
        }
        self.finalize(Verdict::Dismissed).await;
    }

    /// Write review.json, then delete request.json/draft.json and clear markers.
    async fn finalize(&self, verdict: Verdict) {
        let (dir, review, had_request) = {
            let s = self.state.read().await;
            let review = if verdict == Verdict::Dismissed {
                crate::types::Review {
                    id: s.request.as_ref().map(|r| r.id.clone()).unwrap_or_default(),
                    verdict,
                    summary: None,
                    comments: vec![],
                }
            } else {
                s.build_review(verdict, None)
            };
            (s.llls_dir.clone(), review, s.request.is_some())
        };
        if !had_request {
            self.client.show_message(MessageType::WARNING, "No review request is pending.").await;
            return;
        }
        let store = Backend::store(&dir);
        if store.write_review(&review).is_err() {
            self.client.show_message(MessageType::ERROR, "Could not write review.json.").await;
            return;
        }
        store.clear_request_draft();
        {
            let mut s = self.state.write().await;
            s.request = None;
            s.draft = Draft::default();
            s.reviewed.clear();
        }
        refresh_diagnostics(&self.client, &self.state).await;
        self.client.show_message(MessageType::INFO, "Review submitted.").await;
    }

    async fn next_file(&self) {
        let (repo_root, next) = {
            let s = self.state.read().await;
            let next = s.request.as_ref().and_then(|r| {
                r.files.iter().map(|t| t.path.clone()).find(|p| !s.reviewed.contains(p))
            });
            (s.repo_root.clone(), next)
        };
        match next {
            Some(rel) => {
                if let Ok(uri) = Url::from_file_path(repo_root.join(&rel)) {
                    let _ = self.client.show_document(ShowDocumentParams {
                        uri, external: Some(false), take_focus: Some(true), selection: None,
                    }).await;
                }
            }
            None => {
                self.client.show_message(MessageType::INFO, "All requested files reviewed.").await;
            }
        }
    }
}

// ── LanguageServer ────────────────────────────────────────────────────────────

#[async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        let root: Option<PathBuf> = params
            .workspace_folders
            .as_deref()
            .and_then(|f| f.first())
            .and_then(|f| f.uri.to_file_path().ok())
            .or_else(|| {
                #[allow(deprecated)]
                params.root_uri.as_ref().and_then(|u| u.to_file_path().ok())
            })
            .or_else(|| std::env::current_dir().ok());

        if let Some(root) = root {
            let store = Store::discover(&root).unwrap_or(Store { dir: root.join(".llls") });
            let _ = store.ensure();
            let mut s = self.state.write().await;
            s.repo_root = store.repo_root();
            s.llls_dir = store.dir;
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        "llls.addComment".into(),
                        "llls.editComment".into(),
                        "llls.deleteComment".into(),
                        "llls.markReviewed".into(),
                        "llls.submitReview".into(),
                        "llls.dismissReview".into(),
                        "llls.nextFile".into(),
                    ],
                    ..Default::default()
                }),
                text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::NONE),
                    save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                    ..Default::default()
                })),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        let dir = self.state.read().await.llls_dir.clone();

        // Bridge the blocking notify watcher to async via an mpsc of ticks.
        let (tx, mut rx) = mpsc::channel::<()>(8);
        let watch_dir = dir.clone();
        tokio::task::spawn_blocking(move || {
            let (_w, rx_fs) = match crate::watch::watcher(&watch_dir) {
                Ok(v) => v,
                Err(e) => { tracing::warn!("watch failed: {e:#}"); return; }
            };
            if tx.blocking_send(()).is_err() { return; }
            loop {
                match rx_fs.recv_timeout(Duration::from_secs(3)) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if tx.blocking_send(()).is_err() { break; }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });

        let client = self.client.clone();
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            while rx.recv().await.is_some() {
                reload_and_publish(&client, &state).await;
            }
        });

        tracing::info!("llls initialized");
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let pos = params.text_document_position_params;
        let line1 = pos.position.line + 1;
        let s = self.state.read().await;
        let file = match uri_to_repo_path(&pos.text_document.uri, &s.repo_root) {
            Some(f) => f,
            None => return Ok(None),
        };
        let comments = s.comments_for(&file);
        Ok(convert::hover_for(&comments, line1).map(|md| Hover {
            contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value: md }),
            range: None,
        }))
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let s = self.state.read().await;
        let line1 = params.range.start.line + 1;
        let file = match uri_to_repo_path(&params.text_document.uri, &s.repo_root) {
            Some(f) => f,
            None => return Ok(None),
        };
        let is_requested = s.request.as_ref().map(|r| r.files.iter().any(|t| t.path == file)).unwrap_or(false);
        let reviewed = s.reviewed.contains(&file);
        let comment_at_line = s.comment_index_at(&file, line1).is_some();
        let has_draft = !s.draft.comments.is_empty();
        let has_request = s.request.is_some();
        Ok(Some(convert::code_actions(&file, line1, is_requested, reviewed, comment_at_line, has_draft, has_request)))
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> LspResult<Option<Value>> {
        match params.command.as_str() {
            "llls.addComment" => self.add_comment(params.arguments).await,
            "llls.editComment" => self.edit_comment(params.arguments).await,
            "llls.deleteComment" => self.delete_comment(params.arguments).await,
            "llls.markReviewed" => self.mark_reviewed(params.arguments).await,
            "llls.submitReview" => self.submit_review().await,
            "llls.dismissReview" => self.dismiss_review().await,
            "llls.nextFile" => self.next_file().await,
            _ => {}
        }
        Ok(None)
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let closed = match params.text_document.uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return,
        };
        let pending = {
            let s = self.state.read().await;
            s.pending_input.clone().filter(|p| p.buffer == closed)
        };
        let pending = match pending {
            Some(p) => p,
            None => return,
        };

        let text = std::fs::read_to_string(&pending.buffer).unwrap_or_default();
        let _ = std::fs::remove_file(&pending.buffer);
        self.state.write().await.pending_input = None;

        match comment_from_buffer(&text) {
            Some(body) => {
                self.state.write().await.add_or_replace_comment(
                    Comment { file: pending.file, line: pending.line, context: pending.context, body },
                    pending.edit_index,
                );
                persist_draft(&self.state).await;
                refresh_diagnostics(&self.client, &self.state).await;
                self.client.show_message(MessageType::INFO, "Comment added to the review.").await;
            }
            None => {
                // empty buffer: if this was an edit, treat as delete
                if pending.edit_index.is_some() {
                    self.state.write().await.delete_comment(&pending.file, pending.line);
                    persist_draft(&self.state).await;
                    refresh_diagnostics(&self.client, &self.state).await;
                }
                self.client.show_message(MessageType::INFO, "Comment cancelled.").await;
            }
        }
    }
}
