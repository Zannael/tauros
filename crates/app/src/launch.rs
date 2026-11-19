//! Command-line selection and preparation of a ROM or a Gen 3 boot target.
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use tauros_connector_gen3::boot::{map_numbers, BootPlan, BootTarget, GameLayout, Symbols};
use tauros_connector_gen3::toolchain::Toolchain;
use tauros_connector_gen3::Gen3Connector;
use tauros_model::{Connector, FlagId, MapId};

pub const USAGE: &str = "Usage: tauros-app [ROM.gba]\n       tauros-app --project DIR --map MAP_NAME --x X --y Y [--flag FLAG_NAME ...]";

#[derive(Clone, Debug)]
pub enum Launch {
    Rom(PathBuf),
    Point { project: PathBuf, target: BootTarget },
}

impl Launch {
    /// Parses all arguments after the executable name. Empty means file picker.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Self>, String> {
        let args: Vec<_> = args.into_iter().collect();
        if args.is_empty() {
            return Ok(None);
        }
        if args.len() == 1 && !args[0].to_string_lossy().starts_with('-') {
            return Ok(Some(Self::Rom(PathBuf::from(&args[0]))));
        }
        let (mut project, mut map, mut x, mut y) = (None, None, None, None);
        let mut flags = Vec::new();
        let mut pairs = args.chunks_exact(2);
        for pair in &mut pairs {
            let value = || pair[1].to_str().ok_or_else(|| "expected UTF-8 option value".to_owned());
            match pair[0].to_str() {
                Some("--project") if project.is_none() => project = Some(PathBuf::from(&pair[1])),
                Some("--map") if map.is_none() => map = Some(MapId::from(value()?)),
                Some("--x") if x.is_none() => x = Some(value()?.parse::<i16>().map_err(|_| "--x requires an i16 coordinate")?),
                Some("--y") if y.is_none() => y = Some(value()?.parse::<i16>().map_err(|_| "--y requires an i16 coordinate")?),
                Some("--flag") => flags.push(FlagId::from(value()?)),
                _ => return Err(format!("unknown or repeated option: {}\n{USAGE}", pair[0].to_string_lossy())),
            }
        }
        if !pairs.remainder().is_empty() {
            return Err(format!("missing option value\n{USAGE}"));
        }
        let missing = || format!("--project, --map, --x and --y must be supplied together\n{USAGE}");
        Ok(Some(Self::Point {
            project: project.ok_or_else(missing)?,
            target: BootTarget { map: map.ok_or_else(missing)?, x: x.ok_or_else(missing)?, y: y.ok_or_else(missing)?, flags },
        }))
    }

    /// Reads a built project; snapshots pending source edits before compiling the
    /// offset probe under ignored build/tauros. Never builds or patches the ROM.
    /// The project's pokeemerald.gba and .sym must come from the same build.
    pub fn prepare(&self) -> Result<PreparedLaunch, String> {
        let read = |path: &Path| fs::read(path).map_err(|e| format!("{}: {e}", path.display()));
        match self {
            Self::Rom(path) => Ok(PreparedLaunch {
                name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                rom: read(path)?,
                boot: None,
            }),
            Self::Point { project, target } => {
                let rom = read(&project.join("pokeemerald.gba"))?;
                let symbols = fs::read_to_string(project.join("pokeemerald.sym")).map_err(|e| format!("pokeemerald.sym: {e}"))?;
                let connector = Gen3Connector::open(project, None).map_err(|e| e.to_string())?;
                let snapshot = connector.load().map_err(|e| e.to_string())?;
                let maps = map_numbers(project).map_err(|e| e.to_string())?;
                connector.guard().snapshot("measure boot layout").map_err(|e| e.to_string())?;
                let toolchain = Toolchain::for_repository(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))?;
                let layout = GameLayout::measure(&toolchain, project).map_err(|e| e.to_string())?;
                let boot = BootPlan::new(target.clone(), &snapshot, &maps, &Symbols::parse(&symbols), layout).map_err(|e| e.to_string())?;
                Ok(PreparedLaunch { name: format!("{} ({}, {})", target.map.0, target.x, target.y), rom, boot: Some(boot) })
            }
        }
    }
}

pub struct PreparedLaunch {
    pub name: String,
    pub rom: Vec<u8>,
    pub boot: Option<BootPlan>,
}
