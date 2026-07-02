use anyhow::{Context, Result};
use crate::render;
use crate::store::Store;

pub fn run(json: bool) -> Result<i32> {
    let cwd = std::env::current_dir()?;
    let store = Store::discover(&cwd).context("llls must be run inside a git repository")?;
    match store.read_inbox() {
        Some(r) => {
            let out = if json { render::to_json(&r) } else { render::to_markdown(&r) };
            println!("{out}");
            store.clear_inbox();
            Ok(0)
        }
        None => {
            eprintln!("llls: no pending review.");
            Ok(0)
        }
    }
}
