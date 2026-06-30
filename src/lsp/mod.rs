pub mod backend;
pub mod convert;
pub mod server;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "llls=info".to_owned()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(server::serve());
}
