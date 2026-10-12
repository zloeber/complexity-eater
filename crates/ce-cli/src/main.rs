#[tokio::main]
async fn main() {
    let code = ce_cli::run().await;
    std::process::exit(code);
}
