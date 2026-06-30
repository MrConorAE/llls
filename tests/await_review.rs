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
