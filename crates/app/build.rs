use std::path::Path;
use std::process::Command;

/// Builds the Svelte interface into `ui/dist` before Tauri embeds it. When the npm
/// dependencies are not installed, an existing `ui/dist` is used as is.
fn main() {
    for path in ["ui/src", "ui/index.html", "ui/vite.config.js", "ui/package-lock.json"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let ui = Path::new("ui");
    if ui.join("node_modules").is_dir() {
        let status = Command::new("npm")
            .args(["run", "build", "--silent"])
            .current_dir(ui)
            .status()
            .expect("cannot run npm to build the interface");
        assert!(status.success(), "the interface build failed (npm --prefix crates/app/ui run build)");
    } else if !ui.join("dist/index.html").is_file() {
        panic!("the interface is not built: run `npm --prefix crates/app/ui ci` (then cargo builds it)");
    }
    tauri_build::build()
}
