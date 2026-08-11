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
}

impl DetachedChildKillGuard {
    pub(super) fn new(child: std::process::Child) -> Self {
        Self { child: Some(child) }
    }

    pub(super) fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        match self.child.as_mut() {
            Some(child) => child.try_wait(),
            None => Ok(None),
        }
    }

    pub(super) fn disarm(&mut self) {
        self.child = None;
    }
}

impl Drop for DetachedChildKillGuard {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let group_result =
            crate::platform::signal_detached_process_group(child.id(), libc::SIGKILL);
        if group_result.is_err() && child.kill().is_err() {
            return;
        }
        if let Err(err) = child.wait() {
            crate::logging::info(&format!(
                "failed to reap cancelled detached bash process {}: {err}",
                child.id()
            ));
        }
    }
}
