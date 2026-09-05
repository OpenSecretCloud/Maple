use std::process::{Command, Output};

fn assert_secret_value_is_redacted(output: Output, canary: &str) {
    assert_eq!(output.status.code(), Some(2));
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(rendered.contains("[REDACTED]"));
    assert!(!rendered.contains(canary));
}

#[test]
fn invalid_cli_cache_root_is_redacted_by_the_proxy_binary() {
    let canary = "CLI_CACHE_NAMESPACE_ROOT_CANARY";
    let output = Command::new(env!("CARGO_BIN_EXE_maple-proxy"))
        .args(["--cache-namespace-root", canary])
        .output()
        .unwrap();

    assert_secret_value_is_redacted(output, canary);
}

#[test]
fn invalid_environment_cache_root_is_redacted_by_the_proxy_binary() {
    let canary = "ENV_CACHE_NAMESPACE_ROOT_CANARY";
    let output = Command::new(env!("CARGO_BIN_EXE_maple-proxy"))
        .env("MAPLE_CACHE_NAMESPACE_ROOT", canary)
        .output()
        .unwrap();

    assert_secret_value_is_redacted(output, canary);
}
