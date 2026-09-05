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
        let writer_lock = if config.enabled {
            Some(writer_lock(&path)?)
        } else {
            None
        };
        let registry = if config.enabled {
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

    #[cfg(test)]
    pub fn snapshots(&self, now: u64) -> Result<Vec<(String, WorkflowSnapshot)>> {
        self.snapshots_with_native(now, Vec::new())
    }

    pub fn snapshots_with_native(
        &self,
        now: u64,
        samples: Vec<super::NativeSample>,
    ) -> Result<Vec<(String, WorkflowSnapshot)>> {
        if !self.config.enabled {
            return Ok(Vec::new());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workflow registry lock unavailable"))?;
        let previous = serde_json::to_vec(&state.registry)?;
        let mut candidate = state.registry.clone();
        let overflow = super::native::update(
            &mut candidate,
            samples,
            self.config.autospec_enabled,
            now,
            self.config.terminal_retention_seconds,
        );
        state.registry = candidate;
        let mut snapshots: Vec<_> = overflow.into_iter().map(|owner| (owner, WorkflowSnapshot {
            id: "native-capacity".into(), label: "Workflow observer".into(),
            health: WorkflowHealth::ObserverError,
            detail: Some("Native retention capacity reached; some new workers are omitted while existing observations continue".into()),
            ..Default::default()
        })).collect();
        let registry = &mut state.registry;
        for run in registry
            .registrations
            .iter_mut()
            .filter(|_| self.config.autospec_enabled)
        {
            let mut snapshot = super::observe(run, now, self.config.quiet_seconds);
            let native = registry
                .native
                .iter()
                .filter(|native| native.registration_id.as_deref() == Some(&run.id))
                .max_by_key(|native| (native.started_at, &native.session_id));
            if let Some(native) = native {
                super::native::merge_registered(
                    &mut snapshot,
                    native,
                    now,
                    self.config.quiet_seconds,
                    run.lifecycle_at,
                );
                if super::observer::is_terminal(snapshot.health) {
                    run.terminal_at = run.terminal_at.or(native.terminal_at);
                }
            }
            if !run
                .terminal_at
                .is_some_and(|at| now.saturating_sub(at) > self.config.terminal_retention_seconds)
            {
                snapshots.push((run.owner.clone(), snapshot));
            }
        }
        for native in &state.registry.native {
            if native.registration_id.is_none()
                && !native.expired(now, self.config.terminal_retention_seconds)
            {
                snapshots.push((
                    native.owner.clone(),
                    native.snapshot(now, self.config.quiet_seconds),
                ));
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
