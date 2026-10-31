use ingress_gateway_bridge_rs::settings::Settings;
use std::collections::BTreeMap;
fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
#[test]
fn defaults_are_read_only() {
    let s = Settings::parse([], &env(&[])).unwrap();
    assert!(s.dry_run);
    assert!(!s.single_writer);
    assert!(!s.once);
    assert_eq!(s.config, "bridge.yaml");
}
#[test]
fn environment_alone_configures_writer() {
    let s = Settings::parse(
        [],
        &env(&[
            ("BRIDGE_CONFIG", "/etc/bridge/bridge.yaml"),
            ("BRIDGE_DRY_RUN", "false"),
            ("BRIDGE_SINGLE_WRITER", "true"),
            ("BRIDGE_HEALTH_PROBE_BIND_ADDRESS", ":9090"),
        ]),
    )
    .unwrap();
    assert!(!s.dry_run);
    assert!(s.single_writer);
    assert_eq!(s.config, "/etc/bridge/bridge.yaml");
    assert_eq!(s.health_address, "0.0.0.0:9090");
}
#[test]
fn a_single_override_preserves_other_environment_settings() {
    let s = Settings::parse(
        ["--dry-run=true".into()],
        &env(&[
            ("BRIDGE_CONFIG", "custom.yaml"),
            ("BRIDGE_DRY_RUN", "false"),
            ("BRIDGE_SINGLE_WRITER", "true"),
        ]),
    )
    .unwrap();
    assert!(s.dry_run);
    assert!(s.single_writer);
    assert_eq!(s.config, "custom.yaml");
}
#[test]
fn cli_wins_before_validation() {
    let s = Settings::parse(
        ["--dry-run=true".into()],
        &env(&[("BRIDGE_DRY_RUN", "invalid")]),
    )
    .unwrap();
    assert!(s.dry_run);
}
#[test]
fn invalid_environment_and_unsafe_combinations_fail_at_startup() {
    for pairs in [
        vec![("BRIDGE_DRY_RUN", "yes")],
        vec![("BRIDGE_DRY_RUN", "false")],
        vec![
            ("BRIDGE_DRY_RUN", "false"),
            ("BRIDGE_SINGLE_WRITER", "true"),
            ("BRIDGE_ONCE", "true"),
        ],
        vec![("BRIDGE_CONFIG", "")],
    ] {
        assert!(Settings::parse([], &env(&pairs)).is_err());
    }
}
#[test]
fn once_can_be_driven_by_environment_or_cli() {
    assert!(
        Settings::parse([], &env(&[("BRIDGE_ONCE", "true")]))
            .unwrap()
            .once
    );
    assert!(
        !Settings::parse(["--once=false".into()], &env(&[("BRIDGE_ONCE", "true")]))
            .unwrap()
            .once
    );
}
#[test]
fn unknown_flags_and_leader_election_are_rejected() {
    assert!(Settings::parse(["--leader-elect=true".into()], &env(&[])).is_err());
    assert!(Settings::parse(["--unknown=value".into()], &env(&[])).is_err());
}
