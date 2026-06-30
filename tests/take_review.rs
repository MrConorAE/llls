#[test]
fn take_review_drains_inbox_then_reports_empty() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let llls = tmp.path().join(".llls");
    std::fs::create_dir_all(&llls).unwrap();
    std::fs::write(llls.join("inbox.json"),
        r#"{"id":"adhoc-1","verdict":"comment","comments":[{"file":"a.rs","line":3,"context":"x","body":"hi"}]}"#).unwrap();

    let bin = env!("CARGO_BIN_EXE_llls");
    let out = std::process::Command::new(bin)
        .args(["take-review"]).current_dir(tmp.path()).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("## a.rs:3") && stdout.contains("hi"), "got: {stdout}");
    assert!(!llls.join("inbox.json").exists(), "inbox should be cleared");

    // second run: nothing pending
    let out2 = std::process::Command::new(bin)
        .args(["take-review"]).current_dir(tmp.path()).output().unwrap();
    assert!(out2.status.success());
    assert!(String::from_utf8_lossy(&out2.stdout).trim().is_empty());
    assert!(String::from_utf8_lossy(&out2.stderr).contains("no pending review"));
}
