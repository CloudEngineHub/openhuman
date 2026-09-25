//! Report a detached sub-agent that was aborted before it could report itself.
//!
//! A background sub-agent (`spawn_async_subagent`) sends its own terminal
//! `SubagentCompleted` / `SubagentFailed` on the parent's progress channel, and
//! that one event settles everything downstream: the web bridge emits
//! `subagent_failed` to the chat card, the turn-state mirror marks the saved
//! `subagent:*` row, and the run ledger records the end.
//!
//! Cancelling it does not go through that path. `DetachedTaskRegistry`'s
//! `take_and_cancel` (Cancel, Stop, thread delete) aborts the tokio task, so
//! the future is dropped at its current `.await` and never reaches its own
//! `Cancelled` branch. Nothing was sent, and the card spun forever with a
//! "Cancel task" button that could no longer do anything.
//!
//! [`AbortReport`] is armed around the run and disarmed once the run returns
//! (every branch after that reports itself). If the future is dropped while
//! still armed, `Drop` sends the missing `SubagentFailed`. `Drop` cannot await,
//! so it tries `try_send` first; if the channel is momentarily full it hands
//! the event to a spawned task that waits for room, because this event is the
//! only thing that settles the card. A closed channel has no reader left to
//! settle anything, so that case is logged and dropped.

use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::Sender;

use crate::agent::progress::AgentProgress;

/// Error text carried by the synthesised `SubagentFailed`.
pub(crate) const ABORTED_ERROR: &str = "sub-agent was cancelled";

/// Sends `SubagentFailed` for `task_id` if dropped while armed.
pub(crate) struct AbortReport {
    progress: Option<Sender<AgentProgress>>,
    agent_id: String,
    task_id: String,
    armed: bool,
}

impl AbortReport {
    pub(crate) fn arm(
        progress: Option<Sender<AgentProgress>>,
        agent_id: &str,
        task_id: &str,
    ) -> Self {
        Self {
            progress,
            agent_id: agent_id.to_string(),
            task_id: task_id.to_string(),
            armed: true,
        }
    }

    /// The run returned; its own branches report the outcome from here on.
    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for AbortReport {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Some(tx) = self.progress.as_ref() else {
            log::debug!(
                "[subagent_abort_report] aborted without a progress sink task_id={} agent_id={}",
                self.task_id,
                self.agent_id
            );
            return;
        };
        let event = AgentProgress::SubagentFailed {
            agent_id: self.agent_id.clone(),
            task_id: self.task_id.clone(),
            error: ABORTED_ERROR.to_string(),
        };
        match tx.try_send(event) {
            Ok(()) => log::info!(
                "[subagent_abort_report] reported aborted sub-agent task_id={} agent_id={}",
                self.task_id,
                self.agent_id
            ),
            Err(TrySendError::Full(event)) => {
                let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                    log::warn!(
                        "[subagent_abort_report] channel full and no runtime to wait on task_id={} agent_id={}",
                        self.task_id,
                        self.agent_id
                    );
                    return;
                };
                log::info!(
                    "[subagent_abort_report] channel full; deferring report task_id={} agent_id={}",
                    self.task_id,
                    self.agent_id
                );
                let tx = tx.clone();
                let task_id = self.task_id.clone();
                runtime.spawn(async move {
                    if tx.send(event).await.is_err() {
                        log::warn!(
                            "[subagent_abort_report] deferred report lost: channel closed task_id={task_id}"
                        );
                    }
                });
            }
            Err(TrySendError::Closed(_)) => log::warn!(
                "[subagent_abort_report] could not report aborted sub-agent: channel closed task_id={} agent_id={}",
                self.task_id,
                self.agent_id
            ),
        }
    }
}

#[cfg(test)]
#[path = "subagent_abort_report_tests.rs"]
mod tests;
