use std::path::Path;

#[cfg(target_os = "macos")]
mod macos_power {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_void};

    type CFStringRef = *const c_void;
    type IOPMAssertionID = u32;
    type IOReturn = i32;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_IOPM_ASSERTION_LEVEL_ON: u32 = 255;
    const K_IO_RETURN_SUCCESS: IOReturn = 0;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            assertion_level: u32,
            assertion_name: CFStringRef,
            assertion_id: *mut IOPMAssertionID,
        ) -> IOReturn;
        fn IOPMAssertionRelease(assertion_id: IOPMAssertionID) -> IOReturn;
    }

    fn cf_string(value: &str) -> Option<CFStringRef> {
        let c_string = CString::new(value).ok()?;
        let cf = unsafe {
            CFStringCreateWithCString(
                std::ptr::null(),
                c_string.as_ptr(),
                K_CF_STRING_ENCODING_UTF8,
            )
        };
        (!cf.is_null()).then_some(cf)
    }

    pub struct PowerAssertion {
        id: Option<IOPMAssertionID>,
    }

    impl PowerAssertion {
        pub fn prevent_user_idle_system_sleep(reason: &str) -> Self {
            let Some(assertion_type) = cf_string("PreventUserIdleSystemSleep") else {
                return Self { id: None };
            };
            let Some(assertion_name) = cf_string(reason) else {
                unsafe { CFRelease(assertion_type) };
                return Self { id: None };
            };

            let mut id = 0;
            let result = unsafe {
                IOPMAssertionCreateWithName(
                    assertion_type,
                    K_IOPM_ASSERTION_LEVEL_ON,
                    assertion_name,
                    &mut id,
                )
            };
            unsafe {
                CFRelease(assertion_type);
                CFRelease(assertion_name);
            }

            if result == K_IO_RETURN_SUCCESS {
                crate::logging::info(&format!(
                    "Created macOS sleep-prevention assertion while streaming (id={id})"
                ));
                Self { id: Some(id) }
            } else {
                crate::logging::warn(&format!(
                    "Failed to create macOS sleep-prevention assertion while streaming: IOReturn={result}"
                ));
                Self { id: None }
            }
        }

        #[cfg(test)]
        pub fn is_active(&self) -> bool {
            self.id.is_some()
        }
    }

    impl Drop for PowerAssertion {
        fn drop(&mut self) {
            if let Some(id) = self.id.take() {
                let result = unsafe { IOPMAssertionRelease(id) };
                if result != K_IO_RETURN_SUCCESS {
                    crate::logging::warn(&format!(
                        "Failed to release macOS sleep-prevention assertion id={id}: IOReturn={result}"
                    ));
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod macos_power {
    pub struct PowerAssertion;

    impl PowerAssertion {
        pub fn prevent_user_idle_system_sleep(_reason: &str) -> Self {
            Self
        }

        #[cfg(test)]
        pub fn is_active(&self) -> bool {
            false
        }
    }
}

pub use macos_power::PowerAssertion;

#[cfg(any(unix, test))]
fn desired_nofile_soft_limit(current: u64, hard: u64, minimum: u64) -> Option<u64> {
    let desired = current.max(minimum).min(hard);
    (desired > current).then_some(desired)
}

/// Create a symlink (Unix) or copy the file (Windows).
///
/// On Windows, symlinks require elevated privileges or Developer Mode,
/// so we fall back to copying.
pub fn symlink_or_copy(src: &Path, dst: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dst)
    }
    #[cfg(windows)]
    {
        if src.is_dir() {
            std::os::windows::fs::symlink_dir(src, dst).or_else(|_| copy_dir_recursive(src, dst))
        } else {
            std::os::windows::fs::symlink_file(src, dst)
                .or_else(|_| std::fs::copy(src, dst).map(|_| ()))
        }
    }
}

#[cfg(windows)]
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub use jcode_core::fs::{set_directory_permissions_owner_only, set_permissions_owner_only};

/// Set file permissions to owner read/write/execute (0o755).
/// No-op on Windows (executability is determined by file extension).
pub fn set_permissions_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(path, perms)
    }
    #[cfg(windows)]
    {
        let _ = path;
        Ok(())
    }
}

/// Best-effort increase of the current process soft `RLIMIT_NOFILE` on Unix.
///
/// This helps jcode survive short-lived reload/connect spikes even when it was
/// launched from a shell with a conservative `ulimit -n` like 1024.
pub fn raise_nofile_limit_best_effort(minimum_soft_limit: u64) {
    #[cfg(unix)]
    {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
            crate::logging::warn(&format!(
                "Failed to read RLIMIT_NOFILE: {}",
                std::io::Error::last_os_error()
            ));
            return;
        }

        // `rlim_cur`/`rlim_max` are `u64` on Linux/macOS but `i64` on some
        // platforms (e.g. FreeBSD), so cast explicitly to keep builds portable.
        // The cast is a no-op (and clippy-flagged) where the field is already
        // `u64`, hence the allow.
        #[allow(clippy::unnecessary_cast)]
        let current: u64 = limit.rlim_cur as u64;
        #[allow(clippy::unnecessary_cast)]
        let hard: u64 = limit.rlim_max as u64;
        let Some(desired) = desired_nofile_soft_limit(current, hard, minimum_soft_limit) else {
            return;
        };

        let updated = libc::rlimit {
            rlim_cur: desired as libc::rlim_t,
            rlim_max: limit.rlim_max,
        };
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &updated) } == 0 {
            crate::logging::info(&format!(
                "Raised RLIMIT_NOFILE soft limit from {} to {} (hard={})",
                current, desired, hard
            ));
        } else {
            crate::logging::warn(&format!(
                "Failed to raise RLIMIT_NOFILE from {} toward {} (hard={}): {}",
                current,
                desired,
                hard,
                std::io::Error::last_os_error()
            ));
        }
    }

    #[cfg(not(unix))]
    {
        let _ = minimum_soft_limit;
    }
}

/// Check if a process is running by PID.
///
/// On Unix, uses `kill(pid, 0)` to check without sending a signal.
/// On Windows, uses OpenProcess to query the process.
pub fn is_process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let result = unsafe { libc::kill(pid as i32, 0) };
        if result == 0 {
            return true;
        }
        let err = std::io::Error::last_os_error();
        !matches!(err.raw_os_error(), Some(code) if code == libc::ESRCH)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut exit_code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut exit_code);
            CloseHandle(handle);
            ok != 0 && exit_code == STILL_ACTIVE as u32
        }
    }
}

/// Check whether a process is live rather than merely present as a zombie.
pub fn is_process_live(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        stat.rsplit_once(") ")
            .and_then(|(_, fields)| fields.chars().next())
            .is_some_and(|state| state != 'Z')
    }
    #[cfg(not(target_os = "linux"))]
    {
        is_process_running(pid)
    }
}

/// Check whether an isolated process group still contains any live process.
pub fn is_process_group_live(pgid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return false;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                continue;
            };
            let Some((_, fields)) = stat.rsplit_once(") ") else {
                continue;
            };
            let fields = fields.split_whitespace().collect::<Vec<_>>();
            if fields.get(2).and_then(|value| value.parse::<u32>().ok()) == Some(pgid)
                && fields.first().is_some_and(|state| *state != "Z")
            {
                return true;
            }
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        is_process_live(pgid)
    }
}

/// Wait for every live member of an isolated process group to disappear.
pub fn wait_for_process_group_exit(pgid: u32, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while is_process_group_live(pgid) {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    true
}

/// Return the kernel process-start identity for a PID where the platform exposes
/// one. The token is intentionally opaque to callers and must be compared again
/// immediately before any persisted-task signal.
pub fn process_start_token(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let fields = stat
            .rsplit_once(") ")?
            .1
            .split_whitespace()
            .collect::<Vec<_>>();
        // /proc/<pid>/stat field 22 is field 20 after the comm field.
        fields.get(19).map(|token| (*token).to_string())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessIdentityCheck {
    Matching,
    Stopped,
    Missing,
    Unsupported,
    Mismatch,
    SignalFailed,
}

/// Windows tree termination ignores Unix signal numbers, but zero remains
/// reserved for identity-only verification across platforms.
#[cfg(windows)]
pub(crate) const PROCESS_GROUP_TERMINATE_REQUEST: i32 = 1;

/// Verify a PID's live process instance without treating PID liveness alone as
/// ownership proof.
pub fn verify_process_start_token(pid: u32, expected: Option<&str>) -> ProcessIdentityCheck {
    let Some(expected) = expected.filter(|token| !token.is_empty()) else {
        return ProcessIdentityCheck::Missing;
    };
    let Some(actual) = process_start_token(pid) else {
        return if is_process_running(pid) {
            ProcessIdentityCheck::Unsupported
        } else {
            ProcessIdentityCheck::Stopped
        };
    };
    if actual == expected {
        ProcessIdentityCheck::Matching
    } else {
        ProcessIdentityCheck::Mismatch
    }
}

/// Return a live member of `pgid` other than the group leader, together with
/// its process-start token. This is persisted as additive evidence so a group
/// remains safely addressable when its leader exits first.
pub fn process_group_member_identity(pgid: u32) -> Option<(u32, String)> {
    #[cfg(target_os = "linux")]
    {
        let entries = std::fs::read_dir("/proc").ok()?;
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            if pid == pgid {
                continue;
            }
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                continue;
            };
            let Some((_, fields)) = stat.rsplit_once(") ") else {
                continue;
            };
            let fields = fields.split_whitespace().collect::<Vec<_>>();
            if fields.get(2).and_then(|value| value.parse::<u32>().ok()) != Some(pgid) {
                continue;
            }
            let Some(token) = process_start_token(pid) else {
                continue;
            };
            if fields.first().is_some_and(|state| *state != "Z") {
                return Some((pid, token));
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pgid;
        None
    }
}

/// Verify the persisted leader identity, or the additive member evidence when
/// the leader has stopped. A matching member must still be in the original
/// process group immediately before a signal is sent.
pub fn verify_process_group_identity(
    leader_pid: u32,
    leader_token: Option<&str>,
    member: Option<(u32, &str)>,
) -> ProcessIdentityCheck {
    match verify_process_start_token(leader_pid, leader_token) {
        ProcessIdentityCheck::Matching => ProcessIdentityCheck::Matching,
        ProcessIdentityCheck::Stopped => {
            let Some((member_pid, member_token)) = member else {
                return if is_process_group_live(leader_pid) {
                    ProcessIdentityCheck::Mismatch
                } else {
                    ProcessIdentityCheck::Stopped
                };
            };
            if verify_process_start_token(member_pid, Some(member_token))
                != ProcessIdentityCheck::Matching
            {
                return ProcessIdentityCheck::Mismatch;
            }
            #[cfg(unix)]
            {
                let actual_group = unsafe { libc::getpgid(member_pid as libc::pid_t) };
                if actual_group != leader_pid as libc::pid_t {
                    return ProcessIdentityCheck::Mismatch;
                }
            }
            ProcessIdentityCheck::Matching
        }
        other => other,
    }
}

/// Verify process identity immediately before signaling its isolated process
/// group. Missing and mismatched identity fail closed.
pub fn signal_verified_process_group(
    pid: u32,
    expected_start_token: Option<&str>,
    signal: i32,
) -> ProcessIdentityCheck {
    let check = verify_process_start_token(pid, expected_start_token);
    if check != ProcessIdentityCheck::Matching {
        return check;
    }
    if signal == 0 {
        return ProcessIdentityCheck::Matching;
    }
    match signal_detached_process_group(pid, signal) {
        Ok(()) => ProcessIdentityCheck::Matching,
        Err(_) if !is_process_running(pid) => ProcessIdentityCheck::Stopped,
        Err(_) => ProcessIdentityCheck::SignalFailed,
    }
}

/// Identity-verified process-group signal with optional leader-independent
/// member evidence.
pub fn signal_verified_process_group_with_member(
    pid: u32,
    expected_start_token: Option<&str>,
    member: Option<(u32, &str)>,
    signal: i32,
) -> ProcessIdentityCheck {
    let check = verify_process_group_identity(pid, expected_start_token, member);
    if check != ProcessIdentityCheck::Matching {
        return check;
    }
    if signal == 0 {
        return ProcessIdentityCheck::Matching;
    }
    match signal_detached_process_group(pid, signal) {
        Ok(()) => ProcessIdentityCheck::Matching,
        Err(_) if !is_process_group_live(pid) => ProcessIdentityCheck::Stopped,
        Err(_) => ProcessIdentityCheck::SignalFailed,
    }
}

/// Send a signal to an entire detached process group/session led by `pid`.
///
/// On Unix, detached tasks are spawned with `setsid()`, so the leader PID is
/// also the process-group/session ID. Signaling `-pid` reaches the full tree.
pub fn signal_detached_process_group(pid: u32, signal: i32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(-(pid as i32), signal) };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        let _ = signal;
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            CREATE_NO_WINDOW, OpenProcess, PROCESS_TERMINATE, TerminateProcess,
        };

        // Detached commands commonly run through cmd.exe or PowerShell. Killing
        // only that shell leaves compilers, test runners, and other descendants
        // alive. taskkill's /T flag walks the Windows process tree. Keep the
        // direct Win32 termination below as a fallback if taskkill is missing or
        // the tree operation fails.
        let tree_status = std::process::Command::new("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        if tree_status.is_ok_and(|status| status.success()) {
            return Ok(());
        }
        // taskkill can report failure when a descendant exits while it walks the
        // tree, even though it successfully terminated the leader and remaining
        // descendants. Avoid turning that benign race into a misleading access
        // denied error from the direct-handle fallback.
        for _ in 0..20 {
            if !is_process_running(pid) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }

        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let ok = TerminateProcess(handle, 1);
            CloseHandle(handle);
            if ok == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }
}

/// Best-effort non-blocking reap for a child process owned by the current process.
///
/// Returns:
/// - `Ok(Some(exit_code))` if the child exited and was reaped now
/// - `Ok(None)` if it is still running or is not our child
pub fn try_reap_child_process(pid: u32) -> std::io::Result<Option<i32>> {
    #[cfg(unix)]
    {
        let mut status = 0;
        let rc = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
        if rc == 0 {
            return Ok(None);
        }
        if rc == -1 {
            let err = std::io::Error::last_os_error();
            if matches!(err.raw_os_error(), Some(code) if code == libc::ECHILD) {
                return Ok(None);
            }
            return Err(err);
        }

        if libc::WIFEXITED(status) {
            Ok(Some(libc::WEXITSTATUS(status)))
        } else if libc::WIFSIGNALED(status) {
            Ok(Some(128 + libc::WTERMSIG(status)))
        } else {
            Ok(Some(-1))
        }
    }
    #[cfg(windows)]
    {
        let _ = pid;
        Ok(None)
    }
}

/// Atomically swap a symlink by creating a temp symlink and renaming.
///
/// On Unix: creates temp symlink, then renames over target (atomic).
/// On Windows: removes target, copies source (not atomic, but best effort).
pub fn atomic_symlink_swap(src: &Path, dst: &Path, temp: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(temp);
        std::os::unix::fs::symlink(src, temp)?;
        std::fs::rename(temp, dst)?;
    }
    #[cfg(windows)]
    {
        let _ = std::fs::remove_file(temp);
        let _ = std::fs::remove_file(dst);
        std::fs::copy(src, dst).map(|_| ())?;
    }
    Ok(())
}

/// Spawn a process detached from the current client session.
///
/// This is used for launching new terminal windows (for `/resume`, `/split`,
/// crash restore, etc.) so the new client survives if the invoking jcode
/// process exits or its terminal closes.
pub fn spawn_detached(cmd: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};

        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }

    cmd.spawn()
}

/// Reap a detached child without blocking the caller.
pub fn reap_detached(child: std::process::Child) {
    #[cfg(unix)]
    {
        let mut child = child;
        let _ = std::thread::Builder::new()
            .name("jcode-detached-child".to_string())
            .spawn(move || {
                let _ = child.wait();
            });
    }

    #[cfg(windows)]
    {
        // Closing the process handle is sufficient on Windows. Unlike Unix,
        // the child does not need to be waited on to avoid a zombie process.
        drop(child);
    }
}

#[cfg(windows)]
fn spawn_replacement_process(
    cmd: &mut std::process::Command,
) -> std::io::Result<std::process::Child> {
    cmd.spawn()
}

/// Replace the current process with a new command (exec on Unix).
///
/// On Unix, this calls exec() which never returns on success.
/// On Windows, this spawns the process and exits.
///
/// Returns an error only if the operation fails. On success (Unix exec),
/// this function never returns.
pub fn replace_process(cmd: &mut std::process::Command) -> std::io::Error {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        crate::logging::error(&format!(
            "replace_process failed: {} ({})",
            err,
            crate::util::process_fd_diagnostic_snapshot()
        ));
        err
    }
    #[cfg(windows)]
    {
        match spawn_replacement_process(cmd) {
            Ok(_child) => std::process::exit(0),
            Err(e) => e,
        }
    }
}

#[cfg(test)]
#[path = "platform_tests.rs"]
mod platform_tests;
