use crate::error::Result;
use crate::session::{EditFeedback, LspSessionManager};
use std::path::Path;
use std::sync::Arc;

pub async fn synchronize_after_edit(
    manager: &Arc<LspSessionManager>,
    cwd: &Path,
    file: &Path,
    text: &str,
    version: i64,
) -> Result<Option<EditFeedback>> {
    manager.feedback_after_edit(cwd, file, text, version).await
}
