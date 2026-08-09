#![cfg_attr(test, allow(clippy::items_after_test_module))]

pub use jcode_storage::*;

use anyhow::Result;
use serde::de::DeserializeOwned;
use std::path::Path;

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    jcode_storage::read_json_with_recovery_handler(path, |event| match event {
        jcode_storage::StorageRecoveryEvent::CorruptPrimary { path, error } => {
            crate::logging::warn(&format!(
                "Corrupt JSON at {}, trying backup: {}",
                path.display(),
                error
            ));
        }
        jcode_storage::StorageRecoveryEvent::RecoveredFromBackup { backup_path } => {
            crate::logging::info(&format!("Recovered from backup: {}", backup_path.display()));
        }
    })
}

#[cfg(any(test, feature = "test-support"))]
use std::ffi::OsString;

#[cfg(any(test, feature = "test-support"))]
use std::sync::{Mutex, MutexGuard, OnceLock};

#[cfg(any(test, feature = "test-support"))]
pub fn test_env_lock() -> &'static Mutex<()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(any(test, feature = "test-support"))]
pub struct TestEnvGuard {
    _lock: MutexGuard<'static, ()>,
    saved_credentials: Vec<(String, Option<OsString>)>,
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for TestEnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved_credentials.drain(..) {
            if let Some(value) = value {
                crate::env::set_var(&key, value);
            } else {
                crate::env::remove_var(&key);
            }
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn lock_test_env() -> TestEnvGuard {
    let lock = test_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let saved_credentials = test_credential_env_keys()
        .into_iter()
        .map(|key| {
            let value = std::env::var_os(&key);
            crate::env::remove_var(&key);
            (key, value)
        })
        .collect();

    TestEnvGuard {
        _lock: lock,
        saved_credentials,
    }
}

#[cfg(any(test, feature = "test-support"))]
fn test_credential_env_keys() -> Vec<String> {
    let mut keys = vec![
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "ANTHROPIC_API_KEY",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "AZURE_OPENAI_API_KEY",
        "GOOGLE_API_KEY",
        "GEMINI_API_KEY",
        "CURSOR_API_KEY",
        "CURSOR_ACCESS_TOKEN",
        "CURSOR_REFRESH_TOKEN",
        "BEDROCK_API_KEY",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect::<Vec<_>>();

    keys.extend(
        crate::provider_catalog::openai_compatible_profiles()
            .iter()
            .map(|profile| profile.api_key_env.to_string()),
    );
    keys.sort();
    keys.dedup();
    keys
}

#[cfg(test)]
mod tests;
