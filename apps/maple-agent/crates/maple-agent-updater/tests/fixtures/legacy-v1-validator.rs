use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;

pub const MAX_ENVELOPE_BYTES: usize = 128 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_ARTIFACT_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    DarwinAarch64,
    LinuxX86_64,
}

impl Target {
    pub fn key(self) -> &'static str {
        match self {
            Self::DarwinAarch64 => "darwin-aarch64",
            Self::LinuxX86_64 => "linux-x86_64",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Downloads {
    pub macos: Artifact,
    #[serde(rename = "linux-appimage", skip_serializing_if = "Option::is_none")]
    pub linux_appimage: Option<Artifact>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub product: String,
    pub channel: String,
    pub version: String,
    pub build: String,
    pub source_sha: String,
    pub pub_date: String,
    pub notes: String,
    pub artifacts: BTreeMap<String, Artifact>,
    pub downloads: Downloads,
}

/// An authenticated manifest and its exact envelope bytes. Public fields allow
/// presentation, but every authority-bearing operation authenticates them again.
#[derive(Clone, Debug)]
pub struct VerifiedRelease {
    pub manifest: Manifest,
    pub envelope: String,
    pub origin: Url,
}

impl VerifiedRelease {
    pub fn version(&self) -> Version {
        // Only constructed after schema validation; mutation is rechecked at use.
        Version::parse(&self.manifest.version).expect("validated release version")
    }

    pub fn artifact(&self, target: Target) -> Result<&Artifact> {
        self.manifest
            .artifacts
            .get(target.key())
            .context("release does not support this target")
    }

    pub fn artifact_url(&self, target: Target) -> Result<Url> {
        Ok(self.origin.join(&self.artifact(target)?.path)?)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    manifest: String,
    signature: String,
}

pub fn verify_envelope(bytes: &[u8], origin: &Url, keys: &[String]) -> Result<VerifiedRelease> {
    ensure!(
        bytes.len() <= MAX_ENVELOPE_BYTES,
        "update envelope is too large"
    );
    let envelope: Envelope = serde_json::from_slice(bytes).context("invalid update envelope")?;
    ensure!(
        envelope.manifest.len() <= MAX_MANIFEST_BYTES,
        "update manifest is too large"
    );
    ensure!(
        envelope.signature.len() <= 4096,
        "update signature is too large"
    );
    // Require exactly the four minisign lines. The parser otherwise ignores
    // additional lines, which makes malformed publisher output ambiguous.
    ensure!(
        envelope.signature.lines().count() == 4,
        "invalid signature text"
    );
    let signature = Signature::decode(&envelope.signature).context("invalid update signature")?;
    ensure!(
        keys.iter().any(|key| {
            let key = if key.starts_with("untrusted comment:") {
                PublicKey::decode(key)
            } else {
                PublicKey::from_base64(key)
            };
            key.is_ok_and(|key| {
                key.verify(envelope.manifest.as_bytes(), &signature, false)
                    .is_ok()
            })
        }),
        "update signature was not made by a pinned key"
    );
    // Signature and its trusted comment are authenticated before interpreting
    // product, paths, sizes, version or any other manifest field.
    let manifest: Manifest =
        serde_json::from_str(&envelope.manifest).context("invalid signed manifest")?;
    validate_manifest(&manifest)?;
    ensure!(
        signature.trusted_comment()
            == format!(
                "product=maple-agent channel=stable version={}",
                manifest.version
            ),
        "signed comment does not match update identity"
    );
    Ok(VerifiedRelease {
        manifest,
        envelope: std::str::from_utf8(bytes)?.to_owned(),
        origin: origin.origin().ascii_serialization().parse()?,
    })
}

pub fn stable_version(value: &str) -> Result<Version> {
    ensure!(value.len() <= 64, "version is too long");
    let version = Version::parse(value).context("invalid update version")?;
    ensure!(
        version.pre.is_empty() && version.build.is_empty() && version.to_string() == value,
        "update version must be canonical stable SemVer"
    );
    Ok(version)
}

fn lowercase_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn validate_manifest(manifest: &Manifest) -> Result<()> {
    ensure!(
        manifest.format == "maple-agent-update-v1",
        "unsupported update format"
    );
    ensure!(manifest.product == "maple-agent", "wrong update product");
    ensure!(manifest.channel == "stable", "wrong update channel");
    stable_version(&manifest.version)?;
    ensure!(
        !manifest.build.is_empty()
            && manifest.build.len() <= 18
            && !manifest.build.starts_with('0')
            && manifest.build.bytes().all(|b| b.is_ascii_digit()),
        "invalid update build number"
    );
    ensure!(
        lowercase_hex(&manifest.source_sha, 40),
        "invalid update source SHA"
    );
    let published =
        OffsetDateTime::parse(&manifest.pub_date, &Rfc3339).context("invalid publication time")?;
    ensure!(
        manifest.pub_date.ends_with('Z') && published.offset().is_utc(),
        "publication time must be UTC"
    );
    ensure!(
        manifest.notes.len() <= 16 * 1024
            && !manifest
                .notes
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t'),
        "invalid release notes"
    );
    ensure!(
        manifest.artifacts.contains_key("darwin-aarch64"),
        "macOS artifact is required"
    );
    for (target, artifact) in &manifest.artifacts {
        let suffix = match target.as_str() {
            "darwin-aarch64" => ".app.tar.gz",
            "linux-x86_64" => ".AppImage",
            _ => bail!("unsupported update target"),
        };
        validate_artifact(artifact, &manifest.version, suffix)?;
    }
    validate_artifact(&manifest.downloads.macos, &manifest.version, ".dmg")?;
    ensure!(
        manifest.artifacts.contains_key("linux-x86_64")
            == manifest.downloads.linux_appimage.is_some(),
        "Linux updater and download inventory differ"
    );
    if let Some(artifact) = &manifest.downloads.linux_appimage {
        validate_artifact(artifact, &manifest.version, ".AppImage")?;
        ensure!(
            Some(artifact) == manifest.artifacts.get("linux-x86_64"),
            "Linux download differs from updater payload"
        );
    }
    Ok(())
}

fn validate_artifact(artifact: &Artifact, version: &str, suffix: &str) -> Result<()> {
    ensure!(
        artifact.size > 0 && artifact.size <= MAX_ARTIFACT_BYTES,
        "invalid artifact size"
    );
    ensure!(
        lowercase_hex(&artifact.sha256, 64),
        "invalid artifact SHA-256"
    );
    let prefix = format!("/agent/stable/releases/{version}/");
    let filename = artifact
        .path
        .strip_prefix(&prefix)
        .context("artifact path has wrong release prefix")?;
    ensure!(
        !filename.is_empty()
            && filename.len() <= 200
            && filename.ends_with(suffix)
            && !filename.starts_with('.')
            && filename
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
        "invalid artifact filename"
    );
    Ok(())
}
