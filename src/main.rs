use clap::{Parser, Subcommand};
use llls::{await_review, lsp};

#[derive(Parser)]
#[command(name = "llls", about = "LLM Language Server — an editor-driven review loop")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the language server over stdio for your editor.
    Lsp,
    /// Request a review of some files and block until it is submitted.
    AwaitReview {
        /// Files (comma-separated), each optionally with `:LINE` or `:START-END`.
        #[arg(long = "for", value_delimiter = ',', required = true)]
        files: Vec<String>,
        /// A short note shown to the reviewer.
        #[arg(long, default_value = "")]
        message: String,
        /// Review round number (cosmetic; increment on follow-up rounds).
        #[arg(long, default_value_t = 1)]
        round: u32,
        /// Emit the review as JSON instead of markdown.
        #[arg(long)]
        json: bool,
        /// Give up after this many seconds (default: wait forever).
        #[arg(long)]
        timeout: Option<u64>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Lsp => {
            lsp::run();
            Ok(())
        }
        Cmd::AwaitReview { files, message, round, json, timeout } => {
            let code = await_review::run(await_review::Args { files, message, round, json, timeout })?;
            std::process::exit(code);
        }
    }
}
