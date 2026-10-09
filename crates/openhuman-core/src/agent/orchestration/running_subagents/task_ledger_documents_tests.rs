use super::*;
use crate::storage::{MemoryStorage, Scope, StorageBackend};
use tinyagents_tasks::{OrchestrationTaskKind, OrchestrationTaskStatus};

fn repo_in(storage: &MemoryStorage, scope: &str) -> Repo {
    let scoped = storage.for_scope(&Scope::new(scope).unwrap()).unwrap();
    Repo::over(&scoped, DOMAIN, collections)
}

fn spec(id: &str) -> OrchestrationTaskSpec {
    OrchestrationTaskSpec::new(id, OrchestrationTaskKind::SubAgent)
}

#[test]
fn transitions_persist_and_replay_on_reopen() {
    let storage = MemoryStorage::new();
    let store = DocumentTaskStore::open(repo_in(&storage, "local")).unwrap();
    store.insert(spec("t1")).unwrap();
    store.mark_running(&TaskId::from("t1")).unwrap();
    store.insert(spec("t2")).unwrap();
    store.fail(&TaskId::from("t2"), "boom".into()).unwrap_err();
    store.mark_running(&TaskId::from("t2")).unwrap();
    store.fail(&TaskId::from("t2"), "boom".into()).unwrap();

    let reopened = DocumentTaskStore::open(repo_in(&storage, "local")).unwrap();
    let t1 = reopened.get(&TaskId::from("t1")).unwrap();
    assert_eq!(t1.status, OrchestrationTaskStatus::Running);
    let t2 = reopened.get(&TaskId::from("t2")).unwrap();
    assert_eq!(t2.status, OrchestrationTaskStatus::Failed);
    assert_eq!(t2.error.as_deref(), Some("boom"));
    assert_eq!(reopened.list(OrchestrationTaskFilter::default()).len(), 2);
    // pending -> running -> failed
    assert_eq!(reopened.history(&TaskId::from("t2")).len(), 3);
}

#[test]
fn cancel_requests_persist() {
    let storage = MemoryStorage::new();
    let store = DocumentTaskStore::open(repo_in(&storage, "local")).unwrap();
    store.insert(spec("t1")).unwrap();
    store.mark_running(&TaskId::from("t1")).unwrap();
    store.request_cancel(&TaskId::from("t1")).unwrap();
    let reopened = DocumentTaskStore::open(repo_in(&storage, "local")).unwrap();
    assert_eq!(
        reopened.get(&TaskId::from("t1")).unwrap().status,
        OrchestrationTaskStatus::CancelRequested
    );
}

#[test]
fn scopes_do_not_share_a_ledger() {
    let storage = MemoryStorage::new();
    let alice = DocumentTaskStore::open(repo_in(&storage, "alice")).unwrap();
    alice.insert(spec("t1")).unwrap();
    let bob = DocumentTaskStore::open(repo_in(&storage, "bob")).unwrap();
    assert!(bob.list(OrchestrationTaskFilter::default()).is_empty());
    assert!(bob.get(&TaskId::from("t1")).is_none());
    let alice_again = DocumentTaskStore::open(repo_in(&storage, "alice")).unwrap();
    assert!(alice_again.get(&TaskId::from("t1")).is_some());
}
