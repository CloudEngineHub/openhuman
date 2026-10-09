//! Durable sub-agent sessions on the `tinystoragedrivers` document port.
//!
//! Used instead of `<workspace>/.openhuman/subagent_sessions.json` when the
//! host configured a storage backend ([`crate::storage`]); `store.rs` picks it
//! per call, so the sessions live under the acting agent's storage scope
//! (`local` on a single-user host).
//!
//! # Layout
//!
//! | Collection | Document id | Holds |
//! | --- | --- | --- |
//! | `subagent_sessions` | `all` | `sessions`: the whole list the file held |
//!
//! The file store loads and saves the whole list, last writer wins; this keeps
//! that contract so the callers in `ops.rs` are unchanged.

use anyhow::Result;
use serde_json::json;
use tinystoragedrivers::{CollectionSpec, Precondition};

use super::types::DurableSubagentSession;
use crate::storage::documents::Repo;

const SESSIONS: &str = "subagent_sessions";
const LIST_ID: &str = "all";
const DOMAIN: &str = "subagent_sessions::store";

fn collections() -> Vec<CollectionSpec> {
    vec![CollectionSpec::new(SESSIONS)]
}

/// The document store for this call, when the host configured one.
pub(super) fn current() -> Result<Option<Docs>> {
    Ok(Repo::current(DOMAIN, collections)?.map(Docs))
}

/// The session list over one scoped document handle.
pub(super) struct Docs(Repo);

impl Docs {
    #[cfg(test)]
    pub(super) fn over(scoped: &crate::storage::ScopedStorage) -> Self {
        Self(Repo::over(scoped, DOMAIN, collections))
    }

    pub(super) fn load(&self) -> Result<Vec<DurableSubagentSession>> {
        let stored = self
            .0
            .run(|docs| async move { docs.get(SESSIONS, LIST_ID).await })?;
        let Some(stored) = stored else {
            return Ok(Vec::new());
        };
        let sessions = stored.doc.get("sessions").cloned().unwrap_or_default();
        Ok(serde_json::from_value(sessions)?)
    }

    pub(super) fn save(&self, sessions: &[DurableSubagentSession]) -> Result<()> {
        let doc = json!({ "sessions": sessions });
        log::debug!("[subagent_sessions] document save count={}", sessions.len());
        self.0.run(|docs| async move {
            docs.put(SESSIONS, LIST_ID, doc, Precondition::None)
                .await
                .map(|_| ())
        })
    }
}

#[cfg(test)]
#[path = "store_documents_tests.rs"]
mod tests;
