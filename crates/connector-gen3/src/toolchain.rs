//! Builds, tests and Poryscript compilation in the FND-002 toolchain container
//! (G3-005).
//!
//! Every command runs through [`Toolchain::hardened_args`], which mirrors
//! `hardened_run` in `scripts/setup_toolchain.py` (no network, host uid/gid, no
//! capabilities, no new privileges, read-only root, tmpfs `/tmp`, only the project
//! mounted); a test keeps the two in step. `make` is incremental: the container is
//! fresh but the project directory keeps its build tree between runs. Compiler,
//! assembler, trainerproc and make errors are parsed into [`BuildDiagnostic`]s, and
//! the expansion's headless test runner (`make check`) into [`TestCase`]s.

use crate::json::{Document, Node};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Mount point of the project inside the container.
const WORKDIR: &str = "/work";
/// Poryscript's configs ship with its release in the image (toolchain/gba/Dockerfile).
const PORYSCRIPT_FONTS: &str = "/opt/poryscript-linux/font_config.json";
const PORYSCRIPT_COMMANDS: &str = "/opt/poryscript-linux/command_config.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolchain {
    /// Docker image tag, e.g. `tauros-gba-toolchain:d75267a55161`.
    pub image: String,
    pub jobs: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildDiagnostic {
    pub level: Level,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct BuildReport {
    pub success: bool,
    pub duration: Duration,
    pub diagnostics: Vec<BuildDiagnostic>,
    /// Combined stdout and stderr.
    pub log: String,
}

impl BuildReport {
    pub fn errors(&self) -> impl Iterator<Item = &BuildDiagnostic> {
        self.diagnostics.iter().filter(|d| d.level == Level::Error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestCase {
    /// Index the runner printed (`[N]`), the test name and its result as printed
    /// (`PASS`, `FAIL`, `KNOWN_FAILING`, `ASSUMPTION_FAIL`, `TODO`, ...).
    pub runner: u32,
    pub name: String,
    pub result: String,
    /// `file:line` of a failure, from the runner's summary.
    pub location: Option<(String, u32)>,
    /// Output the test printed (assertion messages).
    pub output: String,
}

impl TestCase {
    pub fn passed(&self) -> bool {
        self.result == "PASS"
    }
}

#[derive(Clone, Debug)]
pub struct TestReport {
    /// The runner exited successfully (no unexpected failures).
    pub success: bool,
    pub duration: Duration,
    pub cases: Vec<TestCase>,
    /// Totals from the runner's summary (`PASSED`, `FAILED`, `TOTAL`, ...).
    pub totals: BTreeMap<String, u32>,
    /// Build errors when the test ROM did not build.
    pub diagnostics: Vec<BuildDiagnostic>,
    pub log: String,
}

impl Toolchain {
    /// The image `scripts/setup_toolchain.py` builds for this repository: its name
    /// from `toolchain/sources.json` and the first 12 hex digits of the Dockerfile's
    /// sha256.
    pub fn for_repository(repo: &Path) -> Result<Self, String> {
        let manifest = fs::read_to_string(repo.join("toolchain/sources.json")).map_err(|e| e.to_string())?;
        let doc = Document::parse(&manifest).map_err(|e| e.to_string())?;
        let image = doc.root.as_object().and_then(|o| o.get("image")).and_then(Node::as_object).ok_or("no image in sources.json")?;
        let name = image.get("name").and_then(Node::as_str).ok_or("no image.name")?;
        let dockerfile = image.get("dockerfile").and_then(Node::as_str).ok_or("no image.dockerfile")?;
        let bytes = fs::read(repo.join(dockerfile)).map_err(|e| e.to_string())?;
        let digest = hex(&Sha256::digest(&bytes));
        let jobs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
        Ok(Self { image: format!("{name}:{}", &digest[..12]), jobs })
    }

    /// `docker run` arguments, identical to `hardened_run` in setup_toolchain.py.
    pub fn hardened_args(&self, workdir: Option<&Path>, script: &str) -> Vec<String> {
        let (uid, gid) = user_ids();
        let mut args: Vec<String> = [
            "docker", "run", "--rm", "--network", "none", "--user", &format!("{uid}:{gid}"), "--cap-drop", "ALL",
            "--security-opt", "no-new-privileges", "--read-only", "--tmpfs", "/tmp:rw,exec,size=1g", "--init",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if let Some(dir) = workdir {
            args.extend(["--volume".into(), format!("{}:{WORKDIR}:rw", dir.display()), "--workdir".into(), WORKDIR.into()]);
        }
        args.extend([self.image.clone(), "bash".into(), "-c".into(), script.to_owned()]);
        args
    }

    fn run(&self, workdir: Option<&Path>, script: &str) -> Result<(bool, String, Duration), String> {
        let args = self.hardened_args(workdir, script);
        let start = Instant::now();
        let output = Command::new(&args[0])
            .args(&args[1..])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("cannot run docker: {e}"))?;
        let mut log = String::from_utf8_lossy(&output.stdout).into_owned();
        log.push_str(&String::from_utf8_lossy(&output.stderr));
        Ok((output.status.success(), log, start.elapsed()))
    }

    /// Runs `make -j<jobs> <targets>` in the project; incremental across calls.
    pub fn make(&self, project: &Path, targets: &[&str]) -> Result<BuildReport, String> {
        let script = format!("make -j{} {}", self.jobs, targets.join(" "));
        let (success, log, duration) = self.run(Some(project), &script)?;
        let mut diagnostics = parse_diagnostics(&log);
        if !success && !diagnostics.iter().any(|d| d.level == Level::Error) {
            diagnostics.push(fallback_error(&log));
        }
        Ok(BuildReport { success, duration, diagnostics, log })
    }

    /// The ROM and symbols, from one invocation so both come from the same ELF.
    pub fn build_rom(&self, project: &Path) -> Result<BuildReport, String> {
        self.make(project, &["rom", "syms"])
    }

    /// Builds and runs the expansion's headless test runner (`make check`), limited
    /// to tests whose name starts with `filter` (or a `.c` file path) when given.
    pub fn run_tests(&self, project: &Path, filter: Option<&str>) -> Result<TestReport, String> {
        let mut script = format!("make -j{} check", self.jobs);
        if let Some(f) = filter {
            script.push_str(&format!(" TESTS={}", shell_quote(f)));
        }
        let (success, log, duration) = self.run(Some(project), &script)?;
        let (cases, totals) = parse_test_output(&log);
        let mut diagnostics = parse_diagnostics(&log);
        if !success && cases.is_empty() && !diagnostics.iter().any(|d| d.level == Level::Error) {
            diagnostics.push(fallback_error(&log));
        }
        Ok(TestReport { success, duration, cases, totals, diagnostics, log })
    }

    /// Compiles one Poryscript source to assembly, in a scratch directory holding
    /// only that file.
    pub fn compile_poryscript(&self, source: &str) -> Result<String, String> {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir: PathBuf = std::env::temp_dir().join(format!("tauros-poryscript-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let result = (|| {
            fs::write(dir.join("scene.pory"), source).map_err(|e| e.to_string())?;
            let script = format!("poryscript -i scene.pory -o scene.inc -fc {PORYSCRIPT_FONTS} -cc {PORYSCRIPT_COMMANDS} -lm=false");
            let (ok, log, _) = self.run(Some(&dir), &script)?;
            if !ok {
                return Err(log.trim().to_owned());
            }
            fs::read_to_string(dir.join("scene.inc")).map_err(|e| e.to_string())
        })();
        let _ = fs::remove_dir_all(&dir);
        result
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The process's real uid and gid (Linux `/proc/self/status`), like `os.getuid()`.
fn user_ids() -> (u32, u32) {
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    (field("Uid:"), field("Gid:"))
}

/// Removes ANSI colour escapes (`ESC [ ... letter`).
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn project_path(path: &str) -> String {
    path.strip_prefix(&format!("{WORKDIR}/")).unwrap_or(path).to_owned()
}

/// Parses `file:line[:col]: error|warning: message` (gcc, as, trainerproc,
/// mapjson) and `make: *** [file:line: target] Error N` lines.
pub fn parse_diagnostics(log: &str) -> Vec<BuildDiagnostic> {
    let mut out: Vec<BuildDiagnostic> = Vec::new();
    for raw in strip_ansi(log).lines() {
        let line = raw.trim_end();
        if let Some(d) = parse_located(line) {
            if !out.contains(&d) {
                out.push(d);
            }
        } else if let Some(rest) = line.strip_prefix("make: *** [").or_else(|| line.split_once("]: *** [").map(|(_, r)| r)) {
            let inner = rest.split(']').next().unwrap_or(rest);
            let mut parts = inner.splitn(3, ':');
            let file = parts.next().map(str::to_owned);
            let number = parts.next().and_then(|n| n.trim().parse().ok());
            out.push(BuildDiagnostic { level: Level::Error, file, line: number, column: None, message: line.to_owned() });
        } else if line.contains("undefined reference to") || line.contains("multiple definition of") {
            out.push(BuildDiagnostic { level: Level::Error, file: None, line: None, column: None, message: line.to_owned() });
        }
    }
    out
}

fn parse_located(line: &str) -> Option<BuildDiagnostic> {
    for (marker, level) in [
        (": fatal error: ", Level::Error),
        (": error: ", Level::Error),
        (": Error: ", Level::Error),
        (": warning: ", Level::Warning),
        (": Warning: ", Level::Warning),
    ] {
        let Some(at) = line.find(marker) else { continue };
        let location = &line[..at];
        let message = line[at + marker.len()..].to_owned();
        let mut parts = location.split(':');
        let file = parts.next()?;
        if file.is_empty() || file.contains(' ') {
            return None;
        }
        let number: Option<u32> = parts.next().and_then(|n| n.parse().ok());
        let column = parts.next().and_then(|n| n.parse().ok());
        // `arm-none-eabi-ld: warning: ...` names a tool, not a file.
        if number.is_none() {
            return Some(BuildDiagnostic { level, file: None, line: None, column: None, message: line.to_owned() });
        }
        return Some(BuildDiagnostic { level, file: Some(project_path(file)), line: number, column, message });
    }
    None
}

fn fallback_error(log: &str) -> BuildDiagnostic {
    let clean = strip_ansi(log);
    let tail: Vec<&str> = clean.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = tail[tail.len().saturating_sub(10)..].join("\n");
    BuildDiagnostic { level: Level::Error, file: None, line: None, column: None, message: format!("build failed:\n{tail}") }
}

/// Parses mgba-rom-test-hydra's output: `[N] Name: RESULT` lines (followed by the
/// test's own output), failure summaries `- file:line: Name: message` and totals
/// `- Tests PASSED: 12`.
pub fn parse_test_output(log: &str) -> (Vec<TestCase>, BTreeMap<String, u32>) {
    let clean = strip_ansi(log);
    let mut cases: Vec<TestCase> = Vec::new();
    let mut totals = BTreeMap::new();
    let mut in_summary = false;
    for line in clean.lines() {
        if let Some(case) = parse_case(line) {
            cases.push(case);
            in_summary = false;
            continue;
        }
        let trimmed = line.trim();
        if trimmed.ends_with("tests:") || trimmed.ends_with(":") && trimmed.contains("ASSUMPTIONS_FAILED") {
            in_summary = true;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- ") {
            if let Some((label, count)) = rest.split_once(':') {
                let number = count.split_whitespace().next().and_then(|n| n.parse::<u32>().ok());
                let key = label.trim().trim_start_matches("Tests").trim();
                if let Some(n) = number {
                    if !key.is_empty() && key.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                        totals.insert(key.to_owned(), n);
                        continue;
                    }
                }
            }
            if in_summary {
                // file:line: Name: message
                let mut parts = rest.splitn(4, ": ");
                if let (Some(location), Some(name)) = (parts.next(), parts.next()) {
                    if let Some((file, number)) = location.rsplit_once(':').and_then(|(f, n)| n.parse().ok().map(|n| (f, n))) {
                        let message = parts.collect::<Vec<_>>().join(": ");
                        for case in cases.iter_mut().filter(|c| c.name == name && c.location.is_none()) {
                            case.location = Some((file.to_owned(), number));
                            if case.output.is_empty() {
                                case.output = message.clone();
                            }
                        }
                    }
                }
            }
            continue;
        }
        if !in_summary {
            if let Some(last) = cases.last_mut() {
                if !trimmed.is_empty() {
                    if !last.output.is_empty() {
                        last.output.push('\n');
                    }
                    last.output.push_str(line);
                }
            }
        }
    }
    (cases, totals)
}

fn parse_case(line: &str) -> Option<TestCase> {
    let rest = line.strip_prefix('[')?;
    let (index, rest) = rest.split_once("] ")?;
    let runner = index.trim().parse().ok()?;
    let (name, result) = rest.rsplit_once(": ")?;
    let result = result.trim();
    if result.is_empty() || !result.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
        return None;
    }
    Some(TestCase { runner, name: name.to_owned(), result: result.to_owned(), location: None, output: String::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_and_make_errors_are_structured() {
        let log = "\
src/battle_main.c:12:5: error: 'foo' undeclared (first use in this function)
/work/src/data/trainers.party:42: error: expected one of 'EVs', 'IVs'
data/maps/X/scripts.inc:7: Error: unknown pseudo-op: `.bogus'
src/a.c:3:1: warning: unused variable 'x'
arm-none-eabi-ld: warning: pokeemerald.elf has a LOAD segment with RWX permissions
make: *** [Makefile:442: build/modern/src/battle_main.o] Error 1
make[1]: *** [Makefile:12: sub] Error 2
";
        let d = parse_diagnostics(log);
        assert_eq!(d[0], BuildDiagnostic { level: Level::Error, file: Some("src/battle_main.c".into()), line: Some(12), column: Some(5), message: "'foo' undeclared (first use in this function)".into() });
        assert_eq!((d[1].file.as_deref(), d[1].line), (Some("src/data/trainers.party"), Some(42)));
        assert_eq!((d[2].level, d[2].line), (Level::Error, Some(7)));
        assert_eq!(d[3].level, Level::Warning);
        assert_eq!((d[4].level, d[4].file.as_deref()), (Level::Warning, None));
        assert_eq!((d[5].file.as_deref(), d[5].line), (Some("Makefile"), Some(442)));
        assert_eq!(d.len(), 7);
    }

    #[test]
    fn runner_output_is_structured() {
        let log = "\
[0] Trainer Party Pools: \u{1b}[32mPASS\u{1b}[0m
[1] Trainer Party broken: \u{1b}[31mFAIL\u{1b}[0m
expected 3, got 2

  \u{1b}[31mFAILED\u{1b}[0m tests:
  - \u{1b}[31mtest/battle/trainer_control.c:99\u{1b}[0m: Trainer Party broken: expected 3, got 2

- Tests \u{1b}[31mFAILED\u{1b}[0m :         1    Add TESTS='X' to run tests with the defined prefix.
- Tests \u{1b}[32mPASSED\u{1b}[0m:          1
- Tests \u{1b}[34mTOTAL\u{1b}[0m:           2
";
        let (cases, totals) = parse_test_output(log);
        assert_eq!(cases.len(), 2);
        assert!(cases[0].passed());
        assert_eq!(cases[1].result, "FAIL");
        assert_eq!(cases[1].location, Some(("test/battle/trainer_control.c".into(), 99)));
        assert_eq!(cases[1].output, "expected 3, got 2");
        assert_eq!(totals.get("PASSED"), Some(&1));
        assert_eq!(totals.get("FAILED"), Some(&1));
        assert_eq!(totals.get("TOTAL"), Some(&2));
    }
}
