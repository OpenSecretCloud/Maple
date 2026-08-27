mod common;

use common::parse_pcr0_environment;
use opensecret::AttestationEnvironment;

#[test]
fn pcr_environment_defaults_to_production() {
    assert_eq!(
        parse_pcr0_environment(None).unwrap(),
        AttestationEnvironment::Production
    );
}

#[test]
fn pcr_environment_accepts_exact_supported_values() {
    assert_eq!(
        parse_pcr0_environment(Some("prod")).unwrap(),
        AttestationEnvironment::Production
    );
    assert_eq!(
        parse_pcr0_environment(Some("dev")).unwrap(),
        AttestationEnvironment::Development
    );
}

#[test]
fn pcr_environment_rejects_empty_differently_cased_and_unknown_values() {
    for value in ["", "Prod", "production", "development", " dev "] {
        assert!(parse_pcr0_environment(Some(value)).is_err());
    }
}
