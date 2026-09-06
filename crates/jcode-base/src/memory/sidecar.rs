//! Session-owned transcript extraction at the existing memory-manager boundary.
use super::{MemoryCategory, MemoryEntry, MemoryManager, TrustLevel, memory_llm_judge_available};
use crate::sidecar::Sidecar;
use anyhow::Result;

impl MemoryManager {
    /// Extract memories from a session transcript using the Haiku sidecar
    pub async fn extract_from_transcript(
        &self,
        transcript: &str,
        session_id: &str,
    ) -> Result<Vec<String>> {
        if !memory_llm_judge_available() {
            crate::logging::info("Memory transcript extraction skipped: LLM judge unavailable");
            return Ok(Vec::new());
        }

        let sidecar = Sidecar::new().with_memory_operation(
            Some(session_id),
            crate::sidecar::MemoryOperationKind::IncrementalExtraction,
        );
        let extracted = sidecar.extract_memories(transcript).await?;

        let mut ids = Vec::new();
        for memory in extracted {
            let category: MemoryCategory = memory.category.parse().unwrap_or(MemoryCategory::Fact);
            let trust = match memory.trust.as_str() {
                "high" => TrustLevel::High,
                "medium" => TrustLevel::Medium,
                _ => TrustLevel::Low,
            };

            let entry = MemoryEntry::new(category, memory.content)
                .with_source(session_id)
                .with_trust(trust);

            // Store in project scope by default
            let id = self.remember_project(entry)?;
            ids.push(id);
        }

        Ok(ids)
    }
}
