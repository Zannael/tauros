//! Lossless reading and writing of `.party` trainer files (G3-002).
//!
//! A file is a preamble followed by `=== TRAINER_X ===` blocks; concatenating them
//! gives back the original bytes. Each block is parsed line by line the way the
//! expansion's `tools/trainerproc` reads it, and edits rewrite only the lines whose
//! values changed: an unchanged trainer keeps every byte, a new level rewrites one
//! `Level:` line. Names are mapped to constants through [`Vocabulary`].

use crate::names::{Kind, Vocabulary};
use std::collections::BTreeSet;
use tauros_model::{Extensions, ItemId, MoveId, Party, PartyMember, SpeciesId, Trainer, TrainerId, Value};

/// trainerproc's default level when a Pokémon has no `Level:` (main.c `default_level`).
const DEFAULT_LEVEL: u8 = 100;

/// Trainer attribute keys the model names; every other key rides in `extensions`.
const TRAINER_MODEL_KEYS: [&str; 6] = ["Name", "Class", "Items", "AI", "Double Battle", "Battle Type"];
/// Pokémon attribute keys the model names. `Gender` comes from the header line.
const MEMBER_MODEL_KEYS: [&str; 2] = ["Level", "Ability"];
/// Order used when a new Pokémon is written.
const MEMBER_KEY_ORDER: [&str; 10] =
    ["IVs", "EVs", "Ball", "Happiness", "Nature", "Shiny", "Dynamax Level", "Gigantamax", "Tera Type", "Tags"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyError {
    /// 1-based line in the file.
    pub line: u32,
    pub message: String,
}

/// A `.party` file split into its preamble and trainer blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyFile {
    pub preamble: String,
    pub blocks: Vec<Block>,
    default_level: u8,
}

/// One trainer: its raw text from the `===` line up to the next one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub id: String,
    pub text: String,
    /// 1-based line of the `===` header in the file.
    pub line: u32,
}

impl PartyFile {
    pub fn parse(text: &str) -> Result<Self, PartyError> {
        let mut preamble = String::new();
        let mut blocks: Vec<Block> = Vec::new();
        for (n, line) in text.split_inclusive('\n').enumerate() {
            if line.starts_with("===") {
                let id = section_id(line).ok_or_else(|| PartyError {
                    line: n as u32 + 1,
                    message: "malformed '=== TRAINER_X ===' header".into(),
                })?;
                blocks.push(Block { id, text: String::new(), line: n as u32 + 1 });
            }
            match blocks.last_mut() {
                Some(block) => block.text.push_str(line),
                None => preamble.push_str(line),
            }
        }
        let mut seen = BTreeSet::new();
        for block in &blocks {
            if !seen.insert(block.id.as_str()) {
                return Err(PartyError { line: block.line, message: format!("duplicate trainer {}", block.id) });
            }
        }
        let default_level = pragma_level(&preamble).unwrap_or(DEFAULT_LEVEL);
        Ok(Self { preamble, blocks, default_level })
    }

    pub fn emit(&self) -> String {
        let mut out = self.preamble.clone();
        for block in &self.blocks {
            out.push_str(&block.text);
        }
        out
    }

    /// Records every name the file uses, so later writes reuse its spelling.
    pub fn observe_names(&self, vocab: &mut Vocabulary) -> Result<(), PartyError> {
        for block in &self.blocks {
            let parsed = ParsedBlock::parse(&block.text, block.line)?;
            parsed.observe(vocab);
        }
        Ok(())
    }

    pub fn trainers(&self, vocab: &Vocabulary) -> Result<Vec<Trainer>, PartyError> {
        self.blocks
            .iter()
            .map(|b| ParsedBlock::parse(&b.text, b.line).map(|p| p.to_model(vocab, self.default_level)))
            .collect()
    }

    pub fn trainer(&self, id: &str, vocab: &Vocabulary) -> Result<Option<Trainer>, PartyError> {
        match self.blocks.iter().find(|b| b.id == id) {
            Some(b) => Ok(Some(ParsedBlock::parse(&b.text, b.line)?.to_model(vocab, self.default_level))),
            None => Ok(None),
        }
    }

    /// Writes `trainer`: rewrites only the changed lines of an existing block, or
    /// appends a new block at the end of the file.
    pub fn upsert(&mut self, trainer: &Trainer, vocab: &Vocabulary) -> Result<(), PartyError> {
        let default_level = self.default_level;
        match self.blocks.iter_mut().find(|b| b.id == trainer.id.0) {
            Some(block) => {
                let parsed = ParsedBlock::parse(&block.text, block.line)?;
                let old = parsed.to_model(vocab, default_level);
                if old != *trainer {
                    if parsed.has_comments {
                        return Err(PartyError {
                            line: block.line,
                            message: format!("{} contains comments; edit it by hand", trainer.id),
                        });
                    }
                    block.text = parsed.rewrite(&old, trainer, vocab);
                }
            }
            None => {
                let text = render_trainer(trainer, vocab);
                if let Some(last) = self.blocks.last_mut() {
                    if !last.text.ends_with('\n') {
                        last.text.push('\n');
                    }
                    if !last.text.ends_with("\n\n") {
                        last.text.push('\n');
                    }
                } else if !self.preamble.is_empty() && !self.preamble.ends_with('\n') {
                    self.preamble.push('\n');
                }
                let line = self.emit().lines().count() as u32 + 1;
                self.blocks.push(Block { id: trainer.id.0.clone(), text, line });
            }
        }
        Ok(())
    }

    /// True when the trainer's block holds C comments or preprocessor lines, which
    /// the writer does not edit around.
    pub fn has_comments(&self, id: &str) -> bool {
        self.blocks.iter().find(|b| b.id == id).is_some_and(|b| strip_comments(&b.text).1)
    }

    /// Removes a trainer's block; returns false when there is none.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.blocks.len();
        self.blocks.retain(|b| b.id != id);
        self.blocks.len() != before
    }
}

fn section_id(line: &str) -> Option<String> {
    let rest = line.strip_prefix("===")?.trim_start_matches([' ', '\t']);
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '\''))?;
    let (id, tail) = rest.split_at(end);
    let tail = tail.trim_start_matches([' ', '\t']).strip_prefix("===")?;
    (!id.is_empty() && tail.trim().is_empty()).then(|| id.to_owned())
}

fn pragma_level(preamble: &str) -> Option<u8> {
    preamble.lines().find_map(|l| {
        let rest = l.trim().strip_prefix("#pragma")?.trim().strip_prefix("trainerproc")?.trim();
        rest.strip_prefix("level")?.trim().parse().ok()
    })
}

/// A Pokémon header: `Nickname (Species) (G) @ Item`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Header {
    nickname: Option<String>,
    species: String,
    gender: Option<String>,
    item: Option<String>,
}

fn parse_header(line: &str) -> Option<Header> {
    let line = line.trim_end_matches(['\n', '\r']);
    let (main, item) = match line.find('@') {
        Some(at) => (&line[..at], Some(line[at + 1..].trim().to_owned())),
        None => (line, None),
    };
    let mut parts = Vec::new();
    let first_end = main.find('(').unwrap_or(main.len());
    parts.push(main[..first_end].trim().to_owned());
    let mut rest = &main[first_end..];
    while let Some(open) = rest.find('(') {
        let close = rest[open..].find(')')? + open;
        parts.push(rest[open + 1..close].trim().to_owned());
        rest = &rest[close + 1..];
    }
    if !rest.trim().is_empty() || parts[0].is_empty() || parts.len() > 3 {
        return None;
    }
    let header = match parts.len() {
        1 => Header { species: parts.remove(0), ..Header::default() },
        2 if parts[1].chars().count() == 1 => {
            Header { species: parts[0].clone(), gender: Some(parts[1].clone()), ..Header::default() }
        }
        2 => Header { nickname: Some(parts[0].clone()), species: parts[1].clone(), ..Header::default() },
        _ => Header { nickname: Some(parts[0].clone()), species: parts[1].clone(), gender: Some(parts[2].clone()), item: None },
    };
    Some(Header { item: item.filter(|i| !i.is_empty()), ..header })
}

fn render_header(h: &Header) -> String {
    let mut out = match &h.nickname {
        Some(nick) => format!("{nick} ({})", h.species),
        None => h.species.clone(),
    };
    if let Some(g) = &h.gender {
        out.push_str(&format!(" ({g})"));
    }
    if let Some(item) = &h.item {
        out.push_str(&format!(" @ {item}"));
    }
    out
}

/// `Key: Value` or `X Nature`; returns the key and the trimmed value.
fn parse_attribute(line: &str) -> Option<(String, String)> {
    let line = line.trim_end_matches(['\n', '\r']).trim();
    if let Some(nature) = line.strip_suffix("Nature") {
        let nature = nature.trim_end();
        if !nature.is_empty() && nature.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '\'') {
            return Some(("Nature".into(), nature.to_owned()));
        }
    }
    let colon = line.find(':')?;
    let key = line[..colon].trim();
    let valid = !key.is_empty()
        && key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' ' | '\''));
    valid.then(|| (key.to_owned(), line[colon + 1..].trim().to_owned()))
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// Replaces C block comments and preprocessor lines with spaces, keeping newlines,
/// as the C preprocessor does before trainerproc runs.
fn strip_comments(text: &str) -> (String, bool) {
    let mut out = String::with_capacity(text.len());
    let mut found = false;
    let mut in_comment = false;
    let mut chars = text.chars().peekable();
    let mut line_start = true;
    while let Some(c) = chars.next() {
        if in_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                out.push_str("  ");
                in_comment = false;
            } else {
                out.push(if c == '\n' { '\n' } else { ' ' });
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            out.push_str("  ");
            in_comment = true;
            found = true;
        } else if c == '#' && line_start {
            found = true;
            out.push(' ');
            for d in chars.by_ref() {
                if d == '\n' {
                    out.push('\n');
                    break;
                }
                out.push(' ');
            }
            line_start = true;
            continue;
        } else {
            out.push(c);
        }
        line_start = c == '\n' || (line_start && (c == ' ' || c == '\t'));
    }
    (out, found)
}

#[derive(Clone, Debug)]
struct Attr {
    line: usize,
    key: String,
    value: String,
}

#[derive(Clone, Debug)]
struct MonLines {
    /// First line (the header) and one past the last line of this Pokémon.
    start: usize,
    end: usize,
    header: Header,
    attrs: Vec<Attr>,
    moves: Vec<(usize, String)>,
}

/// A trainer block split into lines (each with its own terminator).
#[derive(Clone, Debug)]
struct ParsedBlock {
    id: String,
    lines: Vec<String>,
    attrs: Vec<Attr>,
    attrs_end: usize,
    mons: Vec<MonLines>,
    has_comments: bool,
}

impl ParsedBlock {
    fn parse(text: &str, first_line: u32) -> Result<Self, PartyError> {
        let (stripped, has_comments) = strip_comments(text);
        let lines: Vec<String> = text.split_inclusive('\n').map(str::to_owned).collect();
        let view: Vec<&str> = stripped.split_inclusive('\n').collect();
        let err = |i: usize, message: &str| PartyError { line: first_line + i as u32, message: message.to_owned() };
        let id = section_id(view.first().copied().unwrap_or("")).ok_or_else(|| err(0, "expected '=== TRAINER_X ==='"))?;
        let mut i = 1;
        let mut attrs = Vec::new();
        while i < view.len() && !is_blank(view[i]) {
            let (key, value) = parse_attribute(view[i]).ok_or_else(|| err(i, "expected 'Key: Value' trainer attribute"))?;
            attrs.push(Attr { line: i, key, value });
            i += 1;
        }
        let attrs_end = i;
        let mut mons = Vec::new();
        loop {
            while i < view.len() && is_blank(view[i]) {
                i += 1;
            }
            if i >= view.len() {
                break;
            }
            let start = i;
            let header = parse_header(view[i]).ok_or_else(|| err(i, "expected Pokémon header"))?;
            i += 1;
            let mut mon = MonLines { start, end: start, header, attrs: Vec::new(), moves: Vec::new() };
            while i < view.len() && !is_blank(view[i]) {
                let line = view[i].trim();
                if let Some(mv) = line.strip_prefix('-') {
                    mon.moves.push((i, mv.trim().to_owned()));
                } else if !mon.moves.is_empty() {
                    return Err(err(i, "moves must be the last lines of a Pokémon"));
                } else {
                    let (key, value) = parse_attribute(view[i]).ok_or_else(|| err(i, "expected Pokémon attribute or move"))?;
                    mon.attrs.push(Attr { line: i, key, value });
                }
                i += 1;
            }
            mon.end = i;
            mons.push(mon);
        }
        Ok(Self { id, lines, attrs, attrs_end, mons, has_comments })
    }

    fn observe(&self, vocab: &mut Vocabulary) {
        for attr in &self.attrs {
            match attr.key.as_str() {
                "Items" => split_list(&attr.value).for_each(|v| vocab.observe(Kind::Item, v)),
                "AI" => split_list(&attr.value).for_each(|v| vocab.observe(Kind::AiFlag, v)),
                _ => {}
            }
        }
        for mon in &self.mons {
            let species = mon.header.species.as_str();
            if !vocab.rules.gendered.iter().any(|[s, _, _]| s == species) && !vocab.rules.itemed.iter().any(|[f, _, _]| f == species) {
                vocab.observe(Kind::Species, &mon.header.species);
            }
            if let Some(item) = &mon.header.item {
                vocab.observe(Kind::Item, item);
            }
            for attr in mon.attrs.iter().filter(|a| a.key == "Ability") {
                vocab.observe(Kind::Ability, &attr.value);
            }
            for (_, mv) in &mon.moves {
                vocab.observe(Kind::Move, mv);
            }
        }
    }

    fn to_model(&self, vocab: &Vocabulary, default_level: u8) -> Trainer {
        let mut trainer = Trainer {
            id: TrainerId(self.id.clone()),
            name: String::new(),
            class: None,
            items: Vec::new(),
            double_battle: false,
            ai_flags: Vec::new(),
            party: Party::default(),
            extensions: Extensions::new(),
        };
        for attr in &self.attrs {
            match attr.key.as_str() {
                "Name" => trainer.name = attr.value.clone(),
                "Class" => trainer.class = Some(attr.value.clone()),
                "Items" => trainer.items = split_list(&attr.value).map(|v| ItemId(vocab.resolve(Kind::Item, v))).collect(),
                "AI" => trainer.ai_flags = split_list(&attr.value).map(|v| vocab.resolve(Kind::AiFlag, v)).collect(),
                "Double Battle" => trainer.double_battle = attr.value == "Yes",
                "Battle Type" => trainer.double_battle = attr.value == "Doubles",
                key => {
                    trainer.extensions.insert(key.to_owned(), Value::Text(attr.value.clone()));
                }
            }
        }
        trainer.party.members = self.mons.iter().map(|m| mon_to_model(m, vocab, default_level)).collect();
        trainer
    }

    /// The block with only the lines whose values differ between `old` and `new`
    /// rewritten, added or removed.
    fn rewrite(&self, old: &Trainer, new: &Trainer, vocab: &Vocabulary) -> String {
        let mut out: Vec<String> = Vec::new();
        out.push(self.lines[0].clone());
        let edits = trainer_attr_edits(old, new, vocab, &self.attrs);
        apply_attr_edits(&mut out, &self.lines, &self.attrs, edits, 1, self.attrs_end);
        let mut cursor = self.attrs_end;
        for (index, member) in new.party.members.iter().enumerate() {
            match self.mons.get(index) {
                Some(mon) => {
                    out.extend(self.lines[cursor..mon.start].iter().cloned());
                    let old_member = &old.party.members[index];
                    if old_member == member {
                        out.extend(self.lines[mon.start..mon.end].iter().cloned());
                    } else {
                        out.extend(rewrite_mon(&self.lines, mon, old_member, member, vocab));
                    }
                    cursor = mon.end;
                }
                None => {
                    terminate_last(&mut out);
                    out.push("\n".into());
                    out.extend(render_mon(member, vocab));
                }
            }
        }
        // Removed Pokémon drop their lines and the blank lines before them.
        if let Some(last_old) = self.mons.last() {
            if new.party.members.len() < self.mons.len() {
                cursor = last_old.end;
            }
        }
        let tail = &self.lines[cursor.min(self.lines.len())..];
        if !tail.is_empty() {
            terminate_last(&mut out);
        }
        out.extend(tail.iter().cloned());
        out.concat()
    }
}

fn terminate_last(out: &mut [String]) {
    if let Some(last) = out.last_mut() {
        if !last.ends_with('\n') {
            last.push('\n');
        }
    }
}

fn split_list(value: &str) -> impl Iterator<Item = &str> {
    value.split('/').map(str::trim).filter(|v| !v.is_empty())
}

fn mon_to_model(mon: &MonLines, vocab: &Vocabulary, default_level: u8) -> PartyMember {
    let mut species_text = mon.header.species.clone();
    let mut gender = mon.header.gender.clone();
    let mut item_text = mon.header.item.clone();
    // trainerproc's tables, read from the project (names::SpeciesRules).
    if let Some([_, male, female]) = vocab.rules.gendered.iter().find(|[s, _, _]| *s == species_text) {
        match gender.as_deref() {
            Some("M") => species_text = male.clone(),
            Some("F") => species_text = female.clone(),
            _ => {}
        }
        gender = None;
    }
    if item_text.is_none() {
        if let Some([_, species, item]) = vocab.rules.itemed.iter().find(|[form, _, _]| *form == species_text) {
            species_text = species.clone();
            item_text = Some(item.clone());
        }
    }
    let mut member = PartyMember {
        species: SpeciesId(vocab.resolve(Kind::Species, &species_text)),
        level: default_level,
        held_item: item_text.map(|i| ItemId(vocab.resolve(Kind::Item, &i))),
        moves: mon.moves.iter().map(|(_, m)| MoveId(vocab.resolve(Kind::Move, m))).collect(),
        nickname: mon.header.nickname.clone(),
        ability: None,
        extensions: Extensions::new(),
    };
    if let Some(g) = gender {
        member.extensions.insert("Gender".into(), Value::Text(g));
    }
    for attr in &mon.attrs {
        match attr.key.as_str() {
            "Level" => member.level = attr.value.parse().unwrap_or(0),
            "Ability" => member.ability = Some(vocab.resolve(Kind::Ability, &attr.value)),
            key => {
                member.extensions.insert(key.to_owned(), Value::Text(attr.value.clone()));
            }
        }
    }
    member
}

/// Text written for an extension value.
pub fn value_text(value: &Value) -> String {
    match value {
        Value::Bool(true) => "Yes".into(),
        Value::Bool(false) => "No".into(),
        Value::Int(n) => n.to_string(),
        Value::Text(t) => t.clone(),
        Value::List(items) => items.iter().map(value_text).collect::<Vec<_>>().join(" / "),
    }
}

/// Rendered list reusing the old spelling where the constant at a position is unchanged.
fn render_list(kind: Kind, old_texts: &[&str], new: &[String], vocab: &Vocabulary) -> String {
    new.iter()
        .enumerate()
        .map(|(i, constant)| match old_texts.get(i) {
            Some(text) if vocab.resolve(kind, text) == *constant => (*text).to_owned(),
            _ => vocab.display(kind, constant),
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Desired value per attribute key; `None` removes the line.
type AttrEdits = Vec<(String, Option<String>)>;

fn trainer_attr_edits(old: &Trainer, new: &Trainer, vocab: &Vocabulary, attrs: &[Attr]) -> AttrEdits {
    let mut edits: AttrEdits = Vec::new();
    let old_value = |key: &str| attrs.iter().find(|a| a.key == key).map(|a| a.value.clone()).unwrap_or_default();
    if old.name != new.name {
        edits.push(("Name".into(), Some(new.name.clone())));
    }
    if old.class != new.class {
        edits.push(("Class".into(), new.class.clone()));
    }
    if old.items != new.items {
        let old_items = old_value("Items");
        let olds: Vec<&str> = split_list(&old_items).collect();
        let items: Vec<String> = new.items.iter().map(|i| i.0.clone()).collect();
        edits.push(("Items".into(), (!items.is_empty()).then(|| render_list(Kind::Item, &olds, &items, vocab))));
    }
    if old.ai_flags != new.ai_flags {
        let old_ai = old_value("AI");
        let olds: Vec<&str> = split_list(&old_ai).collect();
        edits.push(("AI".into(), (!new.ai_flags.is_empty()).then(|| render_list(Kind::AiFlag, &olds, &new.ai_flags, vocab))));
    }
    if old.double_battle != new.double_battle {
        if attrs.iter().any(|a| a.key == "Battle Type") {
            edits.push(("Battle Type".into(), Some(if new.double_battle { "Doubles" } else { "Singles" }.into())));
        } else {
            edits.push(("Double Battle".into(), Some(if new.double_battle { "Yes" } else { "No" }.into())));
        }
    }
    extension_edits(&old.extensions, &new.extensions, &mut edits);
    edits
}

fn extension_edits(old: &Extensions, new: &Extensions, edits: &mut AttrEdits) {
    let keys: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    for key in keys {
        if old.get(key) != new.get(key) {
            edits.push((key.clone(), new.get(key).map(value_text)));
        }
    }
}

/// Copies `lines[from..to]` (attribute lines) into `out`, applying `edits` in place
/// and appending attributes that did not exist at the end of the section.
fn apply_attr_edits(out: &mut Vec<String>, lines: &[String], attrs: &[Attr], mut edits: AttrEdits, from: usize, to: usize) {
    for (i, line) in lines.iter().enumerate().take(to).skip(from) {
        let attr = attrs.iter().find(|a| a.line == i);
        let edit = attr.and_then(|a| edits.iter().position(|(k, _)| *k == a.key));
        match (attr, edit) {
            (Some(attr), Some(at)) => {
                let (_, value) = edits.remove(at);
                if let Some(value) = value {
                    out.push(replace_value(line, attr, &value));
                }
            }
            _ => out.push(line.clone()),
        }
    }
    let new_lines: Vec<String> =
        edits.into_iter().filter_map(|(key, value)| value.map(|v| format!("{key}: {v}\n"))).collect();
    if !new_lines.is_empty() {
        terminate_last(out);
        out.extend(new_lines);
    }
}

/// Rewrites one attribute line, keeping its key spelling, indentation and form.
fn replace_value(line: &str, attr: &Attr, value: &str) -> String {
    let ending = if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    };
    let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let body = line.trim();
    if attr.key == "Nature" && !body.contains(':') {
        return format!("{indent}{value} Nature{ending}");
    }
    let colon = body.find(':').unwrap_or(body.len());
    format!("{indent}{}: {value}{ending}", &body[..colon].trim_end())
}

fn rewrite_mon(
    lines: &[String],
    mon: &MonLines,
    old: &PartyMember,
    new: &PartyMember,
    vocab: &Vocabulary,
) -> Vec<String> {
    let mut out = Vec::new();
    let header_changed = old.species != new.species
        || old.held_item != new.held_item
        || old.nickname != new.nickname
        || old.extensions.get("Gender") != new.extensions.get("Gender");
    if header_changed {
        let ending = if lines[mon.start].ends_with('\n') { "\n" } else { "" };
        out.push(format!("{}{ending}", render_header(&header_for(new, Some(&mon.header), old, vocab))));
    } else {
        out.push(lines[mon.start].clone());
    }
    let mut edits: AttrEdits = Vec::new();
    if old.level != new.level {
        edits.push(("Level".into(), Some(new.level.to_string())));
    }
    if old.ability != new.ability {
        let old_text = mon.attrs.iter().find(|a| a.key == "Ability").map(|a| a.value.as_str());
        edits.push((
            "Ability".into(),
            new.ability.as_ref().map(|a| match old_text {
                Some(text) if vocab.resolve(Kind::Ability, text) == *a => text.to_owned(),
                _ => vocab.display(Kind::Ability, a),
            }),
        ));
    }
    let mut old_ext = old.extensions.clone();
    let mut new_ext = new.extensions.clone();
    old_ext.remove("Gender");
    new_ext.remove("Gender");
    extension_edits(&old_ext, &new_ext, &mut edits);
    let attrs_end = mon.moves.first().map(|(i, _)| *i).unwrap_or(mon.end);
    apply_attr_edits(&mut out, lines, &mon.attrs, edits, mon.start + 1, attrs_end);
    if old.moves == new.moves {
        out.extend(lines[attrs_end..mon.end].iter().cloned());
    } else {
        terminate_last(&mut out);
        let olds: Vec<&str> = mon.moves.iter().map(|(_, m)| m.as_str()).collect();
        let indent: String = mon
            .moves
            .first()
            .map(|(i, _)| lines[*i].chars().take_while(|c| *c == ' ' || *c == '\t').collect())
            .unwrap_or_default();
        for (j, mv) in new.moves.iter().enumerate() {
            let text = match olds.get(j) {
                Some(t) if vocab.resolve(Kind::Move, t) == mv.0 => (*t).to_owned(),
                _ => vocab.display(Kind::Move, &mv.0),
            };
            out.push(format!("{indent}- {text}\n"));
        }
        if let Some(last) = out.last_mut() {
            if !lines[mon.end - 1].ends_with('\n') && mon.end == lines.len() {
                last.pop();
            }
        }
    }
    out
}

/// A header for `new`, reusing the old spelling of unchanged parts.
fn header_for(new: &PartyMember, old_header: Option<&Header>, old: &PartyMember, vocab: &Vocabulary) -> Header {
    let species = match old_header {
        Some(h) if old.species == new.species => h.species.clone(),
        _ => vocab.display(Kind::Species, &new.species.0),
    };
    let item = new.held_item.as_ref().map(|item| match old_header.and_then(|h| h.item.as_ref()) {
        Some(text) if old.held_item.as_ref() == Some(item) => text.clone(),
        _ => vocab.display(Kind::Item, &item.0),
    });
    // A species kept with its old spelling and gender keeps the gendered form.
    let gender = match (old_header, new.extensions.get("Gender")) {
        (_, Some(g)) => Some(value_text(g)),
        (Some(h), None) if old.species == new.species && !old.extensions.contains_key("Gender") => h.gender.clone(),
        _ => None,
    };
    Header { nickname: new.nickname.clone(), species, gender, item }
}

fn render_mon(member: &PartyMember, vocab: &Vocabulary) -> Vec<String> {
    let mut out = Vec::new();
    let blank = PartyMember { species: SpeciesId(String::new()), held_item: None, ..member.clone() };
    out.push(format!("{}\n", render_header(&header_for(member, None, &blank, vocab))));
    out.push(format!("Level: {}\n", member.level));
    if let Some(ability) = &member.ability {
        out.push(format!("Ability: {}\n", vocab.display(Kind::Ability, ability)));
    }
    let mut keys: Vec<&String> = member.extensions.keys().filter(|k| k.as_str() != "Gender").collect();
    keys.sort_by_key(|k| (MEMBER_KEY_ORDER.iter().position(|o| o == k).unwrap_or(usize::MAX), (*k).clone()));
    for key in keys {
        out.push(format!("{key}: {}\n", value_text(&member.extensions[key])));
    }
    for mv in &member.moves {
        out.push(format!("- {}\n", vocab.display(Kind::Move, &mv.0)));
    }
    out
}

fn render_trainer(trainer: &Trainer, vocab: &Vocabulary) -> String {
    let mut out = format!("=== {} ===\nName: {}\n", trainer.id, trainer.name);
    if let Some(class) = &trainer.class {
        out.push_str(&format!("Class: {class}\n"));
    }
    for (key, value) in &trainer.extensions {
        out.push_str(&format!("{key}: {}\n", value_text(value)));
    }
    if !trainer.items.is_empty() {
        let items: Vec<String> = trainer.items.iter().map(|i| i.0.clone()).collect();
        out.push_str(&format!("Items: {}\n", render_list(Kind::Item, &[], &items, vocab)));
    }
    out.push_str(&format!("Double Battle: {}\n", if trainer.double_battle { "Yes" } else { "No" }));
    if !trainer.ai_flags.is_empty() {
        out.push_str(&format!("AI: {}\n", render_list(Kind::AiFlag, &[], &trainer.ai_flags, vocab)));
    }
    for member in &trainer.party.members {
        out.push('\n');
        out.push_str(&render_mon(member, vocab).concat());
    }
    out
}

/// Keys a model trainer or member must not carry in `extensions` (the model names them).
pub fn reserved_extension(key: &str, member: bool) -> bool {
    if member {
        MEMBER_MODEL_KEYS.contains(&key)
    } else {
        TRAINER_MODEL_KEYS.contains(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "/* preamble */\n\n=== TRAINER_NONE ===\nName:\nClass: Pkmn Trainer 1\nPic: Hiker\n\n=== TRAINER_A ===\nName: A\nClass: Hiker\nPic: Hiker\nDouble Battle: No\nAI: Check Bad Move / Try To Faint\n\nGeodude @ Oran Berry\nLevel: 21\nIVs: 0 HP / 0 Atk\nAdamant Nature\n- Tackle\n- Rock Throw\n\nBob (Nidoran) (M)\nLevel: 5\n\n=== TRAINER_B ===\nName: B\nPic: Hiker\n\nZubat\n";

    fn vocab() -> Vocabulary {
        let mut v = Vocabulary::default();
        v.rules = crate::names::SpeciesRules::parse(
            "static const struct { } gendered_species[] = {\n    { \"Nidoran\", \"Nidoran-M\", \"Nidoran-F\" },\n    { NULL, NULL, NULL }\n};\n",
        );
        PartyFile::parse(SAMPLE).unwrap().observe_names(&mut v).unwrap();
        v
    }

    #[test]
    fn roundtrip_is_identical() {
        let file = PartyFile::parse(SAMPLE).unwrap();
        assert_eq!(file.emit(), SAMPLE);
        assert_eq!(file.blocks.len(), 3);
        let v = vocab();
        let trainers = file.trainers(&v).unwrap();
        let a = &trainers[1];
        assert_eq!(a.ai_flags, ["AI_FLAG_CHECK_BAD_MOVE", "AI_FLAG_TRY_TO_FAINT"]);
        let geodude = &a.party.members[0];
        assert_eq!(geodude.species.0, "SPECIES_GEODUDE");
        assert_eq!(geodude.held_item.as_ref().unwrap().0, "ITEM_ORAN_BERRY");
        assert_eq!(geodude.moves[1].0, "MOVE_ROCK_THROW");
        assert_eq!(geodude.extensions["Nature"], Value::Text("Adamant".into()));
        let bob = &a.party.members[1];
        assert_eq!((bob.species.0.as_str(), bob.nickname.as_deref()), ("SPECIES_NIDORAN_M", Some("Bob")));
        assert_eq!(trainers[2].party.members[0].level, 100);
        let mut copy = file.clone();
        for t in &trainers {
            copy.upsert(t, &v).unwrap();
        }
        assert_eq!(copy.emit(), SAMPLE);
    }

    #[test]
    fn edits_touch_only_their_lines() {
        let v = vocab();
        let mut file = PartyFile::parse(SAMPLE).unwrap();
        let mut a = file.trainer("TRAINER_A", &v).unwrap().unwrap();
        a.party.members[0].level = 22;
        a.party.members[0].moves[0] = MoveId("MOVE_ROCK_THROW".into());
        a.party.members[0].extensions.insert("Nature".into(), Value::Text("Brave".into()));
        a.ai_flags.pop();
        file.upsert(&a, &v).unwrap();
        let out = file.emit();
        let changed: Vec<(&str, &str)> = SAMPLE.lines().zip(out.lines()).filter(|(x, y)| x != y).collect();
        assert_eq!(
            changed,
            [
                ("AI: Check Bad Move / Try To Faint", "AI: Check Bad Move"),
                ("Level: 21", "Level: 22"),
                ("Adamant Nature", "Brave Nature"),
                ("- Tackle", "- Rock Throw"),
            ]
        );
        assert_eq!(file.trainer("TRAINER_A", &v).unwrap().unwrap(), a);
    }

    #[test]
    fn members_can_be_added_and_removed() {
        let v = vocab();
        let mut file = PartyFile::parse(SAMPLE).unwrap();
        let mut a = file.trainer("TRAINER_A", &v).unwrap().unwrap();
        a.party.members.pop();
        file.upsert(&a, &v).unwrap();
        assert_eq!(file.trainer("TRAINER_A", &v).unwrap().unwrap(), a);
        assert!(!file.emit().contains("Bob"));
        let mut b = file.trainer("TRAINER_B", &v).unwrap().unwrap();
        b.party.members.push(a.party.members[0].clone());
        file.upsert(&b, &v).unwrap();
        assert_eq!(file.trainer("TRAINER_B", &v).unwrap().unwrap(), b);
        assert!(file.emit().ends_with("Zubat\n\nGeodude @ Oran Berry\nLevel: 21\nIVs: 0 HP / 0 Atk\nNature: Adamant\n- Tackle\n- Rock Throw\n"));
        let mut c = b.clone();
        c.id = TrainerId("TRAINER_C".into());
        file.upsert(&c, &v).unwrap();
        assert_eq!(file.trainer("TRAINER_C", &v).unwrap().unwrap(), c);
        assert!(file.remove("TRAINER_C"));
        assert!(!file.remove("TRAINER_C"));
    }
}
