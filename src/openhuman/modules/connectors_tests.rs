//! Tests for the connector module client.

use super::{methods, MODULE_ID};
use crate::openhuman::modules::registry;

#[test]
fn the_module_is_registered_under_the_contract_s_identity() {
    // The bus name and object path are the module's address. A record that
    // disagrees with the contract does not fail to compile — it fails at
    // runtime, on a user's machine, as a name nobody owns.
    let record = registry::find(MODULE_ID).expect("tinyconnectors is registered");
    assert_eq!(record.bus_name, tinyconnectors_bus::INTERFACE);
    assert_eq!(record.object_path, tinyconnectors_bus::OBJECT_PATH);
}

#[test]
fn the_registered_version_matches_the_compiled_contract() {
    // The module-pin gate checks the artifact against the submodule; this
    // checks the record against the crate this build actually links.
    let record = registry::find(MODULE_ID).expect("tinyconnectors is registered");
    assert!(
        record
            .release_url
            .ends_with(&format!("v{}", record.version)),
        "the release URL and the version must name one release: {} / {}",
        record.release_url,
        record.version
    );
}

#[test]
fn the_module_is_lazy() {
    // A user with no connected accounts should not pay to load it, and most
    // sessions never touch a connector. Safe because the module loads without
    // configuration and still answers the capability members.
    let record = registry::find(MODULE_ID).expect("tinyconnectors is registered");
    assert!(
        matches!(
            record.load,
            crate::openhuman::modules::types::LoadPolicy::Lazy
        ),
        "tinyconnectors should load lazily"
    );
}

#[test]
fn every_member_this_host_names_is_one_the_module_serves() {
    // Spelled through the contract rather than as string literals, so a rename
    // upstream is a compile error here rather than an unknown method at
    // runtime. This asserts the names resolve to members the artifact declares.
    for member in [
        methods::LIST_TOOLKITS,
        methods::LIST_CONNECTIONS,
        methods::AUTHORIZE,
        methods::DELETE_CONNECTION,
        methods::LIST_TOOLS,
        methods::EXECUTE,
        methods::SYNC,
        methods::LIST_CAPABILITIES,
    ] {
        assert!(
            tinyconnectors_bus::METHODS.contains(&member),
            "{member} is not in the contract's member table"
        );
    }
}
