//! One-shot `openhuman-core` subcommands honor the configured storage URL
//! (`OPENHUMAN_STORAGE_URL`) the way the server does, instead of reading the
//! classic on-disk layout. The `storage` domain's URL rule is exercised
//! through the real binary.

use std::process::Command;

fn core(workspace: &std::path::Path, url: Option<&str>, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_openhuman-core"));
    cmd.args(args)
        .env("HOME", workspace)
        .env("USERPROFILE", workspace)
        .env("OPENHUMAN_WORKSPACE", workspace)
        .env_remove("OPENHUMAN_STORAGE_URL");
    if let Some(url) = url {
        cmd.env("OPENHUMAN_STORAGE_URL", url);
    }
    cmd.output().expect("run openhuman-core")
}

#[test]
fn a_one_shot_command_refuses_a_storage_url_it_cannot_open() {
    let tmp = tempfile::tempdir().unwrap();
    let output = core(tmp.path(), Some("nonsense://nowhere"), &["cron", "list"]);
    assert!(!output.status.success(), "a bad URL must fail the command");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("storage"), "{stderr}");
}

#[test]
fn a_one_shot_command_opens_a_valid_storage_url_and_help_does_not_need_one() {
    let tmp = tempfile::tempdir().unwrap();
    let output = core(tmp.path(), Some("memory"), &["cron", "list"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("opening the configured storage backend"),
        "{stderr}"
    );
    // Help never opens the backend, even a bad one.
    let help = core(tmp.path(), Some("nonsense://nowhere"), &["--help"]);
    assert!(help.status.success());
}
