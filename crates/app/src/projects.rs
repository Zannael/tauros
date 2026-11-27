//! Game projects on disk (`APP-008`): the default projects folder, discovery of
//! the projects it contains, and the arguments that relaunch Tauros on one.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// `$TAUROS_DATA/projects`, where Tauros keeps its game projects by default.
pub fn default_projects_dir() -> PathBuf {
    let data = std::env::var_os("TAUROS_DATA").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/tauros")
    });
    data.join("projects")
}

/// A Gen 3 decomp project as Porymap and the connector see it.
pub fn looks_like_project(dir: &Path) -> bool {
    dir.join("data/maps/map_groups.json").is_file() && dir.join("include").is_dir()
}

/// Whether the project holds a ROM and its symbols from a build.
pub fn is_built(dir: &Path) -> bool {
    dir.join("pokeemerald.gba").is_file() && dir.join("pokeemerald.sym").is_file()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProjectEntry {
    pub name: String,
    pub path: String,
    pub built: bool,
}

/// Projects directly inside `dir`, by name. Unreadable or unrelated folders are skipped.
pub fn list_projects(dir: &Path) -> Result<Vec<ProjectEntry>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut projects: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && looks_like_project(path))
        .map(|path| ProjectEntry {
            name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            built: is_built(&path),
            path: path.to_string_lossy().into_owned(),
        })
        .collect();
    projects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(projects)
}

/// Command-line arguments that start Tauros on `project`, at `point` when the
/// project is built (a boot point needs the built ROM and symbols).
pub fn launch_args(project: &Path, point: Option<(&str, i16, i16)>) -> Result<Vec<OsString>, String> {
    if !project.is_dir() || !looks_like_project(project) {
        return Err(format!("{} is not a Gen 3 decomp project (data/maps/map_groups.json is missing)", project.display()));
    }
    let mut args: Vec<OsString> = vec!["--project".into(), project.as_os_str().to_owned()];
    if let (Some((map, x, y)), true) = (point, is_built(project)) {
        args.extend([
            "--map".into(), map.into(), "--x".into(), x.to_string().into(), "--y".into(), y.to_string().into(),
        ]);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(root: &Path, name: &str, built: bool) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(dir.join("data/maps")).unwrap();
        fs::create_dir_all(dir.join("include")).unwrap();
        fs::write(dir.join("data/maps/map_groups.json"), "{}").unwrap();
        if built {
            fs::write(dir.join("pokeemerald.gba"), [0u8; 4]).unwrap();
            fs::write(dir.join("pokeemerald.sym"), "").unwrap();
        }
        dir
    }

    #[test]
    fn lists_projects_with_their_build_state_and_skips_other_folders() {
        let root = std::env::temp_dir().join(format!("tauros-projects-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        project(&root, "beta-hack", false);
        project(&root, "Alpha", true);
        fs::create_dir_all(root.join("notes")).unwrap();
        fs::write(root.join("readme.txt"), "").unwrap();
        let found = list_projects(&root).unwrap();
        assert_eq!(found.iter().map(|p| (p.name.as_str(), p.built)).collect::<Vec<_>>(), [("Alpha", true), ("beta-hack", false)]);
        assert!(list_projects(&root.join("missing")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_boot_point_is_passed_only_for_built_projects() {
        let root = std::env::temp_dir().join(format!("tauros-launch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let built = project(&root, "built", true);
        let unbuilt = project(&root, "unbuilt", false);
        let args = launch_args(&built, Some(("MAP_PETALBURG_CITY", 15, 10))).unwrap();
        assert_eq!(args.iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>()[2..],
            ["--map", "MAP_PETALBURG_CITY", "--x", "15", "--y", "10"]);
        assert_eq!(launch_args(&unbuilt, Some(("MAP_PETALBURG_CITY", 15, 10))).unwrap().len(), 2);
        assert_eq!(launch_args(&built, None).unwrap().len(), 2);
        assert!(launch_args(&root.join("missing"), None).is_err());
        assert!(launch_args(&root, None).is_err(), "a folder of projects is not a project");
        fs::remove_dir_all(root).unwrap();
    }
}
