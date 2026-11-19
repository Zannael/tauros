//! Layer 1 MCP wiring: attaches `Gen3Connector` and `Mgba` (`EmuSession` +
//! `Gen3BootHook`) to `TaurosServer`, and hosts an in-process server over a
//! private local Unix domain socket for the TypeScript Agent SDK sidecar (`AGT-002`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use tauros_agent_bridge::McpServerConfig;
use tauros_connector_gen3::boot::{map_numbers, BootPlan, BootTarget, GameLayout, Symbols};
use tauros_connector_gen3::toolchain::Toolchain;
use tauros_connector_gen3::Gen3Connector;
use tauros_emu::{Emulator, Mgba};
use tauros_mcp::emulator::{BootHook, EmuSession, SymbolTable};
use tauros_mcp::{serve_io, TaurosServer};
use tauros_model::{Connector, FlagId, MapId};
use tokio::net::UnixListener;
use tokio::sync::oneshot;

/// Standard battle-outcome constants from pokeemerald-expansion `include/constants/battle.h`.
pub fn gen3_standard_constants() -> BTreeMap<String, i64> {
    BTreeMap::from([
        ("B_OUTCOME_WON".to_owned(), 1),
        ("B_OUTCOME_LOST".to_owned(), 2),
        ("B_OUTCOME_DREW".to_owned(), 3),
        ("B_OUTCOME_RAN".to_owned(), 4),
        ("B_OUTCOME_PLAYER_TELEPORTED".to_owned(), 5),
        ("B_OUTCOME_MON_FLED".to_owned(), 6),
        ("B_OUTCOME_CAUGHT".to_owned(), 7),
        ("B_OUTCOME_NO_SAFARI_BALLS".to_owned(), 8),
        ("B_OUTCOME_FORFEITED".to_owned(), 9),
        ("B_OUTCOME_MON_TELEPORTED".to_owned(), 10),
    ])
}

/// Loads `gen3_standard_constants()` plus any `MAP_*` constants from the project's
/// generated `include/constants/map_groups.h` (packed as `(group << 8) | num` and
/// individual `MAP_GROUP_*`/`MAP_NUM_*` helpers if needed).
pub fn load_project_constants(project_dir: &Path) -> BTreeMap<String, i64> {
    let mut constants = gen3_standard_constants();
    if let Ok(maps) = map_numbers(project_dir) {
        for (map_id, (group, num)) in maps {
            // Packed little-endian (group, num) as u16 matches SaveBlock1->location bytes 0..2:
            // byte 0 = mapGroup, byte 1 = mapNum => u16 LE = (num << 8) | group.
            let packed_le = ((num as i64) << 8) | (group as i64);
            constants.insert(map_id, packed_le);
        }
    }
    constants
}

/// [`BootHook`] backed by `tauros-connector-gen3::boot::BootPlan`.
pub struct Gen3BootHook {
    project_dir: PathBuf,
    connector: Arc<Mutex<Box<dyn Connector + Send>>>,
    toolchain: Option<Toolchain>,
    cached_layout: Option<GameLayout>,
}

impl Gen3BootHook {
    pub fn new(
        project_dir: PathBuf,
        connector: Arc<Mutex<Box<dyn Connector + Send>>>,
        toolchain: Option<Toolchain>,
    ) -> Self {
        Self {
            project_dir,
            connector,
            toolchain,
            cached_layout: None,
        }
    }

    fn layout(&mut self) -> Result<GameLayout, String> {
        if let Some(layout) = self.cached_layout {
            return Ok(layout);
        }
        let toolchain = self
            .toolchain
            .as_ref()
            .ok_or_else(|| "toolchain not configured; cannot measure Gen 3 struct layout".to_owned())?;
        let layout = GameLayout::measure(toolchain, &self.project_dir).map_err(|e| e.to_string())?;
        self.cached_layout = Some(layout);
        Ok(layout)
    }
}

impl BootHook for Gen3BootHook {
    fn boot_to(
        &mut self,
        emu: &mut dyn Emulator,
        map: &str,
        x: i16,
        y: i16,
        flags: &[String],
    ) -> Result<u32, String> {
        let sym_path = self.project_dir.join("pokeemerald.sym");
        let sym_text = fs::read_to_string(&sym_path)
            .map_err(|e| format!("failed to read {}: {e}", sym_path.display()))?;
        let maps = map_numbers(&self.project_dir).map_err(|e| e.to_string())?;
        let snapshot = {
            let guard = self
                .connector
                .lock()
                .map_err(|_| "connector lock poisoned".to_owned())?;
            guard.load().map_err(|e| e.to_string())?
        };
        let layout = self.layout()?;
        let target = BootTarget {
            map: MapId::from(map),
            x,
            y,
            flags: flags.iter().map(|f| FlagId::from(f.as_str())).collect(),
        };
        let plan = BootPlan::new(target, &snapshot, &maps, &Symbols::parse(&sym_text), layout)
            .map_err(|e| e.to_string())?;
        let report = plan.boot(emu).map_err(|e| e.to_string())?;
        Ok(report.frames)
    }
}

/// Reloads `pokeemerald.gba`, `pokeemerald.sym` and constants from `project_dir`
/// into an attached [`EmuSession`].
pub fn reload_project_artifacts(
    project_dir: &Path,
    session: &mut EmuSession,
) -> Result<(), String> {
    let rom_path = project_dir.join("pokeemerald.gba");
    let sym_path = project_dir.join("pokeemerald.sym");
    let rom = fs::read(&rom_path)
        .map_err(|e| format!("failed to read {}: {e}", rom_path.display()))?;
    let sym_text = fs::read_to_string(&sym_path)
        .map_err(|e| format!("failed to read {}: {e}", sym_path.display()))?;
    session.load_rom(rom)?;
    session.set_symbols(SymbolTable::parse(&sym_text));
    session.set_constants(load_project_constants(project_dir));
    Ok(())
}

/// Wires a full Layer 1 [`TaurosServer`] for a Gen 3 project:
/// - `Gen3Connector` (domain tools + G3-001 snapshot guard + Docker build/test)
/// - `EmuSession` wrapping `Mgba` with `Gen3BootHook` and optional unmodified `base_rom`
/// - Post-build hook that automatically reloads `pokeemerald.gba` and `pokeemerald.sym`
///   into the attached `EmuSession` whenever the MCP `build` tool succeeds.
pub fn build_project_mcp_server(
    project_dir: &Path,
    toolchain: Option<Toolchain>,
    base_rom: Option<Vec<u8>>,
) -> Result<TaurosServer, String> {
    let connector = Gen3Connector::open(project_dir, toolchain.clone())
        .map_err(|e| format!("failed to open Gen 3 project {}: {e}", project_dir.display()))?;
    let shared_connector: Arc<Mutex<Box<dyn Connector + Send>>> =
        Arc::new(Mutex::new(Box::new(connector)));

    let mgba = Mgba::gba().map_err(|e| format!("failed to initialize libmgba core: {e}"))?;
    let mut session = EmuSession::new(mgba)
        .with_constants(load_project_constants(project_dir))
        .with_boot_hook(Gen3BootHook::new(
            project_dir.to_path_buf(),
            Arc::clone(&shared_connector),
            toolchain,
        ));

    if let Some(base) = base_rom {
        session = session.with_base_rom(base);
    }

    // If a ROM and symbol table are already built in the project directory, load them eagerly.
    if project_dir.join("pokeemerald.gba").is_file()
        && project_dir.join("pokeemerald.sym").is_file()
    {
        reload_project_artifacts(project_dir, &mut session)?;
    }

    let reload_dir = project_dir.to_path_buf();
    Ok(TaurosServer::with_shared_connector(shared_connector)
        .with_emulator(session)
        .with_post_build_hook(move |emu_session| {
            reload_project_artifacts(&reload_dir, emu_session)
        }))
}

/// Hosts an in-process [`TaurosServer`] over a private local Unix domain socket
/// so the TypeScript Agent SDK sidecar can connect to it via `mcp-bridge.js`
/// without exposing any network listener.
pub struct InProcessMcpHost {
    socket_dir: PathBuf,
    socket_path: PathBuf,
    shutdown_tx: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl InProcessMcpHost {
    /// Starts serving `server` on a new private Unix domain socket.
    pub fn start(server: TaurosServer) -> Result<Self, String> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let socket_dir = std::env::temp_dir().join(format!(
            "tauros-mcp-ipc-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&socket_dir)
            .map_err(|e| format!("failed to create IPC dir {}: {e}", socket_dir.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&socket_dir, fs::Permissions::from_mode(0o700));
        }

        let socket_path = socket_dir.join("tauros.sock");
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let bind_path = socket_path.clone();
        let thread = thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("tokio runtime error: {e}")));
                    return;
                }
            };

            rt.block_on(async move {
                let listener = match UnixListener::bind(&bind_path) {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!(
                            "failed to bind Unix socket {}: {e}",
                            bind_path.display()
                        )));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));

                let mut shutdown_rx = shutdown_rx;
                loop {
                    tokio::select! {
                        _ = &mut shutdown_rx => break,
                        accepted = listener.accept() => {
                            match accepted {
                                Ok((stream, _)) => {
                                    let srv = server.clone();
                                    tokio::spawn(async move {
                                        let (reader, writer) = stream.into_split();
                                        let _ = serve_io(srv, reader, writer).await;
                                    });
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }
            });
        });

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                socket_dir,
                socket_path,
                shutdown_tx: Some(shutdown_tx),
                thread: Some(thread),
            }),
            Ok(Err(err)) => {
                let _ = fs::remove_dir_all(&socket_dir);
                Err(err)
            }
            Err(_) => {
                let _ = fs::remove_dir_all(&socket_dir);
                Err("in-process MCP host thread exited before binding socket".to_owned())
            }
        }
    }

    /// Returns the filesystem path of the local Unix domain socket.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Builds the [`McpServerConfig`] pointing `mcp-bridge.js` at this in-process host.
    pub fn mcp_server_config(&self, node_binary: &Path, mcp_bridge_script: &Path) -> McpServerConfig {
        McpServerConfig::Stdio {
            command: node_binary.to_string_lossy().into_owned(),
            args: vec![
                mcp_bridge_script.to_string_lossy().into_owned(),
                self.socket_path.to_string_lossy().into_owned(),
            ],
            env: BTreeMap::new(),
        }
    }
}

impl Drop for InProcessMcpHost {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        let _ = fs::remove_file(&self.socket_path);
        let _ = fs::remove_dir_all(&self.socket_dir);
    }
}
