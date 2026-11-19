//! A real process boundary, including initialization, discovery, call and EOF.
#[test]
fn stdio_contract() {
    let result = std::process::Command::new("python3")
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/check_mcp.py"))
        .arg(env!("CARGO_BIN_EXE_tauros-mcp"))
        .output()
        .unwrap();
    assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}
