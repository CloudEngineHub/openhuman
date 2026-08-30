//! Bridges `tinymemory-core`'s pre-split `tinyagents::harness::model`
//! vocabulary onto this crate's own `tinyinference::model` types.
//!
//! # Why this exists
//!
//! `tinyagents` was split into focused crates (`tinyagents-harness`,
//! `tinyagents-graph`, …) with the model layer (`ChatModel`, `ModelRequest`,
//! `ModelResponse`, …) moving further upstream into `tinyinference`. OpenHuman
//! migrated onto that split. `tinymemory-core`'s [`ChatHost`](tinymemory_core::chat_host::ChatHost)
//! seam has not: it still depends on the pre-split, **published**
//! `tinyagents` 2.x crate and names `tinyagents::harness::model::{ChatModel,
//! ModelResponse}` (see that crate's `chat_host.rs` module docs). That crate
//! carries its own self-contained copy of the model layer — it does not
//! depend on `tinyinference` at all — so `tinyagents::harness::model::ChatModel`
//! and `tinyinference::model::ChatModel` are two separately compiled,
//! nominally distinct traits, even though they are literally the same
//! provider-neutral request/response schema (`tinyinference`'s model types
//! are what `tinyagents`' `harness::model` looked like before the split: same
//! field names, same `#[serde]` shapes — verified by diffing the two source
//! trees at migration time).
//!
//! Because the two schemas are wire-identical, converting one to the other is
//! a safe, lossless `serde_json` round-trip rather than a real transformation.
//! That is what [`bridge_via_json`] does, and it is the only conversion this
//! module performs — no field is invented, dropped, or approximated.
//!
//! Drop this module once `tinymemory-core` migrates its `ChatHost` seam onto
//! `tinyagents-harness` / `tinyinference` directly.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Round-trips a value between two wire-identical serde types through
/// `serde_json::Value`.
///
/// Safe here specifically because `tinyagents::harness::model::*` and
/// `tinyinference::model::*` are the same schema under two different crate
/// roots (see module docs) — this is not a general-purpose "coerce any two
/// types together" helper.
fn bridge_via_json<A: Serialize, B: DeserializeOwned>(value: &A) -> Result<B, String> {
    let json = serde_json::to_value(value).map_err(|error| {
        format!(
            "[memory][model_bridge] failed to serialize during legacy/current model-type bridge: {error}"
        )
    })?;
    serde_json::from_value(json).map_err(|error| {
        format!(
            "[memory][model_bridge] failed to deserialize during legacy/current model-type bridge: {error}"
        )
    })
}

/// Bridges a legacy (`tinyagents` 2.x) [`ModelResponse`](tinyagents::harness::model::ModelResponse)
/// to this crate's current [`tinyinference::model::ModelResponse`].
///
/// Exposed to [`super::host_impls`] so `ChatHost::usage_from_response` — which
/// receives a legacy response from `tinymemory-core` — can hand it to
/// [`crate::openhuman::agent::tinyagents::model::usage_info_from_response`],
/// which only understands the current type.
pub(crate) fn bridge_response_to_new(
    response: &tinyagents::harness::model::ModelResponse,
) -> Result<tinyinference::model::ModelResponse, String> {
    bridge_via_json(response)
}

fn bridge_request_to_new(
    request: &tinyagents::harness::model::ModelRequest,
) -> Result<tinyinference::model::ModelRequest, String> {
    bridge_via_json(request)
}

fn bridge_response_to_old(
    response: &tinyinference::model::ModelResponse,
) -> Result<tinyagents::harness::model::ModelResponse, String> {
    bridge_via_json(response)
}

fn bridge_profile_to_old(
    profile: &tinyinference::model::ModelProfile,
) -> Result<tinyagents::harness::model::ModelProfile, String> {
    bridge_via_json(profile)
}

/// Maps a current [`tinyinference::Error`] onto the legacy
/// [`tinyagents::TinyAgentsError`] the `ChatHost` seam's `Result` alias
/// expects, preserving the structured [`tinyagents::harness::model::ProviderError`]
/// detail (status/code/retryable) rather than flattening it to a string —
/// same reasoning as `tinyagents_harness::TinyAgentsError`'s own
/// `From<tinyinference::Error>` impl.
fn bridge_error_to_old(error: tinyinference::Error) -> tinyagents::TinyAgentsError {
    use tinyagents::TinyAgentsError as Old;
    match error {
        tinyinference::Error::Model(message) => Old::Model(message),
        tinyinference::Error::Provider(provider_error) => {
            match bridge_via_json::<_, tinyagents::harness::model::ProviderError>(
                provider_error.as_ref(),
            ) {
                Ok(old) => Old::Provider(Box::new(old)),
                // The provider error itself failed to bridge (should not
                // happen given the identical schema) — fall back to a plain
                // message rather than losing the failure.
                Err(_) => Old::Model(provider_error.message.clone()),
            }
        }
        tinyinference::Error::Validation(message) => Old::Validation(message),
        tinyinference::Error::Serialization(error) => Old::Serialization(error),
        tinyinference::Error::Embedding(message) => Old::Embedding(message),
    }
}

/// Presents this host's current `tinyinference::model::ChatModel` as
/// `tinymemory-core`'s legacy `tinyagents::harness::model::ChatModel`.
///
/// Holds a converted, owned copy of the inner model's [`ModelProfile`]
/// (captured once at construction) because the legacy trait's `profile()`
/// returns a borrow — there is no owned legacy value to borrow from without
/// converting eagerly.
///
/// Only `invoke` is bridged; `stream` falls through to the legacy trait's
/// default (replay `invoke` as a three-item stream). The two call sites this
/// bridge exists for (`tinymemory_core::tree::tree_runtime::engine::{run_summarization,
/// rebuild_tree}`) only call `invoke`, so a real streaming bridge is not
/// needed today — add one if a legacy call site starts streaming.
pub(crate) struct LegacyChatModelBridge {
    inner: Arc<dyn tinyinference::model::ChatModel<()>>,
    profile: Option<tinyagents::harness::model::ModelProfile>,
}

impl LegacyChatModelBridge {
    pub(crate) fn new(inner: Arc<dyn tinyinference::model::ChatModel<()>>) -> Self {
        let profile = inner.profile().and_then(|profile| {
            bridge_profile_to_old(profile)
                .inspect_err(|error| {
                    tracing::warn!(
                        error = %error,
                        "[memory][model_bridge] failed to bridge ModelProfile to the legacy type; \
                         reporting no profile to tinymemory-core"
                    );
                })
                .ok()
        });
        Self { inner, profile }
    }
}

#[async_trait]
impl tinyagents::harness::model::ChatModel<()> for LegacyChatModelBridge {
    fn profile(&self) -> Option<&tinyagents::harness::model::ModelProfile> {
        self.profile.as_ref()
    }

    async fn invoke(
        &self,
        state: &(),
        request: tinyagents::harness::model::ModelRequest,
    ) -> tinyagents::Result<tinyagents::harness::model::ModelResponse> {
        let request = bridge_request_to_new(&request).map_err(tinyagents::TinyAgentsError::Model)?;
        let response = self
            .inner
            .invoke(state, request)
            .await
            .map_err(bridge_error_to_old)?;
        bridge_response_to_old(&response).map_err(tinyagents::TinyAgentsError::Model)
    }
}

#[cfg(test)]
#[path = "model_bridge_tests.rs"]
mod tests;
