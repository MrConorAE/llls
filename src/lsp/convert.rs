use tower_lsp::lsp_types::*;

use crate::types::{Comment, FileTarget, Request};

pub const SOURCE: &str = "llls";

fn line_diag(line0: u32, severity: DiagnosticSeverity, message: String) -> Diagnostic {
    Diagnostic {
        range: Range {
            start: Position { line: line0, character: 0 },
            end: Position { line: line0, character: u32::MAX },
        },
        severity: Some(severity),
        source: Some(SOURCE.to_string()),
        message,
        ..Default::default()
    }
}

/// Line-1 (or start-line) marker announcing a review request for a file.
pub fn request_marker(target: &FileTarget, message: &str, reviewed: bool) -> Diagnostic {
    let line0 = target
        .range
        .map(|r| r[0].saturating_sub(1))
        .or_else(|| target.line.map(|l| l.saturating_sub(1)))
        .unwrap_or(0);
    let msg = if reviewed {
        format!("Reviewed — {message}")
    } else if message.is_empty() {
        "Claude requests review of this file".to_string()
    } else {
        format!("Claude requests review — {message}")
    };
    let severity = if reviewed { DiagnosticSeverity::HINT } else { DiagnosticSeverity::INFORMATION };
    line_diag(line0, severity, msg)
}

/// A marker showing a pending review comment inline on its line.
pub fn comment_diag(c: &Comment) -> Diagnostic {
    let line0 = c.line.saturating_sub(1);
    let first = c.body.lines().next().unwrap_or("");
    let preview = if first.chars().count() > 60 {
        let cut: String = first.chars().take(60).collect();
        format!("{cut}…")
    } else {
        first.to_string()
    };
    line_diag(line0, DiagnosticSeverity::HINT, preview)
}

/// All diagnostics for one file: the request marker (if requested) + each comment.
pub fn file_diagnostics(request: Option<&Request>, file: &str, reviewed: bool, comments: &[&Comment]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if let Some(req) = request {
        if let Some(t) = req.files.iter().find(|t| t.path == file) {
            out.push(request_marker(t, &req.message, reviewed));
        }
    }
    for c in comments {
        out.push(comment_diag(c));
    }
    out
}

fn cmd(title: &str, command: &str, arg: serde_json::Value) -> CodeActionOrCommand {
    CodeActionOrCommand::Command(Command {
        title: title.to_string(),
        command: command.to_string(),
        arguments: Some(vec![arg]),
    })
}

pub fn code_actions(
    file: &str,
    line1: u32,
    is_requested: bool,
    reviewed: bool,
    comment_at_line: bool,
    has_draft: bool,
    has_request: bool,
) -> Vec<CodeActionOrCommand> {
    let mut a = Vec::new();
    a.push(cmd("Leave an agent note", "llls.addComment", serde_json::json!({ "file": file, "line": line1 })));
    if comment_at_line {
        a.push(cmd("Edit agent note", "llls.editComment", serde_json::json!({ "file": file, "line": line1 })));
        a.push(cmd("Delete agent note", "llls.deleteComment", serde_json::json!({ "file": file, "line": line1 })));
    }
    if is_requested {
        let label = if reviewed { "Mark file unreviewed" } else { "Mark file reviewed" };
        a.push(cmd(label, "llls.markReviewed", serde_json::json!({ "file": file })));
    }
    if has_request || has_draft {
        a.push(cmd("Send review to Claude", "llls.submitReview", serde_json::json!({})));
        a.push(cmd("Discard review", "llls.dismissReview", serde_json::json!({})));
    }
    if has_request {
        a.push(cmd("Next file to review", "llls.nextFile", serde_json::json!({})));
    }
    a
}

pub fn hover_for(comments: &[&Comment], line1: u32) -> Option<String> {
    let matching: Vec<&&Comment> = comments.iter().filter(|c| c.line == line1).collect();
    if matching.is_empty() {
        return None;
    }
    let mut md = String::new();
    for c in matching {
        md.push_str(&format!("**Agent note**\n\n{}\n\n", c.body));
    }
    Some(md)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Comment, FileTarget};

    #[test]
    fn whole_file_marker_lands_on_line_zero() {
        let t = FileTarget { path: "a.rs".into(), line: None, range: None };
        let d = request_marker(&t, "look here", false);
        assert_eq!(d.range.start.line, 0);
        assert_eq!(d.severity, Some(DiagnosticSeverity::INFORMATION));
        assert!(d.message.contains("look here"));
    }

    #[test]
    fn ranged_marker_lands_on_start_line_zero_indexed() {
        let t = FileTarget { path: "a.rs".into(), line: None, range: Some([40, 80]) };
        assert_eq!(request_marker(&t, "", false).range.start.line, 39);
    }

    #[test]
    fn reviewed_marker_is_hint() {
        let t = FileTarget { path: "a.rs".into(), line: Some(5), range: None };
        let d = request_marker(&t, "m", true);
        assert_eq!(d.severity, Some(DiagnosticSeverity::HINT));
        assert!(d.message.contains("Reviewed"));
    }

    #[test]
    fn file_diagnostics_combines_marker_and_comments() {
        let req = crate::types::Request {
            id: "i".into(), round: 1, created_unix: 0, message: "m".into(),
            files: vec![FileTarget { path: "a.rs".into(), line: None, range: None }],
        };
        let c = Comment { file: "a.rs".into(), line: 10, context: String::new(), body: "note".into() };
        let diags = file_diagnostics(Some(&req), "a.rs", false, &[&c]);
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[1].range.start.line, 9); // comment at 1-indexed line 10
    }

    #[test]
    fn code_actions_offer_edit_delete_only_when_comment_present() {
        let none = code_actions("a.rs", 1, true, false, false, true, true);
        assert!(none.iter().all(|a| !title(a).contains("Edit")));
        let some = code_actions("a.rs", 1, true, false, true, true, true);
        assert!(some.iter().any(|a| title(a).contains("Edit agent note")));
        assert!(some.iter().any(|a| title(a).contains("Send review to Claude")));
        assert!(some.iter().any(|a| title(a).contains("Next file to review")));

        // ad-hoc: no request but a draft present -> Send/Discard offered, no Next
        let adhoc = code_actions("a.rs", 1, false, false, false, true, false);
        assert!(adhoc.iter().any(|a| title(a).contains("Send review to Claude")));
        assert!(adhoc.iter().all(|a| !title(a).contains("Next file to review")));
    }

    #[test]
    fn hover_matches_line() {
        let c = Comment { file: "a.rs".into(), line: 4, context: String::new(), body: "hi".into() };
        assert!(hover_for(&[&c], 4).unwrap().contains("hi"));
        assert!(hover_for(&[&c], 5).is_none());
    }

    fn title(a: &CodeActionOrCommand) -> String {
        match a { CodeActionOrCommand::Command(c) => c.title.clone(), _ => String::new() }
    }
}
