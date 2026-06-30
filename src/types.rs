use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FileTarget {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[u32; 2]>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: String,
    pub round: u32,
    pub created_unix: u64,
    pub files: Vec<FileTarget>,
    #[serde(default)]
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Comment {
    pub file: String,
    pub line: u32,
    #[serde(default)]
    pub context: String,
    pub body: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Draft {
    pub id: String,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approve,
    RequestChanges,
    Comment,
    Dismissed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub id: String,
    pub verdict: Verdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

/// Parse a `--for` token: `path`, `path:LINE`, or `path:START-END`.
/// A trailing `:...` is only treated as a locator when it parses as numbers;
/// otherwise the whole token is the path.
pub fn parse_target(s: &str) -> FileTarget {
    if let Some((path, loc)) = s.rsplit_once(':') {
        if !path.is_empty() {
            if let Some((a, b)) = loc.split_once('-') {
                if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                    return FileTarget { path: path.to_string(), line: None, range: Some([a, b]) };
                }
            } else if let Ok(n) = loc.parse::<u32>() {
                return FileTarget { path: path.to_string(), line: Some(n), range: None };
            }
        }
    }
    FileTarget { path: s.to_string(), line: None, range: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_path() {
        assert_eq!(parse_target("docs/plan.md"),
            FileTarget { path: "docs/plan.md".into(), line: None, range: None });
    }

    #[test]
    fn parse_single_line() {
        assert_eq!(parse_target("src/foo.rs:42"),
            FileTarget { path: "src/foo.rs".into(), line: Some(42), range: None });
    }

    #[test]
    fn parse_range() {
        assert_eq!(parse_target("src/foo.rs:40-80"),
            FileTarget { path: "src/foo.rs".into(), line: None, range: Some([40, 80]) });
    }

    #[test]
    fn non_numeric_suffix_is_part_of_path() {
        // a colon that isn't a line locator stays in the path
        assert_eq!(parse_target("weird:name.rs"),
            FileTarget { path: "weird:name.rs".into(), line: None, range: None });
    }

    #[test]
    fn verdict_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&Verdict::RequestChanges).unwrap(), "\"request_changes\"");
    }

    #[test]
    fn review_round_trips() {
        let r = Review {
            id: "r1-9".into(),
            verdict: Verdict::Approve,
            summary: None,
            comments: vec![Comment { file: "a.rs".into(), line: 3, context: "let x = 1;".into(), body: "nit".into() }],
        };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Review>(&s).unwrap(), r);
    }
}
