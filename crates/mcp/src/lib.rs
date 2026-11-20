//! Layer 2: an embeddable rmcp server. Concrete connectors and emulators are
//! injected through `tauros-model` and `tauros-emu` traits (`MCP-002`/`MCP-003`);
//! no game files or credentials are read here. The stdio binary is only a
//! transport entry point into this same library.

pub mod domain;
pub mod emulator;

use std::sync::{Arc, Mutex};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use serde::Serialize;
use tauros_model::Connector;

use crate::domain::{
    AddSceneRequest, CreateTrainerRequest, DomainToolError, PlaceNpcRequest, RunTestRequest,
};
use crate::emulator::{
    EmuAssertRamRequest, EmuBootRequest, EmuPressInputRequest, EmuScreenshotRequest, EmuSession,
    EmuStateRequest, EmuToolError, VerificationSpec,
};

type SharedConnector = Arc<Mutex<Box<dyn Connector + Send>>>;
type SharedEmulator = Arc<Mutex<EmuSession>>;
type PostBuildHook = Arc<dyn Fn(&mut EmuSession) -> Result<(), String> + Send + Sync>;

#[derive(Clone)]
pub struct TaurosServer {
    tool_router: ToolRouter<Self>,
    connector: Option<SharedConnector>,
    emulator: Option<SharedEmulator>,
    post_build_hook: Option<PostBuildHook>,
    operation_gate: Option<Arc<Mutex<()>>>,
}

impl Default for TaurosServer {
    fn default() -> Self {
        Self {
            tool_router: Self::tool_router(),
            connector: None,
            emulator: None,
            post_build_hook: None,
            operation_gate: None,
        }
    }
}

impl TaurosServer {
    /// Creates a server with an injected game-project [`Connector`].
    pub fn with_connector<C: Connector + Send + 'static>(connector: C) -> Self {
        Self::with_shared_connector(Arc::new(Mutex::new(Box::new(connector))))
    }

    /// Creates a server sharing a [`Connector`] handle with the caller (e.g. Layer 1).
    pub fn with_shared_connector(connector: SharedConnector) -> Self {
        Self {
            tool_router: Self::tool_router(),
            connector: Some(connector),
            emulator: None,
            post_build_hook: None,
            operation_gate: None,
        }
    }

    /// Attaches an [`EmuSession`] to this server and returns it.
    pub fn with_emulator(mut self, session: EmuSession) -> Self {
        self.emulator = Some(Arc::new(Mutex::new(session)));
        self
    }

    /// Attaches a shared [`EmuSession`] handle to this server and returns it.
    pub fn with_shared_emulator(mut self, session: SharedEmulator) -> Self {
        self.emulator = Some(session);
        self
    }

    /// Registers a hook invoked on the attached [`EmuSession`] right after `build` succeeds
    /// (for example to reload the freshly built ROM and `.sym` table).
    pub fn with_post_build_hook<F>(mut self, hook: F) -> Self
    where
        F: Fn(&mut EmuSession) -> Result<(), String> + Send + Sync + 'static,
    {
        self.post_build_hook = Some(Arc::new(hook));
        self
    }

    /// Serializes native editing with domain and emulator operations.
    pub fn with_operation_gate(mut self, gate: Arc<Mutex<()>>) -> Self {
        self.operation_gate = Some(gate);
        self
    }

    pub fn operation_gate(&self) -> Option<Arc<Mutex<()>>> {
        self.operation_gate.clone()
    }

    fn lock_operation(&self) -> Result<Option<std::sync::MutexGuard<'_, ()>>, CallToolResult> {
        self.operation_gate.as_ref().map(|gate| gate.try_lock().map_err(|_| {
            tool_error_json(&serde_json::json!({
                "ok": false, "kind": "project_busy",
                "message": "another project operation is running; retry after it finishes"
            }))
        })).transpose()
    }

    fn with_connector_mut<T, F>(&self, op: F) -> CallToolResult
    where
        T: Serialize,
        F: FnOnce(&mut dyn Connector) -> Result<T, DomainToolError>,
    {
        let _operation = match self.lock_operation() {
            Ok(lock) => lock,
            Err(error) => return error,
        };
        let Some(shared) = &self.connector else {
            return tool_error_json(&DomainToolError::no_connector());
        };
        let mut guard = match shared.lock() {
            Ok(g) => g,
            Err(_) => {
                return tool_error_json(&DomainToolError {
                    ok: false,
                    kind: "internal_error".to_owned(),
                    message: "connector lock poisoned".to_owned(),
                    violations: Vec::new(),
                    diagnostics: Vec::new(),
                });
            }
        };
        match op(guard.as_mut()) {
            Ok(value) => tool_ok(&value),
            Err(err) => tool_error_json(&err),
        }
    }

    fn with_emulator_mut<T, F>(&self, op: F) -> CallToolResult
    where
        T: Serialize,
        F: FnOnce(&mut EmuSession) -> Result<T, EmuToolError>,
    {
        self.with_emulator_content(op, |value| tool_ok(&value))
    }

    fn with_emulator_content<T, F>(
        &self,
        op: F,
        render: impl FnOnce(T) -> CallToolResult,
    ) -> CallToolResult
    where
        F: FnOnce(&mut EmuSession) -> Result<T, EmuToolError>,
    {
        let _operation = match self.lock_operation() {
            Ok(lock) => lock,
            Err(error) => return error,
        };
        let Some(shared) = &self.emulator else {
            return tool_error_json(&EmuToolError::no_emulator());
        };
        let mut guard = match shared.lock() {
            Ok(g) => g,
            Err(_) => {
                return tool_error_json(&EmuToolError::new(
                    "internal_error",
                    "emulator lock poisoned",
                ));
            }
        };
        match op(&mut guard) {
            Ok(value) => render(value),
            Err(err) => tool_error_json(&err),
        }
    }
}

fn tool_ok<T: Serialize>(value: &T) -> CallToolResult {
    let text = serde_json::to_string(value).unwrap_or_else(|_| "{\"ok\":true}".to_owned());
    CallToolResult::success(vec![ContentBlock::text(text)])
}

fn tool_error_json<T: Serialize>(err: &T) -> CallToolResult {
    let text = serde_json::to_string(err).unwrap_or_else(|_| "{\"ok\":false}".to_owned());
    CallToolResult::error(vec![ContentBlock::text(text)])
}

#[tool_router]
impl TaurosServer {
    #[tool(
        description = "Check that the Tauros MCP server is reachable. Returns its version; reads no game files and changes nothing.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    fn tauros_status(&self) -> String {
        format!("Tauros {} ready", env!("CARGO_PKG_VERSION"))
    }

    #[tool(
        description = "Create or update a trainer and party through the common semantic model. Validates before writing and records a git snapshot with provenance.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    fn create_trainer(&self, Parameters(req): Parameters<CreateTrainerRequest>) -> CallToolResult {
        self.with_connector_mut(|connector| domain::create_trainer(connector, req))
    }

    #[tool(
        description = "Place an NPC event on an existing map through the common semantic model. Validates engine limits before writing and records a git snapshot with provenance.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false)
    )]
    fn place_npc(&self, Parameters(req): Parameters<PlaceNpcRequest>) -> CallToolResult {
        self.with_connector_mut(|connector| domain::place_npc(connector, req))
    }

    #[tool(
        description = "Add or update a Poryscript or assembly scene (optionally allocating flags atomically) through the common semantic model. Validates before writing and records a git snapshot with provenance.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    fn add_scene(&self, Parameters(req): Parameters<AddSceneRequest>) -> CallToolResult {
        self.with_connector_mut(|connector| domain::add_scene(connector, req))
    }

    #[tool(
        description = "Run pre-build validation and build the game project ROM through the connector.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    fn build(&self) -> CallToolResult {
        let _operation = match self.lock_operation() {
            Ok(lock) => lock,
            Err(error) => return error,
        };
        let Some(shared) = &self.connector else {
            return tool_error_json(&DomainToolError::no_connector());
        };
        let mut guard = match shared.lock() {
            Ok(g) => g,
            Err(_) => {
                return tool_error_json(&DomainToolError {
                    ok: false,
                    kind: "internal_error".to_owned(),
                    message: "connector lock poisoned".to_owned(),
                    violations: Vec::new(),
                    diagnostics: Vec::new(),
                });
            }
        };
        let response = match domain::build(guard.as_mut()) {
            Ok(r) => r,
            Err(err) => return tool_error_json(&err),
        };
        drop(guard);

        if let (Some(hook), Some(emu)) = (&self.post_build_hook, &self.emulator) {
            if let Ok(mut emu_guard) = emu.lock() {
                if let Err(err) = hook(&mut emu_guard) {
                    return tool_error_json(&DomainToolError {
                        ok: false,
                        kind: "post_build_reload_failed".to_owned(),
                        message: format!("build succeeded, but reloading emulator failed: {err}"),
                        violations: Vec::new(),
                        diagnostics: Vec::new(),
                    });
                }
            }
        }
        tool_ok(&response)
    }

    #[tool(
        description = "Run the game project's headless test suite through the connector, optionally filtered by test name prefix or file.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    fn run_test(&self, Parameters(req): Parameters<RunTestRequest>) -> CallToolResult {
        self.with_connector_mut(|connector| domain::run_test(connector, req))
    }

    #[tool(
        description = "Boot the attached emulator either from cold reset or straight to a target map, coordinates and flags (EMU-003).",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false)
    )]
    fn emu_boot(&self, Parameters(req): Parameters<EmuBootRequest>) -> CallToolResult {
        self.with_emulator_mut(|session| emulator::emu_boot(session, req))
    }

    #[tool(
        description = "Hold a set of console buttons for N frames and optionally release for additional frames.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false)
    )]
    fn emu_press_input(&self, Parameters(req): Parameters<EmuPressInputRequest>) -> CallToolResult {
        self.with_emulator_mut(|session| emulator::emu_press_input(session, req))
    }

    #[tool(
        description = "Capture the current emulator framebuffer as a native PNG image for visual inspection, with frame/hash metadata. Use before and after visual edits; a screenshot alone does not verify collision or interaction. include_base64=false returns metadata only.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    fn emu_screenshot(&self, Parameters(req): Parameters<EmuScreenshotRequest>) -> CallToolResult {
        self.with_emulator_content(|session| emulator::emu_screenshot(session, req), |mut screenshot| {
            // Base64 inside JSON text is not an image input. Keep pixels in the
            // MCP image block and metadata small; never duplicate the payload.
            let png = screenshot.png_base64.take();
            let mut result = tool_ok(&screenshot);
            if let Some(png) = png {
                result.content.push(ContentBlock::image(png, "image/png"));
            }
            result
        })
    }

    #[tool(
        description = "Save the current emulator state into a named slot bound to the loaded ROM hash.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    fn emu_save_state(&self, Parameters(req): Parameters<EmuStateRequest>) -> CallToolResult {
        self.with_emulator_mut(|session| emulator::emu_save_state(session, req))
    }

    #[tool(
        description = "Restore a previously saved emulator state from a named slot.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    fn emu_load_state(&self, Parameters(req): Parameters<EmuStateRequest>) -> CallToolResult {
        self.with_emulator_mut(|session| emulator::emu_load_state(session, req))
    }

    #[tool(
        description = "Assert game state directly from RAM by symbol name (with optional pointer deref for moving save blocks and flag-bit decoding).",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    fn emu_assert_ram(&self, Parameters(req): Parameters<EmuAssertRamRequest>) -> CallToolResult {
        self.with_emulator_mut(|session| emulator::emu_assert_ram(session, req))
    }

    #[tool(
        description = "Execute a VER-001 inert JSON verification spec with assertion-free driver sampling and mandatory counterfactuals, returning PROVEN, FAILED, INCONCLUSIVE or REJECTED.",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    fn run_verification(&self, Parameters(spec): Parameters<VerificationSpec>) -> CallToolResult {
        self.with_emulator_mut(|session| emulator::run_verification(session, spec))
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "tauros",
    instructions = "Tauros ROM hack editor. Exposes domain tools (create_trainer, place_npc, add_scene, build, run_test) over the semantic model and emulator/RAM verification tools (emu_boot, emu_press_input, emu_screenshot, emu_save_state, emu_load_state, emu_assert_ram, run_verification); no free-form file edits."
)]
impl ServerHandler for TaurosServer {}

/// Serve newline-delimited MCP JSON-RPC on stdin/stdout. Diagnostics belong on
/// stderr; stdout must remain exclusively protocol data. EOF closes the server.
pub async fn serve_stdio() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    serve_server_stdio(TaurosServer::default()).await
}

/// Serve a configured [`TaurosServer`] over stdin/stdout.
pub async fn serve_server_stdio(
    server: TaurosServer,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use rmcp::ServiceExt;
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

/// Serve a configured [`TaurosServer`] over an arbitrary async reader/writer
/// pair (used for in-process MCP protocol tests).
pub async fn serve_io<R, W>(
    server: TaurosServer,
    reader: R,
    writer: W,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use rmcp::ServiceExt;
    let service = server.serve((reader, writer)).await?;
    service.waiting().await?;
    Ok(())
}
