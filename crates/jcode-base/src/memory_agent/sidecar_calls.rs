//! Owned memory calls, separated to keep the orchestration file within budget.
use super::{infer_candidate_tag, manager_for_working_dir};
use crate::{memory, sidecar::Sidecar};
use anyhow::Result;

pub(super) async fn run_final_extraction(
    transcript: String,
    session_id: String,
    working_dir: Option<String>,
) {
    run_final_extraction_with_sidecar(transcript, session_id, working_dir, Sidecar::new()).await;
}

pub(super) async fn run_final_extraction_with_sidecar(
    transcript: String,
    session_id: String,
    working_dir: Option<String>,
    sidecar: Sidecar,
) {
    let sidecar = sidecar.with_memory_operation(
        Some(&session_id),
        crate::sidecar::MemoryOperationKind::FinalExtraction,
    );
    crate::logging::info(&format!(
        "Final extraction starting for session {} ({} chars)",
        session_id,
        transcript.len()
    ));

    let manager = manager_for_working_dir(working_dir.as_deref());

    let memories = match manager.list_all() {
        Ok(memories) => memories,
        Err(_) => {
            crate::logging::warn("Final extraction could not read existing memories");
            Vec::new()
        }
    };
    let existing: Vec<String> = memories
        .into_iter()
        .filter(|e| e.active)
        .map(|e| e.content)
        .collect();

    let result = sidecar
        .extract_memories_with_existing(&transcript, &existing)
        .await;

    match result {
        Ok(extracted) if !extracted.is_empty() => {
            let mut stored_count = 0;

            for mem in &extracted {
                let category = crate::memory::MemoryCategory::from_extracted(&mem.category);

                let trust = match mem.trust.as_str() {
                    "high" => crate::memory::TrustLevel::High,
                    "low" => crate::memory::TrustLevel::Low,
                    _ => crate::memory::TrustLevel::Medium,
                };

                let entry = crate::memory::MemoryEntry::new(category, &mem.content)
                    .with_source(&session_id)
                    .with_trust(trust);

                match manager.remember_project(entry) {
                    Ok(_) => stored_count += 1,
                    Err(_) => crate::logging::warn("Failed to store final extraction memory"),
                }
            }

            if stored_count > 0 {
                crate::logging::info(&format!(
                    "Final extraction for session {}: stored {} memories",
                    session_id, stored_count
                ));
            }
        }
        Ok(_) => {
            crate::logging::info(&format!(
                "Final extraction for session {}: no memories extracted",
                session_id
            ));
        }
        Err(e) => {
            crate::logging::info(&format!(
                "Final extraction for session {} failed: {}",
                session_id, e
            ));
        }
    }
}

pub(super) async fn name_cluster_with_sidecar(
    member_contents: &[String],
    session_id: Option<&str>,
) -> Result<String> {
    if !memory::memory_sidecar_enabled() {
        let fallback = infer_candidate_tag(&member_contents.join(" "))
            .unwrap_or_else(|| "shared context".to_string());
        return Ok(fallback);
    }

    name_cluster_with_client(member_contents, session_id, &Sidecar::new()).await
}

pub(super) async fn name_cluster_with_client(
    member_contents: &[String],
    session_id: Option<&str>,
    sidecar: &Sidecar,
) -> Result<String> {
    let sidecar = sidecar.clone().with_memory_operation(
        session_id,
        crate::sidecar::MemoryOperationKind::ClusterNaming,
    );
    let mut prompt = String::from(
        "These memories were retrieved together. Give this cluster a short descriptive name (2-4 words, no quotes):\n",
    );
    for (i, content) in member_contents.iter().enumerate() {
        prompt.push_str(&format!("{}. {}\n", i + 1, content));
    }
    let name = sidecar
        .complete(
            "You name memory clusters. Reply with ONLY the cluster name, 2-4 words, no quotes or punctuation.",
            &prompt,
        )
        .await?;
    let name = name.trim().to_string();
    if name.is_empty() || name.len() > 60 {
        anyhow::bail!("Invalid cluster name");
    }
    Ok(name)
}
