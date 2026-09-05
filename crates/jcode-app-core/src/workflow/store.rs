//! Process-local serialized access to passive registrations. No process or model ownership.
use super::registry::{ObserveInput, Registry};
use crate::{
    bus::{WorkflowHealth, WorkflowSnapshot},
    config::WorkflowConfig,
};
use anyhow::{Result, bail};
use std::{path::PathBuf, sync::Mutex};

struct StoreState {
    registry: Registry,
    dirty: bool,
}

pub struct WorkflowStore {
    // Stable sidecar inode, held until store shutdown. Never lock the atomically replaced JSON.
    _writer_lock: Option<std::fs::File>,
    path: PathBuf,
    config: WorkflowConfig,
    state: Mutex<StoreState>,
}

impl WorkflowStore {
    pub fn open(path: PathBuf, config: WorkflowConfig) -> Result<Self> {
        config.validate().map_err(anyhow::Error::msg)?;
        let writer_lock = if config.enabled && config.autospec_enabled {
            Some(writer_lock(&path)?)
        } else {
            None
        };
        let registry = if config.enabled && config.autospec_enabled {
            Registry::load(&path)?
        } else {
            Registry::default()
        };
        Ok(Self {
            _writer_lock: writer_lock,
            path,
            config,
            state: Mutex::new(StoreState {
                registry,
                dirty: false,
            }),
        })
    }

    pub fn register(&self, owner: &str, input: ObserveInput, now: u64) -> Result<String> {
        if !self.config.enabled || !self.config.autospec_enabled {
            bail!("workflow observation requires workflow.enabled and workflow.autospec_enabled");
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workflow registry lock unavailable"))?;
        let mut candidate = state.registry.clone();
        let id = candidate.register(owner, input, now)?;
        candidate.save(&self.path)?;
        state.registry = candidate;
        state.dirty = false;
        Ok(id)
    }

    pub fn unobserve(&self, owner: &str, id: &str) -> Result<bool> {
        if !self.config.enabled || !self.config.autospec_enabled {
            bail!("workflow observation is disabled");
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workflow registry lock unavailable"))?;
        let mut candidate = state.registry.clone();
        if !candidate.remove(owner, id) {
            return Ok(false);
        }
        candidate.save(&self.path)?;
        state.registry = candidate;
        state.dirty = false;
        Ok(true)
    }

    pub fn snapshots(&self, now: u64) -> Result<Vec<(String, WorkflowSnapshot)>> {
        if !self.config.enabled || !self.config.autospec_enabled {
            return Ok(Vec::new());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workflow registry lock unavailable"))?;
        let previous = serde_json::to_vec(&state.registry)?;
        let mut snapshots = Vec::new();
        for run in &mut state.registry.registrations {
            let snapshot = super::observe(run, now, self.config.quiet_seconds);
            if !run
                .terminal_at
                .is_some_and(|at| now.saturating_sub(at) > self.config.terminal_retention_seconds)
            {
                snapshots.push((run.owner.clone(), snapshot));
            }
        }
        state.dirty |= previous != serde_json::to_vec(&state.registry)?;
        if state.dirty {
            match state.registry.save(&self.path) {
                Ok(()) => state.dirty = false,
                Err(_) => {
                    for (_, snapshot) in &mut snapshots {
                        if !super::observer::is_terminal(snapshot.health) {
                            snapshot.health = WorkflowHealth::ObserverError;
                        }
                        let detail = snapshot.detail.as_deref().unwrap_or("");
                        snapshot.detail = Some(super::display_text(&format!(
                            "{detail} Snapshot persistence failed; reconnect may be stale"
                        )));
                    }
                }
            }
        }
        Ok(snapshots)
    }
}

fn writer_lock(path: &std::path::Path) -> Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path.with_extension("lock"))?;
    if !file.metadata()?.is_file() {
        bail!("workflow registry lock must be a regular file");
    }
    file.try_lock().map_err(|_| {
        anyhow::anyhow!(
            "workflow registry is owned by another observer; use that daemon or wait for it to exit"
        )
    })?;
    Ok(file)
}
