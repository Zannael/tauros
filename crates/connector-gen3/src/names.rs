//! Display names and the game's constants (G3-002).
//!
//! `trainers.party` names things the way Showdown does (`Geodude @ Oran Berry`);
//! the model uses constants (`SPECIES_GEODUDE`, `ITEM_ORAN_BERRY`). The forward
//! mapping reimplements the expansion's own `tools/trainerproc` (`fprint_constant`,
//! `fprint_species`), so a name resolves exactly as the build resolves it. The
//! defined constants and the in-game names are read from the project's headers and
//! data tables; nothing is invented. Credit: pokeemerald-expansion's trainerproc.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Species,
    Item,
    Move,
    Ability,
    AiFlag,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Species, Kind::Item, Kind::Move, Kind::Ability, Kind::AiFlag];

    pub fn prefix(self) -> &'static str {
        match self {
            Kind::Species => "SPECIES",
            Kind::Item => "ITEM",
            Kind::Move => "MOVE",
            Kind::Ability => "ABILITY",
            Kind::AiFlag => "AI_FLAG",
        }
    }

    /// Header defining the constants, and the data tables holding display names.
    fn sources(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Kind::Species => ("include/constants/species.h", &[SPECIES_INFO_DIR]),
            Kind::Item => ("include/constants/items.h", &["src/data/items.h"]),
            Kind::Move => ("include/constants/moves.h", &["src/data/moves_info.h"]),
            Kind::Ability => ("include/constants/abilities.h", &["src/data/abilities.h"]),
            Kind::AiFlag => ("include/constants/battle_ai.h", &[]),
        }
    }
}

const SPECIES_INFO_DIR: &str = "src/data/pokemon/species_info";
/// trainerproc's source, whose species tables [`SpeciesRules`] reads.
pub const TRAINERPROC_SOURCE: &str = "tools/trainerproc/main.c";

/// Project-relative files the vocabulary is read from (for fingerprints).
pub fn source_files(root: &Path) -> Vec<String> {
    let mut files = vec![TRAINERPROC_SOURCE.to_owned()];
    for kind in Kind::ALL {
        let (header, tables) = kind.sources();
        files.push(header.to_owned());
        for table in tables {
            if *table == SPECIES_INFO_DIR {
                files.extend(species_info_files(root));
            } else {
                files.push((*table).to_owned());
            }
        }
    }
    files
}

fn species_info_files(root: &Path) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(root.join(SPECIES_INFO_DIR))
        .map(|dir| {
            dir.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".h"))
                .map(|n| format!("{SPECIES_INFO_DIR}/{n}"))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

/// `fprint_constant`: optional prefix, letters upper-cased, digits kept, `'` dropped,
/// anything else becomes `_`; empty means `NONE`.
pub fn to_constant(prefix: &str, text: &str) -> String {
    let mut out = String::new();
    if !is_constant(text, prefix) {
        out.push_str(prefix);
        out.push('_');
    }
    if text.is_empty() {
        out.push_str("NONE");
        return out;
    }
    for c in text.bytes() {
        match c {
            b'A'..=b'Z' | b'0'..=b'9' => out.push(c as char),
            b'a'..=b'z' => out.push(c.to_ascii_uppercase() as char),
            b'\'' => {}
            _ => out.push('_'),
        }
    }
    out
}

/// `fprint_species`: like [`to_constant`] but runs of separators collapse into one
/// `_`, `♂`/`♀` become `_M`/`_F`, `é` becomes `E`, and `'`, `%`, `’` are dropped.
pub fn to_species_constant(text: &str) -> String {
    let mut out = String::new();
    if !is_constant(text, "SPECIES") {
        out.push_str("SPECIES_");
    }
    if text.is_empty() {
        out.push_str("NONE");
        return out;
    }
    let mut underscore = false;
    for c in text.chars() {
        match c {
            'A'..='Z' | '0'..='9' | 'a'..='z' => {
                if underscore {
                    out.push('_');
                }
                underscore = false;
                out.push(c.to_ascii_uppercase());
            }
            '\'' | '%' | '’' => {}
            '♂' => {
                underscore = false;
                out.push_str("_M");
            }
            '♀' => {
                underscore = false;
                out.push_str("_F");
            }
            'é' => {
                if underscore {
                    out.push('_');
                }
                underscore = false;
                out.push('E');
            }
            _ => underscore = true,
        }
    }
    out
}

/// trainerproc's `is_constant`: the text starts with `PREFIX_`.
fn is_constant(text: &str, prefix: &str) -> bool {
    text.len() > prefix.len() && text.starts_with(prefix) && text.as_bytes()[prefix.len()] == b'_'
}

/// trainerproc's special cases, read from the project's own trainerproc source:
/// `gendered_species` (`Nidoran (M)` is `Nidoran-M`: species, male, female) and
/// `itemed_species` (`Arceus-Fire` with no item is `Arceus @ Flame Plate`: form,
/// species, item).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpeciesRules {
    pub gendered: Vec<[String; 3]>,
    pub itemed: Vec<[String; 3]>,
}

impl SpeciesRules {
    pub fn parse(source: &str) -> Self {
        Self { gendered: string_table(source, "gendered_species[]"), itemed: string_table(source, "itemed_species[]") }
    }
}

/// Rows of three string literals after the line declaring `array`, up to `{ NULL`.
fn string_table(source: &str, array: &str) -> Vec<[String; 3]> {
    let mut rows = Vec::new();
    let Some(start) = source.find(array) else { return rows };
    for line in source[start..].lines().skip(1) {
        let line = line.trim();
        if line.starts_with("{ NULL") || line.starts_with("};") {
            break;
        }
        let quoted: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
        if let [a, b, c] = quoted[..] {
            rows.push([a.to_owned(), b.to_owned(), c.to_owned()]);
        }
    }
    rows
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Table {
    defined: BTreeSet<String>,
    /// Constant to display name, only when the name resolves back to the constant.
    names: BTreeMap<String, String>,
}

/// The constants a project defines and how to name them in `.party` files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vocabulary {
    tables: BTreeMap<Kind, Table>,
    pub rules: SpeciesRules,
}

impl Vocabulary {
    pub fn read(root: &Path) -> Result<Self, String> {
        let mut vocab = Vocabulary {
            rules: SpeciesRules::parse(&fs::read_to_string(root.join(TRAINERPROC_SOURCE)).unwrap_or_default()),
            ..Vocabulary::default()
        };
        for kind in Kind::ALL {
            let (header, tables) = kind.sources();
            let text = fs::read_to_string(root.join(header)).map_err(|e| format!("{header}: {e}"))?;
            let table = vocab.tables.entry(kind).or_default();
            table.defined = defined_constants(&text, kind.prefix());
            let mut files = Vec::new();
            for t in tables {
                if *t == SPECIES_INFO_DIR {
                    files.extend(species_info_files(root));
                } else {
                    files.push((*t).to_owned());
                }
            }
            for file in files {
                let text = fs::read_to_string(root.join(&file)).map_err(|e| format!("{file}: {e}"))?;
                for (constant, name) in table_names(&text, kind.prefix()) {
                    if vocab.resolve(kind, &name) == constant {
                        vocab.tables.get_mut(&kind).unwrap().names.entry(constant).or_insert(name);
                    }
                }
            }
        }
        Ok(vocab)
    }

    /// The constant a name resolves to, as trainerproc would emit it.
    pub fn resolve(&self, kind: Kind, text: &str) -> String {
        match kind {
            Kind::Species => to_species_constant(text),
            _ => to_constant(kind.prefix(), text),
        }
    }

    pub fn is_defined(&self, kind: Kind, constant: &str) -> bool {
        self.tables.get(&kind).is_some_and(|t| t.defined.contains(constant))
    }

    /// Records a name seen in the project, so writes reuse the project's spelling.
    pub fn observe(&mut self, kind: Kind, text: &str) {
        let constant = self.resolve(kind, text);
        if !text.is_empty() && !is_constant(text, kind.prefix()) {
            self.tables.entry(kind).or_default().names.insert(constant, text.to_owned());
        }
    }

    /// A name for `constant` that resolves back to it, or the constant itself.
    pub fn display(&self, kind: Kind, constant: &str) -> String {
        match self.tables.get(&kind).and_then(|t| t.names.get(constant)) {
            Some(name) if self.resolve(kind, name) == constant => name.clone(),
            _ => constant.to_owned(),
        }
    }

    /// Defined constants with their display name (or the constant when none).
    pub fn entries(&self, kind: Kind) -> Vec<(String, String)> {
        self.tables
            .get(&kind)
            .map(|t| t.defined.iter().map(|c| (c.clone(), self.display(kind, c))).collect())
            .unwrap_or_default()
    }
}

/// `PREFIX_X = n,` enum members and `#define PREFIX_X ...` lines.
fn defined_constants(text: &str, prefix: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim_start();
        let (candidate, rest) = match line.strip_prefix("#define") {
            Some(rest) => {
                let rest = rest.trim_start();
                let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
                (&rest[..end], "=")
            }
            None => {
                let end = line.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(line.len());
                (&line[..end], line[end..].trim_start())
            }
        };
        let is_member = rest.starts_with('=') || rest.starts_with(',');
        if is_member && is_constant(candidate, prefix) {
            found.insert(candidate.to_owned());
        }
    }
    found
}

/// `[PREFIX_X] = { ... .name = X("Name") ...` pairs from a data table.
fn table_names(text: &str, prefix: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('[') {
            if let Some(end) = rest.find(']') {
                let key = &rest[..end];
                if is_constant(key, prefix) && rest[end + 1..].trim_start().starts_with('=') {
                    current = Some(key.to_owned());
                    continue;
                }
            }
        }
        if trimmed.starts_with(".name =") || trimmed.starts_with(".speciesName =") {
            if let (Some(constant), Some(start)) = (current.take(), trimmed.find('"')) {
                if let Some(len) = trimmed[start + 1..].find('"') {
                    found.push((constant, trimmed[start + 1..start + 1 + len].to_owned()));
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_follow_trainerproc() {
        assert_eq!(to_constant("ITEM", "Oran Berry"), "ITEM_ORAN_BERRY");
        assert_eq!(to_constant("ITEM", "ITEM_ORAN_BERRY"), "ITEM_ORAN_BERRY");
        assert_eq!(to_constant("MOVE", "King's Shield"), "MOVE_KINGS_SHIELD");
        assert_eq!(to_constant("MOVE", "10,000,000 Volt Thunderbolt"), "MOVE_10_000_000_VOLT_THUNDERBOLT");
        assert_eq!(to_constant("AI_FLAG", "Check Bad Move"), "AI_FLAG_CHECK_BAD_MOVE");
        assert_eq!(to_constant("ITEM", ""), "ITEM_NONE");
        assert_eq!(to_species_constant("Geodude"), "SPECIES_GEODUDE");
        assert_eq!(to_species_constant("Mr. Mime"), "SPECIES_MR_MIME");
        assert_eq!(to_species_constant("Nidoran♀"), "SPECIES_NIDORAN_F");
        assert_eq!(to_species_constant("Farfetch’d"), "SPECIES_FARFETCHD");
        assert_eq!(to_species_constant("Flabébé"), "SPECIES_FLABEBE");
        assert_eq!(to_species_constant("Type: Null"), "SPECIES_TYPE_NULL");
        assert_eq!(to_species_constant("SPECIES_ABRA"), "SPECIES_ABRA");
    }

    #[test]
    fn headers_and_tables_are_read() {
        let header = "enum {\n    SPECIES_NONE = 0,\n    SPECIES_GEODUDE = 74,\n};\n#define SPECIES_EGG (SPECIES_GEODUDE + 1)\n";
        let defined = defined_constants(header, "SPECIES");
        assert_eq!(defined.into_iter().collect::<Vec<_>>(), ["SPECIES_EGG", "SPECIES_GEODUDE", "SPECIES_NONE"]);
        let rules = SpeciesRules::parse("x itemed_species[] = {\n    { \"Arceus-Bug\", \"Arceus\", \"Insect Plate\" },\n    { NULL, NULL, NULL }\n};\n");
        assert_eq!(rules.itemed, [["Arceus-Bug".to_owned(), "Arceus".to_owned(), "Insect Plate".to_owned()]]);
        assert!(rules.gendered.is_empty());
        let table = "    [ITEM_ORAN_BERRY] =\n    {\n        .name = ITEM_NAME(\"Oran Berry\"),\n";
        assert_eq!(table_names(table, "ITEM"), [("ITEM_ORAN_BERRY".to_owned(), "Oran Berry".to_owned())]);
    }
}
