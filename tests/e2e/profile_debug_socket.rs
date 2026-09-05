use crate::test_support::*;
use anyhow::Result;
use jcode::config::SkillsMode;
use jcode::protocol::{Request, ServerEvent, SessionProfileStartup};
use jcode::transport::{ReadHalf, Stream, WriteHalf};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct RawClient {
    reader: BufReader<ReadHalf>,
    writer: WriteHalf,
}

impl RawClient {
    async fn connect(path: &Path) -> Result<Self> {
        let stream = Stream::connect(path).await?;
        let (reader, writer) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(reader),
            writer,
        })
    }

    async fn subscribe(&mut self, profile: Option<SessionProfileStartup>) -> Result<String> {
        let request = Request::Subscribe {
            id: 1,
            working_dir: Some(std::env::current_dir()?.to_string_lossy().into_owned()),
            selfdev: None,
            target_session_id: None,
            client_instance_id: None,
            client_has_local_history: false,
            allow_session_takeover: false,
            crash_on_disconnect: false,
            terminal_env: Vec::new(),
            profile,
        };
        let mut payload = serde_json::to_string(&request)?;
        payload.push('\n');
        self.writer.write_all(payload.as_bytes()).await?;

        let mut session_id = None;
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line).await? == 0 {
                anyhow::bail!("server disconnected during Subscribe");
            }
            let event: ServerEvent = serde_json::from_str(&line)?;
            match event {
                ServerEvent::History { session_id: id, .. } => session_id = Some(id),
                ServerEvent::SessionId { session_id: id } => session_id = Some(id),
                ServerEvent::Error { message, .. } => anyhow::bail!("Subscribe failed: {message}"),
                ServerEvent::Done { id: 1 } => {
                    return session_id.ok_or_else(|| {
                        anyhow::anyhow!("Subscribe completed without a History session id")
                    });
                }
                _ => {}
            }
        }
    }
}

fn profile_startup() -> SessionProfileStartup {
    SessionProfileStartup {
        profile_name: Some("review".to_owned()),
        provider: Some("auto".to_owned()),
        model: Some("profile-model".to_owned()),
        provider_profile: None,
        reasoning_effort: None,
        allowed_tools: Some(vec!["read".to_owned()]),
        disabled_tools: vec!["write".to_owned()],
        skill_names: Vec::new(),
        skills_mode: Some(SkillsMode::None),
        disabled_skills: Vec::new(),
        skill_prompts: Vec::new(),
        instructions: Some("do-not-leak-this-profile-instruction".to_owned()),
    }
}

#[tokio::test]
async fn debug_socket_reports_profile_snapshot_and_keeps_legacy_state_neutral() -> Result<()> {
    let _env = setup_test_env()?;
    let config_path = jcode::config::Config::path().expect("test config path");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))?;
    std::fs::write(
        &config_path,
        r#"[profiles.review]
provider = "auto"
model = "profile-model"
tool_profile = "minimal"
tools = ["read"]
disabled_tools = ["write"]
skills_mode = "none"
instructions = "do-not-leak-this-profile-instruction"
"#,
    )?;
    jcode::config::Config::invalidate_cache();

    let runtime_dir = short_runtime_dir("jcode-profile-debug-socket".to_owned());
    std::fs::create_dir_all(&runtime_dir)?;
    let socket_path = runtime_dir.join("jcode.sock");
    let debug_socket_path = runtime_dir.join("jcode-debug.sock");
    let provider = MockProvider::with_models(vec!["profile-model"]);
    let provider: std::sync::Arc<dyn jcode::provider::Provider> = std::sync::Arc::new(provider);
    let server = jcode::server::Server::new_with_paths(
        provider,
        socket_path.clone(),
        debug_socket_path.clone(),
    );
    let server_handle = tokio::spawn(async move { server.run().await });

    let result = async {
        wait_for_socket(&socket_path).await?;
        wait_for_debug_socket_ready(&debug_socket_path).await?;

        let mut profiled = RawClient::connect(&socket_path).await?;
        let profiled_session = profiled.subscribe(Some(profile_startup())).await?;
        let profile_output = debug_run_command(
            debug_socket_path.clone(),
            "profile",
            Some(&profiled_session),
        )
        .await?;
        let profile: serde_json::Value = serde_json::from_str(&profile_output)?;
        assert_eq!(profile["name"], "review");
        assert_eq!(profile["restore_status"], "Matching");
        assert!(
            profile["snapshot"]["fingerprint"]
                .as_str()
                .is_some_and(|fingerprint| fingerprint.starts_with("sha256:"))
        );
        assert_eq!(profile["tool_policy"]["allowed_tools"][0], "read");
        assert_eq!(profile["skill_policy"]["instructions_present"], true);
        assert_eq!(profile["skill_policy"]["instructions_chars"], 36);
        assert!(!profile_output.contains("do-not-leak-this-profile-instruction"));

        let mut legacy = RawClient::connect(&socket_path).await?;
        let legacy_session = legacy.subscribe(None).await?;
        let legacy_output =
            debug_run_command(debug_socket_path.clone(), "profile", Some(&legacy_session)).await?;
        let legacy_profile: serde_json::Value = serde_json::from_str(&legacy_output)?;
        assert!(legacy_profile["name"].is_null());
        assert!(legacy_profile["snapshot"].is_null());
        assert!(legacy_profile["warning"].is_null());

        let state =
            debug_run_command(debug_socket_path.clone(), "state", Some(&legacy_session)).await?;
        let state: serde_json::Value = serde_json::from_str(&state)?;
        assert_eq!(state["session_id"], legacy_session);
        assert!(state.get("profile").is_none());

        let help = debug_run_command(debug_socket_path.clone(), "help", None).await?;
        assert!(help.contains("state"));
        Ok::<_, anyhow::Error>(())
    }
    .await;

    abort_server_and_cleanup(&server_handle, &socket_path, &debug_socket_path);
    result
}
