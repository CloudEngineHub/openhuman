use super::*;

fn failed(event: AgentProgress) -> (String, String, String) {
    match event {
        AgentProgress::SubagentFailed {
            agent_id,
            task_id,
            error,
        } => (agent_id, task_id, error),
        other => panic!("expected SubagentFailed, got {other:?}"),
    }
}

/// The bug: an aborted background task is dropped mid-`.await`, so it never
/// reports. Aborting a real spawned task that holds an armed guard must still
/// deliver `SubagentFailed` on the parent's channel.
#[tokio::test]
async fn an_aborted_task_still_reports_failed() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _report = AbortReport::arm(Some(tx), "agent_memory", "sub-1");
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
    });
    started_rx.await.expect("task started");
    task.abort();
    let _ = task.await;

    let event = rx.recv().await.expect("an aborted task must report");
    assert_eq!(
        failed(event),
        (
            "agent_memory".to_string(),
            "sub-1".to_string(),
            ABORTED_ERROR.to_string()
        )
    );
}

/// A run that returned reports its own outcome; the guard must stay silent so
/// the card is not settled twice (or settled as failed after a success).
#[tokio::test]
async fn a_disarmed_guard_sends_nothing() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let mut report = AbortReport::arm(Some(tx), "help", "sub-2");
    report.disarm();
    drop(report);
    assert!(rx.recv().await.is_none(), "disarmed guard must not report");
}

/// No progress sink (a run with no chat attached) is a no-op, not a panic.
#[test]
fn no_sink_is_a_no_op() {
    drop(AbortReport::arm(None, "help", "sub-3"));
}
