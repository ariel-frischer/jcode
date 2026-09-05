//! Private passive registrations. Registration never transfers process ownership.
use crate::bus::WorkflowSnapshot;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::Read;
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
    pub last_good: Option<WorkflowSnapshot>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct Registry {
    pub registrations: Vec<Registration>,
}

impl Registry {
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
    if !std::fs::symlink_metadata(path)?.file_type().is_file() {
        bail!("workflow artifact must be a regular file, not a symlink");
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Do not block on a FIFO or follow a leaf swapped after the metadata check.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_ARTIFACT_BYTES {
        bail!("workflow artifact must be a regular file of at most 512 KiB");
    }
    let mut bytes = Vec::new();
    file.take(MAX_ARTIFACT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_ARTIFACT_BYTES {
        bail!("workflow artifact grew beyond 512 KiB");
    }
    Ok(bytes)
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
}
