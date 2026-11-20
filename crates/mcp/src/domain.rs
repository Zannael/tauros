//! Domain MCP tool implementations over `tauros-model`'s [`Connector`] trait
//! (`MCP-002`). All mutations go through [`ChangeSet`] validation and the
//! connector's snapshot-guarded [`Connector::apply`]; no free-form file edits
//! are exposed.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tauros_model::{
    Change, ChangeSet, Connector, ConnectorError, Diagnostic, Event, Extensions, FlagId, ItemId,
    MapId, MoveId, Party, PartyMember, Position, Provenance, Scene, ScriptLabel, ScriptLanguage,
    Severity, SpeciesId, TestCaseOutput, Trainer, TrainerId, Value as ModelValue, Violation,
};

/// Provenance label supplied by an MCP caller (`DEC-003`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProvenanceInput {
    /// AI was never used for this change.
    Human,
    /// Human and AI both took part.
    AiAssisted,
    /// Conceived and built entirely by AI (default for MCP tool calls).
    #[default]
    AiGenerated,
}

impl From<ProvenanceInput> for Provenance {
    fn from(value: ProvenanceInput) -> Self {
        match value {
            ProvenanceInput::Human => Provenance::Human,
            ProvenanceInput::AiAssisted => Provenance::AiAssisted,
            ProvenanceInput::AiGenerated => Provenance::AiGenerated,
        }
    }
}

/// One Pokémon in a trainer's party.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PartyMemberInput {
    /// Species constant (e.g. `SPECIES_GEODUDE`).
    pub species: String,
    /// Level (1..=100).
    pub level: u8,
    /// Optional held item constant (e.g. `ITEM_ORAN_BERRY`).
    #[serde(default)]
    pub held_item: Option<String>,
    /// Optional explicit moveset (empty uses the game's level-up moves).
    #[serde(default)]
    pub moves: Vec<String>,
    /// Optional nickname.
    #[serde(default)]
    pub nickname: Option<String>,
    /// Optional ability name or constant.
    #[serde(default)]
    pub ability: Option<String>,
    /// Optional game-specific extension fields (strings, ints, bools, or lists).
    #[serde(default)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

/// Arguments for the `create_trainer` domain tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CreateTrainerRequest {
    /// Trainer constant (e.g. `TRAINER_DIRECTOR` or an existing constant to update).
    pub id: String,
    /// In-game trainer display name.
    pub name: String,
    /// Optional trainer class (e.g. `Youngster`, `Leader`).
    #[serde(default)]
    pub class: Option<String>,
    /// Bag items carried by the trainer (e.g. `ITEM_POTION`).
    #[serde(default)]
    pub items: Vec<String>,
    /// Whether this trainer fights a double battle.
    #[serde(default)]
    pub double_battle: bool,
    /// AI behaviour flags.
    #[serde(default)]
    pub ai_flags: Vec<String>,
    /// Trainer's party (1..=6 Pokémon).
    pub party: Vec<PartyMemberInput>,
    /// Optional game-specific extension fields (e.g. `Pic`, `Gender`, `Music` on Gen 3).
    #[serde(default)]
    pub extensions: BTreeMap<String, serde_json::Value>,
    /// Optional one-line commit summary.
    #[serde(default)]
    pub summary: Option<String>,
    /// Optional provenance label (defaults to `ai-generated`).
    #[serde(default)]
    pub provenance: ProvenanceInput,
}

/// Arguments for the `place_npc` domain tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlaceNpcRequest {
    /// Target map constant (e.g. `MAP_ROUTE101`).
    pub map: String,
    /// Tile X coordinate on the map.
    pub x: i32,
    /// Tile Y coordinate on the map.
    pub y: i32,
    /// Optional elevation layer (defaults to `3` on Gen 3).
    #[serde(default = "default_elevation")]
    pub elevation: Option<u8>,
    /// Object-event sprite graphics constant (e.g. `OBJ_EVENT_GFX_BOY_1`).
    pub graphics: String,
    /// Optional movement type constant (e.g. `MOVEMENT_TYPE_FACE_DOWN`).
    #[serde(default)]
    pub movement: Option<String>,
    /// Optional script label triggered on interaction.
    #[serde(default)]
    pub script: Option<String>,
    /// Optional visibility flag constant (hides the NPC when set).
    #[serde(default)]
    pub visibility_flag: Option<String>,
    /// Optional local identifier constant.
    #[serde(default)]
    pub local_id: Option<String>,
    /// Optional game-specific extension fields.
    #[serde(default)]
    pub extensions: BTreeMap<String, serde_json::Value>,
    /// Optional one-line commit summary.
    #[serde(default)]
    pub summary: Option<String>,
    /// Optional provenance label (defaults to `ai-generated`).
    #[serde(default)]
    pub provenance: ProvenanceInput,
}

fn default_elevation() -> Option<u8> {
    Some(3)
}

/// Scripting language for `add_scene`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScriptLanguageInput {
    /// Poryscript `.pory` source (compiled by Poryscript CLI on Gen 3).
    #[default]
    Poryscript,
    /// Decomp assembly `.inc` / `.asm` source.
    Assembly,
}

impl From<ScriptLanguageInput> for ScriptLanguage {
    fn from(value: ScriptLanguageInput) -> Self {
        match value {
            ScriptLanguageInput::Poryscript => ScriptLanguage::Poryscript,
            ScriptLanguageInput::Assembly => ScriptLanguage::Assembly,
        }
    }
}

/// Arguments for the `add_scene` domain tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AddSceneRequest {
    /// Global script label (e.g. `Route101_EventScript_Director`).
    pub label: String,
    /// Optional map constant to attach the scene to (e.g. `MAP_ROUTE101`).
    #[serde(default)]
    pub map: Option<String>,
    /// Script source language (defaults to `poryscript`).
    #[serde(default)]
    pub language: ScriptLanguageInput,
    /// Full script source text.
    pub source: String,
    /// Optional new flag constants to allocate atomically in the same change set.
    #[serde(default)]
    pub allocate_flags: Vec<String>,
    /// Optional one-line commit summary.
    #[serde(default)]
    pub summary: Option<String>,
    /// Optional provenance label (defaults to `ai-generated`).
    #[serde(default)]
    pub provenance: ProvenanceInput,
}

/// Arguments for the `run_test` domain tool.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunTestRequest {
    /// Optional test name prefix or `.c` file path filter (e.g. `"Trainer Party"`).
    #[serde(default)]
    pub filter: Option<String>,
}

/// Structured success result returned by mutating domain tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationResult {
    pub ok: bool,
    pub commit: Option<String>,
    pub touched: Vec<String>,
    pub warnings: Vec<Violation>,
}

/// Bounds that keep build and test results small enough for every agent
/// runtime to read inline (MCP-006). A full Emerald build reports hundreds of
/// warnings; an unbounded result was spilled to a file the agent could not open.
pub const MAX_ERRORS: usize = 50;
pub const MAX_WARNINGS: usize = 20;
pub const MAX_CASES: usize = 50;
pub const MAX_MESSAGE_CHARS: usize = 300;
pub const MAX_CASE_OUTPUT_CHARS: usize = 1000;

/// Totals for the diagnostics a tool saw, including those it did not list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticCounts {
    pub errors: usize,
    pub warnings: usize,
    /// Diagnostics counted but not listed (beyond the bounds or duplicates).
    pub omitted: usize,
}

/// Shortens `text` to at most `max` characters, marking the cut.
fn shorten(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => format!("{}… [{} more chars]", &text[..cut], text[cut..].chars().count()),
        None => text.to_owned(),
    }
}

/// Keeps errors (up to [`MAX_ERRORS`]) and the first [`MAX_WARNINGS`] distinct
/// warnings, in that order, with shortened messages and full counts.
pub fn compact_diagnostics(all: Vec<Diagnostic>) -> (Vec<Diagnostic>, DiagnosticCounts) {
    let mut counts = DiagnosticCounts::default();
    let (mut errors, mut warnings) = (Vec::new(), Vec::new());
    let mut seen = std::collections::BTreeSet::new();
    for mut diagnostic in all {
        let is_error = diagnostic.severity == Severity::Error;
        if is_error { counts.errors += 1 } else { counts.warnings += 1 }
        let (list, max) = if is_error { (&mut errors, MAX_ERRORS) } else { (&mut warnings, MAX_WARNINGS) };
        diagnostic.message = shorten(&diagnostic.message, MAX_MESSAGE_CHARS);
        if list.len() < max && (is_error || seen.insert(diagnostic.message.clone())) {
            list.push(diagnostic);
        } else {
            counts.omitted += 1;
        }
    }
    errors.extend(warnings);
    (errors, counts)
}

/// Structured success result returned by the `build` domain tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildToolResult {
    pub ok: bool,
    pub rom: String,
    pub symbols: Option<String>,
    pub rom_sha256: String,
    pub counts: DiagnosticCounts,
    /// Errors first, then distinct warnings, both bounded (see [`compact_diagnostics`]).
    pub diagnostics: Vec<Diagnostic>,
}

/// Structured result returned by the `run_test` domain tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunTestToolResult {
    pub ok: bool,
    /// Failing cases first, at most [`MAX_CASES`]; only failures keep (shortened) output.
    pub cases: Vec<TestCaseOutput>,
    /// Cases run but not listed.
    pub omitted_cases: usize,
    pub totals: BTreeMap<String, u32>,
    pub counts: DiagnosticCounts,
    pub diagnostics: Vec<Diagnostic>,
}

/// Structured error payload returned by domain tools on rejection or failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainToolError {
    pub ok: bool,
    pub kind: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub violations: Vec<Violation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

impl DomainToolError {
    pub fn no_connector() -> Self {
        Self {
            ok: false,
            kind: "no_connector".to_owned(),
            message: "no game project connector is attached to this MCP server".to_owned(),
            violations: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn from_connector_error(error: ConnectorError) -> Self {
        match error {
            ConnectorError::Rejected(violations) => {
                let errors: Vec<Violation> = violations
                    .into_iter()
                    .filter(|v| v.severity == Severity::Error)
                    .collect();
                let message = ConnectorError::Rejected(errors.clone()).to_string();
                Self {
                    ok: false,
                    kind: "validation_rejected".to_owned(),
                    message,
                    violations: errors,
                    diagnostics: Vec::new(),
                }
            }
            ConnectorError::Build(diagnostics) => {
                let (diagnostics, counts) = compact_diagnostics(diagnostics);
                let message = format!(
                    "build failed: {} errors, {} warnings ({} not listed)",
                    counts.errors, counts.warnings, counts.omitted
                );
                Self {
                    ok: false,
                    kind: "build_failed".to_owned(),
                    message,
                    violations: Vec::new(),
                    diagnostics,
                }
            }
            ConnectorError::Snapshot(message) => Self {
                ok: false,
                kind: "snapshot_failed".to_owned(),
                message,
                violations: Vec::new(),
                diagnostics: Vec::new(),
            },
            ConnectorError::NotFound(message) => Self {
                ok: false,
                kind: "not_found".to_owned(),
                message,
                violations: Vec::new(),
                diagnostics: Vec::new(),
            },
            ConnectorError::Format { file, line, message } => Self {
                ok: false,
                kind: "format_error".to_owned(),
                message: ConnectorError::Format { file, line, message }.to_string(),
                violations: Vec::new(),
                diagnostics: Vec::new(),
            },
            ConnectorError::Io(message) => Self {
                ok: false,
                kind: "io_error".to_owned(),
                message,
                violations: Vec::new(),
                diagnostics: Vec::new(),
            },
        }
    }
}

fn convert_json_value(value: serde_json::Value) -> Result<ModelValue, DomainToolError> {
    match value {
        serde_json::Value::Bool(b) => Ok(ModelValue::Bool(b)),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(ModelValue::Int)
            .ok_or_else(|| DomainToolError {
                ok: false,
                kind: "invalid_argument".to_owned(),
                message: format!("extension number {n} must be an integer"),
                violations: Vec::new(),
                diagnostics: Vec::new(),
            }),
        serde_json::Value::String(s) => Ok(ModelValue::Text(s)),
        serde_json::Value::Array(items) => items
            .into_iter()
            .map(convert_json_value)
            .collect::<Result<Vec<_>, _>>()
            .map(ModelValue::List),
        other => Err(DomainToolError {
            ok: false,
            kind: "invalid_argument".to_owned(),
            message: format!("unsupported extension value: {other}"),
            violations: Vec::new(),
            diagnostics: Vec::new(),
        }),
    }
}

fn convert_extensions(
    raw: BTreeMap<String, serde_json::Value>,
) -> Result<Extensions, DomainToolError> {
    let mut out = Extensions::new();
    for (k, v) in raw {
        out.insert(k, convert_json_value(v)?);
    }
    Ok(out)
}

fn apply_validated(
    connector: &mut dyn Connector,
    changes: &ChangeSet,
) -> Result<MutationResult, DomainToolError> {
    let violations = connector.validate(changes);
    if violations.iter().any(|v| v.severity == Severity::Error) {
        return Err(DomainToolError::from_connector_error(
            ConnectorError::Rejected(violations),
        ));
    }
    let warnings: Vec<Violation> = violations
        .into_iter()
        .filter(|v| v.severity == Severity::Warning && v.change.is_some())
        .collect();
    let applied = connector
        .apply(changes)
        .map_err(DomainToolError::from_connector_error)?;
    Ok(MutationResult {
        ok: true,
        commit: applied.commit,
        touched: applied.touched,
        warnings,
    })
}

/// Creates or updates a trainer through the common semantic model.
pub fn create_trainer(
    connector: &mut dyn Connector,
    req: CreateTrainerRequest,
) -> Result<MutationResult, DomainToolError> {
    let summary = req
        .summary
        .unwrap_or_else(|| format!("create_trainer {}", req.id));
    let mut members = Vec::with_capacity(req.party.len());
    for m in req.party {
        members.push(PartyMember {
            species: SpeciesId(m.species),
            level: m.level,
            held_item: m.held_item.map(ItemId),
            moves: m.moves.into_iter().map(MoveId).collect(),
            nickname: m.nickname,
            ability: m.ability,
            extensions: convert_extensions(m.extensions)?,
        });
    }
    let trainer = Trainer {
        id: TrainerId(req.id),
        name: req.name,
        class: req.class,
        items: req.items.into_iter().map(ItemId).collect(),
        double_battle: req.double_battle,
        ai_flags: req.ai_flags,
        party: Party { members },
        extensions: convert_extensions(req.extensions)?,
    };
    let changes =
        ChangeSet::new(summary, req.provenance.into()).with(Change::UpsertTrainer { trainer });
    apply_validated(connector, &changes)
}

/// Places an NPC event on an existing map through the common semantic model.
pub fn place_npc(
    connector: &mut dyn Connector,
    req: PlaceNpcRequest,
) -> Result<MutationResult, DomainToolError> {
    let summary = req
        .summary
        .unwrap_or_else(|| format!("place_npc on {} at ({}, {})", req.map, req.x, req.y));
    let event = Event::Npc {
        local_id: req.local_id,
        graphics: req.graphics,
        position: Position {
            x: req.x,
            y: req.y,
            elevation: req.elevation,
        },
        movement: req.movement,
        script: req.script.map(ScriptLabel),
        visibility_flag: req.visibility_flag.map(FlagId),
        extensions: convert_extensions(req.extensions)?,
    };
    let changes = ChangeSet::new(summary, req.provenance.into()).with(Change::AddEvent {
        map: MapId(req.map),
        event,
    });
    apply_validated(connector, &changes)
}

/// Adds or updates a scripted scene (and optionally allocates new flags) through
/// the common semantic model.
pub fn add_scene(
    connector: &mut dyn Connector,
    req: AddSceneRequest,
) -> Result<MutationResult, DomainToolError> {
    let summary = req
        .summary
        .unwrap_or_else(|| format!("add_scene {}", req.label));
    let mut changes = ChangeSet::new(summary, req.provenance.into());
    for flag in req.allocate_flags {
        changes = changes.with(Change::AllocateFlag { id: FlagId(flag) });
    }
    changes = changes.with(Change::UpsertScene {
        scene: Scene {
            label: ScriptLabel(req.label),
            map: req.map.map(MapId),
            language: req.language.into(),
            source: req.source,
        },
    });
    apply_validated(connector, &changes)
}

/// Builds the project ROM through the connector.
pub fn build(connector: &mut dyn Connector) -> Result<BuildToolResult, DomainToolError> {
    let out = connector
        .build()
        .map_err(DomainToolError::from_connector_error)?;
    let (diagnostics, counts) = compact_diagnostics(out.diagnostics);
    Ok(BuildToolResult {
        ok: true,
        rom: out.rom.to_string_lossy().into_owned(),
        symbols: out.symbols.map(|p| p.to_string_lossy().into_owned()),
        rom_sha256: out.rom_sha256,
        counts,
        diagnostics,
    })
}

/// Runs the project's headless test suite through the connector.
pub fn run_test(
    connector: &mut dyn Connector,
    req: RunTestRequest,
) -> Result<RunTestToolResult, DomainToolError> {
    let out = connector
        .run_tests(req.filter.as_deref())
        .map_err(DomainToolError::from_connector_error)?;
    let (diagnostics, counts) = compact_diagnostics(out.diagnostics);
    let mut cases = out.cases;
    // Stable: failures first, each group in run order.
    cases.sort_by_key(|case| case.passed);
    let omitted_cases = cases.len().saturating_sub(MAX_CASES);
    cases.truncate(MAX_CASES);
    for case in &mut cases {
        // Only failures need their output; passing output is noise for agents.
        case.output = if case.passed { String::new() } else { shorten(&case.output, MAX_CASE_OUTPUT_CHARS) };
    }
    Ok(RunTestToolResult {
        ok: out.success,
        cases,
        omitted_cases,
        totals: out.totals,
        counts,
        diagnostics,
    })
}
