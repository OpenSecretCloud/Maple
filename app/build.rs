//! Bakes the git revision into `--version` so a running binary can be
//! matched back to a checkout. Falls back to `unknown` outside a git
//! repository (e.g. a tarball build), never fails the build.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let mut revision = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
    if git(&["status", "--porcelain"]).is_some_and(|status| !status.is_empty()) {
        revision.push_str("-dirty");
    }
    println!("cargo:rustc-env=MAPLE_GIT_HASH={revision}");
}
