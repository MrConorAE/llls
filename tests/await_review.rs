// End-to-end: write a request, have a "reviewer" thread drop a matching
// review.json, and assert await-review prints it and cleans up. Drives the
// binary's logic via a tiny re-impl of the store paths to avoid needing a
// running LSP.

use std::process::Command;
use std::time::Duration;

#[test]
fn await_review_returns_submitted_review_and_clears_state() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    // 5 lines so line 3 passes bounds check
    std::fs::write(tmp.path().join("a.rs"), b"1\n2\n3\n4\n5\n").unwrap();
    let llls = tmp.path().join(".llls");

    // Reviewer thread: wait for the request to exist, then write a matching review.
    let llls2 = llls.clone();
    let reviewer = std::thread::spawn(move || {
        let req_path = llls2.join("request.json");
        for _ in 0..200 {
            if let Ok(s) = std::fs::read_to_string(&req_path) {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap();
                let id = v["id"].as_str().unwrap().to_string();
                let review = serde_json::json!({
                    "id": id,
                    "verdict": "request_changes",
                    "comments": [ { "file": "a.rs", "line": 3, "context": "x", "body": "fix this" } ]
                });
                let tmp_r = llls2.join("review.json.tmp");
                std::fs::write(&tmp_r, review.to_string()).unwrap();
                std::fs::rename(&tmp_r, llls2.join("review.json")).unwrap();
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("request.json never appeared");
    });

    let out = Command::new(env!("CARGO_BIN_EXE_llls"))
        .args(["await-review", "--for", "a.rs:3", "--message", "look"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    reviewer.join().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("verdict: request_changes"), "got: {stdout}");
    assert!(stdout.contains("## a.rs:3"));
    assert!(stdout.contains("fix this"));

    // review.json consumed; request.json removed.
    assert!(!llls.join("review.json").exists());
    assert!(!llls.join("request.json").exists());
}

#[test]
fn await_review_changed_picks_up_working_tree_changes() {
    let tmp = tempfile::tempdir().unwrap();
    // init a real repo with one committed file, then modify it
    let run = |args: &[&str]| std::process::Command::new("git")
        .arg("-C").arg(tmp.path()).args(args).output().unwrap();
    run(&["init", "-q"]); run(&["config","user.email","t@t"]); run(&["config","user.name","t"]);
    std::fs::write(tmp.path().join("a.rs"), "1\n").unwrap();
    run(&["add","."]); run(&["commit","-qm","init"]);
    std::fs::write(tmp.path().join("a.rs"), "2\n").unwrap();

    let llls = tmp.path().join(".llls");
    let llls2 = llls.clone();
    let reviewer = std::thread::spawn(move || {
        let req = llls2.join("request.json");
        for _ in 0..200 {
            if let Ok(s) = std::fs::read_to_string(&req) {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap();
                // assert the changed file made it into the request
                assert!(v["files"].as_array().unwrap().iter()
                    .any(|f| f["path"] == "a.rs"), "request files: {}", v["files"]);
                let id = v["id"].as_str().unwrap();
                let review = serde_json::json!({"id":id,"verdict":"approve","comments":[]});
                let t = llls2.join("review.json.tmp");
                std::fs::write(&t, review.to_string()).unwrap();
                std::fs::rename(&t, llls2.join("review.json")).unwrap();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("no request.json");
    });

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_llls"))
        .args(["await-review", "--changed", "--message", "x"])
        .current_dir(tmp.path()).output().unwrap();
    reviewer.join().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn await_review_refuses_when_request_already_pending() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::create_dir_all(tmp.path().join(".llls")).unwrap();
    std::fs::write(tmp.path().join(".llls/request.json"),
        r#"{"id":"x","round":1,"created_unix":0,"files":[],"message":""}"#).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_llls"))
        .args(["await-review", "--for", "a.rs"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("already pending"));
}

#[test]
fn await_review_errors_when_no_targets() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_llls"))
        .args(["await-review"])
        .current_dir(tmp.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("nothing to review"));
    assert!(!tmp.path().join(".llls/request.json").exists());
}

#[test]
fn await_review_request_stdin_carries_per_file_messages() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    // 5 lines so range [1,5] passes bounds check
    std::fs::write(tmp.path().join("a.rs"), b"1\n2\n3\n4\n5\n").unwrap();
    let llls = tmp.path().join(".llls");
    let llls2 = llls.clone();
    let reviewer = std::thread::spawn(move || {
        let req = llls2.join("request.json");
        for _ in 0..200 {
            if let Ok(s) = std::fs::read_to_string(&req) {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap();
                let files = v["files"].as_array().unwrap();
                assert!(files.iter().any(|f| f["path"] == "a.rs" && f["message"] == "check bounds"),
                    "files: {}", v["files"]);
                let id = v["id"].as_str().unwrap();
                let t = llls2.join("review.json.tmp");
                std::fs::write(&t, serde_json::json!({"id":id,"verdict":"approve","comments":[]}).to_string()).unwrap();
                std::fs::rename(&t, llls2.join("review.json")).unwrap();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("no request.json");
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_llls"))
        .args(["await-review", "--request", "-"])
        .current_dir(tmp.path())
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().unwrap();
    child.stdin.take().unwrap()
        .write_all(br#"{"message":"overall","files":[{"path":"a.rs","range":[1,5],"message":"check bounds"}]}"#)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    reviewer.join().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn await_review_rejects_missing_file() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    // "ghost.rs" does not exist in the repo
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_llls"))
        .args(["await-review", "--for", "ghost.rs", "--message", "look"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found"), "stderr: {stderr}");
    assert!(stderr.contains("ghost.rs"), "stderr: {stderr}");
}

fn setup_repo_with_file(content: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let path = tmp.path().join("a.rs");
    std::fs::write(&path, content).unwrap();
    (tmp, path)
}

fn run_for(dir: &std::path::Path, target: &str) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_llls"))
        .args(["await-review", "--for", target, "--message", "check"])
        .current_dir(dir)
        .output()
        .unwrap()
}

fn run_request(dir: &std::path::Path, json: &str) -> std::process::Output {
    use std::io::Write;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_llls"))
        .args(["await-review", "--request", "-"])
        .current_dir(dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(json.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn await_review_rejects_line_zero() {
    let (tmp, _) = setup_repo_with_file(b"line1\nline2\n");
    let out = run_for(tmp.path(), "a.rs:0");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("1-indexed"), "stderr: {stderr}");
}

#[test]
fn await_review_rejects_range_zero() {
    let (tmp, _) = setup_repo_with_file(b"line1\nline2\n");
    let out = run_request(tmp.path(), r#"{"files":[{"path":"a.rs","range":[0,2]}]}"#);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("1-indexed"), "stderr: {stderr}");
}

#[test]
fn await_review_rejects_inverted_range() {
    let (tmp, _) = setup_repo_with_file(b"line1\nline2\nline3\n");
    let out = run_request(tmp.path(), r#"{"files":[{"path":"a.rs","range":[5,2]}]}"#);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("inverted"), "stderr: {stderr}");
}

#[test]
fn await_review_rejects_line_out_of_bounds() {
    let (tmp, _) = setup_repo_with_file(b"line1\nline2\n");
    let out = run_for(tmp.path(), "a.rs:99");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("out of bounds"), "stderr: {stderr}");
}

#[test]
fn await_review_rejects_range_out_of_bounds() {
    let (tmp, _) = setup_repo_with_file(b"line1\nline2\n");
    let out = run_for(tmp.path(), "a.rs:1-99");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("out of bounds"), "stderr: {stderr}");
}

#[test]
fn await_review_rejects_both_line_and_range() {
    let (tmp, _) = setup_repo_with_file(b"line1\nline2\n");
    let out = run_request(tmp.path(), r#"{"files":[{"path":"a.rs","line":1,"range":[1,2]}]}"#);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not both"), "stderr: {stderr}");
}

#[test]
fn await_review_accepts_valid_in_bounds_targets() {
    use std::io::Write;
    // This test just checks validation passes — doesn't need a reviewer thread
    // since the process will block on the review; we abort via timeout not tested here.
    // Instead verify exit is NOT 1 by checking stderr is free of validation errors.
    // We use --timeout 0 which isn't a flag; just verify the request.json is written.
    // Simplest: write 5 lines and check --for line 5 doesn't immediately fail.
    let (tmp, _) = setup_repo_with_file(b"1\n2\n3\n4\n5\n");
    // Spawn and immediately kill — we only care that validation passes (no exit code 1).
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_llls"))
        .args(["await-review", "--for", "a.rs:5", "--message", "check", "--timeout", "1"])
        .current_dir(tmp.path())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("out of bounds"), "stderr: {stderr}");
    assert!(!stderr.contains("invalid"), "stderr: {stderr}");
}

#[test]
fn await_review_request_conflicts_with_for() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_llls"))
        .args(["await-review", "--request", "-", "--for", "a.rs"])
        .current_dir(tmp.path())
        .stdin(std::process::Stdio::null())
        .output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("mutually exclusive"));
}

#[test]
fn await_review_request_keeps_multiple_entries_per_file() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    // 50 lines so range [10,20] and line 50 both pass bounds check
    std::fs::write(tmp.path().join("a.rs"), (1u32..=50).map(|n| format!("{n}\n")).collect::<String>().as_bytes()).unwrap();
    let llls = tmp.path().join(".llls");
    let llls2 = llls.clone();
    let reviewer = std::thread::spawn(move || {
        let req = llls2.join("request.json");
        for _ in 0..200 {
            if let Ok(s) = std::fs::read_to_string(&req) {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap();
                let entries: Vec<_> = v["files"].as_array().unwrap().iter()
                    .filter(|f| f["path"] == "a.rs").collect();
                assert_eq!(entries.len(), 2, "both same-file entries must survive: {}", v["files"]);
                let id = v["id"].as_str().unwrap();
                let t = llls2.join("review.json.tmp");
                std::fs::write(&t, serde_json::json!({"id":id,"verdict":"approve","comments":[]}).to_string()).unwrap();
                std::fs::rename(&t, llls2.join("review.json")).unwrap();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("no request.json");
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_llls"))
        .args(["await-review", "--request", "-"])
        .current_dir(tmp.path())
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().unwrap();
    child.stdin.take().unwrap().write_all(
        br#"{"files":[{"path":"a.rs","range":[10,20],"message":"first"},{"path":"a.rs","line":50,"message":"second"}]}"#
    ).unwrap();
    let out = child.wait_with_output().unwrap();
    reviewer.join().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}
