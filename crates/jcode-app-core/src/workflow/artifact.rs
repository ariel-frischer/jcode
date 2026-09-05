//! Bounded artifact I/O. Unix traversal pins every directory descriptor and refuses aliases.
use super::registry::MAX_ARTIFACT_BYTES;
use anyhow::{Result, bail};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub(super) fn bounded_read(path: &Path) -> Result<Vec<u8>> {
    let file = open_regular(path)?;
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

#[cfg(unix)]
fn open_regular(path: &Path) -> Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Component;

    if !path.is_absolute() {
        bail!("workflow artifact path must be absolute");
    }
    let mut directory = File::open("/")?;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let name = match component {
            Component::RootDir => continue,
            Component::Normal(name) => CString::new(name.as_bytes())?,
            _ => bail!("workflow artifact path must be normalized"),
        };
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if components.peek().is_some() {
                libc::O_DIRECTORY
            } else {
                0
            };
        // SAFETY: directory owns a live fd and name is NUL-terminated. No fd escapes on failure.
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: successful openat returned a newly owned fd, transferred exactly once.
        directory = unsafe { File::from_raw_fd(fd) };
    }
    Ok(directory)
}

#[cfg(windows)]
fn open_regular(path: &Path) -> Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const REPARSE_POINT: u32 = 0x400;
    if !path.is_absolute() {
        bail!("workflow artifact path must be absolute");
    }
    let mut ancestors: Vec<_> = path.ancestors().collect();
    ancestors.reverse();
    let mut pinned = Vec::new();
    for ancestor in ancestors {
        let handle = std::fs::OpenOptions::new()
            .read(true)
            // Deny delete sharing: ancestors cannot be renamed/replaced during traversal.
            .share_mode(0x1 | 0x2)
            .custom_flags(OPEN_REPARSE_POINT | BACKUP_SEMANTICS)
            .open(ancestor)?;
        if handle.metadata()?.file_attributes() & REPARSE_POINT != 0 {
            bail!("workflow artifacts must not traverse reparse points");
        }
        pinned.push(handle);
    }
    pinned
        .pop()
        .ok_or_else(|| anyhow::anyhow!("workflow artifact path is empty"))
}

#[cfg(not(any(unix, windows)))]
fn open_regular(_path: &Path) -> Result<File> {
    // Fail closed on platforms without descriptor-relative traversal support.
    bail!("filesystem workflow observation requires safe directory traversal support")
}
