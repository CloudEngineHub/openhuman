//! [`ProfileHost`]: the profiles a SaaS process has open.
//!
//! A profile is opened lazily on first use and kept until it has been idle
//! for [`SaasConfig::idle_evict_secs`] or the host needs its slot
//! ([`SaasConfig::max_profiles_open`]). An profile still in use — anyone holding
//! its [`Profile`] — is never evicted.
//!
//! Each open profile carries its own [`CoreContext`], derived from the operator
//! context with the profile's forced config and `session_agent` set to its id.
//! Running work under that context is what makes the session store, the
//! config loader and the web_chat session cache resolve that user's state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::layout::{self, ProfileLayout};
use super::types::{ProfileId, ProfileMeta, ProfileSummary, LAYOUT_VERSION};
use crate::config::Config;
use crate::core::runtime::{ContextOverlay, CoreContext, DomainSet, SaasConfig};
use crate::tools::toolpacks::ToolGroups;

/// One open profile.
pub struct Profile {
    pub id: ProfileId,
    pub layout: ProfileLayout,
    pub config: Config,
    context: Arc<CoreContext>,
}

impl std::fmt::Debug for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Profile")
            .field("id", &self.id)
            .finish()
    }
}

impl Profile {
    /// The context every piece of this profile's work runs under.
    pub fn context(&self) -> &Arc<CoreContext> {
        &self.context
    }
}

struct Slot {
    state: Arc<Profile>,
    last_used: Instant,
}

/// The open profiles of one SaaS process.
pub struct ProfileHost {
    saas: SaasConfig,
    operator: Arc<CoreContext>,
    open: Mutex<HashMap<ProfileId, Slot>>,
    /// Profiles whose leftovers from a previous process were already swept
    /// (see [`recover_workspace`]). Kept across evictions: a re-open after an
    /// eviction must not mark a turn this process is still running as
    /// interrupted.
    recovered: Mutex<std::collections::HashSet<ProfileId>>,
}

impl std::fmt::Debug for ProfileHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileHost")
            .field("root", &self.saas.root)
            .field("open", &self.open_count())
            .finish()
    }
}

/// The domain families a profile's context serves. Within them, only the
/// methods on [`USER_METHODS`](super::surface::USER_METHODS) are reachable.
pub fn user_domains() -> DomainSet {
    DomainSet {
        threads: true,
        channels: true,
        memory: true,
        ..DomainSet::none()
    }
}

impl ProfileHost {
    pub fn new(saas: SaasConfig, operator: Arc<CoreContext>) -> Self {
        Self {
            saas,
            operator,
            open: Mutex::new(HashMap::new()),
            recovered: Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// The operator's settings this host runs with.
    pub fn saas(&self) -> &SaasConfig {
        &self.saas
    }

    /// Where profile `id`'s state lives, provisioned or not.
    pub fn layout_of(&self, id: &ProfileId) -> ProfileLayout {
        self.layout(id)
    }

    fn layout(&self, id: &ProfileId) -> ProfileLayout {
        ProfileLayout::new(&self.saas.root, id)
    }

    /// Create profile `id`'s directories. Returns whether it was new.
    pub fn provision(&self, id: &ProfileId) -> Result<bool, String> {
        // Under the open-profile lock, like `deprovision`, so the two never
        // interleave on one profile's directory.
        let _guard = self.lock();
        let layout = self.layout(id);
        if layout.meta_path.exists() {
            log::debug!("[profiles] provision profile={id}: already provisioned");
            return Ok(false);
        }
        for dir in [&layout.workspace_dir, &layout.sandbox_dir] {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        }
        let meta = ProfileMeta {
            profile_id: id.clone(),
            created_at: unix_now(),
            layout_version: LAYOUT_VERSION,
        };
        let raw = toml::to_string(&meta).map_err(|e| format!("encoding profile meta: {e}"))?;
        std::fs::write(&layout.meta_path, raw)
            .map_err(|e| format!("writing {}: {e}", layout.meta_path.display()))?;
        log::info!("[profiles] provisioned profile={id}");
        Ok(true)
    }

    /// Close profile `id` and archive its state under `<root>/deprovisioned/`.
    /// Nothing is deleted. Returns whether there was such an profile.
    ///
    /// The open-profile lock is held for the whole operation, so no `open` can
    /// re-open the profile between closing it and moving its directory. An
    /// profile still in use (a request holds its state) is not archived from
    /// under it: deprovisioning fails and can be retried.
    pub fn deprovision(&self, id: &ProfileId) -> Result<bool, String> {
        let mut open = self.lock();
        if let Some(slot) = open.get(id) {
            if Arc::strong_count(&slot.state) > 1 {
                return Err(format!("profile {id} is in use; try again shortly"));
            }
        }
        open.remove(id);
        let layout = self.layout(id);
        if !layout.dir.exists() {
            return Ok(false);
        }
        // Credential secrets live in the process keyring under the profile id,
        // not only in the profile's directory, so archiving the directory alone
        // would let a re-provisioned profile pick the old credential back up.
        if let Err(e) = super::credentials::clear(&layout::profile_config(&layout, id)) {
            log::warn!("[profiles] clearing credentials of profile={id} before archiving: {e}");
        }
        let archive = layout::archive_dir(&self.saas.root);
        std::fs::create_dir_all(&archive)
            .map_err(|e| format!("creating {}: {e}", archive.display()))?;
        // Unique even when one user is deprovisioned twice in a second.
        let dest = archive.join(format!(
            "{id}-{}-{}",
            unix_now(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::rename(&layout.dir, &dest)
            .map_err(|e| format!("archiving {}: {e}", layout.dir.display()))?;
        drop(open);
        log::info!("[profiles] deprovisioned profile={id} (archived)");
        Ok(true)
    }

    /// The forced config of provisioned profile `id`, without opening it (and
    /// so without taking an profile slot).
    pub fn provisioned_config(&self, id: &ProfileId) -> Result<crate::config::Config, String> {
        let layout = self.layout(id);
        if !layout.meta_path.exists() {
            return Err(format!("profile {id} is not provisioned"));
        }
        Ok(layout::profile_config(&layout, id))
    }

    /// Profile `id`, opening it if it is provisioned and not open yet.
    pub fn open(&self, id: &ProfileId) -> Result<Arc<Profile>, String> {
        let now = Instant::now();
        let mut open = self.lock();
        if let Some(slot) = open.get_mut(id) {
            slot.last_used = now;
        }
        // Every open sweeps profiles idle past `idle_evict_secs`, so they close
        // even when no new user arrives.
        self.sweep_idle_locked(&mut open, now);
        if let Some(slot) = open.get(id) {
            return Ok(Arc::clone(&slot.state));
        }

        let layout = self.layout(id);
        if !layout.meta_path.exists() {
            return Err(format!("profile {id} is not provisioned"));
        }
        self.evict_locked(&mut open, now);
        if open.len() >= self.saas.max_profiles_open.max(1) {
            return Err(format!(
                "all {} profile slots are in use; try again shortly",
                self.saas.max_profiles_open
            ));
        }

        let first_open = self
            .recovered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.clone());
        if first_open {
            recover_workspace(id, &layout.workspace_dir);
        }
        let config = layout::profile_config(&layout, id);
        let context = self.operator.derive_with(
            ContextOverlay::new(config.clone(), user_domains(), ToolGroups::none())
                .without_user_skill_roots()
                .session_agent(id.as_str()),
        );
        let state = Arc::new(Profile {
            id: id.clone(),
            layout,
            config,
            context,
        });
        open.insert(
            id.clone(),
            Slot {
                state: Arc::clone(&state),
                last_used: now,
            },
        );
        log::debug!("[profiles] opened profile={id} ({} open)", open.len());
        Ok(state)
    }

    /// Profile `id` if it is open.
    pub fn get(&self, id: &ProfileId) -> Option<Arc<Profile>> {
        self.lock().get(id).map(|slot| Arc::clone(&slot.state))
    }

    pub fn is_open(&self, id: &ProfileId) -> bool {
        self.lock().contains_key(id)
    }

    pub fn open_count(&self) -> usize {
        self.lock().len()
    }

    /// Every provisioned profile, open or not.
    pub fn list(&self) -> Result<Vec<ProfileSummary>, String> {
        let dir = layout::users_dir(&self.saas.root);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("reading {}: {e}", dir.display())),
        };
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(id) = ProfileId::parse(&name) else {
                continue;
            };
            // One unreadable profile must not hide the rest (or stop the
            // background loop for everyone): log it and move on.
            match self.summary(&id) {
                Ok(Some(summary)) => found.push(summary),
                Ok(None) => {}
                Err(error) => log::warn!("[profiles] skipping profile={id} in listing: {error}"),
            }
        }
        found.sort_by(|a, b| a.profile_id.cmp(&b.profile_id));
        Ok(found)
    }

    /// Profile `id`, or `None` when it is not provisioned.
    pub fn summary(&self, id: &ProfileId) -> Result<Option<ProfileSummary>, String> {
        let layout = self.layout(id);
        let raw = match std::fs::read_to_string(&layout.meta_path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("reading {}: {e}", layout.meta_path.display())),
        };
        let meta: ProfileMeta = toml::from_str(&raw)
            .map_err(|e| format!("parsing {}: {e}", layout.meta_path.display()))?;
        Ok(Some(ProfileSummary {
            profile_id: id.clone(),
            created_at: meta.created_at,
            open: self.is_open(id),
            has_credential: super::credentials::has(&layout::profile_config(&layout, id)),
        }))
    }

    /// Close profiles idle past the configured limit that nobody holds.
    pub fn evict_idle(&self) {
        let mut open = self.lock();
        self.evict_locked(&mut open, Instant::now());
    }

    /// Close profiles idle past `idle_evict_secs` that nothing is using.
    fn sweep_idle_locked(&self, open: &mut HashMap<ProfileId, Slot>, now: Instant) {
        let idle_limit = Duration::from_secs(self.saas.idle_evict_secs);
        open.retain(|id, slot| {
            let keep = Arc::strong_count(&slot.state) > 1
                || now.duration_since(slot.last_used) < idle_limit;
            if !keep {
                log::debug!("[profiles] evicted idle profile={id}");
            }
            keep
        });
    }

    fn evict_locked(&self, open: &mut HashMap<ProfileId, Slot>, now: Instant) {
        let in_use = |slot: &Slot| Arc::strong_count(&slot.state) > 1;
        self.sweep_idle_locked(open, now);
        // Still full: make room by closing the least recently used idle one.
        if open.len() >= self.saas.max_profiles_open.max(1) {
            let victim = open
                .iter()
                .filter(|(_, slot)| !in_use(slot))
                .min_by_key(|(_, slot)| slot.last_used)
                .map(|(id, _)| id.clone());
            if let Some(id) = victim {
                open.remove(&id);
                log::debug!("[profiles] evicted least recently used profile={id}");
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ProfileId, Slot>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Settle what a previous process left in profile `id`'s workspace: turns that
/// were mid-flight become interrupted and run-ledger rows left running are
/// closed. The sweep a single-user core runs at boot, run per profile on its
/// first open in this process. Failures are logged; the profile still opens.
pub(crate) fn recover_workspace(id: &ProfileId, workspace_dir: &std::path::Path) {
    let now = chrono::Utc::now().to_rfc3339();
    match tinyagents_session::turn_state::store::mark_all_interrupted(
        workspace_dir.to_path_buf(),
        &now,
    ) {
        Ok(0) => {}
        Ok(turns) => log::info!("[profiles] profile={id} recovered {turns} interrupted turn(s)"),
        Err(error) => log::warn!("[profiles] profile={id} turn recovery failed: {error}"),
    }
    match tinyagents_session::run_ledger::interrupt_orphaned_agent_runs(workspace_dir) {
        Ok(0) => {}
        Ok(runs) => log::info!("[profiles] profile={id} settled {runs} orphaned run(s)"),
        Err(error) => log::warn!("[profiles] profile={id} run recovery failed: {error:#}"),
    }
}

static HOST: OnceLock<Arc<ProfileHost>> = OnceLock::new();

/// Install the process's profile host. A SaaS boot does this once; later calls
/// are ignored.
pub fn install(host: Arc<ProfileHost>) {
    if HOST.set(host).is_err() {
        log::warn!("[profiles] profile host already installed; keeping the first");
    }
}

/// The process's profile host, when this is a SaaS process.
pub fn host() -> Option<Arc<ProfileHost>> {
    HOST.get().cloned()
}

/// The profile the current work runs for, if any.
pub fn current() -> Option<Arc<Profile>> {
    let profile = CoreContext::current()?.session_agent()?.to_owned();
    let id = ProfileId::parse(&profile).ok()?;
    host()?.get(&id)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
