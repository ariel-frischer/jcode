//! Explicit attribution and the narrow local observation sink seam.
use super::{
    Attempt, AuthClass, MemoryCallContext, MemoryOperationKind, MemoryRequestObservation, Sidecar,
    SidecarBackend,
};
use anyhow::{Context, Result};

impl Sidecar {
    /// Attach a controlled local recorder without changing inference options.
    pub fn with_usage_recorder(mut self, recorder: crate::memory_usage::Recorder) -> Self {
        self.usage_recorder = Some(recorder);
        self.observation_tx = None;
        self
    }

    /// Bind an authentic owner before cloning votes or spawning detached work.
    pub fn with_memory_operation(
        mut self,
        session: Option<&str>,
        kind: MemoryOperationKind,
    ) -> Self {
        self.memory_context = Some(MemoryCallContext {
            session_id: session.map(str::to_owned),
            operation_id: uuid::Uuid::new_v4().to_string(),
            operation_kind: kind,
        });
        self
    }

    /// Keep vote/retry identity, or create a sub-operation under the same owner.
    pub(crate) fn for_memory_operation(&self, kind: MemoryOperationKind) -> Self {
        if self
            .memory_context
            .as_ref()
            .is_some_and(|context| context.operation_kind == kind)
        {
            self.clone()
        } else {
            self.clone().with_memory_operation(
                self.memory_context
                    .as_ref()
                    .and_then(|context| context.session_id.as_deref()),
                kind,
            )
        }
    }

    /// Attach the bounded local diagnostic sink. The owning recorder applies the
    /// effective observability controls; this method does not enable persistence.
    pub fn with_observation_sender(
        mut self,
        tx: tokio::sync::mpsc::Sender<MemoryRequestObservation>,
    ) -> Self {
        self.observation_tx = Some(tx);
        self.usage_recorder = None;
        self
    }

    pub(super) async fn send_claude_request(
        &self,
        builder: reqwest::RequestBuilder,
        auth: AuthClass,
    ) -> Result<String> {
        let mut attempt = Attempt::new(self, "claude", &self.model, None, auth);
        let result = async {
            let response = builder
                .send()
                .await
                .context("Failed to send request to Claude API")?;
            Self::parse_claude_response(response, &mut attempt.usage).await
        }
        .await;
        attempt.finish(result.is_err());
        result
    }
    /// Simple completion - send a prompt, get a response.
    /// Routes to the correct API based on the detected backend.
    pub async fn complete(&self, system: &str, user_message: &str) -> Result<String> {
        if self.backend != SidecarBackend::OpenAI && self.reasoning_override.is_some() {
            anyhow::bail!(
                "Memory reasoning effort is configured for OpenAI, but the resolved sidecar model '{}' uses the {} backend; select an OpenAI memory model or remove agents.memory_reasoning_effort",
                self.model,
                self.backend_name()
            );
        }
        let bound = if self.memory_context.is_none() {
            self.for_memory_operation(MemoryOperationKind::Unattributed)
        } else {
            self.clone()
        };
        match bound.backend {
            SidecarBackend::OpenAI => bound.complete_openai(system, user_message).await,
            SidecarBackend::Claude => bound.complete_claude(system, user_message).await,
            SidecarBackend::Provider => bound.complete_via_provider(system, user_message).await,
        }
    }
}
