use dat0_core::cli;

#[test]
fn parses_hidden_telemetry_test_subcommand() {
    let cmd = cli::parse(&["dat0".into(), "__telemetry-test".into()]);
    assert!(
        cmd.is_some(),
        "hidden subcommand must be recognized by cli::parse"
    );
}

#[test]
fn bare_launch_is_not_a_cli_command() {
    assert!(cli::parse(&["dat0".into()]).is_none());
}
