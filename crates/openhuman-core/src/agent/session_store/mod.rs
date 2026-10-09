//! The host-installed session store.
//!
//! A host that keeps conversations in its own database (a cloud deployment
//! serving many users from one process) installs a
//! [`SessionStoreProvider`] here once, before agents run. From then on every
//! agent's transcripts, turn journal, run status, goals and todos go through
//! that agent's [`AgentStores`] instead of files under `workspace_dir`. For a
//! store that is not file-backed ([`replaces_files`]) the file-era mirrors
//! (the session dual-write and its shadow reads) stand down: the host store
//! is the only record.
//!
//! Like the memory engine's host binding
//! ([`crate::memory::engine::install_host_engine`]), this is process-wide:
//! OpenHuman runs one runtime per process. With nothing installed, the core
//! keeps today's on-disk layout.

use std::sync::{Arc, LazyLock, PoisonError, RwLock};

pub use agent_transcripts::{agent_transcript_root, AgentTranscriptFiles};
pub use tinyagents_session::port::{AgentStores, SessionStoreProvider};

mod agent_transcripts;

static PROVIDER: LazyLock<RwLock<Option<Arc<dyn SessionStoreProvider>>>> =
    LazyLock::new(|| RwLock::new(None));

tokio::task_local! {
    /// A provider for one task tree only, ahead of the process-wide one; how
    /// tests in a shared binary use a store without touching each other.
    static SCOPED: Arc<dyn SessionStoreProvider>;
}

/// Runs `future` with `provider` as the session store, for that task only.
/// Tasks it spawns do not inherit it.
pub async fn scope<F: std::future::Future>(
    provider: Arc<dyn SessionStoreProvider>,
    future: F,
) -> F::Output {
    SCOPED.scope(provider, future).await
}

/// Routes every agent's session state through `provider`, replacing any
/// earlier one.
pub fn install(provider: Arc<dyn SessionStoreProvider>) -> Option<Arc<dyn SessionStoreProvider>> {
    tracing::info!(
        destination = ?provider.destination_key(),
        "[session_store] host session store installed"
    );
    PROVIDER
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .replace(provider)
}

/// Removes the installed provider; the on-disk layout applies again. Returns
/// whether one was installed.
pub fn clear() -> bool {
    let had = PROVIDER
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .is_some();
    if had {
        tracing::info!("[session_store] host session store removed");
    }
    had
}

/// Clears the installed provider only when it is still the provider claimed
/// by the caller. This keeps a runtime from removing a replacement installed
/// by another owner after the runtime itself has been dropped.
pub fn clear_if(expected: &Arc<dyn SessionStoreProvider>) -> bool {
    let mut installed = PROVIDER.write().unwrap_or_else(PoisonError::into_inner);
    if installed
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, expected))
    {
        installed.take();
        tracing::info!("[session_store] host session store removed");
        true
    } else {
        false
    }
}

pub fn restore(provider: Option<Arc<dyn SessionStoreProvider>>) {
    *PROVIDER.write().unwrap_or_else(PoisonError::into_inner) = provider;
}

/// The provider in effect: the task's [`scope`]d one, else the installed
/// one, if any.
#[must_use]
pub fn installed() -> Option<Arc<dyn SessionStoreProvider>> {
    SCOPED.try_with(Arc::clone).ok().or_else(|| {
        PROVIDER
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    })
}

/// Whether a host session store is in effect.
#[must_use]
pub fn is_installed() -> bool {
    installed().is_some()
}

/// Whether the store in effect keeps conversations somewhere other than the
/// classic workspace files ([`SessionStoreProvider::workspace_dir`] is
/// `None`), so companions that read those files back have nothing to read.
#[must_use]
pub fn replaces_files() -> bool {
    installed().is_some_and(|provider| provider.workspace_dir().is_none())
}

/// `agent_id`'s stores from the installed provider, or `None` when the core
/// keeps the on-disk layout.
#[must_use]
pub fn for_agent(agent_id: &str) -> Option<AgentStores> {
    installed().map(|provider| provider.for_agent(agent_id))
}

/// `agent_id`'s transcripts from the store in effect, or `None` when the core
/// keeps them as workspace files.
#[must_use]
pub fn transcripts_for(
    agent_id: &str,
) -> Option<Arc<dyn tinyagents_session::transcript::TranscriptLocator>> {
    let stores = for_agent(agent_id)?;
    log::debug!("[session_store] transcripts via the host store agent={agent_id}");
    Some(stores.transcripts)
}

/// [`transcripts_for`] `agent_id`, or the transcript files under
/// `workspace_dir` when no store is in effect — the session host's resolution.
#[must_use]
pub fn transcripts_or_files(
    agent_id: &str,
    workspace_dir: &std::path::Path,
) -> Arc<dyn tinyagents_session::transcript::TranscriptLocator> {
    transcripts_for(agent_id).unwrap_or_else(|| {
        match crate::core::runtime::agent_scope::current_agent_id() {
            Some(embedded) => {
                log::debug!(
                    "[session_store] transcripts under the agent's directory agent={embedded}"
                );
                Arc::new(AgentTranscriptFiles::new(workspace_dir, &embedded))
            }
            None => Arc::new(tinyagents_session::transcript::FileTranscriptLocator::new(
                workspace_dir,
            )),
        }
    })
}

/// The workspace root transcripts are written under: the agent's own
/// directory under an embedded agent's context, else `workspace_dir`.
#[must_use]
pub fn transcript_root(workspace_dir: &std::path::Path) -> std::path::PathBuf {
    match crate::core::runtime::agent_scope::current_agent_id() {
        Some(agent) => agent_transcript_root(workspace_dir, &agent),
        None => workspace_dir.to_path_buf(),
    }
}

/// The stores of the agent the current [`CoreContext`] works for — the one
/// it was derived for ([`CoreContext::session_agent`]), else
/// [`DEFAULT_AGENT`] — or `None` when the core keeps the on-disk layout.
///
/// For code that has a workspace path but no agent id of its own (goals,
/// todos, the turn journal): under an embedded agent's context it lands in
/// that agent's stores.
///
/// # Errors
///
/// In SaaS mode, when the context names no agent: the shared
/// [`DEFAULT_AGENT`] bucket would mix every user's records, so the call is
/// refused instead.
///
/// [`CoreContext`]: crate::core::runtime::CoreContext
/// [`CoreContext::session_agent`]: crate::core::runtime::CoreContext::session_agent
pub fn try_current() -> Result<Option<AgentStores>, String> {
    let Some(provider) = installed() else {
        return Ok(None);
    };
    let agent = crate::core::runtime::CoreContext::current()
        .and_then(|context| context.session_agent().map(str::to_owned));
    let agent = current_agent_from(agent, crate::core::runtime::mode::is_saas())?;
    Ok(Some(provider.for_agent(&agent)))
}

/// [`try_current`]'s agent rule with its inputs made explicit: the acting
/// agent, else [`DEFAULT_AGENT`] — except in SaaS mode, which has no shared
/// bucket and refuses.
///
/// # Errors
///
/// When `saas` and there is no `agent`.
pub fn current_agent_from(agent: Option<String>, saas: bool) -> Result<String, String> {
    match agent {
        Some(agent) => Ok(agent),
        None if saas => Err(
            "no acting agent in SaaS mode; refusing the shared default session store".to_string(),
        ),
        None => Ok(DEFAULT_AGENT.to_string()),
    }
}

/// [`try_current`]'s key-value store for callers that have no error channel
/// (goals, todos): the store itself, or a store that refuses every operation
/// with the SaaS error, or `None` for the on-disk layout.
#[must_use]
pub fn current_kv() -> Option<Arc<dyn tinyagents_harness::store::Store>> {
    match try_current() {
        Ok(stores) => stores.map(|stores| stores.kv),
        Err(error) => {
            tracing::warn!("[session_store] {error}");
            Some(Arc::new(RefusedStore(error)))
        }
    }
}

/// A key-value store that fails every call, standing in for the shared bucket
/// a SaaS call with no acting agent must not reach.
struct RefusedStore(String);

impl RefusedStore {
    fn refuse<T>(&self) -> tinyagents_harness::error::Result<T> {
        Err(tinyagents_harness::error::TinyAgentsError::store(
            self.0.clone(),
        ))
    }
}

#[async_trait::async_trait]
impl tinyagents_harness::store::Store for RefusedStore {
    async fn get(
        &self,
        _namespace: &str,
        _key: &str,
    ) -> tinyagents_harness::error::Result<Option<serde_json::Value>> {
        self.refuse()
    }
    async fn put(
        &self,
        _namespace: &str,
        _key: &str,
        _value: serde_json::Value,
    ) -> tinyagents_harness::error::Result<()> {
        self.refuse()
    }
    async fn delete(&self, _namespace: &str, _key: &str) -> tinyagents_harness::error::Result<()> {
        self.refuse()
    }
    async fn list(&self, _namespace: &str) -> tinyagents_harness::error::Result<Vec<String>> {
        self.refuse()
    }
}

/// The workspace the current [`CoreContext`](crate::core::runtime::CoreContext)
/// is bound to, for a file-backed store that must follow it (the desktop
/// rebinds it when a different user signs in). Before the core has booted —
/// when no store is asked for anything — the default config's workspace.
///
/// # Errors
///
/// In SaaS mode, when the context has no workspace: the default workspace
/// would be the operator's, shared by every user.
pub fn context_workspace_dir() -> Result<std::path::PathBuf, String> {
    let workspace = crate::core::runtime::CoreContext::current()
        .and_then(|context| context.workspace_dir().ok());
    context_workspace_from(workspace, crate::core::runtime::mode::is_saas())
}

/// [`context_workspace_dir`]'s rule with its inputs made explicit.
///
/// # Errors
///
/// When `saas` and there is no `workspace`.
pub fn context_workspace_from(
    workspace: Option<std::path::PathBuf>,
    saas: bool,
) -> Result<std::path::PathBuf, String> {
    match workspace {
        Some(workspace) => Ok(workspace),
        None if saas => Err(
            "no context workspace in SaaS mode; refusing the operator's default workspace"
                .to_string(),
        ),
        None => {
            tracing::warn!("[session_store] no booted context; using the default workspace");
            Ok(crate::config::Config::default().workspace_dir)
        }
    }
}

/// The agent whose stores a turn without a definition id uses. Root turns of
/// the desktop app have no per-user agent; a single shared bucket keeps them
/// together, as the shared workspace always did.
pub const DEFAULT_AGENT: &str = "default";

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
