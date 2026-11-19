//! Headless stdio transport for external MCP clients.
#[tokio::main]
async fn main() {
    if let Err(error) = tauros_mcp::serve_stdio().await {
        eprintln!("tauros-mcp: {error}");
        std::process::exit(1);
    }
}
