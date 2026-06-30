use crate::types::{Review, Verdict};

fn verdict_str(v: Verdict) -> &'static str {
    match v {
        Verdict::Approve => "approve",
        Verdict::RequestChanges => "request_changes",
        Verdict::Comment => "comment",
        Verdict::Dismissed => "dismissed",
    }
}

pub fn to_markdown(r: &Review) -> String {
    if r.verdict == Verdict::Dismissed {
        return "# Review dismissed\n\nThe reviewer declined to review; proceed without a review.\n".to_string();
    }
    let mut s = format!("# Review (verdict: {})\n\n", verdict_str(r.verdict));
    if let Some(summary) = &r.summary {
        if !summary.trim().is_empty() {
            s.push_str(summary.trim());
            s.push_str("\n\n");
        }
    }
    if r.comments.is_empty() {
        s.push_str("_No line comments._\n");
    } else {
        for c in &r.comments {
            s.push_str(&format!("## {}:{}\n", c.file, c.line));
            if !c.context.trim().is_empty() {
                s.push_str(&format!("> {}\n\n", c.context.trim()));
            }
            s.push_str(c.body.trim());
            s.push_str("\n\n");
        }
    }
    s
}

pub fn to_json(r: &Review) -> String {
    serde_json::to_string_pretty(r).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Comment, Review, Verdict};

    fn review(verdict: Verdict, comments: Vec<Comment>) -> Review {
        Review { id: "r1-1".into(), verdict, summary: None, comments }
    }

    #[test]
    fn dismissed_is_explicit() {
        let md = to_markdown(&review(Verdict::Dismissed, vec![]));
        assert!(md.contains("dismissed"));
        assert!(md.to_lowercase().contains("proceed"));
    }

    #[test]
    fn approve_with_no_comments() {
        let md = to_markdown(&review(Verdict::Approve, vec![]));
        assert!(md.contains("verdict: approve"));
        assert!(md.contains("No line comments"));
    }

    #[test]
    fn comments_render_with_anchor_and_context() {
        let md = to_markdown(&review(Verdict::RequestChanges, vec![
            Comment { file: "a.rs".into(), line: 12, context: "let x = 1;".into(), body: "use 2".into() },
        ]));
        assert!(md.contains("verdict: request_changes"));
        assert!(md.contains("## a.rs:12"));
        assert!(md.contains("> let x = 1;"));
        assert!(md.contains("use 2"));
    }

    #[test]
    fn json_is_parseable() {
        let r = review(Verdict::Comment, vec![]);
        assert_eq!(serde_json::from_str::<Review>(&to_json(&r)).unwrap(), r);
    }
}
