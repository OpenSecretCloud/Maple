//! Git provenance and its Cargo inputs, shared with the offline freshness tests.

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct GitMetadata {
    pub revision: String,
    pub source_sha: String,
}

fn git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn watch(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
}

fn watch_reference(mut reference: String) {
    let mut seen = std::collections::HashSet::new();
    while seen.insert(reference.clone()) {
        let Some(reference_path) = git(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            &reference,
        ]) else {
            break;
        };
        let reference_path = PathBuf::from(reference_path);
        let mut existing_path = reference_path.clone();
        // A packed branch has no loose ref. Watching that missing file
        // makes Cargo rerun forever; ignoring it would miss its creation
        // on the next commit. The closest existing parent catches that
        // transition, including nested refs and linked worktrees.
        while !existing_path.exists() {
            if !existing_path.pop() {
                break;
            }
        }
        watch(&existing_path);
        // Symbolic branch aliases can themselves move. Watch every pointer
        // in the chain, as well as the leaf that advances with new commits.
        let Ok(contents) = std::fs::read_to_string(reference_path) else {
            break;
        };
        let Some(next) = contents.trim().strip_prefix("ref: ") else {
            break;
        };
        reference = next.to_string();
    }
}

pub fn emit() -> GitMetadata {
    // HEAD moves on branch switches and detached commits. In a linked
    // worktree it lives outside the common directory that owns branch refs.
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        let head_path = PathBuf::from(git_dir).join("HEAD");
        watch(&head_path);
        if let Ok(head) = std::fs::read_to_string(&head_path)
            && let Some(reference) = head.trim().strip_prefix("ref: ")
        {
            watch_reference(reference.to_string());
        }
        if let Some(packed_refs) = git(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "packed-refs",
        ]) {
            let packed_refs = PathBuf::from(packed_refs);
            // Packing deletes a watched loose ref or changes the watched
            // parent directory. Once present, this file tracks packed refs.
            // Never watch an absent packed-refs file: detached CI checkouts
            // commonly omit it, which otherwise invalidates every build.
            if packed_refs.exists() {
                watch(&packed_refs);
            }
        }
    }
    let mut revision = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
    // Preserve the existing policy: dirty state refreshes with Git pointer
    // events, rather than making every working-tree edit a metadata input.
    if git(&["status", "--porcelain"]).is_some_and(|status| !status.is_empty()) {
        revision.push_str("-dirty");
    }
    let source_sha = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=MAPLE_GIT_HASH={revision}");
    println!("cargo:rustc-env=MAPLE_GIT_SOURCE_SHA={source_sha}");
    GitMetadata {
        revision,
        source_sha,
    }
}
