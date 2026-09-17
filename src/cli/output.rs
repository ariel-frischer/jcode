pub const QUIET_ENV: &str = "JCODE_QUIET";

use std::io::{self, Write};

pub fn set_quiet_enabled(enabled: bool) {
    if enabled {
        crate::env::set_var(QUIET_ENV, "1");
    } else {
        crate::env::remove_var(QUIET_ENV);
    }
}

pub fn quiet_enabled() -> bool {
    std::env::var(QUIET_ENV)
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub fn stderr_info(message: impl AsRef<str>) {
    if !quiet_enabled() {
        write_stderr_line(&crate::output_style::terminal_text(message.as_ref()));
    }
}

pub fn terminal_title(title: impl AsRef<str>) -> String {
    crate::output_style::terminal_text(title.as_ref()).into_owned()
}

pub fn stderr_blank_line() {
    if !quiet_enabled() {
        write_stderr_line("");
    }
}

/// Write one diagnostic line without allowing an unavailable stderr to panic.
///
/// Fatal-error and teardown paths may run after a terminal or pipe has gone
/// away. `eprintln!` panics when its write fails, which can turn the original
/// error into a double panic. The caller intentionally cannot recover from a
/// failed diagnostic write, so this helper treats it as best effort.
pub(crate) fn write_stderr_line(message: &str) {
    let mut stderr = io::stderr().lock();
    match write_line(&mut stderr, message) {
        Ok(()) | Err(_) => {}
    }
}

pub(crate) fn write_line(writer: &mut impl Write, message: &str) -> io::Result<()> {
    writeln!(writer, "{message}")
}

#[cfg(test)]
mod tests {
    use super::write_line;
    use std::io::{self, Write};

    struct FailingWriter(io::ErrorKind);

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(self.0, "diagnostic sink unavailable"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn closed_stderr_writer_returns_an_error_without_panicking() {
        let error = write_line(&mut FailingWriter(io::ErrorKind::BrokenPipe), "fatal error")
            .expect_err("closed stderr should be reported to the caller");
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn full_stderr_writer_returns_an_error_without_panicking() {
        let error = write_line(
            &mut FailingWriter(io::ErrorKind::StorageFull),
            "fatal error",
        )
        .expect_err("full stderr should be reported to the caller");
        assert_eq!(error.kind(), io::ErrorKind::StorageFull);
    }
}
