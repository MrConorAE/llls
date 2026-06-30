use anyhow::{Context, Result};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::store::{repo_relative, Store};
use crate::types::{parse_target, Request};
use crate::{render, watch};

pub struct Args {
    pub files: Vec<String>,
    pub message: String,
    pub round: u32,
    pub json: bool,
    pub timeout: Option<u64>,
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

    let files = args
        .files
        .iter()
        .map(|f| {
            let mut t = parse_target(f);
            t.path = repo_relative(&t.path, &repo_root);
            t
        })
        .collect();

    let request = Request {
        id: id.clone(),
        round: args.round,
        created_unix: now.as_secs(),
        files,
        message: args.message,
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
