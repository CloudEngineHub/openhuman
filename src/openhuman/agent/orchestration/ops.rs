//! In-memory high-level orchestration control plane.

use super::types::{
    AgentSnapshot, AgentStatus, SpawnAgentRequest, SpawnAgentResponse, WaitAgentOptions,
    WaitAgentResponse,
};
use crate::core::bus::BUS;
use crate::core::events::DomainEvent;
use crate::openhuman::agent::harness::definition::{AgentDefinition, AgentDefinitionRegistry};
use crate::openhuman::agent::harness::fork_context::{
    current_parent, with_parent_context, ParentExecutionContext,
};
use crate::openhuman::agent::harness::subagent_runner::{
    run_subagent, SubagentRunOptions, SubagentRunOutcome,
};
use crate::openhuman::agent::progress::AgentProgress;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{mpsc, Mutex, Notify};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

#[derive(Debug, Error)]
pub enum OrchestrationError {
    #[error("agent orchestration requires an active parent agent turn")]
    NoParentContext,
    #[error("agent definition registry has not been initialized")]
    RegistryUnavailable,
    #[error("agent definition '{0}' not found")]
    DefinitionNotFound(String),
    #[error("orchestration agent '{0}' not found")]
    AgentNotFound(String),
    #[error("agent_id and prompt are required")]
    InvalidSpawnRequest,
}

#[derive(Clone)]
pub struct AgentOrchestrationSession {
    session_id: String,
    state: Arc<Mutex<SessionState>>,
    notify: Arc<Notify>,
}

#[derive(Default)]
struct SessionState {
    agents: HashMap<String, AgentRecord>,
    tasks: HashMap<String, JoinHandle<()>>,
}

#[derive(Clone)]
struct AgentRecord {
    snapshot: AgentSnapshot,
    progress_sink: Option<mpsc::Sender<AgentProgress>>,
}

impl AgentOrchestrationSession {
    /// Create an in-memory orchestration session.
    ///
    /// The `session_id` identifies the parent orchestration run in emitted
    /// [`DomainEvent`] payloads. The session starts empty and remains
    /// process-local until a future persistence layer stores snapshots.
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            state: Arc::new(Mutex::new(SessionState::default())),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Return the stable parent orchestration session id.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Spawn a child agent from the active parent agent turn.
    ///
    /// `request` must provide a non-empty `agent_id` and `prompt`; optional
    /// context, toolkit, model, parent id, and metadata are carried into the
    /// child record and sub-agent run options. On success this returns the
    /// accepted child id and initial status while a background task executes the
    /// child through [`run_subagent`].
    ///
    /// Returns [`OrchestrationError::NoParentContext`] when called outside an
    /// agent turn, [`OrchestrationError::InvalidSpawnRequest`] for an empty
    /// agent id or prompt, [`OrchestrationError::RegistryUnavailable`] when the
    /// agent definition registry is not initialized, or
    /// [`OrchestrationError::DefinitionNotFound`] for an unknown agent id. Side
    /// effects include storing a pending snapshot, publishing an
    /// [`DomainEvent::AgentOrchestrationSpawned`] event, emitting parent
    /// progress when available, and waking waiters.
    pub async fn spawn_agent(
        &self,
        request: SpawnAgentRequest,
    ) -> Result<SpawnAgentResponse, OrchestrationError> {
        let parent = current_parent().ok_or(OrchestrationError::NoParentContext)?;
        let definition = resolve_definition(&request)?;
        self.spawn_agent_with_definition(parent, definition, request)
            .await
    }

    /// Wait for one or more child agents to reach terminal status.
    ///
    /// `options.orchestration_ids` names the children to observe. An empty id
    /// list returns the current full session snapshot immediately. When
    /// `timeout_ms` is present, the wait returns a partial response with
    /// `completed = false` after the timeout instead of failing.
    ///
    /// Returns [`OrchestrationError::AgentNotFound`] if any requested child id
    /// is unknown. Side effects are limited to waiting on internal notifications;
    /// no snapshots or events are mutated.
    pub async fn wait_agents(
        &self,
        options: WaitAgentOptions,
    ) -> Result<WaitAgentResponse, OrchestrationError> {
        if options.orchestration_ids.is_empty() {
            return Ok(WaitAgentResponse {
                completed: true,
                agents: self.all_snapshots().await,
            });
        }

        let wait = async {
            loop {
                let agents = self.snapshots_for(&options.orchestration_ids).await?;
                let completed = agents.iter().all(|agent| agent.status.is_terminal());
                if completed {
                    return Ok(WaitAgentResponse { completed, agents });
                }
                self.notify.notified().await;
            }
        };

        match options.timeout_ms {
            Some(ms) => match timeout(Duration::from_millis(ms), wait).await {
                Ok(response) => response,
                Err(_) => Ok(WaitAgentResponse {
                    completed: false,
                    agents: self.snapshots_for(&options.orchestration_ids).await?,
                }),
            },
            None => wait.await,
        }
    }

    /// Abort every in-flight child task and mark non-terminal children
    /// [`AgentStatus::Cancelled`].
    ///
    /// Used by the workflow engine on stop/interrupt to drain a session's
    /// running children without going through per-child `close_agent` calls.
    /// Idempotent — children already terminal are left untouched. Wakes waiters
    /// so any pending `wait_agents` resolves immediately.
    pub async fn abort_all(&self) {
        let mut state = self.state.lock().await;
        let task_ids: Vec<String> = state.tasks.keys().cloned().collect();
        for id in task_ids {
            if let Some(task) = state.tasks.remove(&id) {
                task.abort();
            }
        }
        for record in state.agents.values_mut() {
            if !record.snapshot.status.is_terminal() {
                record.snapshot.status = AgentStatus::Cancelled;
                record.snapshot.updated_at = now();
            }
        }
        drop(state);
        self.notify.notify_waiters();
    }

    /// Every child snapshot known to this session, ordered by creation time.
    async fn all_snapshots(&self) -> Vec<AgentSnapshot> {
        let state = self.state.lock().await;
        let mut agents: Vec<AgentSnapshot> = state
            .agents
            .values()
            .map(|record| record.snapshot.clone())
            .collect();
        agents.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        agents
    }

    async fn snapshots_for(
        &self,
        ids: &[String],
    ) -> Result<Vec<AgentSnapshot>, OrchestrationError> {
        let state = self.state.lock().await;
        ids.iter()
            .map(|id| {
                state
                    .agents
                    .get(id)
                    .map(|record| record.snapshot.clone())
                    .ok_or_else(|| OrchestrationError::AgentNotFound(id.clone()))
            })
            .collect()
    }

    async fn spawn_agent_with_definition(
        &self,
        parent: ParentExecutionContext,
        definition: AgentDefinition,
        request: SpawnAgentRequest,
    ) -> Result<SpawnAgentResponse, OrchestrationError> {
        let agent_id = request.agent_id.trim().to_string();
        let prompt = request.prompt.trim().to_string();
        if agent_id.is_empty() || prompt.is_empty() {
            return Err(OrchestrationError::InvalidSpawnRequest);
        }

        let orchestration_id = format!("agent-{}", uuid::Uuid::new_v4());
        let now = now();
        let snapshot = AgentSnapshot {
            orchestration_id: orchestration_id.clone(),
            agent_id: agent_id.clone(),
            parent_agent_id: request.parent_agent_id.clone(),
            status: AgentStatus::Pending,
            prompt: prompt.clone(),
            result_summary: None,
            error: None,
            created_at: now.clone(),
            updated_at: now,
            metadata: request.metadata.clone(),
        };
        let record = AgentRecord {
            snapshot,
            progress_sink: parent.on_progress.clone(),
        };

        {
            let mut state = self.state.lock().await;
            state.agents.insert(orchestration_id.clone(), record);
        }

        BUS.publish(DomainEvent::AgentOrchestrationSpawned {
            session_id: self.session_id.clone(),
            orchestration_id: orchestration_id.clone(),
            agent_id: agent_id.clone(),
            parent_agent_id: request.parent_agent_id,
        });

        if let Some(progress) = parent.on_progress.clone() {
            let resolved_display_name = AgentDefinitionRegistry::global()
                .and_then(|reg| reg.get(&agent_id))
                .map(|def| def.display_name().to_string());
            let _ = progress
                .send(AgentProgress::SubagentSpawned {
                    agent_id: agent_id.clone(),
                    task_id: orchestration_id.clone(),
                    mode: "typed".to_string(),
                    dedicated_thread: false,
                    prompt_chars: prompt.chars().count(),
                    prompt: prompt.clone(),
                    worker_thread_id: None,
                    display_name: resolved_display_name,
                })
                .await;
        }

        let parent_workspace_descriptor = parent.workspace_descriptor.clone();
        let parent_worktree_action_dir = parent_workspace_descriptor
            .as_ref()
            .map(|descriptor| descriptor.root.clone());
        if let Some(descriptor) = parent_workspace_descriptor.as_ref() {
            tracing::debug!(
                orchestration_id = %orchestration_id,
                agent_id = %agent_id,
                workspace_root = %descriptor.root.display(),
                policy_id = %descriptor.policy_id,
                "[agent_orchestration] inheriting parent workspace descriptor"
            );
        }

        let options = SubagentRunOptions {
            skill_filter_override: None,
            toolkit_override: request.toolkit,
            context: request.context,
            model_override: request.model,
            task_id: Some(orchestration_id.clone()),
            worker_thread_id: None,
            initial_history: None,
            checkpoint_dir: None,
            worktree_action_dir: parent_worktree_action_dir,
            workspace_descriptor: parent_workspace_descriptor,
            run_queue: None,
        };

        let task_session = self.clone();
        let task_id = orchestration_id.clone();
        // Captured on *this* task: a `tokio::task_local` does not cross
        // `tokio::spawn`, so the turn's origin label and workspace root are
        // carried across the same boundary the parent execution context
        // already is. Without the origin the spawned agent's external-effect
        // tools reach the approval gate unlabelled and are refused.
        let task = tokio::spawn(crate::openhuman::agent::turn_origin::propagate(
            crate::openhuman::agent::turn_workspace::propagate(async move {
                task_session.mark_running(&task_id).await;
                let result = with_parent_context(parent, async move {
                    run_subagent(&definition, &prompt, options).await
                })
                .await;
                task_session.finish_agent(&task_id, result).await;
            }),
        ));

        {
            let mut state = self.state.lock().await;
            state.tasks.insert(orchestration_id.clone(), task);
        }
        self.notify.notify_waiters();

        Ok(SpawnAgentResponse {
            orchestration_id,
            agent_id,
            status: AgentStatus::Pending,
        })
    }

    async fn mark_running(&self, orchestration_id: &str) {
        let mut state = self.state.lock().await;
        if let Some(record) = state.agents.get_mut(orchestration_id) {
            if !record.snapshot.status.is_terminal() {
                record.snapshot.status = AgentStatus::Running;
                record.snapshot.updated_at = now();
            }
        }
        drop(state);
        self.notify.notify_waiters();
    }

    async fn finish_agent(
        &self,
        orchestration_id: &str,
        result: Result<SubagentRunOutcome, crate::openhuman::agent::harness::SubagentRunError>,
    ) {
        let mut completed_event = None;
        let mut failed_event = None;
        let mut progress_event = None;
        let mut state = self.state.lock().await;
        state.tasks.remove(orchestration_id);
        if let Some(record) = state.agents.get_mut(orchestration_id) {
            if record.snapshot.status == AgentStatus::Closed {
                drop(state);
                self.notify.notify_waiters();
                return;
            }
            match result {
                Ok(outcome) => {
                    record.snapshot.status = AgentStatus::Completed;
                    record.snapshot.result_summary = Some(outcome.output.clone());
                    record.snapshot.updated_at = now();
                    if let Some(progress) = record.progress_sink.clone() {
                        progress_event = Some((
                            progress,
                            AgentProgress::SubagentCompleted {
                                agent_id: outcome.agent_id.clone(),
                                task_id: orchestration_id.to_string(),
                                elapsed_ms: outcome.elapsed.as_millis() as u64,
                                iterations: outcome.iterations as u32,
                                output_chars: outcome.output.chars().count(),
                                output: outcome.output.clone(),
                                // Not a dropped value: these three describe a
                                // worker's *own* isolated checkout, and this
                                // path never creates one — it only inherits the
                                // parent's descriptor (above). `spawn_parallel_
                                // agents` populates them from the descriptor it
                                // freshly created per worker, and reports `None`
                                // for an inherited one for the same reason. A
                                // path with no isolation of its own has no
                                // worktree to report, so `None` is correct here
                                // rather than merely unfilled.
                                worktree_path: None,
                                changed_files: Vec::new(),
                                dirty_status: None,
                            },
                        ));
                    }
                    completed_event = Some(outcome);
                }
                Err(error) => {
                    let message = error.to_string();
                    record.snapshot.status = AgentStatus::Failed;
                    record.snapshot.error = Some(message.clone());
                    record.snapshot.updated_at = now();
                    if let Some(progress) = record.progress_sink.clone() {
                        progress_event = Some((
                            progress,
                            AgentProgress::SubagentFailed {
                                agent_id: record.snapshot.agent_id.clone(),
                                task_id: orchestration_id.to_string(),
                                error: message.clone(),
                            },
                        ));
                    }
                    failed_event = Some((record.snapshot.agent_id.clone(), message));
                }
            }
        }
        drop(state);

        if let Some(outcome) = completed_event {
            BUS.publish(DomainEvent::AgentOrchestrationCompleted {
                session_id: self.session_id.clone(),
                orchestration_id: orchestration_id.to_string(),
                agent_id: outcome.agent_id,
                elapsed_ms: outcome.elapsed.as_millis() as u64,
                output_chars: outcome.output.chars().count(),
                iterations: outcome.iterations,
            });
        }
        if let Some((agent_id, error)) = failed_event {
            BUS.publish(DomainEvent::AgentOrchestrationFailed {
                session_id: self.session_id.clone(),
                orchestration_id: orchestration_id.to_string(),
                agent_id,
                error,
            });
        }
        if let Some((progress, event)) = progress_event {
            let _ = progress.send(event).await;
        }
        self.notify.notify_waiters();
    }
}

fn resolve_definition(request: &SpawnAgentRequest) -> Result<AgentDefinition, OrchestrationError> {
    let agent_id = request.agent_id.trim();
    if agent_id.is_empty() || request.prompt.trim().is_empty() {
        return Err(OrchestrationError::InvalidSpawnRequest);
    }
    let registry =
        AgentDefinitionRegistry::global().ok_or(OrchestrationError::RegistryUnavailable)?;
    registry
        .get(agent_id)
        .cloned()
        .ok_or_else(|| OrchestrationError::DefinitionNotFound(agent_id.to_string()))
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_statuses_are_explicit() {
        assert!(AgentStatus::Completed.is_terminal());
        assert!(AgentStatus::Failed.is_terminal());
        assert!(AgentStatus::Cancelled.is_terminal());
        assert!(AgentStatus::Closed.is_terminal());
        assert!(!AgentStatus::Pending.is_terminal());
        assert!(!AgentStatus::Running.is_terminal());
        assert!(!AgentStatus::Waiting.is_terminal());
    }

    #[tokio::test]
    async fn empty_wait_lists_current_agents() {
        let session = AgentOrchestrationSession::new("test-session");
        let response = session
            .wait_agents(WaitAgentOptions {
                orchestration_ids: Vec::new(),
                timeout_ms: Some(1),
            })
            .await
            .unwrap();

        assert!(response.completed);
        assert!(response.agents.is_empty());
    }

    #[tokio::test]
    async fn unknown_wait_target_returns_not_found() {
        let session = AgentOrchestrationSession::new("test-session");
        let err = session
            .wait_agents(WaitAgentOptions {
                orchestration_ids: vec!["missing".to_string()],
                timeout_ms: Some(1),
            })
            .await
            .unwrap_err();

        assert!(matches!(err, OrchestrationError::AgentNotFound(id) if id == "missing"));
    }
}
