pub struct Args {
    pub files: Vec<String>,
    pub message: String,
    pub round: u32,
    pub json: bool,
    pub timeout: Option<u64>,
}

pub fn run(_args: Args) -> anyhow::Result<i32> {
    eprintln!("await-review: not yet implemented");
    Ok(0)
}
