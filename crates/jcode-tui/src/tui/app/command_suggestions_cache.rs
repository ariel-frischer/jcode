#[derive(Clone, Debug, Default)]
pub(super) struct CommandCandidatesCache {
    pub(super) candidates: Vec<(String, &'static str)>,
}

/// Memoized result of command suggestions for one exact input buffer.
#[derive(Clone, Debug)]
pub(super) struct CommandSuggestionsCache {
    pub(super) input: String,
    pub(super) signature: CommandSuggestionsSignature,
    pub(super) epoch: u64,
    pub(super) suggestions: Vec<(String, &'static str)>,
}

/// Non-input state that command suggestions inspect before the input buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CommandSuggestionsSignature {
    pub(super) pending_login: bool,
    pub(super) pending_account_input: bool,
    pub(super) pending_ssh_remote_name: bool,
    pub(super) inline_preview_kind: Option<crate::tui::PickerKind>,
}
