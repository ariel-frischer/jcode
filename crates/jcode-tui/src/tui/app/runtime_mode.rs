#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppRuntimeMode {
    /// Normal product TUI. The client renders state owned by the jcode server.
    RemoteClient,
    /// Deterministic playback of recorded session/server events. Never calls live providers.
    Replay,
    /// Local in-process harness used by unit tests and transitional UI fixtures only.
    TestHarness,
}
