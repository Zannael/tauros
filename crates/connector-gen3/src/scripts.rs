//! Scenes (G3-004): assembly scripts read losslessly, Poryscript scenes compiled by
//! the Poryscript CLI.
//!
//! - *Assembly.* A `scripts.inc` file is split at its global labels (`Label::` at
//!   the start of a line); each label up to the next one is a [`Scene`]. Joining the
//!   chunks gives the file back, and editing a scene replaces only its chunk.
//! - *Poryscript.* Tauros writes a scene as `<scene dir>/<Label>.pory`, compiles it
//!   with Poryscript (in the toolchain container) into `<Label>.inc` beside it and
//!   adds one `.include` line to `data/event_scripts.s`. The scene directory is
//!   `data/maps/<Map>/scenes` for a map's scene and `data/scripts/scenes` otherwise.

use std::collections::BTreeMap;

pub const EVENT_SCRIPTS: &str = "data/event_scripts.s";
pub const GLOBAL_SCENE_DIR: &str = "data/scripts/scenes";
pub const SHARED_SCRIPTS_DIR: &str = "data/scripts";

/// A script file split at global labels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptFile {
    pub chunks: Vec<Chunk>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    /// `None` for text before the first global label.
    pub label: Option<String>,
    pub text: String,
}

/// The label defined by a line such as `Route101_EventScript_Boy::` (comments after
/// the label are allowed).
pub fn global_label(line: &str) -> Option<&str> {
    let end = line.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(line.len());
    let label = &line[..end];
    let starts_ok = label.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    (starts_ok && line[end..].starts_with("::")).then_some(label)
}

pub fn is_identifier(text: &str) -> bool {
    text.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl ScriptFile {
    pub fn parse(text: &str) -> Self {
        let mut chunks: Vec<Chunk> = Vec::new();
        for line in text.split_inclusive('\n') {
            if let Some(label) = global_label(line) {
                chunks.push(Chunk { label: Some(label.to_owned()), text: String::new() });
            } else if chunks.is_empty() {
                chunks.push(Chunk { label: None, text: String::new() });
            }
            chunks.last_mut().unwrap().text.push_str(line);
        }
        Self { chunks }
    }

    pub fn render(&self) -> String {
        self.chunks.iter().map(|c| c.text.as_str()).collect()
    }

    /// Labels defined more than once (e.g. in both branches of an `.if`).
    pub fn duplicate_labels(&self) -> Vec<String> {
        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        for c in &self.chunks {
            if let Some(l) = &c.label {
                *seen.entry(l).or_default() += 1;
            }
        }
        seen.into_iter().filter(|(_, n)| *n > 1).map(|(l, _)| l.to_owned()).collect()
    }

    /// Replaces the chunk of `label`; returns false when the label is absent.
    pub fn replace(&mut self, label: &str, source: &str) -> bool {
        match self.chunks.iter_mut().find(|c| c.label.as_deref() == Some(label)) {
            Some(chunk) => {
                chunk.text = source.to_owned();
                true
            }
            None => false,
        }
    }

    /// Appends a new chunk, separated from the previous one by a blank line.
    pub fn append(&mut self, label: &str, source: &str) {
        if let Some(last) = self.chunks.last_mut() {
            if !last.text.ends_with('\n') {
                last.text.push('\n');
            }
            if !last.text.ends_with("\n\n") {
                last.text.push('\n');
            }
        }
        self.chunks.push(Chunk { label: Some(label.to_owned()), text: source.to_owned() });
    }

    /// Removes the chunk of `label`; returns false when the label is absent.
    pub fn remove(&mut self, label: &str) -> bool {
        let before = self.chunks.len();
        self.chunks.retain(|c| c.label.as_deref() != Some(label));
        before != self.chunks.len()
    }
}

/// Checks an assembly scene's source: it must define exactly `label` as its only
/// global label, on its first line, and end with a newline (so that reading the
/// file back yields the same source).
pub fn check_assembly(label: &str, source: &str) -> Result<(), String> {
    let first = source.lines().next().unwrap_or("");
    if global_label(first) != Some(label) {
        return Err(format!("assembly scene must start with '{label}::'"));
    }
    if let Some(other) = source.lines().skip(1).find_map(global_label) {
        return Err(format!("assembly scene {label} also defines global label {other}; use one scene per label"));
    }
    if !source.ends_with('\n') {
        return Err(format!("assembly scene {label} must end with a newline"));
    }
    Ok(())
}

/// Checks a Poryscript scene's source defines `script <label>` (any options).
pub fn check_poryscript(label: &str, source: &str) -> Result<(), String> {
    let defines = source.lines().any(|line| {
        let Some(rest) = line.trim_start().strip_prefix("script") else { return false };
        let rest = match rest.trim_start().strip_prefix('(') {
            Some(opts) => match opts.find(')') {
                Some(close) => &opts[close + 1..],
                None => return false,
            },
            None if rest.starts_with([' ', '\t']) => rest,
            None => return false,
        };
        let rest = rest.trim_start();
        rest.strip_prefix(label).is_some_and(|after| after.is_empty() || after.starts_with([' ', '\t', '{']))
    });
    if defines {
        Ok(())
    } else {
        Err(format!("Poryscript scene must define 'script {label}'"))
    }
}

/// Project-relative scene directory for a map directory name (or global).
pub fn scene_dir(map_dir: Option<&str>) -> String {
    match map_dir {
        Some(dir) => format!("data/maps/{dir}/scenes"),
        None => GLOBAL_SCENE_DIR.to_owned(),
    }
}

pub fn include_line(inc_path: &str) -> String {
    format!("\t.include \"{inc_path}\"\n")
}

/// Adds `.include "<inc_path>"` right after the include of `anchor` (the map's own
/// `scripts.inc`, so the scene is assembled under the same conditions as its map)
/// and the scene includes already following it; at the end of the file when there
/// is no anchor. No-op when already present.
pub fn add_include(event_scripts: &str, inc_path: &str, anchor: Option<&str>) -> String {
    let line = include_line(inc_path);
    if event_scripts.split_inclusive('\n').any(|l| l.trim() == line.trim()) {
        return event_scripts.to_owned();
    }
    let lines: Vec<&str> = event_scripts.split_inclusive('\n').collect();
    let scene_dir = inc_path.rsplit_once('/').map(|(d, _)| format!(".include \"{d}/")).unwrap_or_default();
    let at = anchor
        .and_then(|a| lines.iter().position(|l| l.trim() == include_line(a).trim()))
        .map(|mut i| {
            i += 1;
            while i < lines.len() && !scene_dir.is_empty() && lines[i].trim().starts_with(&scene_dir) {
                i += 1;
            }
            i
        })
        .unwrap_or(lines.len());
    let mut out: String = lines[..at].concat();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&line);
    out.push_str(&lines[at..].concat());
    out
}

pub fn remove_include(event_scripts: &str, inc_path: &str) -> String {
    let wanted = include_line(inc_path);
    event_scripts.split_inclusive('\n').filter(|l| l.trim() != wanted.trim()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const INC: &str = "Route101_MapScripts::\n\tmap_script 1, X\n\t.byte 0\n\nRoute101_OnTransition:\n\tend\n\nRoute101_EventScript_Boy:: @ comment\n\tmsgbox Route101_Text_Boy, MSGBOX_NPC\n\tend\n";

    #[test]
    fn split_is_lossless_and_local() {
        let mut file = ScriptFile::parse(INC);
        assert_eq!(file.render(), INC);
        let labels: Vec<_> = file.chunks.iter().filter_map(|c| c.label.clone()).collect();
        assert_eq!(labels, ["Route101_MapScripts", "Route101_EventScript_Boy"]);
        assert!(file.replace("Route101_EventScript_Boy", "Route101_EventScript_Boy::\n\tend\n"));
        assert!(file.render().starts_with(&INC[..INC.find("Route101_EventScript_Boy::").unwrap()]));
        file.append("Route101_EventScript_New", "Route101_EventScript_New::\n\tend\n");
        assert!(file.render().ends_with("\tend\n\nRoute101_EventScript_New::\n\tend\n"));
        assert!(file.remove("Route101_EventScript_New"));
    }

    #[test]
    fn sources_are_checked() {
        assert!(check_assembly("A", "A::\n\tend\n").is_ok());
        assert!(check_assembly("A", "B::\n\tend\n").is_err());
        assert!(check_assembly("A", "A::\n\tend\nB::\n\tend\n").is_err());
        assert!(check_assembly("A", "A::\n\tend").is_err());
        assert!(check_poryscript("A", "script A {\n  end\n}\n").is_ok());
        assert!(check_poryscript("A", "script(local) A {\n}\n").is_ok());
        assert!(check_poryscript("A", "script AB {\n}\n").is_err());
    }

    #[test]
    fn includes_go_after_their_map_scripts() {
        let s = "\t.include \"asm/macros.inc\"\n\t.include \"data/maps/A/scripts.inc\"\n.if IS_FRLG\n\t.include \"data/maps/B/scripts.inc\"\n.endif\n\t.include \"data/scripts/x.inc\"\n";
        let added = add_include(s, "data/maps/A/scenes/S.inc", Some("data/maps/A/scripts.inc"));
        assert_eq!(added, s.replace("A/scripts.inc\"\n", "A/scripts.inc\"\n\t.include \"data/maps/A/scenes/S.inc\"\n"));
        let second = add_include(&added, "data/maps/A/scenes/T.inc", Some("data/maps/A/scripts.inc"));
        assert!(second.contains("S.inc\"\n\t.include \"data/maps/A/scenes/T.inc\"\n.if"));
        assert_eq!(add_include(&added, "data/maps/A/scenes/S.inc", Some("data/maps/A/scripts.inc")), added);
        assert_eq!(remove_include(&added, "data/maps/A/scenes/S.inc"), s);
        assert!(add_include(s, "data/scripts/scenes/G.inc", None).ends_with("x.inc\"\n\t.include \"data/scripts/scenes/G.inc\"\n"));
    }
}
