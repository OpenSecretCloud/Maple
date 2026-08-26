mod common;

use common::parse_pcr0_environment;
use opensecret::Pcr0Environment;

#[test]
fn pcr_environment_defaults_to_production() {
    assert_eq!(
        parse_pcr0_environment(None).unwrap(),
        Pcr0Environment::Production
    );
}

#[test]
fn pcr_environment_accepts_exact_supported_values() {
    assert_eq!(
        parse_pcr0_environment(Some("production")).unwrap(),
        Pcr0Environment::Production
    );
    assert_eq!(
        parse_pcr0_environment(Some("development")).unwrap(),
        Pcr0Environment::Development
    );
}

#[test]
fn pcr_environment_rejects_empty_differently_cased_and_unknown_values() {
    for value in ["", "Production", "dev", " development "] {
        assert!(parse_pcr0_environment(Some(value)).is_err());
    }
}
