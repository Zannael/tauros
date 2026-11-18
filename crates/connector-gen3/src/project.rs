//! [`Gen3Connector`]: the [`Connector`] for a pokeemerald-expansion project.
//!
//! The project files stay the source of truth. `load` reads trainers
//! (`src/data/trainers.party`), maps (`data/maps/*/map.json` with
//! `data/layouts/layouts.json`), scenes (map and shared `scripts.inc` files and
//! Tauros' Poryscript scenes), species and flags. `apply` validates, computes every
//! file edit in memory, and only then writes them in one [`GitGuard::write`], so a
//! change set is all-or-nothing, snapshotted and recorded with its provenance.

use crate::flags::{self, FlagDef, FLAGS_HEADER, VALUE_HEADERS};
use crate::maps::{Layouts, MapFile, WarpIds, LAYOUTS_FILE, MAPS_DIR, WARP_IDS_HEADER};
use crate::names::{self, Kind, Vocabulary};
use crate::party::{self, PartyFile};
use crate::scripts::{self, ScriptFile, EVENT_SCRIPTS, SHARED_SCRIPTS_DIR};
use crate::snapshot::GitGuard;
use crate::toolchain::{Level, Toolchain};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use tauros_model::validate;
use tauros_model::{
    Applied, BuildOutput, Capabilities, Change, ChangeSet, Connector, ConnectorError, Diagnostic, Event, EventKind, Flag,
    FlagId, GameFamily, GameInfo, Map, MapGeometry, MapId, ProjectSnapshot, Scene, ScriptLabel, ScriptLanguage, Species,
    SpeciesId, Trainer, Value, Violation,
};

pub const TRAINERS_FILE: &str = "src/data/trainers.party";
const OPPONENTS_HEADER: &str = "include/constants/opponents.h";
const EXPANSION_HEADER: &str = "include/constants/expansion.h";
/// Headers the generic capabilities are read from.
const CAPABILITY_HEADERS: [&str; 3] = ["include/constants/global.h", "include/constants/pokemon.h", "include/data.h"];
/// Directories scanned to decide which flags are in use, with the extensions read.
const USAGE_ROOTS: [(&str, &[&str]); 3] =
    [("data", &["inc", "s", "pory", "json"]), ("src", &["c", "h"]), ("include", &["h"])];
/// Flag definitions are not uses.
const FLAG_DEFINITION_FILES: [&str; 2] = [FLAGS_HEADER, "include/constants/flags_frlg.h"];
const BACKGROUND_TYPES: [&str; 3] = ["sign", "hidden_item", "secret_base"];

pub struct Gen3Connector {
    root: PathBuf,
    guard: GitGuard,
    toolchain: Option<Toolchain>,
}

/// Where a scene's source lives.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SceneSource {
    /// A chunk of an assembly file; `duplicate` when the label is defined twice
    /// (e.g. in both branches of an `.if`), which makes it read-only.
    Assembly { file: String, duplicate: bool },
    /// `<dir>/<label>.pory`, compiled to `<dir>/<label>.inc`.
    Poryscript { dir: String },
}

/// Everything `load` reads, kept for validation and lossless writing.
#[derive(Clone)]
struct State {
    vocab: Vocabulary,
    party: PartyFile,
    layouts: Layouts,
    /// Map id to (directory name, file, model).
    maps: BTreeMap<MapId, (String, MapFile, Map)>,
    scripts: BTreeMap<String, ScriptFile>,
    scenes: BTreeMap<ScriptLabel, (SceneSource, Scene)>,
    event_scripts: String,
    flags_text: String,
    value_headers: Vec<String>,
    flags: Vec<FlagDef>,
    /// Every `FLAG_*` defined in any branch of flags.h (allocation must avoid all).
    all_flag_names: BTreeSet<String>,
    used_flags: BTreeSet<String>,
    trainer_constants: BTreeSet<String>,
    /// `MAP_*` enum members of maps.h that are not maps (`MAP_DYNAMIC`, `MAP_UNDEFINED`).
    pseudo_maps: Vec<MapId>,
    snapshot: ProjectSnapshot,
}

fn io_err(path: &str, e: impl std::fmt::Display) -> ConnectorError {
    ConnectorError::Io(format!("{path}: {e}"))
}

fn format_err(file: &str, line: Option<u32>, message: impl Into<String>) -> ConnectorError {
    ConnectorError::Format { file: file.to_owned(), line, message: message.into() }
}

impl Gen3Connector {
    /// Opens a project and takes it over for writing (git snapshots, push disabled).
    /// Without a toolchain, `build` and Poryscript scenes are unavailable.
    pub fn open(root: impl Into<PathBuf>, toolchain: Option<Toolchain>) -> Result<Self, ConnectorError> {
        let root = root.into();
        if !root.join(TRAINERS_FILE).is_file() || !root.join(LAYOUTS_FILE).is_file() {
            return Err(ConnectorError::NotFound(format!("{} is not a pokeemerald-expansion project", root.display())));
        }
        let guard = GitGuard::take_over(&root).map_err(|e| ConnectorError::Snapshot(e.to_string()))?;
        Ok(Self { root, guard, toolchain })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn guard(&self) -> &GitGuard {
        &self.guard
    }

    pub fn toolchain(&self) -> Option<&Toolchain> {
        self.toolchain.as_ref()
    }

    fn read(&self, rel: &str) -> Result<String, ConnectorError> {
        fs::read_to_string(self.root.join(rel)).map_err(|e| io_err(rel, e))
    }

    fn list(&self, dir: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.root.join(dir))
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    /// Project-relative files `load` reads (sorted): what [`Connector::fingerprint`] hashes.
    fn source_files(&self) -> Vec<String> {
        let mut files: BTreeSet<String> = BTreeSet::new();
        for f in [TRAINERS_FILE, LAYOUTS_FILE, WARP_IDS_HEADER, EVENT_SCRIPTS, FLAGS_HEADER, OPPONENTS_HEADER, EXPANSION_HEADER] {
            files.insert(f.to_owned());
        }
        files.extend(VALUE_HEADERS.iter().map(|s| s.to_string()));
        files.extend(CAPABILITY_HEADERS.iter().map(|s| s.to_string()));
        files.extend(names::source_files(&self.root));
        for (dir, exts) in USAGE_ROOTS {
            walk(&self.root, dir, &mut |rel| {
                if exts.iter().any(|e| rel.ends_with(&format!(".{e}"))) {
                    files.insert(rel.to_owned());
                }
            });
        }
        files.retain(|f| self.root.join(f).is_file());
        files.into_iter().collect()
    }

    fn capabilities(&self) -> Capabilities {
        let mut macros = BTreeMap::new();
        for header in CAPABILITY_HEADERS {
            if let Ok(text) = self.read(header) {
                for (name, _, body) in flags::active_defines(&text, &BTreeMap::new()) {
                    macros.entry(name).or_insert(body);
                }
            }
        }
        let get = |name: &str, default: i64| flags::evaluate(name, &macros).unwrap_or(default) as u8;
        Capabilities {
            max_party_size: get("PARTY_SIZE", 6),
            max_moves: get("MAX_MON_MOVES", 4),
            min_level: get("MIN_LEVEL", 1),
            max_level: get("MAX_LEVEL", 100),
            held_items: true,
            abilities: true,
        }
    }

    fn engine_name(&self) -> String {
        let text = self.read(EXPANSION_HEADER).unwrap_or_default();
        let macros: BTreeMap<String, String> =
            flags::active_defines(&text, &BTreeMap::new()).into_iter().map(|(n, _, b)| (n, b)).collect();
        let part = |n: &str| flags::evaluate(n, &macros);
        match (part("EXPANSION_VERSION_MAJOR"), part("EXPANSION_VERSION_MINOR"), part("EXPANSION_VERSION_PATCH")) {
            (Some(a), Some(b), Some(c)) => format!("pokeemerald-expansion {a}.{b}.{c}"),
            _ => "pokeemerald-expansion (version unknown)".into(),
        }
    }

    fn state(&self) -> Result<State, ConnectorError> {
        let mut vocab = Vocabulary::read(&self.root).map_err(ConnectorError::Io)?;
        let party_text = self.read(TRAINERS_FILE)?;
        let party = PartyFile::parse(&party_text).map_err(|e| format_err(TRAINERS_FILE, Some(e.line), e.message))?;
        party.observe_names(&mut vocab).map_err(|e| format_err(TRAINERS_FILE, Some(e.line), e.message))?;
        let trainers = party.trainers(&vocab).map_err(|e| format_err(TRAINERS_FILE, Some(e.line), e.message))?;

        let layouts = Layouts::parse(&self.read(LAYOUTS_FILE)?).map_err(|e| format_err(LAYOUTS_FILE, None, e.0))?;
        let maps_header = self.read(WARP_IDS_HEADER).unwrap_or_default();
        let warp_ids = WarpIds::parse(&maps_header);
        let mut maps = BTreeMap::new();
        for dir in self.list(MAPS_DIR) {
            let rel = format!("{MAPS_DIR}/{dir}/map.json");
            if !self.root.join(&rel).is_file() {
                continue;
            }
            let file = MapFile::parse(&self.read(&rel)?).map_err(|e| format_err(&rel, None, e.0))?;
            let map = file.to_model(&layouts, &warp_ids).map_err(|e| format_err(&rel, None, e.0))?;
            maps.insert(map.id.clone(), (dir, file, map));
        }

        let mut scripts = BTreeMap::new();
        let mut scenes: BTreeMap<ScriptLabel, (SceneSource, Scene)> = BTreeMap::new();
        let mut script_files: Vec<(String, Option<MapId>)> = Vec::new();
        for (id, (dir, _, _)) in &maps {
            script_files.push((format!("{MAPS_DIR}/{dir}/scripts.inc"), Some(id.clone())));
        }
        for name in self.list(SHARED_SCRIPTS_DIR).into_iter().filter(|n| n.ends_with(".inc")) {
            script_files.push((format!("{SHARED_SCRIPTS_DIR}/{name}"), None));
        }
        for (rel, map) in script_files {
            if !self.root.join(&rel).is_file() {
                continue;
            }
            let file = ScriptFile::parse(&self.read(&rel)?);
            let duplicates = file.duplicate_labels();
            for chunk in &file.chunks {
                let Some(label) = &chunk.label else { continue };
                let key = ScriptLabel(label.clone());
                let duplicate = duplicates.contains(label) || scenes.contains_key(&key);
                if let Some((SceneSource::Assembly { duplicate: d, .. }, _)) = scenes.get_mut(&key) {
                    *d = true;
                    continue;
                }
                let scene = Scene { label: key.clone(), map: map.clone(), language: ScriptLanguage::Assembly, source: chunk.text.clone() };
                scenes.insert(key, (SceneSource::Assembly { file: rel.clone(), duplicate }, scene));
            }
            scripts.insert(rel, file);
        }
        let mut scene_dirs: Vec<(String, Option<MapId>)> =
            maps.iter().map(|(id, (dir, _, _))| (scripts::scene_dir(Some(dir)), Some(id.clone()))).collect();
        scene_dirs.push((scripts::scene_dir(None), None));
        for (dir, map) in scene_dirs {
            for name in self.list(&dir).into_iter().filter(|n| n.ends_with(".pory")) {
                let label = ScriptLabel(name.trim_end_matches(".pory").to_owned());
                let rel = format!("{dir}/{name}");
                let source = self.read(&rel)?;
                if scenes.contains_key(&label) {
                    return Err(format_err(&rel, None, format!("scene {label} is defined twice")));
                }
                let scene = Scene { label: label.clone(), map: map.clone(), language: ScriptLanguage::Poryscript, source };
                scenes.insert(label, (SceneSource::Poryscript { dir: dir.clone() }, scene));
            }
        }

        let flags_text = self.read(FLAGS_HEADER)?;
        let value_headers: Vec<String> = VALUE_HEADERS.iter().map(|h| self.read(h).unwrap_or_default()).collect();
        let header_refs: Vec<&str> = value_headers.iter().map(String::as_str).collect();
        let flag_defs = flags::read_flags(&flags_text, &header_refs);
        let mut all_flag_names = BTreeSet::new();
        for line in flags_text.lines() {
            let mut words = line.split_whitespace();
            if let (Some("#define"), Some(name)) = (words.next(), words.next()) {
                if name.starts_with("FLAG_") {
                    all_flag_names.insert(name.to_owned());
                }
            }
        }
        let mut used_flags = BTreeSet::new();
        for (dir, exts) in USAGE_ROOTS {
            let mut files = Vec::new();
            walk(&self.root, dir, &mut |rel| {
                if exts.iter().any(|e| rel.ends_with(&format!(".{e}"))) && !FLAG_DEFINITION_FILES.contains(&rel) {
                    files.push(rel.to_owned());
                }
            });
            for rel in files {
                if let Ok(bytes) = fs::read(self.root.join(&rel)) {
                    flags::flag_tokens(&String::from_utf8_lossy(&bytes), &mut used_flags);
                }
            }
        }
        let trainer_constants: BTreeSet<String> = self
            .read(OPPONENTS_HEADER)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let mut w = l.split_whitespace();
                match (w.next(), w.next()) {
                    (Some("#define"), Some(name)) if name.starts_with("TRAINER_") => Some(name.to_owned()),
                    _ => None,
                }
            })
            .collect();

        let pseudo_maps: Vec<MapId> = maps_header
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                let name = l.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next()?;
                (name.starts_with("MAP_") && l[name.len()..].trim_start().starts_with('=')).then(|| MapId(name.to_owned()))
            })
            .filter(|id| !maps.contains_key(id))
            .collect();
        let species = vocab
            .entries(Kind::Species)
            .into_iter()
            .map(|(id, name)| (SpeciesId(id.clone()), Species { id: SpeciesId(id), name, national_dex: None }))
            .collect();
        let snapshot = ProjectSnapshot {
            game: GameInfo { family: GameFamily::Gen3, engine: self.engine_name(), capabilities: self.capabilities() },
            trainers: trainers.into_iter().map(|t| (t.id.clone(), t)).collect(),
            maps: maps.iter().map(|(id, (_, _, m))| (id.clone(), m.clone())).collect(),
            scenes: scenes.iter().map(|(l, (_, s))| (l.clone(), s.clone())).collect(),
            species,
            flags: flag_defs
                .iter()
                .map(|f| {
                    let id = FlagId(f.name.clone());
                    let flag = Flag { id: id.clone(), value: f.value.map(|v| v as u32), in_use: used_flags.contains(&f.name) };
                    (id, flag)
                })
                .collect(),
        };
        Ok(State {
            vocab,
            party,
            layouts,
            maps,
            scripts,
            scenes,
            event_scripts: self.read(EVENT_SCRIPTS)?,
            flags_text,
            value_headers,
            flags: flag_defs,
            all_flag_names,
            used_flags,
            trainer_constants,
            pseudo_maps,
            snapshot,
        })
    }

    fn check(&self, state: &State, changes: &ChangeSet) -> Vec<Violation> {
        // Engine pseudo-maps (MAP_DYNAMIC) are valid warp destinations.
        let mut view = state.snapshot.clone();
        for id in &state.pseudo_maps {
            view.maps.entry(id.clone()).or_insert_with(|| Map {
                id: id.clone(),
                name: id.0.clone(),
                geometry: MapGeometry::External { format: "engine".into(), reference: WARP_IDS_HEADER.into() },
                events: Vec::new(),
                connections: Vec::new(),
                properties: Default::default(),
            });
        }
        let mut found = validate::check(&view, changes);
        let mut allocations = 0usize;
        let mut new_scenes: BTreeSet<ScriptLabel> = BTreeSet::new();
        for (index, change) in changes.changes.iter().enumerate() {
            let i = Some(index);
            match change {
                Change::UpsertTrainer { trainer } => found.extend(check_trainer(state, trainer, i)),
                Change::RemoveTrainer { .. } => {}
                Change::UpsertMap { map } => found.extend(check_map(state, map, i)),
                Change::AddEvent { map, event } => {
                    if let Some((_, file, m)) = state.maps.get(map) {
                        if file.shares_events() {
                            found.push(Violation::error(i, "map.shared_events", format!("{map} uses another map's events")));
                        }
                        found.extend(check_event(state, m, event, i));
                    }
                }
                Change::RemoveEvent { .. } => {}
                Change::UpsertScene { scene } => {
                    found.extend(check_scene(self, state, scene, i, &new_scenes));
                    if !state.scenes.contains_key(&scene.label) {
                        new_scenes.insert(scene.label.clone());
                    }
                }
                Change::RemoveScene { label } => {
                    if let Some((SceneSource::Assembly { duplicate: true, .. }, _)) = state.scenes.get(label) {
                        found.push(Violation::error(i, "scene.duplicate", format!("{label} is defined twice; edit it by hand")));
                    }
                }
                Change::AllocateFlag { id } => {
                    allocations += 1;
                    found.extend(check_flag(state, &id.0, allocations, i));
                }
            }
        }
        found
    }

    /// Computes every file edit for `changes`, in memory.
    fn plan(&self, state: &State, changes: &ChangeSet) -> Result<BTreeMap<String, Option<Vec<u8>>>, ConnectorError> {
        let mut work = state.clone();
        let mut new_files: BTreeMap<String, Option<Vec<u8>>> = BTreeMap::new();
        for change in &changes.changes {
            match change {
                Change::UpsertTrainer { trainer } => work
                    .party
                    .upsert(trainer, &work.vocab)
                    .map_err(|e| format_err(TRAINERS_FILE, Some(e.line), e.message))?,
                Change::RemoveTrainer { id } => {
                    if !work.party.remove(&id.0) {
                        return Err(ConnectorError::NotFound(id.to_string()));
                    }
                }
                Change::UpsertMap { map } => set_map(&mut work, map.clone())?,
                Change::AddEvent { map, event } => {
                    let mut m = work.maps.get(map).ok_or_else(|| ConnectorError::NotFound(map.to_string()))?.2.clone();
                    m.events.push(event.clone());
                    set_map(&mut work, m)?;
                }
                Change::RemoveEvent { map, kind, index } => {
                    let mut m = work.maps.get(map).ok_or_else(|| ConnectorError::NotFound(map.to_string()))?.2.clone();
                    let at = m
                        .events
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| e.kind() == *kind)
                        .nth(*index)
                        .map(|(at, _)| at)
                        .ok_or_else(|| ConnectorError::NotFound(format!("{map} {kind:?} #{index}")))?;
                    m.events.remove(at);
                    set_map(&mut work, m)?;
                }
                Change::UpsertScene { scene } => self.plan_scene(&mut work, scene, &mut new_files)?,
                Change::RemoveScene { label } => {
                    let (source, _) = work.scenes.remove(label).ok_or_else(|| ConnectorError::NotFound(label.to_string()))?;
                    match source {
                        SceneSource::Assembly { file, .. } => {
                            work.scripts.get_mut(&file).expect("loaded").remove(&label.0);
                        }
                        SceneSource::Poryscript { dir } => {
                            new_files.insert(format!("{dir}/{label}.pory"), None);
                            new_files.insert(format!("{dir}/{label}.inc"), None);
                            work.event_scripts = scripts::remove_include(&work.event_scripts, &format!("{dir}/{label}.inc"));
                        }
                    }
                }
                Change::AllocateFlag { id } => {
                    let header_refs: Vec<&str> = work.value_headers.iter().map(String::as_str).collect();
                    let defs = flags::read_flags(&work.flags_text, &header_refs);
                    let (start, end) = event_flag_range(&work);
                    let pick = flags::allocation_candidate(&defs, &work.used_flags, start, end)
                        .ok_or_else(|| format_err(FLAGS_HEADER, None, "no free FLAG_UNUSED_* placeholder left"))?;
                    let mut lines: Vec<String> = work.flags_text.split_inclusive('\n').map(str::to_owned).collect();
                    lines[pick.line] = flags::rename_line(&lines[pick.line], &pick.name, &id.0);
                    work.flags_text = lines.concat();
                    work.used_flags.insert(id.0.clone());
                }
            }
        }
        // Collect the files whose content differs from what was loaded.
        let mut out = new_files;
        let mut put = |rel: &str, before: &str, after: String| {
            if before != after {
                out.insert(rel.to_owned(), Some(after.into_bytes()));
            }
        };
        put(TRAINERS_FILE, &state.party.emit(), work.party.emit());
        for (id, (dir, file, _)) in &work.maps {
            let before = state.maps.get(id).map(|(_, f, _)| f.render()).unwrap_or_default();
            put(&format!("{MAPS_DIR}/{dir}/map.json"), &before, file.render());
        }
        for (rel, file) in &work.scripts {
            let before = state.scripts.get(rel).map(ScriptFile::render).unwrap_or_default();
            put(rel, &before, file.render());
        }
        put(EVENT_SCRIPTS, &state.event_scripts, work.event_scripts.clone());
        put(FLAGS_HEADER, &state.flags_text, work.flags_text.clone());
        Ok(out)
    }

    fn plan_scene(&self, work: &mut State, scene: &Scene, files: &mut BTreeMap<String, Option<Vec<u8>>>) -> Result<(), ConnectorError> {
        let existing = work.scenes.get(&scene.label).map(|(s, _)| s.clone());
        match (&scene.language, existing) {
            (ScriptLanguage::Assembly, Some(SceneSource::Assembly { file, .. })) => {
                work.scripts.get_mut(&file).expect("loaded").replace(&scene.label.0, &scene.source);
            }
            (ScriptLanguage::Assembly, None) => {
                let map = scene.map.as_ref().ok_or_else(|| format_err(EVENT_SCRIPTS, None, "a new assembly scene needs a map"))?;
                let dir = work.maps.get(map).map(|(d, _, _)| d.clone()).ok_or_else(|| ConnectorError::NotFound(map.to_string()))?;
                let rel = format!("{MAPS_DIR}/{dir}/scripts.inc");
                work.scripts.entry(rel.clone()).or_insert_with(|| ScriptFile { chunks: Vec::new() }).append(&scene.label.0, &scene.source);
                work.scenes.insert(scene.label.clone(), (SceneSource::Assembly { file: rel, duplicate: false }, scene.clone()));
            }
            (ScriptLanguage::Poryscript, existing) => {
                let map_dir = scene.map.as_ref().and_then(|m| work.maps.get(m)).map(|(d, _, _)| d.clone());
                let anchor = map_dir.as_ref().map(|d| format!("{MAPS_DIR}/{d}/scripts.inc"));
                let dir = match existing {
                    Some(SceneSource::Poryscript { dir }) => dir,
                    Some(SceneSource::Assembly { .. }) => {
                        return Err(format_err(EVENT_SCRIPTS, None, format!("{} is an assembly scene", scene.label)))
                    }
                    None => scripts::scene_dir(map_dir.as_deref()),
                };
                let toolchain = self.toolchain.as_ref().ok_or_else(|| ConnectorError::Build(vec![Diagnostic {
                    file: None,
                    line: None,
                    message: "no toolchain configured to compile Poryscript".into(),
                }]))?;
                let compiled = toolchain.compile_poryscript(&scene.source).map_err(|message| {
                    ConnectorError::Build(vec![Diagnostic { file: Some(format!("{dir}/{}.pory", scene.label)), line: None, message }])
                })?;
                let inc = format!("{dir}/{}.inc", scene.label);
                files.insert(format!("{dir}/{}.pory", scene.label), Some(scene.source.clone().into_bytes()));
                files.insert(inc.clone(), Some(compiled.into_bytes()));
                work.event_scripts = scripts::add_include(&work.event_scripts, &inc, anchor.as_deref());
                work.scenes.insert(scene.label.clone(), (SceneSource::Poryscript { dir }, scene.clone()));
            }
            (ScriptLanguage::Assembly, Some(SceneSource::Poryscript { .. })) | (ScriptLanguage::Other(_), _) => {
                return Err(format_err(EVENT_SCRIPTS, None, format!("cannot write {} as {:?}", scene.label, scene.language)))
            }
        }
        Ok(())
    }

    /// Builds the ROM and symbols (incrementally) and returns structured results.
    pub fn build_report(&self) -> Result<crate::toolchain::BuildReport, ConnectorError> {
        let toolchain = self.toolchain.as_ref().ok_or_else(|| ConnectorError::Build(vec![Diagnostic {
            file: None,
            line: None,
            message: "no toolchain configured".into(),
        }]))?;
        toolchain.build_rom(&self.root).map_err(ConnectorError::Io)
    }
}

/// Event flags live above the temporary flags and below the trainer flags.
fn event_flag_range(state: &State) -> (i64, i64) {
    let header_refs: Vec<&str> = state.value_headers.iter().map(String::as_str).collect();
    let mut macros: BTreeMap<String, String> = BTreeMap::new();
    for h in header_refs.iter().copied().chain([state.flags_text.as_str()]) {
        for (name, _, body) in flags::active_defines(h, &[("IS_FRLG".to_owned(), 0), ("TESTING".to_owned(), 0)].into()) {
            macros.entry(name).or_insert(body);
        }
    }
    let start = flags::evaluate("TEMP_FLAGS_END", &macros).map(|v| v + 1).unwrap_or(0x20);
    let end = flags::evaluate("TRAINER_FLAGS_START", &macros).unwrap_or(0x500);
    (start, end)
}

/// Events grouped by kind in file order (each kind is its own JSON array).
fn normalized(mut map: Map) -> Map {
    let order = |k: EventKind| match k {
        EventKind::Npc => 0,
        EventKind::Warp => 1,
        EventKind::Trigger => 2,
        EventKind::Background => 3,
    };
    map.events.sort_by_key(|e| order(e.kind()));
    map
}

fn set_map(work: &mut State, map: Map) -> Result<(), ConnectorError> {
    let map = normalized(map);
    let (dir, file, old) = work.maps.get_mut(&map.id).ok_or_else(|| ConnectorError::NotFound(map.id.to_string()))?;
    let rel = format!("{MAPS_DIR}/{dir}/map.json");
    file.apply(old, &map).map_err(|e| format_err(&rel, None, e.0))?;
    *old = map;
    Ok(())
}

fn walk(root: &Path, dir: &str, visit: &mut dyn FnMut(&str)) {
    let Ok(entries) = fs::read_dir(root.join(dir)) else { return };
    let mut names: Vec<(String, bool)> = entries
        .filter_map(|e| e.ok())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.file_type().map(|t| t.is_dir()).unwrap_or(false)))
        .collect();
    names.sort();
    for (name, is_dir) in names {
        let rel = format!("{dir}/{name}");
        if is_dir {
            walk(root, &rel, visit);
        } else {
            visit(&rel);
        }
    }
}

fn no_newline(text: &str) -> bool {
    !text.contains('\n') && !text.contains('\r')
}

fn check_trainer(state: &State, trainer: &Trainer, i: Option<usize>) -> Vec<Violation> {
    let mut found = Vec::new();
    let id = &trainer.id.0;
    let exists = state.party.blocks.iter().any(|b| b.id == *id);
    if !exists && !state.trainer_constants.contains(id) {
        found.push(Violation::error(
            i,
            "trainer.constant",
            format!("{id} is not defined in {OPPONENTS_HEADER}; creating trainer constants is not supported yet"),
        ));
    }
    if !exists && !trainer.extensions.contains_key("Pic") {
        found.push(Violation::error(i, "trainer.pic", format!("{id}: a new trainer needs a 'Pic' extension")));
    }
    if exists && state.snapshot.trainers.get(&trainer.id).is_some_and(|t| t.extensions.contains_key("Pic")) && !trainer.extensions.contains_key("Pic") {
        found.push(Violation::error(i, "trainer.pic", format!("{id}: 'Pic' cannot be removed")));
    }
    let texts = std::iter::once(&trainer.name).chain(trainer.class.iter());
    if texts.into_iter().any(|t| !no_newline(t)) {
        found.push(Violation::error(i, "trainer.text", format!("{id}: names cannot contain line breaks")));
    }
    let mut check_ext = |ext: &tauros_model::Extensions, member: bool, who: &str| {
        for (key, value) in ext {
            if party::reserved_extension(key, member) || !no_newline(key) || key.contains(':') {
                found.push(Violation::error(i, "trainer.extension", format!("{who}: extension key '{key}' is not allowed")));
            }
            if !no_newline(&party::value_text(value)) {
                found.push(Violation::error(i, "trainer.extension", format!("{who}: '{key}' cannot contain line breaks")));
            }
        }
    };
    check_ext(&trainer.extensions, false, id);
    for (slot, m) in trainer.party.members.iter().enumerate() {
        check_ext(&m.extensions, true, &format!("{id} slot {}", slot + 1));
    }
    let mut unknown = |kind: Kind, constant: &str, what: &str| {
        if !state.vocab.is_defined(kind, constant) {
            found.push(Violation::error(i, &format!("{what}.exists"), format!("{id}: unknown {what} {constant}")));
        }
    };
    for item in &trainer.items {
        unknown(Kind::Item, &item.0, "item");
    }
    for flag in &trainer.ai_flags {
        unknown(Kind::AiFlag, flag, "ai_flag");
    }
    for m in &trainer.party.members {
        if let Some(item) = &m.held_item {
            unknown(Kind::Item, &item.0, "item");
        }
        for mv in &m.moves {
            unknown(Kind::Move, &mv.0, "move");
        }
        if let Some(a) = &m.ability {
            unknown(Kind::Ability, a, "ability");
        }
    }
    if trainer.items.len() > 4 {
        found.push(Violation::error(i, "trainer.items", format!("{id}: at most 4 items (MAX_TRAINER_ITEMS)")));
    }
    for m in &trainer.party.members {
        if m.nickname.as_ref().is_some_and(|n| n.chars().count() > 12 || n.contains(['(', ')', '@']) || !no_newline(n)) {
            found.push(Violation::error(i, "member.nickname", format!("{id}: nickname too long or with '(', ')', '@'")));
        }
    }
    if exists && state.party.has_comments(id) && state.snapshot.trainers.get(&trainer.id) != Some(trainer) {
        found.push(Violation::error(i, "trainer.comments", format!("{id} contains comments; edit it by hand")));
    }
    found
}

fn check_map(state: &State, map: &Map, i: Option<usize>) -> Vec<Violation> {
    let mut found = Vec::new();
    let Some((_, file, current)) = state.maps.get(&map.id) else {
        return vec![Violation::error(i, "map.create", format!("{}: creating maps is not supported yet", map.id))];
    };
    match &map.geometry {
        MapGeometry::Grid { layout, width, height } => match state.layouts.get(&layout.0) {
            Some(l) if l.width == *width && l.height == *height => {}
            Some(l) => found.push(Violation::error(
                i,
                "map.layout_size",
                format!("{}: {} is {}x{}; resizing layouts is not supported yet", map.id, layout, l.width, l.height),
            )),
            None => found.push(Violation::error(i, "map.layout", format!("{}: unknown layout {layout}", map.id))),
        },
        MapGeometry::External { .. } => found.push(Violation::error(i, "map.geometry", "Gen 3 maps are grids")),
    }
    if file.shares_events() && map.events != current.events {
        found.push(Violation::error(i, "map.shared_events", format!("{} uses another map's events", map.id)));
    }
    for c in &map.connections {
        if !state.maps.contains_key(&c.map) {
            found.push(Violation::error(i, "connection.map", format!("{}: connection to unknown map {}", map.id, c.map)));
        }
    }
    for (key, value) in &map.properties {
        if ["id", "name", "layout", "connections", "object_events", "warp_events", "coord_events", "bg_events"].contains(&key.as_str()) {
            found.push(Violation::error(i, "map.property", format!("{}: '{key}' is not a property", map.id)));
        }
        if matches!(value, Value::List(_)) {
            found.push(Violation::error(i, "map.property", format!("{}: '{key}' must be a scalar", map.id)));
        }
    }
    for event in &map.events {
        found.extend(check_event(state, map, event, i));
    }
    found
}

fn check_event(_state: &State, map: &Map, event: &Event, i: Option<usize>) -> Vec<Violation> {
    let mut found = Vec::new();
    match event {
        Event::Npc { graphics, .. } if graphics.is_empty() => {
            found.push(Violation::error(i, "event.graphics", format!("{}: NPC without graphics", map.id)))
        }
        Event::Trigger { script, extensions, .. } => {
            let weather = matches!(extensions.get("type"), Some(Value::Text(t)) if t == "weather");
            if script.0.is_empty() && !weather {
                found.push(Violation::error(i, "event.script", format!("{}: trigger without script", map.id)));
            }
        }
        Event::Background { background, .. } if !BACKGROUND_TYPES.contains(&background.as_str()) => found.push(Violation::error(
            i,
            "event.background",
            format!("{}: background type must be one of {BACKGROUND_TYPES:?}", map.id),
        )),
        _ => {}
    }
    found
}

fn check_scene(connector: &Gen3Connector, state: &State, scene: &Scene, i: Option<usize>, new_scenes: &BTreeSet<ScriptLabel>) -> Vec<Violation> {
    let mut found = Vec::new();
    let label = &scene.label.0;
    if !scripts::is_identifier(label) {
        found.push(Violation::error(i, "scene.label", format!("'{label}' is not a valid label")));
    }
    let existing = state.scenes.get(&scene.label);
    match &scene.language {
        ScriptLanguage::Assembly => {
            if let Err(e) = scripts::check_assembly(label, &scene.source) {
                found.push(Violation::error(i, "scene.source", e));
            }
            if matches!(existing, Some((SceneSource::Poryscript { .. }, _))) {
                found.push(Violation::error(i, "scene.language", format!("{label} is a Poryscript scene")));
            }
            if existing.is_none() && !new_scenes.contains(&scene.label) && scene.map.is_none() {
                found.push(Violation::error(i, "scene.map", format!("new assembly scene {label} needs a map")));
            }
        }
        ScriptLanguage::Poryscript => {
            if let Err(e) = scripts::check_poryscript(label, &scene.source) {
                found.push(Violation::error(i, "scene.source", e));
            }
            if matches!(existing, Some((SceneSource::Assembly { .. }, _))) {
                found.push(Violation::error(i, "scene.language", format!("{label} is an assembly scene")));
            }
            if connector.toolchain.is_none() {
                found.push(Violation::error(i, "scene.compiler", "Poryscript scenes need the toolchain"));
            }
        }
        ScriptLanguage::Other(name) => {
            found.push(Violation::error(i, "scene.language", format!("{name} scenes are not supported by Gen 3")))
        }
    }
    if let Some((source, current)) = existing {
        if current.map != scene.map {
            found.push(Violation::error(i, "scene.map", format!("{label} belongs to {:?}; moving scenes is not supported", current.map)));
        }
        if matches!(source, SceneSource::Assembly { duplicate: true, .. }) {
            found.push(Violation::error(i, "scene.duplicate", format!("{label} is defined twice; edit it by hand")));
        }
    }
    found
}

fn check_flag(state: &State, id: &str, allocations: usize, i: Option<usize>) -> Vec<Violation> {
    let mut found = Vec::new();
    if !id.starts_with("FLAG_") || !scripts::is_identifier(id) {
        found.push(Violation::error(i, "flag.name", format!("'{id}' must be a FLAG_ identifier")));
    }
    if state.all_flag_names.contains(id) || state.used_flags.contains(id) {
        found.push(Violation::error(i, "flag.unique", format!("{id} is already defined or referenced")));
    }
    let (start, end) = event_flag_range(state);
    let mut free = 0;
    let mut defs = state.flags.clone();
    while let Some(pick) = flags::allocation_candidate(&defs, &state.used_flags, start, end) {
        let name = pick.name.clone();
        defs.retain(|d| d.name != name);
        free += 1;
        if free >= allocations {
            break;
        }
    }
    if free < allocations {
        found.push(Violation::error(i, "flag.space", "no free FLAG_UNUSED_* placeholder left"));
    }
    found
}

impl Connector for Gen3Connector {
    fn info(&self) -> GameInfo {
        GameInfo { family: GameFamily::Gen3, engine: self.engine_name(), capabilities: self.capabilities() }
    }

    fn load(&self) -> Result<ProjectSnapshot, ConnectorError> {
        Ok(self.state()?.snapshot)
    }

    fn validate(&self, changes: &ChangeSet) -> Vec<Violation> {
        match self.state() {
            Ok(state) => self.check(&state, changes),
            Err(e) => vec![Violation::error(None, "project.load", e.to_string())],
        }
    }

    fn apply(&mut self, changes: &ChangeSet) -> Result<Applied, ConnectorError> {
        let state = self.state()?;
        let violations = self.check(&state, changes);
        if violations.iter().any(|v| v.severity == tauros_model::Severity::Error) {
            return Err(ConnectorError::Rejected(violations));
        }
        let files = self.plan(&state, changes)?;
        if files.is_empty() {
            return Ok(Applied { commit: None, touched: Vec::new() });
        }
        let receipt = self
            .guard
            .write(&changes.summary, changes.provenance, |root| -> Result<(), std::io::Error> {
                for (rel, content) in &files {
                    let path = root.join(rel);
                    match content {
                        Some(bytes) => {
                            if let Some(parent) = path.parent() {
                                fs::create_dir_all(parent)?;
                            }
                            fs::write(&path, bytes)?;
                        }
                        None => {
                            if path.exists() {
                                fs::remove_file(&path)?;
                            }
                        }
                    }
                }
                Ok(())
            })
            .map_err(|e| ConnectorError::Snapshot(e.to_string()))?;
        Ok(Applied { commit: receipt.commit, touched: files.into_keys().collect() })
    }

    fn build(&mut self) -> Result<BuildOutput, ConnectorError> {
        let report = self.build_report()?;
        let diagnostics: Vec<Diagnostic> = report
            .diagnostics
            .iter()
            .map(|d| Diagnostic {
                file: d.file.clone(),
                line: d.line,
                message: format!("{}: {}", if d.level == Level::Error { "error" } else { "warning" }, d.message),
            })
            .collect();
        if !report.success {
            return Err(ConnectorError::Build(diagnostics));
        }
        let rom = self.root.join("pokeemerald.gba");
        let bytes = fs::read(&rom).map_err(|e| io_err("pokeemerald.gba", e))?;
        let symbols = self.root.join("pokeemerald.sym");
        Ok(BuildOutput {
            rom,
            symbols: symbols.is_file().then_some(symbols),
            rom_sha256: Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect(),
            diagnostics,
        })
    }

    fn fingerprint(&self) -> Result<String, ConnectorError> {
        let mut hasher = Sha256::new();
        for rel in self.source_files() {
            let bytes = fs::read(self.root.join(&rel)).map_err(|e| io_err(&rel, e))?;
            hasher.update(rel.as_bytes());
            hasher.update([0]);
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(&bytes);
        }
        Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }
}
