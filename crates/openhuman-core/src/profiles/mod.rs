//! User agents: in SaaS mode, each user is served as one agent.
//!
//! The gateway authenticates users; this domain turns a gateway user id into
//! the agent that serves that user ([`ProfileId`]), lays out the agent's
//! private state ([`layout`]), forces the config it runs with
//! ([`layout::profile_config`]), and keeps the open agents of the process
//! ([`ProfileHost`]). The operator plane provisions and inspects them through
//! the `profiles.*` controllers ([`schemas`]).
//!
//! The isolation boundary is the agent's own [`CoreContext`]: its config,
//! workspace and `session_agent`. Work for one user runs under that context,
//! which is what the config loader, the session store and the per-thread
//! caches key on.
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

pub use host::{current, ProfileHost, Profile};
pub use schemas::{all_profiles_controller_schemas, all_profiles_registered_controllers};
pub use types::{ProfileId, ProfileSummary};
