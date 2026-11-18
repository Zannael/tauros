//! Event flags (G3-004): read `include/constants/flags.h` without rewriting it, and
//! allocate new flags without colliding with existing ones.
//!
//! Only the branch the game build compiles is active (`IS_FRLG` and `TESTING` are
//! 0 for the Emerald build). Values are evaluated from the C expressions, with the
//! macros of the headers flags.h includes. A flag is *in use* when its name appears
//! anywhere in the project outside its definition.
//!
//! Allocation never invents a number: it renames one `FLAG_UNUSED_*` placeholder in
//! the event-flag range (above the temporary flags, below `TRAINER_FLAGS_START`)
//! that no file references and no other flag shares, rewriting only that line.

use std::collections::{BTreeMap, BTreeSet};

pub const FLAGS_HEADER: &str = "include/constants/flags.h";
/// Headers whose macros flags.h values depend on.
pub const VALUE_HEADERS: [&str; 3] = ["include/constants/opponents.h", "include/constants/trainers.h", "include/constants/rematches.h"];
/// Configuration the game build uses when reading flags.h.
const BUILD_CONFIG: [(&str, i64); 2] = [("IS_FRLG", 0), ("TESTING", 0)];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlagDef {
    pub name: String,
    /// 0-based line in flags.h.
    pub line: usize,
    pub value: Option<i64>,
}

/// `#define NAME expr` lines in the branches the build compiles, plus every macro
/// from the supporting headers, first definition wins.
pub fn active_defines(text: &str, config: &BTreeMap<String, i64>) -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    // Each level: (this branch active, some branch already taken, parent active).
    let mut stack: Vec<(bool, bool, bool)> = Vec::new();
    let active = |stack: &Vec<(bool, bool, bool)>| stack.last().is_none_or(|s| s.0);
    let mut defined: BTreeSet<String> = BTreeSet::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let Some(directive) = line.strip_prefix('#') else { continue };
        let directive = directive.trim_start();
        let (word, rest) = directive.split_once(|c: char| c.is_whitespace()).unwrap_or((directive, ""));
        let rest = strip_comment(rest).trim();
        let parent = active(&stack);
        match word {
            "if" => {
                let cond = parent && eval_condition(rest, config, &defined);
                stack.push((cond, cond, parent));
            }
            "ifdef" | "ifndef" => {
                let is_def = defined.contains(rest) || config.contains_key(rest);
                let cond = parent && (is_def == (word == "ifdef"));
                stack.push((cond, cond, parent));
            }
            "elif" => {
                if let Some(top) = stack.last_mut() {
                    let cond = top.2 && !top.1 && eval_condition(rest, config, &defined);
                    top.0 = cond;
                    top.1 |= cond;
                }
            }
            "else" => {
                if let Some(top) = stack.last_mut() {
                    top.0 = top.2 && !top.1;
                    top.1 = true;
                }
            }
            "endif" => {
                stack.pop();
            }
            "define" if parent => {
                let name_end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
                let name = &rest[..name_end];
                // Function-like macros are not flag values.
                if !name.is_empty() && !rest[name_end..].starts_with('(') {
                    defined.insert(name.to_owned());
                    out.push((name.to_owned(), n, rest[name_end..].trim().to_owned()));
                }
            }
            _ => {}
        }
    }
    out
}

fn eval_condition(expr: &str, config: &BTreeMap<String, i64>, defined: &BTreeSet<String>) -> bool {
    let mut macros: BTreeMap<String, String> = config.iter().map(|(k, v)| (k.clone(), v.to_string())).collect();
    for d in defined {
        macros.entry(d.clone()).or_insert_with(|| "1".into());
    }
    evaluate(expr, &macros).unwrap_or(0) != 0
}

fn strip_comment(text: &str) -> &str {
    let mut end = text.len();
    if let Some(i) = text.find("//") {
        end = end.min(i);
    }
    if let Some(i) = text.find("/*") {
        end = end.min(i);
    }
    &text[..end]
}

/// Evaluates an integer C preprocessor expression, expanding object-like macros.
pub fn evaluate(expr: &str, macros: &BTreeMap<String, String>) -> Option<i64> {
    let mut seen = BTreeSet::new();
    eval_with(expr, macros, &mut seen)
}

fn eval_with(expr: &str, macros: &BTreeMap<String, String>, seen: &mut BTreeSet<String>) -> Option<i64> {
    let tokens = tokenize(strip_comment(expr))?;
    let mut parser = Expr { tokens, pos: 0, macros, seen };
    let value = parser.binary(0)?;
    (parser.pos == parser.tokens.len()).then_some(value)
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(i64),
    Ident(String),
    Op(&'static str),
}

fn tokenize(s: &str) -> Option<Vec<Tok>> {
    const OPS: [&str; 17] = ["<<", ">>", "&&", "||", "==", "!=", "+", "-", "*", "/", "%", "&", "|", "^", "~", "(", ")"];
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i] as char).is_ascii_alphanumeric() {
                i += 1;
            }
            let lit = s[start..i].trim_end_matches(['u', 'U', 'l', 'L']);
            let value = match lit.strip_prefix("0x").or_else(|| lit.strip_prefix("0X")) {
                Some(hex) => i64::from_str_radix(hex, 16).ok()?,
                None => lit.parse().ok()?,
            };
            out.push(Tok::Num(value));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < bytes.len() && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.push(Tok::Ident(s[start..i].to_owned()));
        } else {
            let op = OPS.iter().find(|op| s[i..].starts_with(**op))?;
            out.push(Tok::Op(op));
            i += op.len();
        }
    }
    Some(out)
}

struct Expr<'a> {
    tokens: Vec<Tok>,
    pos: usize,
    macros: &'a BTreeMap<String, String>,
    seen: &'a mut BTreeSet<String>,
}

impl Expr<'_> {
    fn precedence(op: &str) -> Option<u8> {
        Some(match op {
            "||" => 1,
            "&&" => 2,
            "|" => 3,
            "^" => 4,
            "&" => 5,
            "==" | "!=" => 6,
            "<<" | ">>" => 7,
            "+" | "-" => 8,
            "*" | "/" | "%" => 9,
            _ => return None,
        })
    }

    fn binary(&mut self, min: u8) -> Option<i64> {
        let mut left = self.unary()?;
        while let Some(Tok::Op(op)) = self.tokens.get(self.pos).cloned() {
            let Some(prec) = Self::precedence(op) else { break };
            if prec < min {
                break;
            }
            self.pos += 1;
            let right = self.binary(prec + 1)?;
            left = match op {
                "||" => ((left != 0) || (right != 0)) as i64,
                "&&" => ((left != 0) && (right != 0)) as i64,
                "|" => left | right,
                "^" => left ^ right,
                "&" => left & right,
                "==" => (left == right) as i64,
                "!=" => (left != right) as i64,
                "<<" => left.checked_shl(right as u32)?,
                ">>" => left.checked_shr(right as u32)?,
                "+" => left.checked_add(right)?,
                "-" => left.checked_sub(right)?,
                "*" => left.checked_mul(right)?,
                "/" => left.checked_div(right)?,
                "%" => left.checked_rem(right)?,
                _ => return None,
            };
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<i64> {
        let tok = self.tokens.get(self.pos).cloned()?;
        self.pos += 1;
        match tok {
            Tok::Num(n) => Some(n),
            Tok::Op("-") => self.unary().map(|v| -v),
            Tok::Op("+") => self.unary(),
            Tok::Op("~") => self.unary().map(|v| !v),
            Tok::Op("(") => {
                let v = self.binary(0)?;
                (self.tokens.get(self.pos) == Some(&Tok::Op(")"))).then(|| self.pos += 1)?;
                Some(v)
            }
            Tok::Ident(name) => {
                if !self.seen.insert(name.clone()) {
                    return None;
                }
                let body = self.macros.get(&name)?.clone();
                let value = eval_with(&body, self.macros, self.seen);
                self.seen.remove(&name);
                value
            }
            _ => None,
        }
    }
}

/// The flags the game build defines, with their values.
pub fn read_flags(flags_h: &str, value_headers: &[&str]) -> Vec<FlagDef> {
    let config: BTreeMap<String, i64> = BUILD_CONFIG.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect();
    let mut macros: BTreeMap<String, String> = BTreeMap::new();
    for header in value_headers {
        for (name, _, body) in active_defines(header, &config) {
            macros.entry(name).or_insert(body);
        }
    }
    let defines = active_defines(flags_h, &config);
    for (name, _, body) in &defines {
        macros.entry(name.clone()).or_insert_with(|| body.clone());
    }
    defines
        .into_iter()
        .filter(|(name, _, _)| name.starts_with("FLAG_"))
        .map(|(name, line, body)| FlagDef { value: evaluate(&body, &macros), name, line })
        .collect()
}

/// Every `FLAG_*` token in `text` (used to decide which flags are in use).
pub fn flag_tokens(text: &str, into: &mut BTreeSet<String>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(found) = text[i..].find("FLAG_") {
        let start = i + found;
        let boundary = start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
        let mut end = start + 5;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        if boundary && end > start + 5 {
            into.insert(text[start..end].to_owned());
        }
        i = end;
    }
}

/// The placeholder that a new flag would take, if any.
pub fn allocation_candidate<'a>(flags: &'a [FlagDef], used: &BTreeSet<String>, macros_start: i64, macros_end: i64) -> Option<&'a FlagDef> {
    let mut by_value: BTreeMap<i64, usize> = BTreeMap::new();
    for f in flags {
        if let Some(v) = f.value {
            *by_value.entry(v).or_default() += 1;
        }
    }
    flags
        .iter()
        .filter(|f| f.name.starts_with("FLAG_UNUSED_") && !used.contains(&f.name))
        .filter(|f| f.value.is_some_and(|v| v >= macros_start && v < macros_end && by_value[&v] == 1))
        .min_by_key(|f| f.value)
}

/// Rewrites the placeholder's line to define `new_name` with the same value.
pub fn rename_line(line: &str, old_name: &str, new_name: &str) -> String {
    let at = line.find(old_name).expect("placeholder name on its line");
    let after = &line[at + old_name.len()..];
    let spaces = after.len() - after.trim_start().len();
    let width = old_name.len() + spaces;
    let pad = if new_name.len() < width { width - new_name.len() } else { 1 };
    let rest = after.trim_start();
    let value_part = match rest.find("//") {
        Some(c) => rest[..c].trim_end().to_owned(),
        None => rest.trim_end().to_owned(),
    };
    let comment_gap = rest.find("//").map(|c| &rest[value_part.len()..c]).unwrap_or(" ");
    let ending = if line.ends_with('\n') { "\n" } else { "" };
    format!("{}{new_name}{}{value_part}{}// Allocated by Tauros (was {old_name}){ending}", &line[..at], " ".repeat(pad), if comment_gap.is_empty() { " " } else { comment_gap })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLAGS: &str = "#ifndef GUARD\n#define GUARD\n#define TEMP_FLAGS_START 0x0\n#define FLAG_TEMP_1 (TEMP_FLAGS_START + 0x1)\n#if IS_FRLG\n#define FLAG_FRLG 0x20\n#else\n#define FLAG_UNUSED_0x020    0x20 // Unused Flag\n#define FLAG_UNUSED_0x021    0x21 // Unused Flag\n#define FLAG_ALIAS           0x21\n#define FLAG_UNUSED_0x022    0x22 // Unused Flag\n#define FLAG_STORY           0x23\n#endif\n#define TRAINER_FLAGS_START 0x500\n#define TRAINER_FLAGS_END (TRAINER_FLAGS_START + MAX_TRAINERS_COUNT - 1)\n#define FLAG_SYS (TRAINER_FLAGS_END + 1) // FLAG_0x860\n#endif\n";

    #[test]
    fn values_follow_the_build_branch() {
        let flags = read_flags(FLAGS, &["#define MAX_TRAINERS_COUNT 864\n"]);
        let get = |n: &str| flags.iter().find(|f| f.name == n).and_then(|f| f.value);
        assert_eq!(get("FLAG_TEMP_1"), Some(1));
        assert_eq!(get("FLAG_FRLG"), None);
        assert!(flags.iter().all(|f| f.name != "FLAG_FRLG"));
        assert_eq!(get("FLAG_SYS"), Some(0x860));
        assert_eq!(evaluate("(1 << 4) | 2", &BTreeMap::new()), Some(18));
    }

    #[test]
    fn allocation_skips_used_and_shared_values() {
        let flags = read_flags(FLAGS, &["#define MAX_TRAINERS_COUNT 864\n"]);
        let mut used = BTreeSet::new();
        flag_tokens("setflag FLAG_UNUSED_0x020\n", &mut used);
        let pick = allocation_candidate(&flags, &used, 0x20, 0x500).unwrap();
        // 0x020 is referenced and 0x021 shares its value with FLAG_ALIAS.
        assert_eq!(pick.name, "FLAG_UNUSED_0x022");
        let line = FLAGS.lines().nth(pick.line).unwrap();
        assert_eq!(rename_line(line, &pick.name, "FLAG_SCENE"), "#define FLAG_SCENE           0x22 // Allocated by Tauros (was FLAG_UNUSED_0x022)");
        assert_eq!(rename_line("#define FLAG_UNUSED_0x5 0x5\n", "FLAG_UNUSED_0x5", "FLAG_A_LONGER_NAME"), "#define FLAG_A_LONGER_NAME 0x5 // Allocated by Tauros (was FLAG_UNUSED_0x5)\n");
    }

    #[test]
    fn tokens_respect_word_boundaries() {
        let mut found = BTreeSet::new();
        flag_tokens("goto_if_set FLAG_A, X\nMY_FLAG_B FLAG_ FLAG_C\"", &mut found);
        assert_eq!(found.into_iter().collect::<Vec<_>>(), ["FLAG_A", "FLAG_C"]);
    }
}
