use anyhow::{Context, Result};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::store::{repo_relative, Store};
use crate::types::{parse_target, Request};
use crate::{render, watch};

#[derive(serde::Deserialize)]
struct RequestSpec {
    #[serde(default)]
    message: String,
    files: Vec<crate::types::FileTarget>,
}

pub struct Args {
    pub files: Vec<String>,
    pub changed: Option<String>,
    pub message: String,
    pub round: u32,
    pub json: bool,
    pub timeout: Option<u64>,
    pub request: Option<String>,
}

pub fn run(args: Args) -> Result<i32> {
    let cwd = std::env::current_dir()?;
    let store = Store::discover(&cwd).context("llls must be run inside a git repository")?;
    store.ensure()?;

    if store.read_request().is_some() {
        eprintln!(
            "llls: a review is already pending ({}). Submit or discard it in your editor first.",
            store.request_path().display()
        );
        return Ok(1);
    }

    let repo_root = store.repo_root();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let id = format!("r{}-{}", args.round, now.as_nanos());

    let (files, message): (Vec<crate::types::FileTarget>, String) = if let Some(req) = &args.request {
        if !args.files.is_empty() || args.changed.is_some() {
            eprintln!("llls: --request is mutually exclusive with --for/--changed.");
            return Ok(1);
        }
        let raw = if req == "-" {
            use std::io::Read;
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        } else {
            std::fs::read_to_string(req).with_context(|| format!("reading --request file {req}"))?
        };
        let spec: RequestSpec = serde_json::from_str(&raw).context("parsing --request JSON")?;
        // Keep every entry: multiple targets for the same file (different
        // line/range + message) are intentional here. Only the --for/--changed
        // union below dedups by path.
        let files = spec.files.into_iter().map(|mut t| {
            t.path = repo_relative(&t.path, &repo_root);
            t
        }).collect::<Vec<_>>();
        (files, spec.message)
    } else {
        let mut targets: Vec<crate::types::FileTarget> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for f in &args.files {
            let mut t = parse_target(f);
            t.path = repo_relative(&t.path, &repo_root);
            if seen.insert(t.path.clone()) { targets.push(t); }
        }
        if let Some(spec) = &args.changed {
            let base = if spec.is_empty() { None } else { Some(spec.as_str()) };
            for p in crate::changed::changed_files(&repo_root, base)? {
                if seen.insert(p.clone()) {
                    targets.push(crate::types::FileTarget { path: p, line: None, range: None, message: None });
                }
            }
        }
        (targets, args.message.clone())
    };
    if files.is_empty() {
        eprintln!("llls: nothing to review (no files supplied).");
        return Ok(1);
    }

    let request = Request {
        id: id.clone(),
        round: args.round,
        created_unix: now.as_secs(),
        files,
        message,
    };
    store.write_request(&request)?;

    let poll = Duration::from_secs(2);
    let timeout = args.timeout.map(Duration::from_secs);
    let review = watch::wait_until(&store.dir, poll, timeout, || store.matching_review(&id))?;

    match review {
        Some(r) => {
            let out = if args.json { render::to_json(&r) } else { render::to_markdown(&r) };
            println!("{out}");
            store.clear_all(); // best-effort: removes review.json (+ any stray request/draft)
            Ok(0)
        }
        None => {
            eprintln!("llls: no review submitted within the timeout; the request is still pending.");
            Ok(2)
        }
    }
}
