//! Append-only trainer constants. Existing numeric ids are never reused or moved.
use crate::{flags, scripts};
use std::collections::BTreeMap;

pub const HEADER: &str = "include/constants/opponents.h";

/// Allocate in memory; the caller writes this header together with trainers.party
/// in one snapshot-guarded transaction. Only the Emerald table is supported.
pub fn allocate(text: &str, names: impl IntoIterator<Item = String>) -> Result<String, String> {
    let mut out = text.to_owned();
    for name in names {
        if !name.starts_with("TRAINER_") || !scripts::is_identifier(&name) {
            return Err(format!("{name:?} must be a TRAINER_ identifier"));
        }
        let defs = flags::active_defines(&out, &[("IS_FRLG".into(), 0), ("TESTING".into(), 0)].into());
        let macros: BTreeMap<_, _> = defs.iter().map(|(n, _, b)| (n.clone(), b.clone())).collect();
        if macros.contains_key(&name) {
            continue;
        }
        let count = flags::evaluate("TRAINERS_COUNT_EMERALD", &macros).ok_or("cannot resolve TRAINERS_COUNT_EMERALD")?;
        let max = flags::evaluate("MAX_TRAINERS_COUNT_EMERALD", &macros).ok_or("cannot resolve MAX_TRAINERS_COUNT_EMERALD")?;
        if count <= 0 || count >= max {
            return Err(format!("no free Emerald trainer slots ({count}/{max})"));
        }
        if macros.keys().filter(|n| n.starts_with("TRAINER_")).any(|n| flags::evaluate(n, &macros).is_none_or(|v| v >= count)) {
            return Err("trainer ids are unresolved or overlap the allocation range".into());
        }
        let mut lines: Vec<String> = out.split_inclusive('\n').map(str::to_owned).collect();
        let line = defs.iter().find(|(n, _, _)| n == "TRAINERS_COUNT_EMERALD").ok_or("missing trainer count")?.1;
        let source = &lines[line];
        let start = source.find("TRAINERS_COUNT_EMERALD").unwrap() + "TRAINERS_COUNT_EMERALD".len();
        let value_start = start + source[start..].len() - source[start..].trim_start().len();
        let value_end = value_start + source[value_start..].find(char::is_whitespace).unwrap_or(source.len() - value_start);
        if source[value_start..value_end].parse::<i64>().ok() != Some(count) {
            return Err("TRAINERS_COUNT_EMERALD must be a literal decimal count".into());
        }
        let mut updated = source.clone();
        updated.replace_range(value_start..value_end, &(count + 1).to_string());
        let newline = if source.ends_with("\r\n") { "\r\n" } else { "\n" };
        lines[line] = format!("#define {name} {count}{newline}{updated}");
        out = lines.concat();
    }
    Ok(out)
}
