use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

static NEXT_OWNER_ID: AtomicU64 = AtomicU64::new(1);
static OWNED_PROCESS_GROUPS: LazyLock<Mutex<HashMap<u32, u64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct ProcessGroupOwnership {
    pid: Option<u32>,
    owner_id: u64,
}

impl ProcessGroupOwnership {
    fn new(pid: Option<u32>) -> Self {
        let owner_id = NEXT_OWNER_ID.fetch_add(1, Ordering::Relaxed);
        if let Some(pid) = pid {
            owned_process_groups().insert(pid, owner_id);
        }
        Self { pid, owner_id }
    }

    fn disarm(&mut self) {
        self.take();
    }

    fn take(&mut self) -> Option<u32> {
        let pid = self.pid.take()?;
        let mut groups = owned_process_groups();
        if groups.get(&pid) == Some(&self.owner_id) {
            groups.remove(&pid);
            Some(pid)
        } else {
            None
        }
    }
}

fn owned_process_groups() -> std::sync::MutexGuard<'static, HashMap<u32, u64>> {
    OWNED_PROCESS_GROUPS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(super) fn terminate_owned_foreground_process_groups() {
    let pids = {
        let mut groups = owned_process_groups();
        groups.drain().map(|(pid, _)| pid).collect::<Vec<_>>()
    };
    for pid in pids {
        if let Err(err) = crate::platform::signal_detached_process_group(pid, libc::SIGKILL) {
            crate::logging::info(&format!(
                "failed to terminate owned foreground bash process group {pid}: {err}"
            ));
        }
    }
}

pub(super) struct ProcessGroupKillGuard {
    pid: Option<u32>,
}

impl ProcessGroupKillGuard {
    pub(super) fn new(pid: Option<u32>) -> Self {
        Self { pid }
    }

    pub(super) fn disarm(&mut self) {
        self.pid = None;
    }
}

impl Drop for ProcessGroupKillGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.pid
            && let Err(err) = crate::platform::signal_detached_process_group(pid, libc::SIGKILL)
        {
            crate::logging::info(&format!(
                "failed to terminate detached bash process group {pid}: {err}"
            ));
        }
    }
}

pub(super) struct DetachedChildKillGuard {
    child: Option<std::process::Child>,
    ownership: ProcessGroupOwnership,
}

impl DetachedChildKillGuard {
    pub(super) fn new(child: std::process::Child) -> Self {
        let ownership = ProcessGroupOwnership::new(Some(child.id()));
        Self {
            child: Some(child),
            ownership,
        }
    }

    pub(super) fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        match self.child.as_mut() {
            Some(child) => child.try_wait(),
            None => Ok(None),
        }
    }

    pub(super) fn disarm(&mut self) {
        self.ownership.disarm();
        self.child = None;
    }
}

impl Drop for DetachedChildKillGuard {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if self.ownership.take().is_some() {
            let group_result =
                crate::platform::signal_detached_process_group(child.id(), libc::SIGKILL);
            if group_result.is_err() && child.kill().is_err() {
                return;
            }
        }
        if let Err(err) = child.wait() {
            crate::logging::info(&format!(
                "failed to reap cancelled detached bash process {}: {err}",
                child.id()
            ));
        }
    }
}
