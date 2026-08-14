use super::{LibrarianArtifactPaths, LibrarianFailure, LibrarianFailureStage, LibrarianGeneration};
use jcode_base::message::redact_secrets;
use jcode_session_types::{SessionSummary, SourceFingerprint, StructuredSummarySections};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(5);
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct PublicationStore {
    root: PathBuf,
}

impl PublicationStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn claim(
        &self,
        session_id: &str,
        fingerprint: &SourceFingerprint,
    ) -> Result<PublicationClaim, LibrarianFailure> {
        validate_path_component(session_id, "session identifier")?;
        validate_digest(&fingerprint.digest)?;

        let session_directory = self.root.join(session_id);
        fs::create_dir_all(&session_directory).map_err(|error| {
            failure(
                LibrarianFailureStage::Locking,
                "librarian_lock_directory_failed",
                format!("Could not prepare the session artifact directory: {error}"),
            )
        })?;

        let destination = session_directory.join(&fingerprint.digest);
        let paths = LibrarianArtifactPaths::new(destination.clone());
        if destination.exists() {
            validate_published_pair(&paths, session_id, fingerprint)?;
            return Ok(PublicationClaim::Reused(paths));
        }

        let lock_directory = session_directory.join(format!(".{}.lock", fingerprint.digest));
        let started = Instant::now();
        loop {
            match fs::create_dir(&lock_directory) {
                Ok(()) => {
                    let lock = PublicationLock { lock_directory };
                    if destination.exists() {
                        validate_published_pair(&paths, session_id, fingerprint)?;
                        return Ok(PublicationClaim::Reused(paths));
                    }
                    return Ok(PublicationClaim::Generate(PublicationLease {
                        session_id: session_id.to_string(),
                        fingerprint: Box::new(fingerprint.clone()),
                        session_directory,
                        destination,
                        _lock: lock,
                    }));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if destination.exists() {
                        validate_published_pair(&paths, session_id, fingerprint)?;
                        return Ok(PublicationClaim::Reused(paths));
                    }
                    if started.elapsed() >= LOCK_WAIT_TIMEOUT {
                        return Err(failure(
                            LibrarianFailureStage::Locking,
                            "librarian_lock_timeout",
                            "Timed out waiting for another librarian publication to finish.".into(),
                        ));
                    }
                    thread::sleep(LOCK_RETRY_DELAY);
                }
                Err(error) => {
                    return Err(failure(
                        LibrarianFailureStage::Locking,
                        "librarian_lock_failed",
                        format!("Could not acquire the publication lock: {error}"),
                    ));
                }
            }
        }
    }
}

pub(crate) enum PublicationClaim {
    Generate(PublicationLease),
    Reused(LibrarianArtifactPaths),
}

pub(crate) struct PublicationLease {
    session_id: String,
    fingerprint: Box<SourceFingerprint>,
    session_directory: PathBuf,
    destination: PathBuf,
    _lock: PublicationLock,
}

impl PublicationLease {
    pub(crate) fn publish_generation(
        self,
        generation: LibrarianGeneration,
    ) -> Result<LibrarianArtifactPaths, LibrarianFailure> {
        self.publish_generation_with_rename(generation, |staging, destination| {
            fs::rename(staging, destination)
        })
    }

    pub(crate) fn publish_generation_with_rename<F>(
        self,
        generation: LibrarianGeneration,
        rename: F,
    ) -> Result<LibrarianArtifactPaths, LibrarianFailure>
    where
        F: FnOnce(&Path, &Path) -> io::Result<()>,
    {
        let summary = validate_generation(&self.session_id, &self.fingerprint, generation)?;
        let rendered = render_artifacts(&summary)?;

        if self.destination.exists() {
            let paths = LibrarianArtifactPaths::new(self.destination.clone());
            validate_published_pair(&paths, &self.session_id, &self.fingerprint)?;
            return Ok(paths);
        }

        let staging = create_staging_directory(&self.session_directory, &self.fingerprint.digest)?;
        let mut cleanup = StagingCleanup::new(staging.clone());

        write_synced_file(&staging.join("summary.md"), &rendered.markdown)?;
        write_synced_file(&staging.join("summary.json"), &rendered.json)?;
        sync_directory(&staging).map_err(|error| {
            failure(
                LibrarianFailureStage::Publication,
                "librarian_staging_sync_failed",
                format!("Could not sync staged summary artifacts: {error}"),
            )
        })?;

        rename(&staging, &self.destination).map_err(|error| {
            failure(
                LibrarianFailureStage::Publication,
                "librarian_atomic_publish_failed",
                format!("Could not atomically publish the summary artifacts: {error}"),
            )
        })?;
        cleanup.disarm();
        sync_directory(&self.session_directory).map_err(|error| {
            failure(
                LibrarianFailureStage::Publication,
                "librarian_publish_sync_failed",
                format!("The summary was renamed but its directory could not be synced: {error}"),
            )
        })?;

        Ok(LibrarianArtifactPaths::new(self.destination.clone()))
    }
}

struct PublicationLock {
    lock_directory: PathBuf,
}

impl Drop for PublicationLock {
    fn drop(&mut self) {
        drop(fs::remove_dir(&self.lock_directory));
    }
}

struct StagingCleanup {
    path: Option<PathBuf>,
}

impl StagingCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for StagingCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            drop(fs::remove_dir_all(path));
        }
    }
}

struct RenderedArtifacts {
    markdown: Vec<u8>,
    json: Vec<u8>,
}

fn validate_generation(
    session_id: &str,
    fingerprint: &SourceFingerprint,
    generation: LibrarianGeneration,
) -> Result<SessionSummary, LibrarianFailure> {
    let source_value: serde_json::Value =
        serde_json::from_str(&generation.response_json).map_err(|error| {
            validation_failure(format!("Provider output is not valid JSON: {error}"))
        })?;
    let mut summary: SessionSummary =
        serde_json::from_value(source_value.clone()).map_err(|error| {
            validation_failure(format!(
                "Provider output does not match the summary schema: {error}"
            ))
        })?;
    let schema_value = serde_json::to_value(&summary).map_err(|error| {
        validation_failure(format!(
            "The validated summary could not be normalized: {error}"
        ))
    })?;
    if schema_value != source_value {
        return Err(validation_failure(
            "Provider output contains fields or values outside the summary schema.".into(),
        ));
    }

    super::handoff::normalize_summary_handoff(&mut summary)?;

    validate_summary_contract(session_id, fingerprint, &summary, &generation.usage)?;
    reject_recognized_secrets(&summary)?;

    Ok(summary)
}

fn render_artifacts(summary: &SessionSummary) -> Result<RenderedArtifacts, LibrarianFailure> {
    let json = serde_json::to_vec_pretty(summary).map_err(|error| {
        failure(
            LibrarianFailureStage::Rendering,
            "librarian_json_render_failed",
            format!("Could not render summary.json: {error}"),
        )
    })?;
    let markdown = render_markdown(summary).into_bytes();
    Ok(RenderedArtifacts { markdown, json })
}

fn render_markdown(summary: &SessionSummary) -> String {
    let mut output = String::new();
    output.push_str("# Session Summary\n\n");
    field(&mut output, "Format version", &summary.format_version);
    field(&mut output, "Session", &summary.session_id);
    field(
        &mut output,
        "Source fingerprint",
        &summary.source_fingerprint.digest,
    );
    field(
        &mut output,
        "Fingerprint algorithm",
        &summary.source_fingerprint.algorithm_version,
    );
    field(
        &mut output,
        "Generated at",
        &summary
            .generated_at
            .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
    );
    field(&mut output, "Provider", &summary.effective_route.provider);
    field(
        &mut output,
        "API method",
        &summary.effective_route.api_method,
    );
    field(&mut output, "Model", &summary.effective_route.model);
    field(
        &mut output,
        "Reasoning effort",
        &summary.effective_route.reasoning_effort,
    );
    output.push_str("\n## Usage\n\n");
    output.push_str(&format!(
        "- Input tokens: {}\n- Output tokens: {}\n- Requests: {}\n- Elapsed milliseconds: {}\n- Cost micros USD: {}\n",
        summary.usage.input_tokens,
        summary.usage.output_tokens,
        summary.usage.request_count,
        summary.usage.elapsed_ms,
        summary.usage.cost_micros_usd,
    ));

    render_sections(&mut output, &summary.summary);
    output.push_str("\n## Handoff brief\n\n");
    output.push_str(&summary.handoff_brief);
    output.push('\n');
    output.push_str("\n## Relevant files\n\n");
    for path in summary.relevant_files.as_slice() {
        output.push_str("- `");
        output.push_str(&path.to_string_lossy());
        output.push_str("`\n");
    }
    output
}

fn render_sections(output: &mut String, sections: &StructuredSummarySections) {
    output.push_str("\n## Goal\n\n");
    output.push_str(&sections.goal);
    output.push('\n');
    list_section(output, "Outcomes", &sections.outcomes);
    list_section(output, "Decisions", &sections.decisions);
    list_section(output, "Unresolved work", &sections.unresolved_work);
    list_section(output, "Risks", &sections.risks);
    list_section(output, "Next steps", &sections.next_steps);
}

fn list_section(output: &mut String, heading: &str, items: &[String]) {
    output.push_str("\n## ");
    output.push_str(heading);
    output.push_str("\n\n");
    if items.is_empty() {
        output.push_str("None.\n");
    } else {
        for item in items {
            output.push_str("- ");
            output.push_str(item);
            output.push('\n');
        }
    }
}

fn field(output: &mut String, label: &str, value: &str) {
    output.push_str("- **");
    output.push_str(label);
    output.push_str(":** ");
    output.push_str(value);
    output.push('\n');
}

fn validate_published_pair(
    paths: &LibrarianArtifactPaths,
    session_id: &str,
    fingerprint: &SourceFingerprint,
) -> Result<(), LibrarianFailure> {
    let json = fs::read(paths.json()).map_err(|error| {
        failure(
            LibrarianFailureStage::Validation,
            "librarian_existing_pair_invalid",
            format!("Existing summary.json is unreadable: {error}"),
        )
    })?;
    let source_value: serde_json::Value = serde_json::from_slice(&json).map_err(|error| {
        failure(
            LibrarianFailureStage::Validation,
            "librarian_existing_pair_invalid",
            format!("Existing summary.json is invalid: {error}"),
        )
    })?;
    let summary: SessionSummary =
        serde_json::from_value(source_value.clone()).map_err(|error| {
            failure(
                LibrarianFailureStage::Validation,
                "librarian_existing_pair_invalid",
                format!("Existing summary.json does not match the summary schema: {error}"),
            )
        })?;
    let normalized_value = serde_json::to_value(&summary).map_err(|error| {
        failure(
            LibrarianFailureStage::Validation,
            "librarian_existing_pair_invalid",
            format!("Existing summary.json could not be normalized: {error}"),
        )
    })?;
    if normalized_value != source_value {
        return Err(failure(
            LibrarianFailureStage::Validation,
            "librarian_existing_pair_mismatch",
            "Existing summary.json contains fields or values outside the summary schema.".into(),
        ));
    }
    validate_summary_contract(session_id, fingerprint, &summary, &summary.usage)?;
    reject_recognized_secrets(&summary)?;
    let expected_markdown = render_markdown(&summary);
    let markdown = fs::read(paths.markdown()).map_err(|error| {
        failure(
            LibrarianFailureStage::Validation,
            "librarian_existing_pair_invalid",
            format!("Existing summary.md is unreadable: {error}"),
        )
    })?;
    if markdown != expected_markdown.as_bytes() {
        return Err(failure(
            LibrarianFailureStage::Validation,
            "librarian_existing_pair_mismatch",
            "Existing Markdown and JSON artifacts do not describe the same summary.".into(),
        ));
    }
    Ok(())
}

fn create_staging_directory(
    session_directory: &Path,
    digest: &str,
) -> Result<PathBuf, LibrarianFailure> {
    for _ in 0..16 {
        let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = session_directory.join(format!(
            ".{digest}.staging.{}.{}",
            std::process::id(),
            sequence
        ));
        match fs::create_dir(&path) {
            Ok(()) => {
                if let Err(error) = set_private_directory_permissions(&path) {
                    drop(fs::remove_dir(&path));
                    return Err(error);
                }
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(failure(
                    LibrarianFailureStage::Publication,
                    "librarian_staging_create_failed",
                    format!("Could not create a private staging directory: {error}"),
                ));
            }
        }
    }
    Err(failure(
        LibrarianFailureStage::Publication,
        "librarian_staging_collision",
        "Could not allocate a unique staging directory.".into(),
    ))
}

fn validate_summary_contract(
    session_id: &str,
    fingerprint: &SourceFingerprint,
    summary: &SessionSummary,
    measured_usage: &jcode_session_types::BoundedUsage,
) -> Result<(), LibrarianFailure> {
    let configuration = &fingerprint.configuration_identity;
    if summary.format_version != configuration.schema_version
        || summary.session_id != session_id
        || summary.source_fingerprint != *fingerprint
        || summary.effective_route != configuration.route
        || summary.usage != *measured_usage
    {
        return Err(validation_failure(
            "Summary data does not match the requested session, fingerprint, route, or usage."
                .into(),
        ));
    }

    let budgets = &configuration.budgets;
    if summary.usage.input_tokens > budgets.max_input_tokens
        || summary.usage.output_tokens > budgets.max_output_tokens
        || summary.usage.request_count > budgets.max_requests
        || summary.usage.cost_micros_usd > budgets.max_cost_micros_usd
        || summary.usage.elapsed_ms > budgets.deadline_seconds.saturating_mul(1_000)
    {
        return Err(validation_failure(
            "Summary usage exceeds the approved librarian budget.".into(),
        ));
    }
    Ok(())
}

fn reject_recognized_secrets(summary: &SessionSummary) -> Result<(), LibrarianFailure> {
    let serialized = serde_json::to_string(summary).map_err(|error| {
        validation_failure(format!(
            "The validated summary could not be inspected: {error}"
        ))
    })?;
    if redact_secrets(&serialized) != serialized {
        return Err(validation_failure(
            "Summary data still contains recognized sensitive data.".into(),
        ));
    }
    Ok(())
}

fn write_synced_file(path: &Path, bytes: &[u8]) -> Result<(), LibrarianFailure> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| {
        failure(
            LibrarianFailureStage::Publication,
            "librarian_staging_write_failed",
            format!("Could not create {}: {error}", path.display()),
        )
    })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| {
            failure(
                LibrarianFailureStage::Publication,
                "librarian_staging_write_failed",
                format!("Could not flush {}: {error}", path.display()),
            )
        })
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), LibrarianFailure> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
        failure(
            LibrarianFailureStage::Publication,
            "librarian_staging_permissions_failed",
            format!("Could not make the staging directory private: {error}"),
        )
    })
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), LibrarianFailure> {
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn validate_path_component(value: &str, label: &str) -> Result<(), LibrarianFailure> {
    let mut components = Path::new(value).components();
    if value.is_empty()
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(failure(
            LibrarianFailureStage::Locking,
            "librarian_unsafe_artifact_path",
            format!("The {label} is not safe for artifact storage."),
        ));
    }
    Ok(())
}

fn validate_digest(digest: &str) -> Result<(), LibrarianFailure> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(failure(
            LibrarianFailureStage::Locking,
            "librarian_invalid_fingerprint",
            "The source fingerprint is not a lowercase SHA-256 digest.".into(),
        ));
    }
    Ok(())
}

fn validation_failure(message: String) -> LibrarianFailure {
    failure(
        LibrarianFailureStage::Validation,
        "librarian_response_invalid",
        message,
    )
}

fn failure(stage: LibrarianFailureStage, code: &'static str, message: String) -> LibrarianFailure {
    LibrarianFailure {
        stage,
        code,
        message,
        usage: None,
    }
}
