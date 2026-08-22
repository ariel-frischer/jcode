use super::{App, DisplayMessage};

impl App {
    pub(super) fn finish_completed_auto_poke_cycle(&mut self, confidence_label: &str) -> bool {
        // A clean cycle remains armed only when auto-poke is the configured default.
        self.auto_poke_incomplete_todos = self.auto_poke_default_on;
        self.todo_gate_digest_delivered = false;
        self.todo_completion_gate_attempts = 0;
        if !self.auto_poke_incomplete_todos {
            self.todo_confidence_spike_challenged = false;
            self.pending_queued_dispatch = false;
            return false;
        }
        if !self.todo_final_response_requested {
            self.todo_final_response_requested = true;
            self.push_display_message(DisplayMessage::system(format!(
                "✅ All todos done. Completion confidence: {}.",
                confidence_label
            )));
            self.queued_messages
                .push(crate::todo::TODO_FINAL_RESPONSE_CONTINUATION_MESSAGE.to_string());
            self.pending_queued_dispatch = true;
            return true;
        }
        self.pending_queued_dispatch = false;
        false
    }
}
