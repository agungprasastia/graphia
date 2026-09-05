use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{GraphiaError, Result};

const SKILL_FILES: [(&str, &str); 3] = [
    ("SKILL.md", include_str!("../../skills/graphia/SKILL.md")),
    (
        "references/commands.md",
        include_str!("../../skills/graphia/references/commands.md"),
    ),
    (
        "agents/openai.yaml",
        include_str!("../../skills/graphia/agents/openai.yaml"),
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillState {
    Missing,
    Stale,
    Current,
}

#[derive(Debug)]
pub struct SkillTargets {
    root: PathBuf,
    paths: Vec<PathBuf>,
}

impl SkillTargets {
    pub(crate) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn target_count(&self) -> usize {
        self.paths.len()
    }
}

#[derive(Debug, Default)]
pub struct InstallSummary {
    pub installed: usize,
    pub failures: Vec<(PathBuf, String)>,
}

impl std::fmt::Display for SkillState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("missing"),
            Self::Stale => f.write_str("stale"),
            Self::Current => f.write_str("current"),
        }
    }
}

pub fn user_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("GRAPHIA_INSTALL_HOME") {
        return Ok(PathBuf::from(path));
    }

    #[cfg(target_os = "windows")]
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let home = std::env::var_os("HOME").map(PathBuf::from);

    home.ok_or_else(|| GraphiaError::Storage {
        message: "cannot determine user home for Graphia skill installation".into(),
    })
}

pub fn user_targets(home: &Path) -> Result<SkillTargets> {
    let root = canonical_root(home)?;
    let paths = [
        ".codex/skills/graphia",
        ".claude/skills/graphia",
        ".agents/skills/graphia",
        ".copilot/skills/graphia",
        ".config/opencode/skills/graphia",
    ]
    .into_iter()
    .map(|path| root.join(path))
    .collect();
    Ok(SkillTargets { root, paths })
}

pub fn project_target(repo_root: &Path) -> Result<SkillTargets> {
    let root = canonical_root(repo_root)?;
    Ok(SkillTargets {
        paths: vec![root.join(".agents/skills/graphia")],
        root,
    })
}

pub fn status(targets: &SkillTargets) -> Result<SkillState> {
    let mut found = 0;
    let mut missing = 0;
    for target in &targets.paths {
        for (relative, expected) in SKILL_FILES {
            let path = target.join(relative);
            verify_containment(&targets.root, &path)?;
            match fs::read(&path) {
                Ok(actual) => {
                    found += 1;
                    if actual != expected.as_bytes() {
                        return Ok(SkillState::Stale);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing += 1,
                Err(error) => {
                    return Err(GraphiaError::Io {
                        path,
                        message: error.to_string(),
                    });
                }
            }
        }
    }
    Ok(match (found, missing) {
        (0, _) => SkillState::Missing,
        (_, 0) => SkillState::Current,
        _ => SkillState::Stale,
    })
}

pub fn install(targets: &SkillTargets) -> InstallSummary {
    let mut summary = InstallSummary::default();
    if let Err(error) = fs::create_dir_all(&targets.root) {
        summary.failures.push((
            targets.root.clone(),
            GraphiaError::Io {
                path: targets.root.clone(),
                message: error.to_string(),
            }
            .to_string(),
        ));
        return summary;
    }
    for target in &targets.paths {
        match install_target(&targets.root, target) {
            Ok(()) => summary.installed += 1,
            Err(error) => summary.failures.push((target.clone(), error.to_string())),
        }
    }
    summary
}

fn install_target(root: &Path, target: &Path) -> Result<()> {
    for (relative, _) in SKILL_FILES {
        verify_containment(root, &target.join(relative))?;
    }
    for (relative, content) in SKILL_FILES {
        let path = target.join(relative);
        crate::storage::atomic_write(&path, content.as_bytes())?;
        verify_containment(root, &path)?;
    }
    Ok(())
}

fn canonical_root(root: &Path) -> Result<PathBuf> {
    if root.exists() {
        root.canonicalize().map_err(|error| GraphiaError::Io {
            path: root.to_path_buf(),
            message: error.to_string(),
        })
    } else {
        std::path::absolute(root).map_err(|error| GraphiaError::Io {
            path: root.to_path_buf(),
            message: error.to_string(),
        })
    }
}

pub(crate) fn verify_containment(root: &Path, path: &Path) -> Result<()> {
    if !path.starts_with(root) {
        return Err(GraphiaError::InvalidArgument(format!(
            "skill path escapes installation root: {}",
            path.display()
        )));
    }

    if !root.exists() {
        return Ok(());
    }
    let resolved_root = root.canonicalize().map_err(|error| GraphiaError::Io {
        path: root.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut existing = path;
    while !existing.exists() {
        existing = existing.parent().ok_or_else(|| {
            GraphiaError::InvalidArgument(format!(
                "skill path has no existing ancestor: {}",
                path.display()
            ))
        })?;
    }
    let resolved = existing.canonicalize().map_err(|error| GraphiaError::Io {
        path: existing.to_path_buf(),
        message: error.to_string(),
    })?;
    if !resolved.starts_with(&resolved_root) {
        return Err(GraphiaError::InvalidArgument(format!(
            "skill path resolves outside installation root: {}",
            path.display()
        )));
    }
    Ok(())
}

pub fn require_install_success(summary: &InstallSummary) -> Result<()> {
    if summary.installed > 0 {
        return Ok(());
    }
    let message = summary.failures.first().map_or_else(
        || "no skill targets selected".into(),
        |(_, error)| error.clone(),
    );
    Err(GraphiaError::Storage {
        message: format!("Graphia skill installation failed: {message}"),
    })
}

pub fn print_install_warnings(summary: &InstallSummary) {
    for (target, error) in &summary.failures {
        eprintln!(
            "  [!] Could not install Graphia skill at {}: {error}",
            target.display()
        );
    }
}

#[cfg(test)]
fn write_stale_file(targets: &SkillTargets, relative: &str, content: &str) {
    if let Some(target) = targets.paths.first() {
        fs::write(target.join(relative), content).expect("stale write");
    }
}

#[cfg(test)]
fn first_target(targets: &SkillTargets) -> &Path {
    targets.paths.first().expect("skill target")
}

#[cfg(test)]
mod tests {
    use super::{
        SkillState, first_target, install, project_target, require_install_success, status,
        user_targets, write_stale_file,
    };
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn user_install_is_complete_and_idempotent() {
        let home = tempdir().expect("home");
        let targets = user_targets(home.path()).expect("targets");
        assert_eq!(status(&targets).expect("status"), SkillState::Missing);
        let installed = install(&targets);
        require_install_success(&installed).expect("install");
        assert_eq!(installed.installed, 5);
        assert_eq!(status(&targets).expect("status"), SkillState::Current);
        let reinstalled = install(&targets);
        require_install_success(&reinstalled).expect("reinstall");
        assert_eq!(reinstalled.installed, 5);
        assert_eq!(status(&targets).expect("status"), SkillState::Current);
    }

    #[test]
    fn status_detects_stale_content() {
        let home = tempdir().expect("home");
        let targets = user_targets(home.path()).expect("targets");
        require_install_success(&install(&targets)).expect("install");
        write_stale_file(&targets, "SKILL.md", "old");
        assert_eq!(status(&targets).expect("status"), SkillState::Stale);
    }

    #[test]
    fn project_install_uses_shared_agents_location() {
        let repo = tempdir().expect("repo");
        let targets = project_target(repo.path()).expect("targets");
        require_install_success(&install(&targets)).expect("install");
        assert_eq!(status(&targets).expect("status"), SkillState::Current);
        assert!(first_target(&targets).ends_with(".agents/skills/graphia"));
    }

    #[test]
    fn install_continues_after_one_adapter_fails() {
        let home = tempdir().expect("home");
        let targets = user_targets(home.path()).expect("targets");
        fs::create_dir_all(first_target(&targets)).expect("first target");
        fs::write(first_target(&targets).join("references"), "collision").expect("blocking file");

        let summary = install(&targets);
        assert_eq!(summary.installed, 4);
        assert_eq!(summary.failures.len(), 1);
    }

    #[test]
    fn status_propagates_non_not_found_read_errors() {
        let home = tempdir().expect("home");
        let targets = user_targets(home.path()).expect("targets");
        fs::create_dir_all(first_target(&targets).join("SKILL.md")).expect("directory collision");
        assert!(status(&targets).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn install_rejects_target_symlink_outside_root() {
        use std::os::unix::fs::symlink;

        let home = tempdir().expect("home");
        let outside = tempdir().expect("outside");
        fs::create_dir_all(home.path().join(".codex/skills")).expect("parents");
        symlink(outside.path(), home.path().join(".codex/skills/graphia")).expect("symlink");

        let targets = user_targets(home.path()).expect("targets");
        let summary = install(&targets);
        assert_eq!(summary.installed, 4);
        assert_eq!(summary.failures.len(), 1);
        assert!(!outside.path().join("SKILL.md").exists());
    }
}
