//! Layer 1 stdio MCP entry point wired to a Gen 3 game project and libmgba emulator.
//!
//! Usage:
//!   tauros-mcp-project --project DIR [--base-rom ROM.gba] [--repo-root DIR]
//!
//! Stdout is reserved strictly for MCP JSON-RPC; diagnostics go to stderr.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use tauros_app::mcp_host::build_project_mcp_server;
use tauros_connector_gen3::toolchain::Toolchain;
use tauros_mcp::serve_server_stdio;

const USAGE: &str = "Usage: tauros-mcp-project --project DIR [--base-rom ROM.gba] [--repo-root DIR]";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut project: Option<PathBuf> = None;
    let mut base_rom_path: Option<PathBuf> = None;
    let mut repo_root: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                eprintln!("{USAGE}");
                return Ok(());
            }
            "--project" if i + 1 < args.len() => {
                project = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--base-rom" if i + 1 < args.len() => {
                base_rom_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--repo-root" if i + 1 < args.len() => {
                repo_root = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            other => {
                return Err(format!("unknown or incomplete option `{other}`\n{USAGE}").into());
            }
        }
    }

    let project = project.ok_or_else(|| format!("missing --project\n{USAGE}"))?;
    let repo_root = repo_root.unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let toolchain = Toolchain::for_repository(&repo_root).ok();

    let base_rom = match base_rom_path {
        Some(path) => Some(fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?),
        None => None,
    };

    let server = build_project_mcp_server(&project, toolchain, base_rom)
        .map_err(std::io::Error::other)?;
    serve_server_stdio(server).await
}
