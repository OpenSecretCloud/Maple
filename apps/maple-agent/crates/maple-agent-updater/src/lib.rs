//! Agent-only authenticated update discovery, download and installation primitives.
//! Callers must retain installation and host-startup fences around installation.
//!
//! Filesystem security tests target macOS/Linux Unix semantics. Windows CI
//! compiles this crate; Windows application updates are intentionally unsupported.
mod client;
pub mod installer;
pub mod manifest;
mod state;

pub use client::{CheckOutcome, Config, Revalidation, Updater};
pub use manifest::{Artifact, Downloads, Manifest, Target, VerifiedRelease};
pub use semver::Version;
pub use state::PreparedUpdate;
pub use url::Url;

#[cfg(all(test, unix))]
mod tests;
