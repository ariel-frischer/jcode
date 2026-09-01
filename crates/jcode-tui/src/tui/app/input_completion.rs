use super::*;

#[derive(Clone, Debug)]
pub(super) struct TabCompletionState {
    pub(super) suggestion_index: usize,
    pub(super) suggestions: Vec<String>,
}

impl App {
    /// Autocomplete current input - cycles through suggestions on repeated Tab
    pub(super) fn autocomplete(&mut self) -> bool {
        // Get suggestions for current input
        let current_suggestions = self.command_suggestions();

        // Check if we're continuing a tab cycle from a previous base
        if let Some(state) = self.tab_completion_state.clone() {
            // If current input is in base suggestions AND there are multiple options, continue cycling
            if state.suggestions.len() > 1 && state.suggestions.iter().any(|cmd| cmd == &self.input)
            {
                let next_index = (state.suggestion_index + 1) % state.suggestions.len();
                let cmd = &state.suggestions[next_index];
                self.remember_input_undo_state();
                self.input = cmd.clone();
                self.cursor_pos = self.input.len();
                self.tab_completion_state = Some(TabCompletionState {
                    suggestion_index: next_index,
                    ..state
                });
                return true;
            }
            // Otherwise, fall through to start a new cycle with current input
        }

        // Start fresh cycle with current input
        if current_suggestions.is_empty() {
            self.tab_completion_state = None;
            return false;
        }

        // If only one suggestion and it matches exactly, add trailing space for commands
        // that accept arguments, then we're done
        if current_suggestions.len() == 1 && current_suggestions[0].0 == self.input {
            if !self.input.ends_with(' ') && Self::command_accepts_args(&self.input) {
                self.remember_input_undo_state();
                self.input.push(' ');
                self.cursor_pos = self.input.len();
                return true;
            }
            self.tab_completion_state = None;
            return false;
        }

        // Apply first suggestion and start tracking the cycle
        let selected = self
            .command_suggestion_selected
            .min(current_suggestions.len().saturating_sub(1));
        let (cmd, _) = &current_suggestions[selected];
        self.remember_input_undo_state();
        self.input = cmd.clone();
        // If unique match, add trailing space for arg-accepting commands
        if current_suggestions.len() == 1 && Self::command_accepts_args(&self.input) {
            self.input.push(' ');
        }
        self.cursor_pos = self.input.len();
        self.tab_completion_state = Some(TabCompletionState {
            suggestion_index: selected,
            suggestions: current_suggestions
                .into_iter()
                .map(|(suggestion, _)| suggestion)
                .collect(),
        });
        self.command_suggestion_selected = 0;
        true
    }
}
