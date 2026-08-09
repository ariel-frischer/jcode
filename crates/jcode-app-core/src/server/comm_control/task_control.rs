use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "task control checks assignment state, delivery, and safe recovery paths together"
)]
pub(crate) async fn handle_comm_task_control(
    id: u64,
    req_session_id: String,
    action: String,
    task_id: String,
    target_session: Option<String>,
    message: Option<String>,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    sessions: &SessionAgents,
    soft_interrupt_queues: &crate::server::SessionInterruptQueues,
    client_connections: &Arc<RwLock<HashMap<String, ClientConnectionInfo>>>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    swarm_plans: &Arc<RwLock<HashMap<String, VersionedPlan>>>,
    swarm_coordinators: &Arc<RwLock<HashMap<String, String>>>,
    event_history: &Arc<RwLock<std::collections::VecDeque<SwarmEvent>>>,
    event_counter: &Arc<std::sync::atomic::AtomicU64>,
    swarm_event_tx: &broadcast::Sender<SwarmEvent>,
    swarm_mutation_runtime: &SwarmMutationRuntime,
) {
    let Some(action) = TaskControlAction::parse(&action) else {
        send_server_event(client_event_tx, ServerEvent::Error {
            id,
            message: "Unknown task control action. Use start, wake, resume, retry, reassign, replace, or salvage.".to_string(),
            retry_after_secs: None,
        });
        return;
    };

    let swarm_id = match require_plan_driver_swarm(
        id,
        &req_session_id,
        "Only the coordinator can control assigned tasks.",
        client_event_tx,
        swarm_members,
        swarm_plans,
        swarm_coordinators,
    )
    .await
    {
        Some(swarm_id) => swarm_id,
        None => return,
    };

    let task_id = if task_id.trim().is_empty() {
        let Some(target_session) = target_session.as_deref() else {
            send_server_event(
                client_event_tx,
                ServerEvent::Error {
                    id,
                    message: format!(
                        "task_id is required for {} unless target_session uniquely identifies an assigned task.",
                        action.as_str()
                    ),
                    retry_after_secs: None,
                },
            );
            return;
        };
        match task_id_for_target_session(&swarm_id, target_session, action, swarm_plans).await {
            Ok(task_id) => task_id,
            Err(message) => {
                send_server_event(
                    client_event_tx,
                    ServerEvent::Error {
                        id,
                        message,
                        retry_after_secs: None,
                    },
                );
                return;
            }
        }
    } else {
        task_id
    };

    let Some(snapshot) = task_snapshot_for(&swarm_id, &task_id, swarm_plans).await else {
        send_server_event(
            client_event_tx,
            ServerEvent::Error {
                id,
                message: format!("Task '{}' not found in swarm plan", task_id),
                retry_after_secs: None,
            },
        );
        return;
    };

    if !task_control_action_allows_status(action, &snapshot.status) {
        send_server_event(
            client_event_tx,
            ServerEvent::Error {
                id,
                message: task_control_status_error(action, &snapshot.status, &task_id),
                retry_after_secs: None,
            },
        );
        return;
    }

    let current_assignee = snapshot.assigned_to.clone();
    let require_assignee = matches!(
        action,
        TaskControlAction::Start
            | TaskControlAction::Wake
            | TaskControlAction::Resume
            | TaskControlAction::Retry
            | TaskControlAction::Replace
            | TaskControlAction::Salvage
            | TaskControlAction::Reassign
    );
    if require_assignee && current_assignee.is_none() {
        send_server_event(
            client_event_tx,
            ServerEvent::Error {
                id,
                message: format!(
                    "Task '{}' is not currently assigned. Use assign_task to create the first assignment.",
                    task_id
                ),
                retry_after_secs: None,
            },
        );
        return;
    }

    match action {
        TaskControlAction::Start | TaskControlAction::Wake | TaskControlAction::Resume => {
            let Some(assignee) = current_assignee.clone() else {
                send_server_event(
                    client_event_tx,
                    ServerEvent::Error {
                        id,
                        message: format!(
                            "Task '{}' no longer has an assignee. Use assign_task to create the first assignment.",
                            task_id
                        ),
                        retry_after_secs: None,
                    },
                );
                return;
            };
            if let Some(ref requested_target) = target_session
                && requested_target != &assignee
            {
                send_server_event(
                    client_event_tx,
                    ServerEvent::Error {
                        id,
                        message: format!(
                            "Task '{}' is assigned to '{}', not '{}'. Use reassign or replace to change ownership.",
                            task_id, assignee, requested_target
                        ),
                        retry_after_secs: None,
                    },
                );
                return;
            }

            let assignment_text =
                build_control_assignment_text(action, &snapshot.content, message.as_deref());
            // Validate the assignee is actually available BEFORE mutating any
            // plan state. Resuming a plain-'running' task used to requeue it
            // (flipping it to 'queued' and rewriting its progress record)
            // first and only then discover the agent was missing or busy,
            // leaving a live task falsely queued with its run history mangled
            // even though the request was rejected.
            let Some(agent_arc) = task_agent_session(&assignee, sessions).await else {
                send_server_event(
                    client_event_tx,
                    ServerEvent::Error {
                        id,
                        message: format!(
                            "Assigned session '{}' is not available. Use replace or salvage to move the task to another agent.",
                            assignee
                        ),
                        retry_after_secs: None,
                    },
                );
                return;
            };
            let Some(_member) = active_swarm_member(&assignee, swarm_members).await else {
                send_server_event(
                    client_event_tx,
                    ServerEvent::Error {
                        id,
                        message: format!(
                            "Assigned session '{}' is no longer in the swarm. Use replace or salvage to move the task.",
                            assignee
                        ),
                        retry_after_secs: None,
                    },
                );
                return;
            };

            // Some task-control callers are lightweight protocol/test paths with
            // no live Agent or persisted startup stub for the coordinator. An
            // omitted route still means legacy behavior, so the helper returns an
            // empty identity in that case.
            let coordinator = coordinator_task_identity(&req_session_id, sessions).await;
            let route = match resolve_swarm_task_route(
                snapshot.role.as_deref(),
                snapshot.model.as_deref(),
                snapshot.reasoning_effort.as_deref(),
                &crate::config::config().agents,
                &coordinator,
            ) {
                Ok(route) => route,
                Err(error) => {
                    send_server_event(
                        client_event_tx,
                        ServerEvent::Error {
                            id,
                            message: format!("Task '{}' route rejected: {}", task_id, error),
                            retry_after_secs: None,
                        },
                    );
                    return;
                }
            };

            let agent_is_idle = match agent_arc.try_lock() {
                Ok(guard) => {
                    drop(guard);
                    true
                }
                Err(_) => false,
            };

            if agent_is_idle {
                if snapshot.status != "queued"
                    && requeue_existing_assignment(
                        &swarm_id,
                        &req_session_id,
                        &assignee,
                        &task_id,
                        assignment_text.clone(),
                        swarm_plans,
                    )
                    .await
                    .is_some()
                {
                    let swarm_state = SwarmState {
                        members: Arc::clone(swarm_members),
                        swarms_by_id: Arc::clone(swarms_by_id),
                        plans: Arc::clone(swarm_plans),
                        coordinators: Arc::clone(swarm_coordinators),
                    };
                    persist_swarm_state_for(&swarm_id, &swarm_state).await;
                    broadcast_swarm_plan(
                        &swarm_id,
                        Some(format!("task_{}", action.as_str())),
                        swarm_plans,
                        swarm_members,
                        swarms_by_id,
                    )
                    .await;
                }

                spawn_assigned_task_run(
                    agent_arc,
                    assignee.clone(),
                    swarm_id.clone(),
                    task_id.clone(),
                    assignment_text,
                    route,
                    Arc::clone(swarm_members),
                    Arc::clone(swarms_by_id),
                    Arc::clone(swarm_plans),
                    Arc::clone(swarm_coordinators),
                    Arc::clone(event_history),
                    Arc::clone(event_counter),
                    swarm_event_tx.clone(),
                );
                let summary = plan_graph_status_for(&swarm_id, swarm_plans).await;
                send_server_event(
                    client_event_tx,
                    ServerEvent::CommTaskControlResponse {
                        id,
                        action: action.as_str().to_string(),
                        task_id: task_id.clone(),
                        target_session: Some(assignee.clone()),
                        status: "running".to_string(),
                        summary,
                    },
                );
                return;
            }

            if action == TaskControlAction::Wake {
                let assignment_text = append_swarm_completion_report_instructions(&assignment_text);
                let wake_message = format!(
                    "Coordinator requested you wake and continue task '{}'.\n\n{}",
                    task_id, assignment_text
                );
                queue_control_interrupt(
                    &assignee,
                    wake_message,
                    false,
                    soft_interrupt_queues,
                    sessions,
                )
                .await;
                let summary = plan_graph_status_for(&swarm_id, swarm_plans).await;
                send_server_event(
                    client_event_tx,
                    ServerEvent::CommTaskControlResponse {
                        id,
                        action: action.as_str().to_string(),
                        task_id: task_id.clone(),
                        target_session: Some(assignee.clone()),
                        status: "queued".to_string(),
                        summary,
                    },
                );
            } else {
                send_server_event(
                    client_event_tx,
                    ServerEvent::Error {
                        id,
                        message: format!(
                            "Assigned session '{}' is currently busy. Use wake to queue the task, or retry once the agent is idle.",
                            assignee
                        ),
                        retry_after_secs: Some(1),
                    },
                );
            }
        }
        TaskControlAction::Retry => {
            let Some(assignee) = current_assignee.clone() else {
                send_server_event(
                    client_event_tx,
                    ServerEvent::Error {
                        id,
                        message: format!(
                            "Task '{}' no longer has an assignee. Use assign_task to create the first assignment.",
                            task_id
                        ),
                        retry_after_secs: None,
                    },
                );
                return;
            };
            let retry_note = message.as_ref().map_or_else(
                || "Retry this assignment.".to_string(),
                |extra| {
                    format!(
                        "Retry this assignment.\n\nAdditional coordinator instructions:\n{}",
                        extra
                    )
                },
            );
            handle_comm_assign_task_with_mode(
                id,
                req_session_id,
                Some(assignee),
                Some(task_id),
                Some(retry_note),
                AssignDedupMode::AlwaysDispatch,
                client_event_tx,
                sessions,
                soft_interrupt_queues,
                client_connections,
                swarm_members,
                swarms_by_id,
                swarm_plans,
                swarm_coordinators,
                event_history,
                event_counter,
                swarm_event_tx,
                swarm_mutation_runtime,
            )
            .await;
        }
        TaskControlAction::Reassign | TaskControlAction::Replace | TaskControlAction::Salvage => {
            let Some(assignee) = current_assignee.clone() else {
                send_server_event(
                    client_event_tx,
                    ServerEvent::Error {
                        id,
                        message: format!(
                            "Task '{}' no longer has an assignee. Use assign_task to create the first assignment.",
                            task_id
                        ),
                        retry_after_secs: None,
                    },
                );
                return;
            };
            let Some(new_target) = target_session else {
                send_server_event(
                    client_event_tx,
                    ServerEvent::Error {
                        id,
                        message: format!("'target_session' is required for {}.", action.as_str()),
                        retry_after_secs: None,
                    },
                );
                return;
            };

            if new_target == assignee {
                send_server_event(
                    client_event_tx,
                    ServerEvent::Error {
                        id,
                        message: format!(
                            "Task '{}' is already assigned to '{}'.",
                            task_id, assignee
                        ),
                        retry_after_secs: None,
                    },
                );
                return;
            }

            if snapshot.status == "running" {
                send_server_event(
                    client_event_tx,
                    ServerEvent::Error {
                        id,
                        message: format!(
                            "Task '{}' is actively running on '{}'. Wait, wake, or stop that agent before handing the task off.",
                            task_id, assignee
                        ),
                        retry_after_secs: Some(1),
                    },
                );
                return;
            }

            if action == TaskControlAction::Replace
                && !matches!(
                    snapshot.status.as_str(),
                    "queued" | "failed" | "stopped" | "crashed" | "running_stale"
                )
            {
                send_server_event(
                    client_event_tx,
                    ServerEvent::Error {
                        id,
                        message: format!(
                            "Task '{}' is '{}' and cannot be safely replaced.",
                            task_id, snapshot.status
                        ),
                        retry_after_secs: None,
                    },
                );
                return;
            }

            let forwarded_message = if action == TaskControlAction::Salvage {
                let prior_name = active_swarm_member(&assignee, swarm_members)
                    .await
                    .and_then(|member| member.friendly_name);
                let summaries =
                    if let Some(agent_arc) = task_agent_session(&assignee, sessions).await {
                        if let Ok(agent) = agent_arc.try_lock() {
                            agent.get_tool_call_summaries(12)
                        } else {
                            vec![]
                        }
                    } else {
                        vec![]
                    };
                let mut salvage = format_salvage_message(
                    &assignee,
                    prior_name.as_deref(),
                    &summaries,
                    message.as_deref(),
                );
                if let Some(progress) = snapshot.progress.as_ref() {
                    if let Some(summary) = progress.checkpoint_summary.as_deref() {
                        salvage.push_str("\n\nLatest checkpoint summary:\n");
                        salvage.push_str(summary);
                    }
                    if let Some(detail) = progress.last_detail.as_deref() {
                        salvage.push_str("\n\nLatest recorded detail:\n");
                        salvage.push_str(detail);
                    }
                }
                Some(salvage)
            } else if action == TaskControlAction::Replace {
                Some(message.as_ref().map_or_else(
                    || format!("This task is replacing prior assignee '{}'.", assignee),
                    |extra| format!(
                        "This task is replacing prior assignee '{}'.\n\nAdditional coordinator instructions:\n{}",
                        assignee, extra
                    ),
                ))
            } else {
                message
            };

            let displaced_task_id = task_id.clone();
            let displaced_new_target = new_target.clone();
            let displaced_req_session = req_session_id.clone();
            handle_comm_assign_task_with_mode(
                id,
                req_session_id,
                Some(new_target),
                Some(task_id),
                forwarded_message,
                AssignDedupMode::AlwaysDispatch,
                client_event_tx,
                sessions,
                soft_interrupt_queues,
                client_connections,
                swarm_members,
                swarms_by_id,
                swarm_plans,
                swarm_coordinators,
                event_history,
                event_counter,
                swarm_event_tx,
                swarm_mutation_runtime,
            )
            .await;

            // Tell the displaced worker to stand down, but only when the
            // takeover actually landed (the re-dispatch above can still fail,
            // e.g. on an unknown target session, and then the prior assignee
            // keeps the task). Without this the displaced worker keeps
            // editing the same files as its replacement until a human DMs it.
            let takeover_landed = {
                let swarm_plans = swarm_plans.read().await;
                swarm_plans
                    .get(&swarm_id)
                    .and_then(|plan| plan.items.iter().find(|item| item.id == displaced_task_id))
                    .is_some_and(|item| {
                        item.assigned_to.as_deref() == Some(displaced_new_target.as_str())
                    })
            };
            if takeover_landed {
                let stand_down = format!(
                    "Task '{}' has been handed off to '{}' by the coordinator ({}). Stop working \
                     on it immediately: do not make further edits or commits for that task. If \
                     you have uncommitted progress worth keeping, note it in a brief message to \
                     the coordinator, then stand down.",
                    displaced_task_id,
                    displaced_new_target,
                    action.as_str()
                );
                queue_control_interrupt(
                    &assignee,
                    stand_down.clone(),
                    true,
                    soft_interrupt_queues,
                    sessions,
                )
                .await;
                if let Some(member) = swarm_members.read().await.get(&assignee) {
                    send_server_event(
                        &member.event_tx,
                        ServerEvent::Notification {
                            from_session: displaced_req_session,
                            from_name: None,
                            notification_type: NotificationType::Message {
                                scope: Some("dm".to_string()),
                                channel: None,
                                tldr: None,
                            },
                            message: stand_down,
                        },
                    );
                }
            }
        }
    }
}
