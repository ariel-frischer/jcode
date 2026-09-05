//! Request-local finalization. No disk, pricing, logging, or inference work here.
use super::{Sidecar, usage::UsageAccumulator};
use jcode_session_types::memory_usage::{
    AttemptCoverage, AuthClass, CostEstimate, MEMORY_USAGE_SCHEMA_VERSION, MemoryCallContext,
    MemoryOperationKind, MemoryRequestObservation, PricingBasis, ReasoningEffort, RequestOutcome,
};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc::Sender;

// Phase-4 diagnostics must include loss in its coverage reporting, including when
// the sink is unavailable. Never emit raw records/errors from this hot path.
static LOST_OBSERVATIONS: AtomicU64 = AtomicU64::new(0);

pub fn lost_observations() -> u64 {
    LOST_OBSERVATIONS.load(Ordering::Relaxed)
}

pub(super) struct Attempt {
    record: MemoryRequestObservation,
    pub usage: UsageAccumulator,
    tx: Option<Sender<MemoryRequestObservation>>,
    finished: bool,
}

impl Attempt {
    pub fn new(
        sidecar: &Sidecar,
        provider: &str,
        model: &str,
        effort: Option<&str>,
        auth: AuthClass,
    ) -> Self {
        Self {
            record: MemoryRequestObservation {
                schema_version: MEMORY_USAGE_SCHEMA_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                context: sidecar
                    .memory_context
                    .clone()
                    .unwrap_or_else(|| MemoryCallContext {
                        session_id: None,
                        operation_id: uuid::Uuid::new_v4().to_string(),
                        operation_kind: MemoryOperationKind::Unattributed,
                    }),
                recorded_at: chrono::Utc::now(),
                provider: provider.to_string(),
                model: model.to_string(),
                effort: match effort {
                    Some("none") => Some(ReasoningEffort::None),
                    Some("minimal") => Some(ReasoningEffort::Minimal),
                    Some("low") => Some(ReasoningEffort::Low),
                    Some("medium") => Some(ReasoningEffort::Medium),
                    Some("high") => Some(ReasoningEffort::High),
                    Some("xhigh") => Some(ReasoningEffort::Xhigh),
                    Some("max") => Some(ReasoningEffort::Max),
                    _ => None,
                },
                auth_class: auth,
                outcome: RequestOutcome::Cancelled,
                usage: Default::default(),
                attempt_coverage: AttemptCoverage::PhysicalAttempt,
                pricing: CostEstimate {
                    basis: PricingBasis::Unknown,
                    estimate_nano_usd: None,
                    known_subtotal_nano_usd: 0,
                },
            },
            usage: UsageAccumulator::default(),
            tx: sidecar.observation_tx.clone(),
            finished: false,
        }
    }

    pub fn provider_call_only(&mut self) {
        self.record.attempt_coverage = AttemptCoverage::ProviderCallOnly;
    }

    pub fn finish(&mut self, failed: bool) {
        self.finished = true;
        self.record.outcome = if failed {
            RequestOutcome::Error
        } else {
            self.usage.outcome
        };
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        self.record.usage = self.usage.usage;
        if !self.finished {
            self.record.outcome = RequestOutcome::Cancelled;
        }
        // Fixed record shape and bounded identifiers are checked before any sink.
        // try_send never waits for a slow or failed diagnostics worker.
        if self.record.validate().is_err() {
            LOST_OBSERVATIONS.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if let Some(tx) = &self.tx {
            if tx.try_send(self.record.clone()).is_err() {
                LOST_OBSERVATIONS.fetch_add(1, Ordering::Relaxed);
            }
        } else {
            LOST_OBSERVATIONS.fetch_add(1, Ordering::Relaxed);
        }
    }
}
