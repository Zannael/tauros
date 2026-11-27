//! Persisted emulator key bindings and audio/video/playback preferences.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tauros_emu::Button;

/// Largest integer multiple that fits the available space.
pub const DEFAULT_SCALE: &str = "auto";

/// Canonical key bindings in `Button::ALL` order (`a, b, select, start, right, left, up, down, r, l`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyMap {
    pub a: String,
    pub b: String,
    pub select: String,
    pub start: String,
    pub right: String,
    pub left: String,
    pub up: String,
    pub down: String,
    pub r: String,
    pub l: String,
}

impl Default for KeyMap {
    fn default() -> Self {
        Self {
            a: "x".into(),
            b: "z".into(),
            select: "backspace".into(),
            start: "enter".into(),
            right: "arrowright".into(),
            left: "arrowleft".into(),
            up: "arrowup".into(),
            down: "arrowdown".into(),
            r: "s".into(),
            l: "a".into(),
        }
    }
}

impl KeyMap {
    /// Returns the bit index (in `Button::ALL` order) bound to `raw_key` (GDK or Web key name).
    pub fn bit_for_raw_key(&self, raw_key: &str) -> Option<u16> {
        let key = normalize_key_name(raw_key)?;
        self.entries()
            .into_iter()
            .enumerate()
            .find(|(_, (_, bound))| bound.as_str() == key)
            .map(|(idx, _)| idx as u16)
    }

    /// Pairs each `Button` with its canonical bound key name.
    pub fn entries(&self) -> [(Button, &String); 10] {
        [
            (Button::A, &self.a),
            (Button::B, &self.b),
            (Button::Select, &self.select),
            (Button::Start, &self.start),
            (Button::Right, &self.right),
            (Button::Left, &self.left),
            (Button::Up, &self.up),
            (Button::Down, &self.down),
            (Button::R, &self.r),
            (Button::L, &self.l),
        ]
    }

    /// Normalizes every binding and rejects missing or duplicate keys.
    pub fn normalize_and_validate(&mut self) -> Result<(), String> {
        let fields = [
            ("A", &mut self.a),
            ("B", &mut self.b),
            ("Select", &mut self.select),
            ("Start", &mut self.start),
            ("Right", &mut self.right),
            ("Left", &mut self.left),
            ("Up", &mut self.up),
            ("Down", &mut self.down),
            ("R", &mut self.r),
            ("L", &mut self.l),
        ];
        let mut seen = HashSet::new();
        for (label, slot) in fields {
            let norm = normalize_key_name(slot)
                .ok_or_else(|| format!("unsupported key binding for {label}: {slot:?}"))?;
            if !seen.insert(norm.clone()) {
                return Err(format!("duplicate key '{norm}' assigned to {label}"));
            }
            *slot = norm;
        }
        Ok(())
    }
}

/// Normalizes either a GTK/GDK keyval name (`"Return"`, `"BackSpace"`, `"Left"`)
/// or a Web `KeyboardEvent.code`/`key` (`"KeyX"`, `"Enter"`, `"ArrowLeft"`)
/// into one shared lowercase key identifier.
pub fn normalize_key_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(letter) = lower.strip_prefix("key") {
        if letter.len() == 1 && letter.chars().all(|c| c.is_ascii_alphabetic()) {
            return Some(letter.to_owned());
        }
    }
    if let Some(digit) = lower.strip_prefix("digit") {
        if digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()) {
            return Some(digit.to_owned());
        }
    }
    if let Some(kp) = lower.strip_prefix("numpad") {
        if kp.len() == 1 && kp.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("numpad{kp}"));
        }
    }
    let canonical = match lower.as_str() {
        "return" | "enter" | "kp_enter" | "numpadenter" => "enter",
        "backspace" => "backspace",
        "space" | " " => "space",
        "tab" => "tab",
        "escape" | "esc" => "escape",
        "up" | "arrowup" => "arrowup",
        "down" | "arrowdown" => "arrowdown",
        "left" | "arrowleft" => "arrowleft",
        "right" | "arrowright" => "arrowright",
        "shift_l" | "shift_r" | "shiftleft" | "shiftright" | "shift" => "shift",
        "comma" | "," => "comma",
        "period" | "." => "period",
        "slash" | "/" => "slash",
        "semicolon" | ";" => "semicolon",
        "apostrophe" | "quote" | "'" => "quote",
        "bracketleft" | "[" => "bracketleft",
        "bracketright" | "]" => "bracketright",
        "minus" | "-" => "minus",
        "equal" | "=" => "equal",
        other if other.len() == 1 && other.chars().all(|c| c.is_ascii_alphanumeric()) => other,
        other if other.starts_with('f') && other[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) => other,
        _ => return None,
    };
    Some(canonical.to_owned())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmuPrefs {
    pub scale: String,
    pub aspect_locked: bool,
    pub muted: bool,
    pub volume: u8,
    pub speed: u8,
    /// Multiplier used when fast-forward is switched on (2, 4, 8, or 0 for unlimited).
    pub ff_speed: u8,
    pub pause_on_blur: bool,
    pub rewind_enabled: bool,
}

impl Default for EmuPrefs {
    fn default() -> Self {
        Self {
            scale: DEFAULT_SCALE.into(),
            aspect_locked: true,
            muted: false,
            volume: 100,
            speed: 1,
            ff_speed: 4,
            pause_on_blur: true,
            rewind_enabled: true,
        }
    }
}

impl EmuPrefs {
    pub fn sanitize(&mut self) {
        if self.scale == "fit" {
            self.scale = DEFAULT_SCALE.into();
        }
        if !matches!(self.scale.as_str(), "auto" | "1x" | "2x" | "3x" | "4x") {
            self.scale = DEFAULT_SCALE.into();
        }
        self.volume = self.volume.min(100);
        if !matches!(self.speed, 0 | 1 | 2 | 4 | 8) {
            self.speed = 1;
        }
        if !matches!(self.ff_speed, 0 | 2 | 4 | 8) {
            self.ff_speed = 4;
        }
    }

    /// A launch never resumes fast-forward silently: a saved multiplier other than
    /// normal becomes the fast-forward choice and playback starts at 1x.
    pub fn start_at_normal_speed(&mut self) {
        if self.speed != 1 {
            self.ff_speed = self.speed;
            self.speed = 1;
        }
        self.sanitize();
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmuConfig {
    pub keymap: KeyMap,
    pub prefs: EmuPrefs,
}

/// Returns the user-facing `$TAUROS_DATA/emulator` directory.
pub fn emulator_data_dir() -> PathBuf {
    let base = std::env::var_os("TAUROS_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/tauros")
        });
    base.join("emulator")
}

pub fn load_config(dir: &Path) -> EmuConfig {
    let path = dir.join("config.json");
    let Ok(text) = fs::read_to_string(path) else {
        return EmuConfig::default();
    };
    let mut cfg: EmuConfig = serde_json::from_str(&text).unwrap_or_default();
    if cfg.keymap.normalize_and_validate().is_err() {
        cfg.keymap = KeyMap::default();
    }
    cfg.prefs.sanitize();
    cfg
}

pub fn save_config(dir: &Path, config: &EmuConfig) -> Result<(), String> {
    let mut sanitized = config.clone();
    sanitized.keymap.normalize_and_validate()?;
    sanitized.prefs.sanitize();
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let path = dir.join("config.json");
    let tmp = dir.join(format!("config.json.tmp.{}", std::process::id()));
    let json = serde_json::to_string_pretty(&sanitized).map_err(|e| e.to_string())?;
    fs::write(&tmp, json).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path).map_err(|e| format!("cannot save {}: {e}", path.display()))
}

/// Helper for displaying a map from button name (`"a"`, `"start"`, ...) to key.
pub fn keymap_labels(keymap: &KeyMap) -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("a", keymap.a.clone()),
        ("b", keymap.b.clone()),
        ("select", keymap.select.clone()),
        ("start", keymap.start.clone()),
        ("right", keymap.right.clone()),
        ("left", keymap.left.clone()),
        ("up", keymap.up.clone()),
        ("down", keymap.down.clone()),
        ("r", keymap.r.clone()),
        ("l", keymap.l.clone()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_gdk_and_web_key_names_equivalently() {
        assert_eq!(normalize_key_name("KeyX").as_deref(), Some("x"));
        assert_eq!(normalize_key_name("X").as_deref(), Some("x"));
        assert_eq!(normalize_key_name("Return").as_deref(), Some("enter"));
        assert_eq!(normalize_key_name("KP_Enter").as_deref(), Some("enter"));
        assert_eq!(normalize_key_name("BackSpace").as_deref(), Some("backspace"));
        assert_eq!(normalize_key_name("Left").as_deref(), Some("arrowleft"));
        assert_eq!(normalize_key_name("ArrowLeft").as_deref(), Some("arrowleft"));
        assert_eq!(normalize_key_name("Shift_L").as_deref(), Some("shift"));
        assert_eq!(normalize_key_name("ShiftRight").as_deref(), Some("shift"));
    }

    #[test]
    fn rejects_duplicate_bindings_and_roundtrips_on_disk() {
        let mut dup = KeyMap::default();
        dup.b = dup.a.clone();
        assert!(dup.normalize_and_validate().is_err());

        let dir = std::env::temp_dir().join(format!("tauros-emu-cfg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut cfg = EmuConfig::default();
        cfg.keymap.a = "KeyJ".into();
        cfg.keymap.b = "KeyK".into();
        cfg.prefs.volume = 65;
        cfg.prefs.speed = 4;
        save_config(&dir, &cfg).unwrap();

        let loaded = load_config(&dir);
        assert_eq!(loaded.keymap.a, "j");
        assert_eq!(loaded.keymap.b, "k");
        assert_eq!(loaded.prefs.volume, 65);
        assert_eq!(loaded.prefs.speed, 4);
        assert_eq!(loaded.keymap.bit_for_raw_key("J"), Some(0));
        assert_eq!(loaded.keymap.bit_for_raw_key("KeyK"), Some(1));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn launch_starts_at_normal_speed_and_keeps_the_multiplier() {
        let mut prefs = EmuPrefs { speed: 8, ..EmuPrefs::default() };
        prefs.start_at_normal_speed();
        assert_eq!((prefs.speed, prefs.ff_speed), (1, 8));
        let mut normal = EmuPrefs { ff_speed: 2, ..EmuPrefs::default() };
        normal.start_at_normal_speed();
        assert_eq!((normal.speed, normal.ff_speed), (1, 2));
    }
}
