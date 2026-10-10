//! Operator-plane operations on profiles.
//!
//! Gateway user ids are taken here and turned into profile ids at once; they
//! are never logged, stored or returned.

use super::credentials::{self, UserCredentialKind};
use super::host::{self, ProfileHost};
use super::types::{
    CredentialResult, DeprovisionResult, ProfileId, ProfileSummary, ProvisionResult,
};
use crate::core::Outcome;

fn require_host() -> Result<std::sync::Arc<ProfileHost>, String> {
    host::host().ok_or_else(|| "profiles exist only in SaaS mode".to_string())
}

/// Create the profile for gateway user `user_id`, if it does not exist yet.
pub fn provision(user_id: &str) -> Result<Outcome<ProvisionResult>, String> {
    provision_on(&*require_host()?, user_id)
}

pub(crate) fn provision_on(
    host: &ProfileHost,
    user_id: &str,
) -> Result<Outcome<ProvisionResult>, String> {
    let profile_id = ProfileId::for_user(user_id, host.saas().profile_ids)?;
    let created = host.provision(&profile_id)?;
    let log = if created {
        format!("provisioned {profile_id}")
    } else {
        format!("{profile_id} was already provisioned")
    };
    Ok(Outcome::single_log(
        ProvisionResult {
            profile_id,
            created,
        },
        log,
    ))
}

/// Close profile `profile_id` and archive its state.
pub fn deprovision(profile_id: &str) -> Result<Outcome<DeprovisionResult>, String> {
    deprovision_on(&*require_host()?, profile_id)
}

pub(crate) fn deprovision_on(
    host: &ProfileHost,
    profile_id: &str,
) -> Result<Outcome<DeprovisionResult>, String> {
    let profile_id = ProfileId::parse(profile_id)?;
    let removed = host.deprovision(&profile_id)?;
    let log = if removed {
        format!("archived {profile_id}")
    } else {
        format!("{profile_id} was not provisioned")
    };
    Ok(Outcome::single_log(
        DeprovisionResult {
            profile_id,
            removed,
        },
        log,
    ))
}

/// Every provisioned profile.
pub fn list() -> Result<Outcome<Vec<ProfileSummary>>, String> {
    let profiles = require_host()?.list()?;
    let log = format!("{} profile(s)", profiles.len());
    Ok(Outcome::single_log(profiles, log))
}

/// One profile, or an error when it is not provisioned.
pub fn status(profile_id: &str) -> Result<Outcome<ProfileSummary>, String> {
    status_on(&*require_host()?, profile_id)
}

pub(crate) fn status_on(
    host: &ProfileHost,
    profile_id: &str,
) -> Result<Outcome<ProfileSummary>, String> {
    let profile_id = ProfileId::parse(profile_id)?;
    let summary = host
        .summary(&profile_id)?
        .ok_or_else(|| format!("profile {profile_id} is not provisioned"))?;
    Ok(Outcome::single_log(
        summary,
        format!("status of {profile_id}"),
    ))
}

/// Install the backend credential the gateway holds for profile `profile_id`.
pub fn set_credential(
    profile_id: &str,
    kind: UserCredentialKind,
    token: &str,
    expires_at: Option<&str>,
) -> Result<Outcome<CredentialResult>, String> {
    set_credential_on(&*require_host()?, profile_id, kind, token, expires_at)
}

pub(crate) fn set_credential_on(
    host: &ProfileHost,
    profile_id: &str,
    kind: UserCredentialKind,
    token: &str,
    expires_at: Option<&str>,
) -> Result<Outcome<CredentialResult>, String> {
    let profile_id = ProfileId::parse(profile_id)?;
    // From the layout, not `open`: installing or revoking a credential must
    // work even when every profile slot is busy.
    let config = host.provisioned_config(&profile_id)?;
    credentials::store(&config, kind, token, expires_at)?;
    log::info!("[profiles] credential installed kind={kind:?}");
    Ok(Outcome::single_log(
        CredentialResult {
            profile_id: profile_id.clone(),
            has_credential: true,
        },
        format!("credential installed for {profile_id}"),
    ))
}

/// Remove every credential profile `profile_id` holds.
pub fn clear_credential(profile_id: &str) -> Result<Outcome<CredentialResult>, String> {
    clear_credential_on(&*require_host()?, profile_id)
}

pub(crate) fn clear_credential_on(
    host: &ProfileHost,
    profile_id: &str,
) -> Result<Outcome<CredentialResult>, String> {
    let profile_id = ProfileId::parse(profile_id)?;
    let config = host.provisioned_config(&profile_id)?;
    let removed = credentials::clear(&config)?;
    log::info!("[profiles] credential cleared removed={removed}");
    let log = if removed {
        format!("credential cleared for {profile_id}")
    } else {
        format!("{profile_id} held no credential")
    };
    Ok(Outcome::single_log(
        CredentialResult {
            profile_id,
            has_credential: false,
        },
        log,
    ))
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
