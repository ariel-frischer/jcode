//! Mock provider for e2e tests
//!
//! Returns pre-scripted StreamEvent sequences for deterministic testing.

use anyhow::Result;
use async_stream::stream;
use jcode::message::{Message, StreamEvent, ToolDefinition};
use jcode::provider::{EventStream, Provider};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub struct MockProvider {
    responses: Arc<Mutex<VecDeque<Vec<StreamEvent>>>>,
    models: Vec<&'static str>,
    current_model: Arc<Mutex<String>>,
    current_reasoning_effort: Arc<Mutex<Option<String>>>,
    /// Captured system prompts from complete() calls (for testing)
    pub captured_system_prompts: Arc<Mutex<Vec<String>>>,
    /// Captured resume session IDs from complete() calls (for testing)
    pub captured_resume_session_ids: Arc<Mutex<Vec<Option<String>>>>,
    /// Captured model names from complete() calls (for testing)
    pub captured_models: Arc<Mutex<Vec<String>>>,
    /// Captured reasoning effort values from complete() calls
    pub captured_reasoning_efforts: Arc<Mutex<Vec<Option<String>>>>,
    /// Captured provider messages from complete() calls (for request-shape tests)
    pub captured_messages: Arc<Mutex<Vec<Vec<Message>>>>,
    /// Captured tool definitions from complete() calls (for tool-surface tests)
    pub captured_tools: Arc<Mutex<Vec<Vec<ToolDefinition>>>>,
    /// Number of provider requests received by this fixture
    pub request_count: Arc<AtomicUsize>,
}

impl MockProvider {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(VecDeque::new())),
            models: Vec::new(),
            current_model: Arc::new(Mutex::new("mock".to_string())),
            current_reasoning_effort: Arc::new(Mutex::new(None)),
            captured_system_prompts: Arc::new(Mutex::new(Vec::new())),
            captured_resume_session_ids: Arc::new(Mutex::new(Vec::new())),
            captured_models: Arc::new(Mutex::new(Vec::new())),
            captured_reasoning_efforts: Arc::new(Mutex::new(Vec::new())),
            captured_messages: Arc::new(Mutex::new(Vec::new())),
            captured_tools: Arc::new(Mutex::new(Vec::new())),
            request_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn with_models(models: Vec<&'static str>) -> Self {
        let current = models
            .first()
            .map(|m| (*m).to_string())
            .unwrap_or_else(|| "mock".to_string());
        Self {
            responses: Arc::new(Mutex::new(VecDeque::new())),
            models,
            current_model: Arc::new(Mutex::new(current)),
            current_reasoning_effort: Arc::new(Mutex::new(None)),
            captured_system_prompts: Arc::new(Mutex::new(Vec::new())),
            captured_resume_session_ids: Arc::new(Mutex::new(Vec::new())),
            captured_models: Arc::new(Mutex::new(Vec::new())),
            captured_reasoning_efforts: Arc::new(Mutex::new(Vec::new())),
            captured_messages: Arc::new(Mutex::new(Vec::new())),
            captured_tools: Arc::new(Mutex::new(Vec::new())),
            request_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Queue a response (sequence of StreamEvents) to be returned on next complete() call
    pub fn queue_response(&self, events: Vec<StreamEvent>) {
        self.responses.lock().unwrap().push_back(events);
    }
}

#[async_trait::async_trait]
impl Provider for MockProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
        resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        self.captured_messages
            .lock()
            .unwrap()
            .push(messages.to_vec());
        self.captured_tools.lock().unwrap().push(tools.to_vec());
        self.request_count.fetch_add(1, Ordering::SeqCst);
        // Capture the system prompt for testing
        self.captured_system_prompts
            .lock()
            .unwrap()
            .push(system.to_string());
        self.captured_resume_session_ids
            .lock()
            .unwrap()
            .push(resume_session_id.map(|s| s.to_string()));
        self.captured_models.lock().unwrap().push(self.model());
        self.captured_reasoning_efforts
            .lock()
            .unwrap()
            .push(self.reasoning_effort());

        let events = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default();

        let stream = stream! {
            for event in events {
                yield Ok(event);
            }
        };

        Ok(Box::pin(stream))
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn model(&self) -> String {
        self.current_model.lock().unwrap().clone()
    }

    fn set_model(&self, model: &str) -> Result<()> {
        if !self.models.is_empty() && !self.models.contains(&model) {
            anyhow::bail!("Unknown model: {}", model);
        }
        *self.current_model.lock().unwrap() = model.to_string();
        Ok(())
    }

    fn available_models(&self) -> Vec<&'static str> {
        self.models.clone()
    }

    fn reasoning_effort(&self) -> Option<String> {
        self.current_reasoning_effort.lock().unwrap().clone()
    }

    fn set_reasoning_effort(&self, effort: &str) -> Result<()> {
        *self.current_reasoning_effort.lock().unwrap() = Some(effort.to_owned());
        Ok(())
    }

    fn fork(&self) -> Arc<dyn Provider> {
        let current = self.current_model.lock().unwrap().clone();
        let current_reasoning_effort = self.current_reasoning_effort.lock().unwrap().clone();
        Arc::new(MockProvider {
            responses: self.responses.clone(),
            models: self.models.clone(),
            current_model: Arc::new(Mutex::new(current)),
            current_reasoning_effort: Arc::new(Mutex::new(current_reasoning_effort)),
            captured_system_prompts: self.captured_system_prompts.clone(),
            captured_resume_session_ids: self.captured_resume_session_ids.clone(),
            captured_models: self.captured_models.clone(),
            captured_reasoning_efforts: self.captured_reasoning_efforts.clone(),
            captured_messages: self.captured_messages.clone(),
            captured_tools: self.captured_tools.clone(),
            request_count: self.request_count.clone(),
        })
    }
}
