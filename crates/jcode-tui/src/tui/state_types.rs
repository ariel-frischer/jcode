#[derive(Clone)]
pub struct ContextSnapshot {
    pub info: Option<crate::prompt::ContextInfo>,
    pub revision: u64,
    pub fresh: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundTaskRowStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct InlineFilePreview {
    pub display_path: String,
    pub content: String,
    pub markdown: bool,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct InlineFilePreviewKey {
    pub message_index: usize,
    pub message_hash: u64,
}
