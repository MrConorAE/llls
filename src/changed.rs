use anyhow::{Context, Result};
use std::path::Path;

/// Repo-relative paths to review. `base=None` → working-tree changes vs HEAD
/// (tracked modifications + untracked, deletions excluded). `base=Some(r)` →
/// files this branch introduced since diverging from `r` (three-dot `r...HEAD`).
/// Sorted and de-duplicated.
pub fn changed_files(repo_root: &Path, base: Option<&str>) -> Result<Vec<String>> {
    let mut files = match base {
        Some(b) => run_lines(repo_root, &["diff", "--name-only", "--diff-filter=d", &format!("{b}...HEAD")])?,
        None => {
            let mut v = run_lines(repo_root, &["diff", "--name-only", "--diff-filter=d", "HEAD"])?;
            v.extend(run_lines(repo_root, &["ls-files", "--others", "--exclude-standard"])?);
            v
        }
    };
    files.sort();
    files.dedup();
    Ok(files)
}

fn run_lines(repo_root: &Path, args: &[&str]) -> Result<Vec<String>> {
    let out = std::process::Command::new("git")
        .arg("-C").arg(repo_root).args(args).output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !out.status.success() {
        anyhow::bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = Command::new("git").arg("-C").arg(dir).args(args)
            .output().unwrap().status.success();
        assert!(ok, "git {args:?} failed");
    }

    fn init_repo(dir: &std::path::Path) {
        git(dir, &["init", "-q"]);
        git(dir, &["config", "user.email", "t@t"]);
        git(dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "1\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "init"]);
        git(dir, &["checkout", "-q", "-b", "base"]);
    }

    #[test]
    fn working_tree_includes_modified_and_untracked_not_committed() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        std::fs::write(tmp.path().join("a.txt"), "2\n").unwrap();      // modified
        std::fs::write(tmp.path().join("new.txt"), "x\n").unwrap();    // untracked
        let files = changed_files(tmp.path(), None).unwrap();
        assert_eq!(files, vec!["a.txt".to_string(), "new.txt".to_string()]);
    }

    #[test]
    fn working_tree_excludes_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        std::fs::remove_file(tmp.path().join("a.txt")).unwrap();
        assert!(changed_files(tmp.path(), None).unwrap().is_empty());
    }

    #[test]
    fn branch_diff_three_dot_lists_branch_changes() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(tmp.path(), &["checkout", "-q", "-b", "feat"]);
        std::fs::write(tmp.path().join("b.txt"), "x\n").unwrap();
        git(tmp.path(), &["add", "."]);
        git(tmp.path(), &["commit", "-qm", "add b"]);
        let files = changed_files(tmp.path(), Some("base")).unwrap();
        assert_eq!(files, vec!["b.txt".to_string()]);
    }
}
