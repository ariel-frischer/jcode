use crate::protocol::RecallableSoftInterrupt;

#[derive(Debug)]
enum RecallStatus {
    Idle,
    Pending {
        operation_id: String,
    },
    Ready {
        operation_id: String,
        message: RecallableSoftInterrupt,
    },
}

/// Client-side state for one authoritative server soft-interrupt recall.
///
/// A pending operation deliberately survives transport loss. Reusing its stable
/// identity after reconnect lets the server replay the original result instead
/// of recalling a second message. Results are accepted only for the current
/// operation and are consumed at most once into an empty composer.
#[derive(Debug)]
pub(in crate::tui::app) struct SoftInterruptRecallState {
    status: RecallStatus,
    completed_operation_id: Option<String>,
}

impl Default for SoftInterruptRecallState {
    fn default() -> Self {
        Self {
            status: RecallStatus::Idle,
            completed_operation_id: None,
        }
    }
}

impl SoftInterruptRecallState {
    /// Start one operation, or return `None` while an operation/result is active.
    pub(super) fn begin(&mut self) -> Option<&str> {
        if !matches!(self.status, RecallStatus::Idle) {
            return None;
        }

        let operation_id = format!("tui-soft-interrupt-recall-{:032x}", rand::random::<u128>());
        self.status = RecallStatus::Pending { operation_id };
        self.operation_id()
    }

    /// Return the stable identity to send initially or retry after reconnect.
    pub(super) fn operation_id(&self) -> Option<&str> {
        match &self.status {
            RecallStatus::Pending { operation_id } | RecallStatus::Ready { operation_id, .. } => {
                Some(operation_id)
            }
            RecallStatus::Idle => None,
        }
    }

    pub(super) fn is_pending(&self) -> bool {
        matches!(self.status, RecallStatus::Pending { .. })
    }

    /// Accept an authoritative result and return its payload only when the
    /// composer is empty. A payload arriving while the composer is occupied is
    /// retained locally until it can be applied without merging or data loss.
    pub(super) fn handle_result(
        &mut self,
        operation_id: &str,
        message: Option<RecallableSoftInterrupt>,
        composer_is_empty: bool,
    ) -> Option<RecallableSoftInterrupt> {
        if self.completed_operation_id.as_deref() == Some(operation_id) {
            return None;
        }

        let is_match = matches!(
            &self.status,
            RecallStatus::Pending {
                operation_id: pending,
            } if pending == operation_id
        );
        if !is_match {
            return None;
        }

        let Some(message) = message else {
            self.complete(operation_id);
            return None;
        };

        if composer_is_empty {
            self.complete(operation_id);
            Some(message)
        } else {
            self.status = RecallStatus::Ready {
                operation_id: operation_id.to_string(),
                message,
            };
            None
        }
    }

    /// Consume a previously confirmed payload once the composer is empty.
    pub(super) fn take_ready(
        &mut self,
        composer_is_empty: bool,
    ) -> Option<RecallableSoftInterrupt> {
        if !composer_is_empty {
            return None;
        }

        let status = std::mem::replace(&mut self.status, RecallStatus::Idle);
        let RecallStatus::Ready {
            operation_id,
            message,
        } = status
        else {
            self.status = status;
            return None;
        };
        self.completed_operation_id = Some(operation_id);
        Some(message)
    }

    fn complete(&mut self, operation_id: &str) {
        self.completed_operation_id = Some(operation_id.to_string());
        self.status = RecallStatus::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncBufReadExt;

    fn message() -> RecallableSoftInterrupt {
        RecallableSoftInterrupt {
            content: "restore exactly".to_string(),
            images: vec![
                ("image/png".to_string(), "cG5n".to_string()),
                ("image/jpeg".to_string(), "anBlZw==".to_string()),
            ],
        }
    }

    #[test]
    fn pending_operation_is_stable_for_reconnect_retry() {
        let mut state = SoftInterruptRecallState::default();
        let initial = state.begin().expect("operation should start").to_string();

        assert!(state.is_pending());
        assert_eq!(state.operation_id(), Some(initial.as_str()));
        assert!(state.begin().is_none());
        assert_eq!(state.operation_id(), Some(initial.as_str()));
    }

    #[test]
    fn only_matching_result_applies_exact_payload_once() {
        let mut state = SoftInterruptRecallState::default();
        let operation_id = state.begin().expect("operation should start").to_string();
        let expected = message();

        assert!(
            state
                .handle_result("stale-operation", Some(expected.clone()), true)
                .is_none()
        );
        assert_eq!(
            state.handle_result(&operation_id, Some(expected.clone()), true),
            Some(expected.clone())
        );
        assert!(
            state
                .handle_result(&operation_id, Some(expected), true)
                .is_none()
        );
    }

    #[test]
    fn occupied_composer_defers_matching_payload_without_mutation() {
        let mut state = SoftInterruptRecallState::default();
        let operation_id = state.begin().expect("operation should start").to_string();
        let expected = message();

        assert!(
            state
                .handle_result(&operation_id, Some(expected.clone()), false)
                .is_none()
        );
        assert!(state.take_ready(false).is_none());
        assert_eq!(state.take_ready(true), Some(expected));
        assert!(state.take_ready(true).is_none());
    }

    #[test]
    fn unavailable_and_stale_outcomes_leave_operation_retriable() {
        let mut state = SoftInterruptRecallState::default();
        let operation_id = state.begin().expect("operation should start").to_string();

        assert!(state.handle_result("stale", None, true).is_none());
        assert_eq!(state.operation_id(), Some(operation_id.as_str()));
        assert!(state.is_pending());
    }

    #[test]
    fn backend_request_preserves_caller_owned_operation_identity() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let mut remote = crate::tui::backend::RemoteConnection::dummy();
            let peer = remote
                .take_dummy_peer()
                .expect("dummy remote should retain peer stream");
            let (reader, _writer) = peer.into_split();
            let mut reader = tokio::io::BufReader::new(reader);

            let request_id = remote
                .recall_soft_interrupt("stable-operation")
                .await
                .expect("recall request should send");
            assert_eq!(request_id, 1);

            let mut line = String::new();
            reader
                .read_line(&mut line)
                .await
                .expect("recall request should be readable");
            let request: crate::protocol::Request =
                serde_json::from_str(&line).expect("recall request should deserialize");
            assert!(matches!(
                request,
                crate::protocol::Request::RecallSoftInterrupt {
                    id: 1,
                    operation_id,
                } if operation_id == "stable-operation"
            ));
        });
    }
}
