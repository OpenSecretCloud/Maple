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
    // A new commit must re-bake the hash, but cargo only reruns build
    // scripts when declared inputs change. Watch the git pointers: HEAD
    // moves on branch switches, and the ref file it names moves on every
    // commit to that branch. Narrowing the triggers means the -dirty
    // suffix only refreshes alongside these files, which is acceptable:
    // the revision is the load-bearing part.
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={git_dir}/HEAD");
        if let Ok(head) = std::fs::read_to_string(format!("{git_dir}/HEAD"))
            && let Some(reference) = head.trim().strip_prefix("ref: ")
        {
            println!("cargo:rerun-if-changed={git_dir}/{reference}");
        }
    }
    let mut revision = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
    if git(&["status", "--porcelain"]).is_some_and(|status| !status.is_empty()) {
        revision.push_str("-dirty");
    }
    println!("cargo:rustc-env=MAPLE_GIT_HASH={revision}");
}
