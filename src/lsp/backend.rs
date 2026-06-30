use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::types::{Comment, Draft, Review, Verdict};

#[derive(Debug, Clone)]
pub struct PendingInput {
    pub buffer: PathBuf,
    pub file: String,
    pub line: u32,
    pub context: String,
    pub edit_index: Option<usize>,
}

#[derive(Default)]
pub struct BackendState {
    pub repo_root: PathBuf,
    pub llls_dir: PathBuf,
    pub request: Option<crate::types::Request>,
    pub draft: Draft,
    pub reviewed: HashSet<String>,
    pub pending_input: Option<PendingInput>,
    pub published_files: HashSet<String>,
    pub warned_pending_request: bool,
}

impl BackendState {
    pub fn add_or_replace_comment(&mut self, c: Comment, edit_index: Option<usize>) {
        match edit_index {
            Some(i) if i < self.draft.comments.len() => self.draft.comments[i] = c,
            _ => self.draft.comments.push(c),
        }
    }

    pub fn comment_index_at(&self, file: &str, line: u32) -> Option<usize> {
        self.draft.comments.iter().position(|c| c.file == file && c.line == line)
    }

    pub fn delete_comment(&mut self, file: &str, line: u32) {
        self.draft.comments.retain(|c| !(c.file == file && c.line == line));
    }

    pub fn comments_for(&self, file: &str) -> Vec<&Comment> {
        self.draft.comments.iter().filter(|c| c.file == file).collect()
    }

    pub fn toggle_reviewed(&mut self, file: &str) -> bool {
        if self.reviewed.remove(file) {
            false
        } else {
            self.reviewed.insert(file.to_string());
            true
        }
    }

    pub fn build_review(&self, verdict: Verdict, summary: Option<String>) -> Review {
        let id = self.request.as_ref().map(|r| r.id.clone()).unwrap_or_default();
        Review { id, verdict, summary, comments: self.draft.comments.clone() }
    }
}

pub type Shared = Arc<RwLock<BackendState>>;

/// Strip `#`-prefixed hint lines and surrounding whitespace from a scratch
/// buffer; returns `None` when nothing substantive remains (treated as cancel).
pub fn comment_from_buffer(text: &str) -> Option<String> {
    let body: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let body = body.trim();
    if body.is_empty() { None } else { Some(body.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Comment, Request, Verdict};

    fn comment(file: &str, line: u32, body: &str) -> Comment {
        Comment { file: file.into(), line, context: String::new(), body: body.into() }
    }

    #[test]
    fn add_then_edit_then_delete() {
        let mut s = BackendState::default();
        s.add_or_replace_comment(comment("a.rs", 3, "first"), None);
        assert_eq!(s.draft.comments.len(), 1);
        let idx = s.comment_index_at("a.rs", 3).unwrap();
        s.add_or_replace_comment(comment("a.rs", 3, "edited"), Some(idx));
        assert_eq!(s.draft.comments[0].body, "edited");
        s.delete_comment("a.rs", 3);
        assert!(s.draft.comments.is_empty());
    }

    #[test]
    fn toggle_reviewed_round_trips() {
        let mut s = BackendState::default();
        assert!(s.toggle_reviewed("a.rs"));
        assert!(s.reviewed.contains("a.rs"));
        assert!(!s.toggle_reviewed("a.rs"));
        assert!(!s.reviewed.contains("a.rs"));
    }

    #[test]
    fn build_review_uses_request_id_and_draft() {
        let mut s = BackendState::default();
        s.request = Some(Request { id: "r2-9".into(), round: 2, created_unix: 0, files: vec![], message: String::new() });
        s.add_or_replace_comment(comment("a.rs", 1, "x"), None);
        let r = s.build_review(Verdict::RequestChanges, Some("summary".into()));
        assert_eq!(r.id, "r2-9");
        assert_eq!(r.verdict, Verdict::RequestChanges);
        assert_eq!(r.comments.len(), 1);
        assert_eq!(r.summary.as_deref(), Some("summary"));
    }

    #[test]
    fn comment_from_buffer_strips_hash_lines() {
        assert_eq!(comment_from_buffer("# hint\nreal comment\n"), Some("real comment".into()));
    }

    #[test]
    fn comment_from_buffer_empty_is_none() {
        assert_eq!(comment_from_buffer("# hint only\n\n  \n"), None);
    }
}
