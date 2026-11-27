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
use tauros_connector_gen3::boot::{map_numbers, read_live_location, BootPlan, BootTarget, GameLayout, Symbols};
use tauros_connector_gen3::toolchain::Toolchain;
use tauros_connector_gen3::Gen3Connector;
use tauros_emu::{Emulator, Mgba};
use tauros_mcp::domain;
use tauros_mcp::emulator::{emu_boot, BootHook, BootKind, EmuBootRequest, EmuSession, SymbolTable};
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
    cached_layout: Arc<Mutex<Option<(String, GameLayout)>>>,
}

impl Gen3BootHook {
    pub fn new(
        project_dir: PathBuf,
        connector: Arc<Mutex<Box<dyn Connector + Send>>>,
        toolchain: Option<Toolchain>,
    ) -> Self {
        Self::with_shared_layout(project_dir, connector, toolchain, Arc::new(Mutex::new(None)))
    }

    pub fn with_shared_layout(
        project_dir: PathBuf,
        connector: Arc<Mutex<Box<dyn Connector + Send>>>,
        toolchain: Option<Toolchain>,
        cached_layout: Arc<Mutex<Option<(String, GameLayout)>>>,
    ) -> Self {
        Self {
            project_dir,
            connector,
            toolchain,
            cached_layout,
        }
    }

    fn layout(&mut self) -> Result<GameLayout, String> {
        // Native edits can change C struct layouts; only reuse a measurement at
        // the same source revision. Failed recordings invalidate it as well.
        let revision = git_head_revision(&self.project_dir)?;
        if let Some((cached_revision, layout)) = self.cached_layout.lock().unwrap().as_ref() {
            if *cached_revision == revision {
                return Ok(*layout);
            }
        }
        let toolchain = self
            .toolchain
            .as_ref()
            .ok_or_else(|| "toolchain not configured; cannot measure Gen 3 struct layout".to_owned())?;
        let layout = GameLayout::measure(toolchain, &self.project_dir).map_err(|e| e.to_string())?;
        *self.cached_layout.lock().unwrap() = Some((revision, layout));
        Ok(layout)
    }
}

fn git_head_revision(project_dir: &Path) -> Result<String, String> {
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_dir)
        .output()
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&revision.stdout).trim().to_owned())
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

/// A Gen 3 project opened by Layer 1: one connector and one emulator session,
/// shared by the app's window and by the MCP server the agent uses, so what the
/// agent builds and boots is what the window shows.
pub struct ProjectSession {
    dir: PathBuf,
    connector: Arc<Mutex<Box<dyn Connector + Send>>>,
    emulator: Arc<Mutex<EmuSession>>,
    operation_gate: Arc<Mutex<()>>,
    cached_layout: Arc<Mutex<Option<(String, GameLayout)>>>,
    last_build_sha256: Arc<Mutex<Option<String>>>,
    last_boot: Arc<Mutex<Option<LastBootTarget>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LastBootTarget {
    pub map: String,
    pub x: i16,
    pub y: i16,
    pub flags: Vec<String>,
    pub rom_sha256: String,
}

/// Live emulator location and build-sync context shared with the UI and agent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AgentEmuContext {
    pub selected_map: Option<String>,
    pub selected_x: Option<i16>,
    pub selected_y: Option<i16>,
    pub launch_target: Option<LastBootTarget>,
    pub live_status: String,
    pub live_map: Option<String>,
    pub live_group: Option<u8>,
    pub live_num: Option<u8>,
    pub live_x: Option<i16>,
    pub live_y: Option<i16>,
    pub loaded_rom_sha256: Option<String>,
    pub built_rom_sha256: Option<String>,
    pub rom_stale: bool,
    pub new_build_pending: bool,
}

impl AgentEmuContext {
    /// Formats a concise structured context block for the agent, keeping the
    /// UI-selected map, launch target, and live emulator position distinct.
    pub fn to_prompt_block(&self) -> String {
        let selected = match (&self.selected_map, self.selected_x, self.selected_y) {
            (Some(m), Some(x), Some(y)) => format!("{m} ({x}, {y})"),
            (Some(m), _, _) => m.clone(),
            _ => "none".to_owned(),
        };
        let launch = match &self.launch_target {
            Some(t) if t.flags.is_empty() => format!("{} ({}, {})", t.map, t.x, t.y),
            Some(t) => format!("{} ({}, {}) flags={}", t.map, t.x, t.y, t.flags.join(",")),
            None => "none".to_owned(),
        };
        let live = match (&self.live_map, self.live_x, self.live_y) {
            (Some(m), Some(x), Some(y)) => format!("{} at {m} ({x}, {y})", self.live_status),
            (None, Some(x), Some(y)) => format!(
                "{} at map ({:?}, {:?}) ({x}, {y})",
                self.live_status, self.live_group, self.live_num
            ),
            _ => self.live_status.clone(),
        };
        let loaded = self.loaded_rom_sha256.as_deref().map(|s| &s[..s.len().min(12)]).unwrap_or("none");
        let built = self.built_rom_sha256.as_deref().map(|s| &s[..s.len().min(12)]).unwrap_or("none");
        format!(
            "[Tauros Workspace Context]\n\
             selected_preview_map: {selected}\n\
             launch_target: {launch}\n\
             live_emulator: {live}\n\
             loaded_rom: {loaded} | disk_built_rom: {built} | rom_stale: {} | new_build_pending: {}\n",
            self.rom_stale, self.new_build_pending
        )
    }
}

/// Bounds and a bounded starting point for a preview destination. Walkability
/// remains game-specific and is not inferred from the rectangle's centre.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MapPreviewInfo {
    pub id: String,
    pub width: u16,
    pub height: u16,
    pub default_x: i16,
    pub default_y: i16,
}
impl MapPreviewInfo {
    fn from_map(map: &tauros_model::Map) -> Option<Self> {
        let tauros_model::MapGeometry::Grid { width, height, .. } = map.geometry else { return None };
        Some(Self { id: map.id.0.clone(), width, height,
            default_x: (width / 2) as i16, default_y: (height / 2) as i16 })
    }
    pub fn validate(&self, x: i16, y: i16) -> Result<(), String> {
        if x < 0 || y < 0 || x as u16 >= self.width || y as u16 >= self.height {
            return Err(format!("({x}, {y}) is outside {} ({}x{}); choose coordinates inside the map",
                self.id, self.width, self.height));
        }
        Ok(())
    }
}

/// What [`ProjectSession::build_and_boot`] did.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BuildAndBoot {
    pub rom_sha256: String,
    pub boot_frames: u32,
}

impl ProjectSession {
    /// Opens `project_dir` with `Gen3Connector` and an `Mgba` session carrying the
    /// Gen 3 boot hook and constants; an already built ROM and `.sym` are loaded.
    pub fn open(project_dir: &Path, toolchain: Option<Toolchain>, base_rom: Option<Vec<u8>>) -> Result<Self, String> {
        let connector = Gen3Connector::open(project_dir, toolchain.clone())
            .map_err(|e| format!("failed to open Gen 3 project {}: {e}", project_dir.display()))?;
        let connector: Arc<Mutex<Box<dyn Connector + Send>>> = Arc::new(Mutex::new(Box::new(connector)));
        let cached_layout = Arc::new(Mutex::new(None));
        let mgba = Mgba::gba().map_err(|e| format!("failed to initialize libmgba core: {e}"))?;
        let mut session = EmuSession::new(mgba)
            .with_constants(load_project_constants(project_dir))
            .with_boot_hook(Gen3BootHook::with_shared_layout(
                project_dir.to_path_buf(),
                Arc::clone(&connector),
                toolchain,
                Arc::clone(&cached_layout),
            ));
        if let Some(base) = base_rom {
            session = session.with_base_rom(base);
        }
        if project_dir.join("pokeemerald.gba").is_file() && project_dir.join("pokeemerald.sym").is_file() {
            reload_project_artifacts(project_dir, &mut session)?;
        }
        Ok(Self {
            dir: project_dir.to_path_buf(),
            connector,
            emulator: Arc::new(Mutex::new(session)),
            operation_gate: Arc::new(Mutex::new(())),
            cached_layout,
            last_build_sha256: Arc::new(Mutex::new(None)),
            last_boot: Arc::new(Mutex::new(None)),
        })
    }

    /// Seeds the cached struct layout when measured externally (e.g. during CLI launch).
    pub fn record_layout(&self, layout: GameLayout) {
        if let Ok(rev) = git_head_revision(&self.dir) {
            *self.cached_layout.lock().unwrap() = Some((rev, layout));
        }
    }

    /// Records a boot target performed at launch or via preview/build.
    pub fn record_boot(&self, map: &str, x: i16, y: i16, flags: &[String], rom_sha256: &str) {
        *self.last_boot.lock().unwrap() = Some(LastBootTarget {
            map: map.to_owned(),
            x,
            y,
            flags: flags.to_vec(),
            rom_sha256: rom_sha256.to_owned(),
        });
        if self.last_build_sha256.lock().unwrap().as_deref() == Some(rom_sha256) {
            *self.last_build_sha256.lock().unwrap() = None;
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The emulator session shared with the MCP server.
    pub fn emulator(&self) -> Arc<Mutex<EmuSession>> {
        Arc::clone(&self.emulator)
    }

    pub fn operation_gate(&self) -> Arc<Mutex<()>> {
        Arc::clone(&self.operation_gate)
    }

    /// An MCP server on this project's connector and emulator; a successful `build`
    /// reloads the new ROM and `.sym` into the shared session.
    pub fn mcp_server(&self) -> TaurosServer {
        let reload_dir = self.dir.clone();
        let build_sha = Arc::clone(&self.last_build_sha256);
        TaurosServer::with_shared_connector(Arc::clone(&self.connector))
            .with_operation_gate(Arc::clone(&self.operation_gate))
            .with_shared_emulator(self.emulator())
            .with_post_build_hook(move |session| {
                reload_project_artifacts(&reload_dir, session)?;
                *build_sha.lock().unwrap() = session.rom_sha256().map(str::to_owned);
                Ok(())
            })
    }

    /// The project's map ids, sorted.
    pub fn maps(&self) -> Result<Vec<String>, String> {
        let _operation = self.operation_gate.try_lock().map_err(|_| "another project operation is running")?;
        let snapshot = self.connector.lock().map_err(|_| "connector lock poisoned")?.load().map_err(|e| e.to_string())?;
        Ok(snapshot.maps.keys().map(|id| id.0.clone()).collect())
    }

    /// Map bounds used by the picker; does not build or touch the emulator.
    pub fn preview_maps(&self) -> Result<Vec<MapPreviewInfo>, String> {
        let _operation = self.operation_gate.try_lock().map_err(|_| "another project operation is running")?;
        let snapshot = self.connector.lock().map_err(|_| "connector lock poisoned")?.load().map_err(|e| e.to_string())?;
        Ok(snapshot.maps.values().filter_map(MapPreviewInfo::from_map).collect())
    }

    // Caller owns the project operation gate. Run before build or ROM reload.
    fn validate_preview_target(&self, map: &str, x: i16, y: i16) -> Result<(), String> {
        let snapshot = self.connector.lock().map_err(|_| "connector lock poisoned")?.load().map_err(|e| e.to_string())?;
        snapshot.maps.get(&MapId::from(map)).and_then(MapPreviewInfo::from_map)
            .ok_or_else(|| format!("unknown or unsupported preview map {map}"))?.validate(x, y)
    }

    /// Boot a fresh preview from the existing ROM/symbol build pair. Does not
    /// invoke the ROM builder or include pending source edits. The Gen 3 hook
    /// can measure/cache struct offsets on its first boot at a source revision.
    pub fn preview_built(&self, map: &str, x: i16, y: i16, flags: &[String]) -> Result<BuildAndBoot, String> {
        let _operation = self.operation_gate.try_lock().map_err(|_| "another project operation is running")?;
        self.validate_preview_target(map, x, y)?;
        let mut session = self.emulator.lock().map_err(|_| "emulator lock poisoned")?;
        reload_project_artifacts(&self.dir, &mut session)?;
        let rom_sha256 = session.rom_sha256().ok_or("no built ROM loaded")?.to_owned();
        let request = EmuBootRequest { kind: BootKind::BootTo, map: Some(map.to_owned()), x: Some(x), y: Some(y), flags: flags.to_vec(), frames: 0 };
        let booted = emu_boot(&mut session, request).map_err(|e| format!("{}: {}", e.kind, e.message))?;
        self.record_boot(map, x, y, flags, &rom_sha256);
        Ok(BuildAndBoot { rom_sha256, boot_frames: booted.frame_count })
    }

    /// Validates and builds the project (snapshot first, as every build), loads the
    /// new ROM and `.sym` into the shared session and boots it at `map` (`x`, `y`)
    /// with `flags` set.
    pub fn build_and_boot(&self, map: &str, x: i16, y: i16, flags: &[String]) -> Result<BuildAndBoot, String> {
        let _operation = self.operation_gate.try_lock().map_err(|_| "another project operation is running")?;
        self.validate_preview_target(map, x, y)?;
        let built = {
            let mut connector = self.connector.lock().map_err(|_| "connector lock poisoned")?;
            domain::build(connector.as_mut()).map_err(|e| format!("{}: {}", e.kind, e.message))?
        };
        let mut session = self.emulator.lock().map_err(|_| "emulator lock poisoned")?;
        reload_project_artifacts(&self.dir, &mut session)?;
        let request = EmuBootRequest { kind: BootKind::BootTo, map: Some(map.to_owned()), x: Some(x), y: Some(y), flags: flags.to_vec(), frames: 0 };
        let booted = emu_boot(&mut session, request).map_err(|e| format!("{}: {}", e.kind, e.message))?;
        self.record_boot(map, x, y, flags, &built.rom_sha256);
        Ok(BuildAndBoot { rom_sha256: built.rom_sha256, boot_frames: booted.frame_count })
    }

    /// Validates and builds the project without booting it (snapshot first, as every
    /// build). The running emulator keeps its ROM; the new build shows up as pending.
    pub fn build_only(&self) -> Result<domain::BuildToolResult, String> {
        let _operation = self.operation_gate.try_lock().map_err(|_| "another project operation is running")?;
        let mut connector = self.connector.lock().map_err(|_| "connector lock poisoned")?;
        domain::build(connector.as_mut()).map_err(|e| format!("{}: {}", e.kind, e.message))
    }

    /// Runs the project's headless test suite, optionally filtered by test name or file.
    pub fn run_tests(&self, filter: Option<String>) -> Result<domain::RunTestToolResult, String> {
        let _operation = self.operation_gate.try_lock().map_err(|_| "another project operation is running")?;
        let mut connector = self.connector.lock().map_err(|_| "connector lock poisoned")?;
        domain::run_test(connector.as_mut(), domain::RunTestRequest { filter })
            .map_err(|e| format!("{}: {}", e.kind, e.message))
    }

    /// Reads the live emulator location and build/selection state without running builds.
    pub fn agent_context(
        &self,
        selected_map: Option<String>,
        selected_x: Option<i16>,
        selected_y: Option<i16>,
        launch_target: Option<LastBootTarget>,
    ) -> AgentEmuContext {
        use sha2::{Digest, Sha256};
        let rom_path = self.dir.join("pokeemerald.gba");
        let built_rom_sha256 = fs::read(&rom_path)
            .ok()
            .map(|bytes| format!("{:x}", Sha256::digest(&bytes)));
        let (loaded_rom_sha256, live_status, live_map, live_group, live_num, live_x, live_y) =
            match self.emulator.try_lock() {
                Err(_) => (None, "emulator_busy".to_owned(), None, None, None, None, None),
                Ok(session) => {
                    let loaded_sha = session.rom_sha256().map(str::to_owned);
                    if !session.has_rom() {
                        (loaded_sha, "no_rom".to_owned(), None, None, None, None, None)
                    } else if built_rom_sha256.is_some() && loaded_sha != built_rom_sha256 {
                        (loaded_sha, "stale_rom".to_owned(), None, None, None, None, None)
                    } else {
                        let cached = self.cached_layout.lock().unwrap().clone();
                        let current_rev = git_head_revision(&self.dir).ok();
                        let layout = cached.and_then(|(rev, l)| {
                            (current_rev.as_deref() == Some(rev.as_str())).then_some(l)
                        });
                        let sb1_addr = session.symbols().get("gSaveBlock1Ptr");
                        match (layout, sb1_addr) {
                            (Some(layout), Some(sb1_ptr)) => {
                                match read_live_location(session.emulator(), sb1_ptr, layout) {
                                    Ok(Some(loc)) => {
                                        let map_name = map_numbers(&self.dir).ok().and_then(|maps| {
                                            maps.into_iter()
                                                .find(|(_, (g, n))| *g == loc.group && *n == loc.num)
                                                .map(|(name, _)| name)
                                        });
                                        (
                                            loaded_sha,
                                            "ok".to_owned(),
                                            map_name,
                                            Some(loc.group),
                                            Some(loc.num),
                                            Some(loc.x),
                                            Some(loc.y),
                                        )
                                    }
                                    Ok(None) => (
                                        loaded_sha,
                                        "title_or_uninitialized".to_owned(),
                                        None,
                                        None,
                                        None,
                                        None,
                                        None,
                                    ),
                                    Err(_) => (
                                        loaded_sha,
                                        "unavailable".to_owned(),
                                        None,
                                        None,
                                        None,
                                        None,
                                        None,
                                    ),
                                }
                            }
                            _ => (
                                loaded_sha,
                                "layout_unmeasured".to_owned(),
                                None,
                                None,
                                None,
                                None,
                                None,
                            ),
                        }
                    }
                }
            };
        let rom_stale = built_rom_sha256.is_some()
            && loaded_rom_sha256.is_some()
            && built_rom_sha256 != loaded_rom_sha256;
        let pending_sha = self.last_build_sha256.lock().unwrap().clone();
        let new_build_pending = pending_sha.is_some() && pending_sha == loaded_rom_sha256;
        let effective_launch = launch_target.or_else(|| self.last_boot.lock().unwrap().clone());
        AgentEmuContext {
            selected_map,
            selected_x,
            selected_y,
            launch_target: effective_launch,
            live_status,
            live_map,
            live_group,
            live_num,
            live_x,
            live_y,
            loaded_rom_sha256,
            built_rom_sha256,
            rom_stale,
            new_build_pending,
        }
    }
}

/// Wires a full Layer 1 [`TaurosServer`] for a Gen 3 project (see [`ProjectSession`]):
/// `Gen3Connector` domain tools, an `Mgba` session with the Gen 3 boot hook and an
/// optional unmodified `base_rom`, and a ROM/`.sym` reload after every successful build.
pub fn build_project_mcp_server(
    project_dir: &Path,
    toolchain: Option<Toolchain>,
    base_rom: Option<Vec<u8>>,
) -> Result<TaurosServer, String> {
    Ok(ProjectSession::open(project_dir, toolchain, base_rom)?.mcp_server())
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

#[cfg(test)]
#[path = "../tests/fixtures/synthetic_framebuffer.rs"]
mod preview_framebuffer;

#[cfg(test)]
mod preview_tests {
    use super::*;
    use serde_json::json;
    use tauros_model::{memory::MemoryConnector, ProjectSnapshot};
    type Calls = Arc<Mutex<Vec<(String, i16, i16, Vec<String>)>>>;
    struct SyntheticBoot(Calls);
    impl BootHook for SyntheticBoot {
        fn boot_to(&mut self, _: &mut dyn tauros_emu::Emulator, map: &str, x: i16, y: i16, flags: &[String]) -> Result<u32, String> {
            self.0.lock().unwrap().push((map.into(), x, y, flags.to_vec()));
            Ok(42)
        }
    }

    #[test]
    fn preview_bounds_reject_before_build_or_reload_and_preview_never_builds() {
        let snapshot: ProjectSnapshot = serde_json::from_value(json!({
            "game":{"family":"gen3","engine":"synthetic","capabilities":{
                "max_party_size":6,"max_moves":4,"min_level":1,"max_level":100,"held_items":true,"abilities":true}},
            "trainers":{},"maps":{"MAP_TEST_MART":{
                "id":"MAP_TEST_MART","name":"Test","geometry":{"kind":"grid","layout":"TEST","width":11,"height":8},
                "events":[],"connections":[],"properties":{}}},
            "scenes":{},"species":{},"flags":{}
        })).unwrap();
        let dir = std::env::temp_dir().join(format!("tauros-preview-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("pokeemerald.gba"), [1, 2, 3]).unwrap();
        fs::write(dir.join("pokeemerald.sym"), "02000000 g 00000004 Synthetic\n").unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let emu = preview_framebuffer::SyntheticFramebuffer(Arc::new(Mutex::new(preview_framebuffer::picture(false, false))));
        let emulator = EmuSession::new(emu).with_rom(vec![9]).unwrap().with_boot_hook(SyntheticBoot(Arc::clone(&calls)));
        let project = ProjectSession { dir: dir.clone(),
            connector: Arc::new(Mutex::new(Box::new(MemoryConnector::new(snapshot)))),
            emulator: Arc::new(Mutex::new(emulator)), operation_gate: Arc::new(Mutex::new(())),
            cached_layout: Arc::new(Mutex::new(None)),
            last_build_sha256: Arc::new(Mutex::new(None)),
            last_boot: Arc::new(Mutex::new(None)) };
        let ctx_before = project.agent_context(Some("MAP_TEST_MART".into()), Some(5), Some(4), None);
        assert_eq!(ctx_before.live_status, "stale_rom");
        assert!(ctx_before.rom_stale);
        assert!(!ctx_before.new_build_pending);
        let bounds = project.preview_maps().unwrap();
        assert_eq!((bounds[0].default_x, bounds[0].default_y), (5, 4));
        let original_hash = project.emulator.lock().unwrap().rom_sha256().unwrap().to_owned();
        // MemoryConnector::build always fails; the coordinate error must precede it.
        assert!(project.build_and_boot("MAP_TEST_MART", 15, 10, &[]).unwrap_err().contains("outside"));
        for (map, x, y) in [("MAP_TEST_MART", 11, 4), ("MAP_TEST_MART", 5, 8),
                            ("MAP_TEST_MART", -1, 0), ("MAP_MISSING", 0, 0)] {
            assert!(project.preview_built(map, x, y, &[]).is_err());
        }
        assert_eq!(project.emulator.lock().unwrap().rom_sha256(), Some(original_hash.as_str()));
        assert!(calls.lock().unwrap().is_empty());
        let report = project.preview_built("MAP_TEST_MART", 5, 4, &["FLAG_TEST".into()]).unwrap();
        assert_eq!(report.boot_frames, 42);
        assert_ne!(report.rom_sha256, original_hash);
        assert_eq!(calls.lock().unwrap().as_slice(), &[("MAP_TEST_MART".into(), 5, 4, vec!["FLAG_TEST".into()])]);
        let gate = project.operation_gate.lock().unwrap();
        assert!(project.preview_built("MAP_TEST_MART", 5, 4, &[]).unwrap_err().contains("operation"));
        drop(gate);
        assert_eq!(fs::read(dir.join("pokeemerald.gba")).unwrap(), [1, 2, 3]);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }
}
