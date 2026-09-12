//! Opt-in swarm completion delivery through the existing interrupt queue.
//!
//! A one-shot reservation closes the interval between the last interrupt drain
//! and releasing an active turn's agent lock. The queue remains authoritative:
//! consumption or cancellation before reservation means there is nothing to do.

use super::live_turn::{LiveTurnSwarmContext, spawn_tracked_live_turn};
use super::{SessionAgents, SessionInterruptQueues, queue_soft_interrupt_for_session};
use jcode_agent_runtime::SoftInterruptSource;
use std::sync::Arc;

pub(super) async fn dispatch_member_completion(
    event: &crate::bus::SwarmMemberCompleted,
    sessions: &SessionAgents,
    queues: &SessionInterruptQueues,
    swarm: LiveTurnSwarmContext,
) {
    if !crate::config::config().agents.swarm_completion_wake {
        return;
    }
    let owned = {
        let members = swarm.members.read().await;
        members.get(&event.worker_session_id).is_some_and(|worker| {
            worker.report_back_to_session_id.as_deref() == Some(&event.owner_session_id)
                && worker.swarm_id.as_deref() == Some(&event.swarm_id)
        }) && members.get(&event.owner_session_id).is_some_and(|owner| {
            owner.swarm_id.as_deref() == Some(&event.swarm_id)
                && owner.session_id != event.worker_session_id
        })
    };
    if !owned {
        return;
    }
    if super::background_tasks::emit_external_wake(
        &event.owner_session_id,
        "swarm_member_completed",
        &event.notification,
        &swarm.members,
    )
    .await
    {
        return;
    }
    if !queue_completion_wake(
        &event.owner_session_id,
        &event.notification,
        sessions,
        queues,
        swarm,
    )
    .await
    {
        crate::logging::warn(&format!(
            "Failed to schedule owned swarm completion for session {}",
            event.owner_session_id
        ));
    }
}

pub(super) async fn queue_completion_wake(
    session_id: &str,
    notification: &str,
    sessions: &SessionAgents,
    queues: &SessionInterruptQueues,
    swarm: LiveTurnSwarmContext,
) -> bool {
    let Some(agent) = sessions.read().await.get(session_id).cloned() else {
        return false;
    };
    if !has_live_attachment(session_id, &swarm).await {
        return false;
    }
    if !queue_soft_interrupt_for_session(
        session_id,
        notification.to_string(),
        false,
        SoftInterruptSource::BackgroundTask,
        queues,
        sessions,
    )
    .await
    {
        return false;
    }

    let session_id = session_id.to_string();
    let notification = notification.to_string();
    let sessions = Arc::clone(sessions);
    let queues = Arc::clone(queues);
    tokio::spawn(async move {
        let reservation = Arc::clone(&agent).lock_owned().await;
        let same_agent = sessions
            .read()
            .await
            .get(&session_id)
            .is_some_and(|current| Arc::ptr_eq(current, &agent));
        if !same_agent
            || !crate::config::config().agents.swarm_completion_wake
            || !has_live_attachment(&session_id, &swarm).await
        {
            return;
        }
        // The existing queue helper persists when a busy agent has not yet
        // registered its queue. Restore only that fallback, never an already
        // registered live queue's persisted snapshot.
        if !queues.read().await.contains_key(&session_id) {
            reservation.restore_persisted_soft_interrupts();
            super::register_session_interrupt_queue(
                &queues,
                &session_id,
                reservation.soft_interrupt_queue(),
            )
            .await;
        }
        let queue = reservation.soft_interrupt_queue();
        let pending = match queue.lock() {
            Ok(mut queue) => queue
                .iter()
                .position(|message| {
                    message.source == SoftInterruptSource::BackgroundTask
                        && message.content == notification
                })
                .map(|index| queue.remove(index)),
            Err(_) => {
                crate::logging::warn("Swarm completion queue lock is unavailable");
                return;
            }
        };
        let Some(pending) = pending else {
            return;
        };
        reservation.persist_soft_interrupt_snapshot();
        spawn_tracked_live_turn(
            &session_id,
            reservation,
            pending.content,
            Some(
                "A swarm completion is ready. Review the result and continue the requested work."
                    .to_string(),
            ),
            Some(crate::session::StoredDisplayRole::BackgroundTask),
            Some("Processing swarm completion".to_string()),
            swarm,
        )
        .await;
    });
    true
}

async fn has_live_attachment(session_id: &str, swarm: &LiveTurnSwarmContext) -> bool {
    swarm
        .members
        .read()
        .await
        .get(session_id)
        .is_some_and(|member| !member.event_txs.is_empty() || !member.event_tx.is_closed())
}
