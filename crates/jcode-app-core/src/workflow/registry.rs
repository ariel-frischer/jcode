//! Private passive registrations. Registration never transfers process ownership.
use crate::bus::WorkflowSnapshot;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

pub(super) const MAX_ARTIFACT_BYTES: u64 = 512 * 1024;
pub(super) const MAX_REGISTRATIONS: usize = 64;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObserveInput {
    pub working_dir: PathBuf,
    pub tasks_file: PathBuf,
    pub status_file: Option<PathBuf>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Registration {
    pub id: String,
    pub owner: String,
    pub working_dir: PathBuf,
    pub tasks_file: PathBuf,
    pub status_file: Option<PathBuf>,
    pub label: String,
    pub registered_at: u64,
    pub checkpoint_at: Option<u64>,
    #[serde(default)]
    pub activity_at: Option<u64>,
    #[serde(default)]
    pub terminal_at: Option<u64>,
    #[serde(default)]
    pub lifecycle: Option<super::ObservedLifecycle>,
    pub last_good: Option<WorkflowSnapshot>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct Registry {
    pub registrations: Vec<Registration>,
}

impl Registry {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = match bounded_read(path) {
            Ok(bytes) => bytes,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.context("workflow registry is unreadable")),
        };
        let registry: Self = serde_json::from_slice(&bytes).map_err(|_| {
            anyhow::anyhow!("workflow registry is invalid; preserve it before repair")
        })?;
        registry.validate()?;
        Ok(registry)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if serde_json::to_vec(self)?.len() as u64 > MAX_ARTIFACT_BYTES {
            bail!("workflow registry exceeds 512 KiB");
        }
        crate::storage::write_json_secret(path, self)
    }

    fn validate(&self) -> Result<()> {
        if self.registrations.len() > MAX_REGISTRATIONS {
            bail!("workflow registry exceeds registration limit");
        }
        let mut ids = HashSet::new();
        let mut roots = HashSet::new();
        for run in &self.registrations {
            if run.id.is_empty()
                || run.owner.is_empty()
                || !run.working_dir.is_absolute()
                || !ids.insert(&run.id)
                || !roots.insert(&run.working_dir)
            {
                bail!("workflow registry contains invalid or duplicate ownership");
            }
            for path in std::iter::once(&run.tasks_file).chain(run.status_file.iter()) {
                let relative = path
                    .strip_prefix(&run.working_dir)
                    .context("workflow registry artifact is outside its worktree")?;
                if relative.as_os_str().is_empty()
                    || relative
                        .components()
                        .any(|c| !matches!(c, Component::Normal(_)))
                {
                    bail!("workflow registry artifact path is not normalized");
                }
            }
        }
        Ok(())
    }

    pub fn register(&mut self, owner: &str, input: ObserveInput, now: u64) -> Result<String> {
        if owner.is_empty() || !input.working_dir.is_absolute() {
            bail!("observation requires a session owner and an absolute worktree path");
        }
        let root = input
            .working_dir
            .canonicalize()
            .context("observed worktree is unavailable")?;
        if !root.is_dir() {
            bail!("observed worktree must be a directory");
        }
        let tasks_file = artifact_path(&root, &input.tasks_file)?;
        let status_file = input
            .status_file
            .as_deref()
            .map(|p| artifact_path(&root, p))
            .transpose()?;
        if let Some(existing) = self.registrations.iter().find(|r| r.working_dir == root) {
            if existing.owner != owner
                || existing.tasks_file != tasks_file
                || existing.status_file != status_file
            {
                bail!(
                    "worktree is already observed; only its owner may unobserve the existing run"
                );
            }
            return Ok(existing.id.clone());
        }
        if self.registrations.len() >= MAX_REGISTRATIONS {
            bail!("workflow observation limit reached; unobserve finished runs first");
        }
        let label = super::display_text(input.label.as_deref().unwrap_or("Autospec"));
        if label.trim().is_empty() {
            bail!("workflow label must not be blank");
        }
        let id = crate::id::new_id("workflow");
        self.registrations.push(Registration {
            id: id.clone(),
            owner: owner.into(),
            working_dir: root,
            tasks_file,
            status_file,
            label,
            registered_at: now,
            checkpoint_at: None,
            activity_at: None,
            terminal_at: None,
            lifecycle: None,
            last_good: None,
        });
        Ok(id)
    }

    pub fn remove(&mut self, owner: &str, id: &str) -> bool {
        let before = self.registrations.len();
        self.registrations
            .retain(|r| r.owner != owner || r.id != id);
        before != self.registrations.len()
    }
}

pub(super) fn bounded_read(path: &Path) -> Result<Vec<u8>> {
    super::artifact::bounded_read(path)
}

pub(super) fn read_artifact(root: &Path, path: &Path) -> Result<Vec<u8>> {
    // Revalidate on every observation, including artifacts that did not exist at registration.
    artifact_path(root, path)?;
    bounded_read(path)
}

fn artifact_path(root: &Path, path: &Path) -> Result<PathBuf> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root)
            .context("artifact must be inside observed worktree")?
    } else {
        path
    };
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        bail!("artifact must be a normalized worktree-relative file path");
    }
    let joined = root.join(relative);
    // Missing future artifacts are valid, but existing ancestor aliases may not escape.
    for ancestor in joined.ancestors() {
        match ancestor.canonicalize() {
            Ok(existing) => {
                if !existing.starts_with(root) {
                    bail!("artifact resolves outside observed worktree");
                }
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(root: &Path) -> ObserveInput {
        ObserveInput {
            working_dir: root.into(),
            tasks_file: "specs/demo/tasks.yaml".into(),
            status_file: None,
            label: Some("Contracts".into()),
        }
    }

    #[test]
    fn workflow_registration_is_idempotent_owned_and_passive() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::default();
        let id = registry
            .register("parent-a", input(dir.path()), 100)
            .unwrap();
        assert_eq!(
            registry
                .register("parent-a", input(dir.path()), 101)
                .unwrap(),
            id
        );
        assert!(
            registry
                .register("parent-b", input(dir.path()), 101)
                .is_err()
        );
        assert!(!registry.remove("parent-b", &id));
        assert!(registry.remove("parent-a", &id));
        assert!(!dir.path().join("specs").exists());
    }

    #[test]
    fn workflow_paths_cannot_escape_registered_worktree() {
        let dir = tempfile::tempdir().unwrap();
        assert!(artifact_path(dir.path(), Path::new("../credentials.json")).is_err());
        assert!(artifact_path(dir.path(), Path::new("/outside/tasks.yaml")).is_err());
        assert!(artifact_path(dir.path(), Path::new("specs/demo/tasks.yaml")).is_ok());
    }

    #[test]
    fn workflow_reads_are_bounded_and_regular_file_only() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tasks.yaml");
        std::fs::write(&file, b"phases: []").unwrap();
        assert_eq!(bounded_read(&file).unwrap(), b"phases: []");
        std::fs::write(&file, vec![b'x'; MAX_ARTIFACT_BYTES as usize + 1]).unwrap();
        assert!(bounded_read(&file).is_err());
        assert!(bounded_read(dir.path()).is_err());
        #[cfg(unix)]
        {
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&file, &link).unwrap();
            assert!(bounded_read(&link).is_err());
        }
    }

    #[test]
    fn workflow_registry_restores_identity_and_last_good_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("private/registry.json");
        let mut registry = Registry::default();
        let id = registry.register("owner", input(dir.path()), 10).unwrap();
        registry.registrations[0].last_good = Some(WorkflowSnapshot {
            id: id.clone(),
            completed: Some(2),
            total: Some(3),
            ..Default::default()
        });
        registry.save(&state).unwrap();
        let restored = Registry::load(&state).unwrap();
        assert_eq!(restored.registrations[0].id, id);
        assert_eq!(
            restored.registrations[0]
                .last_good
                .as_ref()
                .unwrap()
                .completed,
            Some(2)
        );
        std::fs::write(&state, b"{").unwrap();
        assert!(Registry::load(&state).is_err());
        assert!(
            Registry::load(&dir.path().join("missing"))
                .unwrap()
                .registrations
                .is_empty()
        );
    }

    #[test]
    fn workflow_registry_rejects_duplicate_ownership_and_oversized_state() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("registry.json");
        let mut registry = Registry::default();
        registry.register("owner", input(dir.path()), 10).unwrap();
        let mut duplicate = registry.registrations[0].clone();
        duplicate.owner = "intruder".into();
        registry.registrations.push(duplicate);
        std::fs::write(&state, serde_json::to_vec(&registry).unwrap()).unwrap();
        assert!(Registry::load(&state).is_err());
        std::fs::write(&state, vec![b' '; MAX_ARTIFACT_BYTES as usize + 1]).unwrap();
        assert!(Registry::load(&state).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn workflow_read_rejects_parent_replaced_with_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("specs")).unwrap();
        let path = artifact_path(dir.path(), Path::new("specs/tasks.yaml")).unwrap();
        std::fs::write(outside.path().join("tasks.yaml"), b"private").unwrap();
        std::fs::remove_dir(dir.path().join("specs")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("specs")).unwrap();
        assert!(read_artifact(dir.path(), &path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn workflow_reads_never_follow_concurrently_swapped_ancestors() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let parent = dir.path().join("specs");
        let backup = dir.path().join("safe");
        let link = dir.path().join("alias");
        std::fs::create_dir(&parent).unwrap();
        std::fs::write(parent.join("tasks.yaml"), b"public").unwrap();
        std::fs::write(outside.path().join("tasks.yaml"), b"private").unwrap();
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for _ in 0..200 {
                    std::fs::rename(&parent, &backup).unwrap();
                    std::fs::rename(&link, &parent).unwrap();
                    std::fs::rename(&parent, &link).unwrap();
                    std::fs::rename(&backup, &parent).unwrap();
                }
            });
            for _ in 0..1000 {
                if let Ok(bytes) = read_artifact(dir.path(), &parent.join("tasks.yaml")) {
                    assert_eq!(bytes, b"public");
                }
            }
        });
    }
}
