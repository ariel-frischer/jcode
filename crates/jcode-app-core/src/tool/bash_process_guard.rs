use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

static NEXT_OWNER_ID: AtomicU64 = AtomicU64::new(1);
type OwnedProcessGroup = (u64, Option<String>);

static OWNED_PROCESS_GROUPS: LazyLock<Mutex<HashMap<u32, OwnedProcessGroup>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct ProcessGroupOwnership {
    pid: Option<u32>,
    owner_id: u64,
    process_instance: Option<String>,
}

impl ProcessGroupOwnership {
    fn new(pid: Option<u32>) -> Self {
        let owner_id = NEXT_OWNER_ID.fetch_add(1, Ordering::Relaxed);
        let process_instance = pid.and_then(crate::platform::process_start_token);
        if let Some(pid) = pid {
            owned_process_groups().insert(pid, (owner_id, process_instance.clone()));
        }
        Self {
            pid,
            owner_id,
            process_instance,
        }
    }

    fn disarm(&mut self) {
        self.take();
    }

    fn take(&mut self) -> Option<u32> {
        let pid = self.pid.take()?;
        let mut groups = owned_process_groups();
        if groups.get(&pid).map(|(owner_id, _)| *owner_id) == Some(self.owner_id) {
            groups.remove(&pid);
            Some(pid)
        } else {
            None
        }
    }
}

impl Drop for ProcessGroupOwnership {
    fn drop(&mut self) {
        if let Some(pid) = self.take()
            && crate::platform::signal_verified_process_group(
                pid,
                self.process_instance.as_deref(),
                libc::SIGKILL,
            ) != crate::platform::ProcessIdentityCheck::Matching
        {
            crate::logging::info(&format!(
                "failed to terminate owned foreground bash process group {pid}"
            ));
        }
    }
}

fn owned_process_groups() -> std::sync::MutexGuard<'static, HashMap<u32, (u64, Option<String>)>> {
    OWNED_PROCESS_GROUPS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(super) fn terminate_owned_foreground_process_groups() {
    let pids = {
        let mut groups = owned_process_groups();
        groups
            .drain()
            .map(|(pid, (_, token))| (pid, token))
            .collect::<Vec<_>>()
    };
    for (pid, token) in pids {
        if crate::platform::signal_verified_process_group(pid, token.as_deref(), libc::SIGKILL)
            != crate::platform::ProcessIdentityCheck::Matching
        {
            crate::logging::info(&format!(
                "failed to terminate owned foreground bash process group {pid}"
            ));
        }
    }
}

pub(super) struct ProcessGroupKillGuard {
    pid: Option<u32>,
    process_instance: Option<String>,
}

impl ProcessGroupKillGuard {
    pub(super) fn new(pid: Option<u32>) -> Self {
        Self {
            process_instance: pid.and_then(crate::platform::process_start_token),
            pid,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.pid = None;
    }

    pub(super) fn terminate_verified(&self) -> crate::platform::ProcessIdentityCheck {
        let Some(pid) = self.pid else {
            return crate::platform::ProcessIdentityCheck::Missing;
        };
        crate::platform::signal_verified_process_group(
            pid,
            self.process_instance.as_deref(),
            libc::SIGKILL,
        )
    }
}

impl Drop for ProcessGroupKillGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.pid
            && crate::platform::signal_verified_process_group(
                pid,
                self.process_instance.as_deref(),
                libc::SIGKILL,
            ) != crate::platform::ProcessIdentityCheck::Matching
        {
            crate::logging::info(&format!(
                "failed to terminate detached bash process group {pid}"
            ));
        }
    }
}

pub(super) struct ForegroundProcessGuard {
    ownership: ProcessGroupOwnership,
    task_abort: Option<tokio::task::AbortHandle>,
}

impl ForegroundProcessGuard {
    pub(super) fn new(pid: Option<u32>) -> Self {
        Self {
            ownership: ProcessGroupOwnership::new(pid),
            task_abort: None,
        }
    }

    pub(super) fn attach_task(&mut self, handle: tokio::task::AbortHandle) {
        self.task_abort = Some(handle);
    }

    pub(super) fn disarm(&mut self) {
        self.task_abort = None;
        self.ownership.disarm();
    }
}

impl Drop for ForegroundProcessGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.task_abort.take() {
            handle.abort();
        }
    }
}

pub(super) fn isolate_process_group(command: &mut tokio::process::Command) {
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

pub(super) struct DetachedChildKillGuard {
    child: Option<std::process::Child>,
    ownership: ProcessGroupOwnership,
    process_instance: Option<String>,
}

impl DetachedChildKillGuard {
    pub(super) fn new(child: std::process::Child) -> Self {
        let process_instance = crate::platform::process_start_token(child.id());
        let ownership = ProcessGroupOwnership::new(Some(child.id()));
        Self {
            child: Some(child),
            ownership,
            process_instance,
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
            let group_result = crate::platform::signal_verified_process_group(
                child.id(),
                self.process_instance.as_deref(),
                libc::SIGKILL,
            );
            if group_result == crate::platform::ProcessIdentityCheck::Matching {
                if let Err(err) = child.wait() {
                    crate::logging::info(&format!(
                        "failed to reap cancelled detached bash process {}: {err}",
                        child.id()
                    ));
                }
            } else if let Ok(None) = child.try_wait() {
                crate::logging::info(&format!(
                    "refused to signal detached bash process {} because identity verification failed",
                    child.id()
                ));
            }
        }
    }
}
