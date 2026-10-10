//! Profiles: in SaaS mode, each user is served as one profile.
//!
//! A profile is the tenant: one user's config, credential, workspace and
//! sandbox, laid out like the desktop's user directory
//! (`<root>/users/<profile-id>/`, the shared
//! [`ProfileLayout`](crate::config::schema::ProfileLayout)). The gateway
//! authenticates users; this domain turns a gateway user id into the
//! profile that serves that user ([`ProfileId`], per
//! [`ProfileIdMode`]), lays out its private state ([`layout`]), forces the
//! config it runs with ([`layout::profile_config`]), and keeps the open
//! profiles of the process ([`ProfileHost`]). The operator plane provisions
//! and inspects them through the `profiles.*` controllers ([`schemas`]).
//!
//! The isolation boundary is the profile's own [`CoreContext`]: its config,
//! workspace and `session_agent` (set to the profile id). Work for one user
//! runs under that context, which is what the config loader, the session
//! store and the per-thread caches key on.
//!
//! [`CoreContext`]: crate::core::runtime::CoreContext

pub mod background;
pub mod credentials;
pub mod gateway;
pub mod host;
pub mod layout;
pub mod ops;
pub mod schemas;
pub mod surface;
pub mod tools;
pub mod types;

pub use host::{current, Profile, ProfileHost};
pub use layout::ProfileLayout;
pub use schemas::{all_profiles_controller_schemas, all_profiles_registered_controllers};
pub use types::{ProfileId, ProfileIdMode, ProfileSummary};
