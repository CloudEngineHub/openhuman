//! Reaching the `tinyconnectors` module.
//!
//! The connector implementation — the Composio client, the OAuth handoff, the
//! execute pipeline, the trigger archive, the sync providers — lives in the
//! `tinyconnectors` module, not in this crate. This is how the host calls it.
//!
//! # What stays here
//!
//! Three things deliberately did not move, and callers must keep applying them
//! around these calls:
//!
//! - **Egress policy.** [`crate::openhuman::security::egress`] refuses outbound
//!   tool calls under local-only mode and discloses every external transfer.
//!   That is host policy about the *user's* data, and the module cannot see the
//!   reasons behind it. Apply it before calling [`methods::EXECUTE`].
//! - **Which route to use.** Whether the user is signed in, whether they
//!   supplied their own Composio key, and which the product prefers are all
//!   decisions this crate makes. They become the module's configuration blob,
//!   and the module honours it rather than choosing.
//! - **Webhook delivery.** The backend HMAC-verifies Composio webhooks and fans
//!   them out over the user's sockets. The module has no socket, so the
//!   existing trigger subscriber keeps its job.
//!
//! # What the module now owns
//!
//! Scope enforcement. `ListTools` hides what the user's preference forbids and
//! `Execute` refuses it. Do **not** re-filter here against a separately stored
//! preference: two sources of truth for a permission is how one of them ends up
//! stale and permissive.

use serde::Serialize;
use serde::de::DeserializeOwned;
use tinybus::Proxy;
use tinyconnectors_bus::names;

use super::{ops, registry};
use crate::openhuman::config::Config;

/// The module's id in [`registry`].
pub const MODULE_ID: &str = "tinyconnectors";

/// The names of the members this host calls.
///
/// Re-exported so a call site spells a member through the contract rather than
/// as a string literal: a renamed member is then a compile error here instead
/// of an "unknown method" at runtime on a user's machine.
pub use names::methods;

/// A proxy to the connector module, loading it if this is the first call.
///
/// The module is registered `Lazy`, so this is where a user with connected
/// accounts pays to load it and a user without one never does.
///
/// # Errors
///
/// Returns a message naming what went wrong: modules disabled in configuration,
/// the artifact missing or failing its digest check, or the bus refusing the
/// proxy.
pub async fn proxy(config: &Config) -> Result<Proxy, String> {
    ops::ensure_loaded(config, MODULE_ID).await?;

    let record =
        registry::find(MODULE_ID).ok_or_else(|| format!("unknown module '{MODULE_ID}'"))?;
    let runtime = super::host::runtime()
        .await
        .map_err(|error| format!("the module runtime is unavailable: {error}"))?;

    runtime
        .proxy(record.bus_name, record.object_path)
        .map_err(|error| format!("could not reach '{MODULE_ID}': {error}"))
}

/// Call one member with an argument and decode its reply.
///
/// # Errors
///
/// Returns the member's failure as the module rendered it.
///
/// Note what is *not* an error: a Composio action the provider refused comes
/// back as a successful reply carrying `successful: false` and a formatted
/// message. A caller that checks only for `Err` here will report a failed send
/// as a success.
pub async fn call<Request, Reply>(
    config: &Config,
    member: &str,
    request: Request,
) -> Result<Reply, String>
where
    Request: Serialize + Send,
    Reply: DeserializeOwned,
{
    let proxy = proxy(config).await?;
    proxy
        .call::<Reply>(member, (request,))
        .await
        .map_err(|error| format!("{member}: {error}"))
}

/// Call a member that takes no arguments.
///
/// # Errors
///
/// As [`call`].
pub async fn call_bare<Reply: DeserializeOwned>(
    config: &Config,
    member: &str,
) -> Result<Reply, String> {
    let proxy = proxy(config).await?;
    proxy
        .call::<Reply>(member, ())
        .await
        .map_err(|error| format!("{member}: {error}"))
}

#[cfg(test)]
#[path = "connectors_tests.rs"]
mod tests;
