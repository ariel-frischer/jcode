use super::{LibrarianFailure, LibrarianFailureStage};
use crate::tool::session_transition::{MAX_RELEVANT_FILES, prepare_handoff_prompt};
use jcode_base::message::redact_secrets;
use jcode_session_types::{LibrarianRelevantFiles, SessionSummary};
use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};

const MAX_RELEVANT_PATH_BYTES: usize = 1024;

pub(super) fn normalize_summary_handoff(
    summary: &mut SessionSummary,
) -> Result<(), LibrarianFailure> {
    summary.handoff_brief = summary.handoff_brief.trim().to_owned();

    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for path in summary.relevant_files.as_slice() {
        let Some(path) = normalize_relevant_path(path) else {
            continue;
        };
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    if paths.len() > MAX_RELEVANT_FILES {
        return Err(validation_failure(
            "Handoff relevant_files exceeds the 32-path limit.",
        ));
    }
    summary.relevant_files =
        LibrarianRelevantFiles::new(paths).map_err(|message| validation_failure(&message))?;

    let projected_paths = summary
        .relevant_files
        .as_slice()
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    prepare_handoff_prompt(Some(summary.handoff_brief.clone()), None, &projected_paths)
        .map_err(|error| validation_failure(&error.to_string()))?;

    if summary.handoff_brief.is_empty() && !projected_paths.is_empty() {
        return Err(validation_failure(
            "Continuation files require a non-empty handoff brief.",
        ));
    }

    Ok(())
}

fn normalize_relevant_path(raw: &Path) -> Option<PathBuf> {
    let raw = raw.to_string_lossy();
    let raw = raw.trim();
    if raw.is_empty()
        || raw.len() > MAX_RELEVANT_PATH_BYTES
        || redact_secrets(raw) != raw
        || is_sensitive_path(raw)
    {
        return None;
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(raw).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir if normalized.pop() => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!normalized.as_os_str().is_empty()).then_some(normalized)
}

fn is_sensitive_path(path: &str) -> bool {
    Path::new(path).components().any(|component| {
        let Component::Normal(component) = component else {
            return false;
        };
        let component = component.to_string_lossy().to_ascii_lowercase();
        component == ".env"
            || component.starts_with(".env.")
            || component == "credentials"
            || component == "credentials.json"
            || component == "id_rsa"
            || component == "id_ed25519"
    })
}

fn validation_failure(message: &str) -> LibrarianFailure {
    LibrarianFailure {
        stage: LibrarianFailureStage::Validation,
        code: "librarian_summary_validation_failed",
        message: message.to_owned(),
        usage: None,
    }
}
