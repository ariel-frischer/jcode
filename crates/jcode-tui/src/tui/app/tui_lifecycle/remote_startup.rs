use super::*;

impl App {
    /// Create an App instance for remote mode (connecting to server).
    pub fn new_for_remote(resume_session: Option<String>) -> Self {
        Self::new_for_remote_with_options(resume_session, false)
    }

    pub fn new_for_remote_with_options(resume_session: Option<String>, fresh_spawn: bool) -> Self {
        let provider: Arc<dyn Provider> =
            Arc::new(InertRuntimeProvider::new(AppRuntimeMode::RemoteClient));
        let registry = Registry::empty();
        let session = resume_session
            .as_ref()
            .and_then(|session_id| Session::load_startup_stub(session_id).ok())
            .unwrap_or_else(|| Session::create(None, None));
        let mut app = Self::new_minimal_with_session(provider, registry, session);
        app.is_remote = true;
        app.runtime_mode = AppRuntimeMode::RemoteClient;
        app.remote_startup_phase = Some(RemoteStartupPhase::Connecting);
        app.remote_startup_phase_started = Some(Instant::now());

        let reload_fast_start = std::env::var("JCODE_RELOAD_FAST_START")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        crate::env::remove_var("JCODE_RELOAD_FAST_START");

        if let Some(ref session_id) = resume_session {
            if reload_fast_start {
                crate::logging::info(&format!(
                    "Remote reload fast start: deferring persisted transcript for {} until server history",
                    session_id
                ));
            } else {
                app.restore_remote_startup_history(session_id);
            }
            if fresh_spawn && !reload_fast_start {
                crate::logging::info(&format!(
                    "Remote startup fresh-spawn path: restored persisted transcript for {} while awaiting server history",
                    session_id
                ));
            }
            if let Some(restored) = Self::restore_input_for_reload(session_id) {
                app.apply_restored_reload_input(restored);
            }
        }

        app.resume_session_id = resume_session;
        app
    }

    pub fn set_server_spawning(&mut self) {
        self.server_spawning = true;
        self.remote_startup_phase = Some(RemoteStartupPhase::StartingServer);
        self.remote_startup_phase_started = Some(Instant::now());
    }
}
