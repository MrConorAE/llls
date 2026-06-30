use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::types::{Draft, Request, Review};

pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    /// Walk up from `start` until a `.git` entry is found; the store lives in
    /// `<repo_root>/.llls`.
    pub fn discover(start: &Path) -> Result<Store> {
        let mut dir = start;
        loop {
            if dir.join(".git").exists() {
                return Ok(Store { dir: dir.join(".llls") });
            }
            dir = dir.parent().context("reached filesystem root without finding .git")?;
        }
    }

    pub fn repo_root(&self) -> PathBuf {
        self.dir.parent().map(Path::to_path_buf).unwrap_or_else(|| self.dir.clone())
    }

    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir).with_context(|| format!("creating {}", self.dir.display()))?;
        let gi = self.dir.join(".gitignore");
        if !gi.exists() {
            std::fs::write(&gi, "*\n")?;
        }
        Ok(())
    }

    pub fn request_path(&self) -> PathBuf { self.dir.join("request.json") }
    pub fn draft_path(&self) -> PathBuf { self.dir.join("draft.json") }
    pub fn review_path(&self) -> PathBuf { self.dir.join("review.json") }
    pub fn inbox_path(&self) -> PathBuf { self.dir.join("inbox.json") }

    pub fn read_request(&self) -> Option<Request> { read_json(&self.request_path()) }
    pub fn read_draft(&self) -> Option<Draft> { read_json(&self.draft_path()) }
    pub fn read_review(&self) -> Option<Review> { read_json(&self.review_path()) }
    pub fn read_inbox(&self) -> Option<Review> { read_json(&self.inbox_path()) }

    pub fn write_request(&self, r: &Request) -> Result<()> { write_json(&self.request_path(), r) }
    pub fn write_draft(&self, d: &Draft) -> Result<()> { write_json(&self.draft_path(), d) }
    pub fn write_review(&self, r: &Review) -> Result<()> { write_json(&self.review_path(), r) }
    pub fn write_inbox(&self, r: &Review) -> Result<()> { write_json(&self.inbox_path(), r) }

    /// A review only satisfies a request whose `id` it carries.
    pub fn matching_review(&self, id: &str) -> Option<Review> {
        self.read_review().filter(|r| r.id == id)
    }

    fn remove(p: &Path) { let _ = std::fs::remove_file(p); }
    pub fn clear_request_draft(&self) {
        Self::remove(&self.request_path());
        Self::remove(&self.draft_path());
    }
    pub fn clear_all(&self) {
        self.clear_request_draft();
        Self::remove(&self.review_path());
    }
    pub fn clear_inbox(&self) { Self::remove(&self.inbox_path()); }
}

fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Option<T> {
    let s = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&s).ok()
}

fn write_json<T: serde::Serialize>(p: &Path, v: &T) -> Result<()> {
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(v)?)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

/// Normalise a possibly-absolute or cwd-relative path to repo-relative.
pub fn repo_relative(path: &str, repo_root: &Path) -> String {
    let p = Path::new(path);
    let abs = if p.is_absolute() { p.to_path_buf() } else { repo_root.join(p) };
    abs.strip_prefix(repo_root)
        .map(|r| r.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Review, Verdict};

    fn store_in(tmp: &std::path::Path) -> Store {
        std::fs::create_dir_all(tmp.join(".git")).unwrap();
        let s = Store::discover(tmp).unwrap();
        s.ensure().unwrap();
        s
    }

    #[test]
    fn discover_finds_git_root_and_appends_llls() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store_in(tmp.path());
        assert_eq!(s.dir, tmp.path().join(".llls"));
        assert_eq!(s.repo_root(), tmp.path());
    }

    #[test]
    fn ensure_writes_self_ignoring_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store_in(tmp.path());
        assert_eq!(std::fs::read_to_string(s.dir.join(".gitignore")).unwrap(), "*\n");
    }

    #[test]
    fn review_round_trips_through_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store_in(tmp.path());
        let r = Review { id: "r1-1".into(), verdict: Verdict::Comment, summary: None, comments: vec![] };
        s.write_review(&r).unwrap();
        assert_eq!(s.read_review().unwrap(), r);
    }

    #[test]
    fn matching_review_requires_id_match() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store_in(tmp.path());
        let r = Review { id: "OLD".into(), verdict: Verdict::Approve, summary: None, comments: vec![] };
        s.write_review(&r).unwrap();
        assert!(s.matching_review("NEW").is_none());
        assert!(s.matching_review("OLD").is_some());
    }

    #[test]
    fn clear_request_draft_leaves_review() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store_in(tmp.path());
        std::fs::write(s.request_path(), "{}").unwrap();
        std::fs::write(s.draft_path(), "{}").unwrap();
        let r = Review { id: "x".into(), verdict: Verdict::Approve, summary: None, comments: vec![] };
        s.write_review(&r).unwrap();
        s.clear_request_draft();
        assert!(!s.request_path().exists());
        assert!(!s.draft_path().exists());
        assert!(s.review_path().exists());
    }

    #[test]
    fn inbox_round_trips_and_clears() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store_in(tmp.path());
        let r = crate::types::Review {
            id: "adhoc-1".into(), verdict: crate::types::Verdict::Comment,
            summary: None, comments: vec![],
        };
        s.write_inbox(&r).unwrap();
        assert_eq!(s.read_inbox().unwrap(), r);
        s.clear_inbox();
        assert!(s.read_inbox().is_none());
    }

    #[test]
    fn repo_relative_strips_root() {
        let root = std::path::Path::new("/home/u/repo");
        assert_eq!(repo_relative("/home/u/repo/src/a.rs", root), "src/a.rs");
        assert_eq!(repo_relative("src/a.rs", root), "src/a.rs");
    }
}
