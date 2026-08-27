//! Offline trust policy for OpenSecret Nitro enclave releases.
//!
//! The generated snapshot embedded by this module is an output of the SDK's
//! Sigstore verification/update tool. Runtime clients never fetch release
//! metadata from GitHub or query Rekor. They accept an attestation only when
//! its complete PCR0/PCR1/PCR2 tuple occurs in the snapshot for the explicitly
//! selected environment.

use crate::{
    attestation::AttestationDocument,
    error::{Error, Result},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const SNAPSHOT_SCHEMA: &str = "https://opensecret.cloud/sdk/trusted-enclave-releases/v1";
const MANIFEST_SCHEMA: &str = "https://opensecret.cloud/attestations/nitro-eif-release/v1";
const EXPECTED_OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";
const EXPECTED_SOURCE_REPOSITORY: &str = "OpenSecretCloud/opensecret";
const EXPECTED_SOURCE_REPOSITORY_ID: u64 = 921_901_924;
const EXPECTED_SOURCE_REPOSITORY_OWNER_ID: u64 = 185_423_582;
const EXPECTED_WORKFLOW_PATH: &str = ".github/workflows/release-nitro-eif.yml";
const EXPECTED_WORKFLOW_NAME: &str = "Nitro EIF Release";
const EXPECTED_WORKFLOW_TRIGGER: &str = "workflow_dispatch";
const EXPECTED_WORKFLOW_ENVIRONMENT: &str = "production-release";
const EXPECTED_EIF_MEDIA_TYPE: &str = "application/vnd.aws.nitro.eif";
const SHA256_HEX_LEN: usize = 64;
const SHA384_HEX_LEN: usize = 96;
const SHA384_BYTES_LEN: usize = 48;

const EMBEDDED_RELEASE_SNAPSHOT: &str =
    include_str!("../assets/trusted_enclave_releases.generated.json");

/// Signed release environment authorized by an attestation policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttestationEnvironment {
    Production,
    Development,
}

impl AttestationEnvironment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Production => "prod",
            Self::Development => "dev",
        }
    }
}

/// A validated, immutable set of trusted enclave measurements for one
/// deployment environment.
///
/// Constructing a custom policy is deliberately explicit: callers must provide
/// a snapshot in the same strict format as the generated production asset and
/// select the environment it is allowed to authorize.
#[derive(Clone, Debug)]
pub struct TrustedReleasePolicy {
    expected_environment: String,
    snapshot_id: String,
    releases: Vec<TrustedRelease>,
}

#[derive(Clone, Debug)]
struct TrustedRelease {
    tag: String,
    pcr0: [u8; SHA384_BYTES_LEN],
    pcr1: [u8; SHA384_BYTES_LEN],
    pcr2: [u8; SHA384_BYTES_LEN],
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleaseSnapshot {
    schema: String,
    policy: SnapshotPolicy,
    snapshot_id: String,
    releases: Vec<SnapshotRelease>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotPolicy {
    oidc_issuer: String,
    source_repository: String,
    source_repository_id: u64,
    source_repository_owner_id: u64,
    workflow: SnapshotWorkflow,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotWorkflow {
    path: String,
    name: String,
    trigger: String,
    environment: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotRelease {
    manifest_sha256: String,
    bundle_sha256: String,
    signer: SnapshotSigner,
    transparency_log: SnapshotTransparencyLog,
    manifest: ReleaseManifest,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotSigner {
    oidc_issuer: String,
    identity: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleaseManifest {
    schema: String,
    environment: String,
    source: SnapshotSource,
    release: SnapshotReleaseIdentity,
    artifact: SnapshotArtifact,
    measurements: SnapshotMeasurements,
    build: SnapshotBuild,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotSource {
    repository: String,
    repository_id: u64,
    owner_id: u64,
    r#ref: String,
    commit: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotReleaseIdentity {
    tag: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotArtifact {
    name: String,
    media_type: String,
    sha256: String,
    size: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotMeasurements {
    algorithm: String,
    required_pcrs: [u8; 3],
    pcrs: SnapshotPcrs,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SnapshotPcrs {
    #[serde(rename = "0")]
    pcr0: String,
    #[serde(rename = "1")]
    pcr1: String,
    #[serde(rename = "2")]
    pcr2: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotTransparencyLog {
    log_index: String,
    log_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotBuild {
    system: String,
    flake_lock_sha256: String,
    derivation: String,
    workflow_run: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotIdInput<'a> {
    schema: &'a str,
    policy: &'a SnapshotPolicy,
    releases: &'a [SnapshotRelease],
}

impl TrustedReleasePolicy {
    /// Loads the embedded snapshot and selects exactly one environment.
    pub fn embedded(environment: AttestationEnvironment) -> Result<Self> {
        let policy = Self::from_snapshot_json(EMBEDDED_RELEASE_SNAPSHOT, environment.as_str())?;

        let raw: ReleaseSnapshot = serde_json::from_str(EMBEDDED_RELEASE_SNAPSHOT)
            .map_err(|error| Error::TrustedReleasePolicy(error.to_string()))?;
        validate_official_policy(&raw.policy)?;

        Ok(policy)
    }

    /// Loads the build-time snapshot for the production OpenSecret service.
    ///
    /// An empty, well-formed snapshot is accepted here so SDK artifacts can be
    /// prepared before the first signed release is published. Verification
    /// still fails closed with [`Error::UnreleasedAttestationPolicy`].
    pub fn embedded_production() -> Result<Self> {
        Self::embedded(AttestationEnvironment::Production)
    }

    /// Loads the build-time snapshot for an explicitly selected development
    /// OpenSecret enclave.
    pub fn embedded_development() -> Result<Self> {
        Self::embedded(AttestationEnvironment::Development)
    }

    /// Validates a generated snapshot and binds it to exactly one environment.
    ///
    /// This is intended for explicitly configured development or self-hosted
    /// deployments. It does not weaken Nitro document verification.
    pub fn from_snapshot_json(
        snapshot_json: &str,
        expected_environment: impl Into<String>,
    ) -> Result<Self> {
        let expected_environment = expected_environment.into();
        validate_environment(&expected_environment)?;

        let raw: ReleaseSnapshot = serde_json::from_str(snapshot_json)
            .map_err(|error| Error::TrustedReleasePolicy(error.to_string()))?;
        if raw.schema != SNAPSHOT_SCHEMA {
            return Err(policy_error(format!(
                "unsupported snapshot schema '{}'",
                raw.schema
            )));
        }
        validate_hex("snapshotId", &raw.snapshot_id, SHA256_HEX_LEN)?;
        validate_snapshot_id(&raw)?;
        validate_nonempty("policy.oidcIssuer", &raw.policy.oidc_issuer)?;
        validate_nonempty("policy.sourceRepository", &raw.policy.source_repository)?;
        if raw.policy.source_repository_id == 0 {
            return Err(policy_error(
                "policy.sourceRepositoryId must be greater than zero",
            ));
        }
        if raw.policy.source_repository_owner_id == 0 {
            return Err(policy_error(
                "policy.sourceRepositoryOwnerId must be greater than zero",
            ));
        }
        validate_workflow_path(&raw.policy.workflow.path)?;
        validate_nonempty("policy.workflow.name", &raw.policy.workflow.name)?;
        validate_nonempty("policy.workflow.trigger", &raw.policy.workflow.trigger)?;
        validate_nonempty(
            "policy.workflow.environment",
            &raw.policy.workflow.environment,
        )?;

        let mut releases = Vec::new();
        let mut release_keys = HashSet::new();
        let mut manifest_hashes = HashSet::new();
        for release in raw.releases {
            validate_release(&release, &raw.policy)?;
            let release_key = format!(
                "{}:{}",
                release.manifest.environment, release.manifest.release.tag
            );
            if !release_keys.insert(release_key.clone()) {
                return Err(policy_error(format!(
                    "duplicate trusted release entry '{release_key}'"
                )));
            }
            if !manifest_hashes.insert(release.manifest_sha256.clone()) {
                return Err(policy_error(format!(
                    "duplicate trusted release manifest '{}'",
                    release.manifest_sha256
                )));
            }
            if release.manifest.environment != expected_environment {
                continue;
            }

            releases.push(TrustedRelease {
                tag: release.manifest.release.tag,
                pcr0: decode_pcr(
                    "manifest.measurements.pcrs.0",
                    &release.manifest.measurements.pcrs.pcr0,
                )?,
                pcr1: decode_pcr(
                    "manifest.measurements.pcrs.1",
                    &release.manifest.measurements.pcrs.pcr1,
                )?,
                pcr2: decode_pcr(
                    "manifest.measurements.pcrs.2",
                    &release.manifest.measurements.pcrs.pcr2,
                )?,
            });
        }

        Ok(Self {
            expected_environment,
            snapshot_id: raw.snapshot_id,
            releases,
        })
    }

    /// The environment this policy is permitted to authorize.
    pub fn environment(&self) -> &str {
        &self.expected_environment
    }

    /// Stable identifier of the generated release snapshot.
    pub fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    /// Verifies the complete PCR0/PCR1/PCR2 tuple atomically.
    pub fn verify_attestation(&self, document: &AttestationDocument) -> Result<()> {
        if self.releases.is_empty() {
            return Err(Error::UnreleasedAttestationPolicy {
                environment: self.expected_environment.clone(),
            });
        }
        if document.digest != "SHA384" {
            return Err(Error::AttestationVerificationFailed(format!(
                "Attestation digest must be SHA384, got '{}'",
                document.digest
            )));
        }

        let pcr0 = attestation_pcr(document, 0)?;
        let pcr1 = attestation_pcr(document, 1)?;
        let pcr2 = attestation_pcr(document, 2)?;

        if self
            .releases
            .iter()
            .any(|release| release.pcr0 == pcr0 && release.pcr1 == pcr1 && release.pcr2 == pcr2)
        {
            return Ok(());
        }

        let release_tags = self
            .releases
            .iter()
            .map(|release| release.tag.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Err(Error::AttestationVerificationFailed(format!(
            "PCR0/PCR1/PCR2 tuple is not present in trusted snapshot {} for environment '{}' (published releases: {})",
            self.snapshot_id, self.expected_environment, release_tags
        )))
    }
}

fn validate_snapshot_id(snapshot: &ReleaseSnapshot) -> Result<()> {
    let input = SnapshotIdInput {
        schema: &snapshot.schema,
        policy: &snapshot.policy,
        releases: &snapshot.releases,
    };
    let actual = hex::encode(Sha256::digest(canonical_json_bytes(&input)?));
    if snapshot.snapshot_id != actual {
        return Err(policy_error(format!(
            "snapshotId '{}' does not match snapshot contents '{}'",
            snapshot.snapshot_id, actual
        )));
    }
    Ok(())
}

fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value)
        .map_err(|error| policy_error(format!("failed to serialize trusted policy: {error}")))?;
    let mut bytes = serde_json::to_vec_pretty(&sort_json(value))
        .map_err(|error| policy_error(format!("failed to serialize trusted policy: {error}")))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn sort_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(sort_json).collect())
        }
        serde_json::Value::Object(values) => {
            let mut entries = values.into_iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            let mut sorted = serde_json::Map::new();
            for (key, value) in entries {
                sorted.insert(key, sort_json(value));
            }
            serde_json::Value::Object(sorted)
        }
        value => value,
    }
}

fn validate_official_policy(policy: &SnapshotPolicy) -> Result<()> {
    for (field, actual, expected) in [
        (
            "policy.oidcIssuer",
            policy.oidc_issuer.clone(),
            EXPECTED_OIDC_ISSUER.to_string(),
        ),
        (
            "policy.sourceRepository",
            policy.source_repository.clone(),
            EXPECTED_SOURCE_REPOSITORY.to_string(),
        ),
        (
            "policy.sourceRepositoryId",
            policy.source_repository_id.to_string(),
            EXPECTED_SOURCE_REPOSITORY_ID.to_string(),
        ),
        (
            "policy.sourceRepositoryOwnerId",
            policy.source_repository_owner_id.to_string(),
            EXPECTED_SOURCE_REPOSITORY_OWNER_ID.to_string(),
        ),
        (
            "policy.workflow.path",
            policy.workflow.path.clone(),
            EXPECTED_WORKFLOW_PATH.to_string(),
        ),
        (
            "policy.workflow.name",
            policy.workflow.name.clone(),
            EXPECTED_WORKFLOW_NAME.to_string(),
        ),
        (
            "policy.workflow.trigger",
            policy.workflow.trigger.clone(),
            EXPECTED_WORKFLOW_TRIGGER.to_string(),
        ),
        (
            "policy.workflow.environment",
            policy.workflow.environment.clone(),
            EXPECTED_WORKFLOW_ENVIRONMENT.to_string(),
        ),
    ] {
        if actual != expected {
            return Err(policy_error(format!(
                "{field} must be '{expected}', got '{actual}'"
            )));
        }
    }
    Ok(())
}

fn validate_release(release: &SnapshotRelease, policy: &SnapshotPolicy) -> Result<()> {
    validate_hex(
        "release.manifestSha256",
        &release.manifest_sha256,
        SHA256_HEX_LEN,
    )?;
    let canonical_manifest = canonical_json_bytes(&release.manifest)?;
    let actual_manifest_sha256 = hex::encode(Sha256::digest(&canonical_manifest));
    if release.manifest_sha256 != actual_manifest_sha256 {
        return Err(policy_error(format!(
            "release manifestSha256 '{}' does not match embedded manifest '{}'",
            release.manifest_sha256, actual_manifest_sha256
        )));
    }
    validate_hex(
        "release.bundleSha256",
        &release.bundle_sha256,
        SHA256_HEX_LEN,
    )?;
    if release.signer.oidc_issuer != policy.oidc_issuer {
        return Err(policy_error(format!(
            "release signer issuer '{}' does not match policy issuer '{}'",
            release.signer.oidc_issuer, policy.oidc_issuer
        )));
    }

    let manifest = &release.manifest;
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(policy_error(format!(
            "unsupported release manifest schema '{}'",
            manifest.schema
        )));
    }
    validate_environment(&manifest.environment)?;
    if manifest.source.repository != policy.source_repository {
        return Err(policy_error(format!(
            "release source repository '{}' does not match policy repository '{}'",
            manifest.source.repository, policy.source_repository
        )));
    }
    if manifest.source.repository_id != policy.source_repository_id
        || manifest.source.owner_id != policy.source_repository_owner_id
    {
        return Err(policy_error(
            "release source repository IDs do not match snapshot policy",
        ));
    }
    validate_hex(
        "release.manifest.source.commit",
        &manifest.source.commit,
        40,
    )?;
    validate_release_tag(&manifest.release.tag)?;
    let expected_ref = format!("refs/tags/{}", manifest.release.tag);
    if manifest.source.r#ref != expected_ref {
        return Err(policy_error(format!(
            "release source ref '{}' does not match tag '{}'",
            manifest.source.r#ref, manifest.release.tag
        )));
    }
    let expected_identity = format!(
        "https://github.com/{}/{}@{}",
        policy.source_repository, policy.workflow.path, manifest.source.r#ref
    );
    if release.signer.identity != expected_identity {
        return Err(policy_error(format!(
            "release signer identity '{}' does not match '{}'",
            release.signer.identity, expected_identity
        )));
    }

    let expected_artifact_name = format!(
        "opensecret-{}-{}.eif",
        manifest.release.tag, manifest.environment
    );
    if manifest.artifact.name != expected_artifact_name {
        return Err(policy_error(format!(
            "release artifact name '{}' does not match '{}'",
            manifest.artifact.name, expected_artifact_name
        )));
    }
    validate_artifact_name(&manifest.artifact.name)?;
    if manifest.artifact.media_type != EXPECTED_EIF_MEDIA_TYPE {
        return Err(policy_error(format!(
            "release artifact media type must be '{EXPECTED_EIF_MEDIA_TYPE}'"
        )));
    }
    if manifest.artifact.size == 0 {
        return Err(policy_error(
            "release artifact size must be greater than zero",
        ));
    }
    validate_hex(
        "release.artifact.sha256",
        &manifest.artifact.sha256,
        SHA256_HEX_LEN,
    )?;
    if manifest.measurements.algorithm != "sha384" {
        return Err(policy_error(
            "release measurement algorithm must be 'sha384'",
        ));
    }
    if manifest.measurements.required_pcrs != [0, 1, 2] {
        return Err(policy_error(
            "release requiredPcrs must be exactly [0, 1, 2]",
        ));
    }
    decode_pcr(
        "release.measurements.pcrs.0",
        &manifest.measurements.pcrs.pcr0,
    )?;
    decode_pcr(
        "release.measurements.pcrs.1",
        &manifest.measurements.pcrs.pcr1,
    )?;
    decode_pcr(
        "release.measurements.pcrs.2",
        &manifest.measurements.pcrs.pcr2,
    )?;

    if release.transparency_log.log_index.is_empty()
        || !release
            .transparency_log
            .log_index
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        || (release.transparency_log.log_index.len() > 1
            && release.transparency_log.log_index.starts_with('0'))
    {
        return Err(policy_error(
            "release transparency log index must be an unsigned decimal integer",
        ));
    }
    validate_hex(
        "release.transparencyLog.logId",
        &release.transparency_log.log_id,
        SHA256_HEX_LEN,
    )?;

    if manifest.build.system != "nix" {
        return Err(policy_error("release build system must be 'nix'"));
    }
    validate_hex(
        "release.manifest.build.flakeLockSha256",
        &manifest.build.flake_lock_sha256,
        SHA256_HEX_LEN,
    )?;
    let expected_derivation = format!("eif-{}", manifest.environment);
    if manifest.build.derivation != expected_derivation {
        return Err(policy_error(format!(
            "release build derivation '{}' does not match environment '{}'",
            manifest.build.derivation, manifest.environment
        )));
    }
    validate_workflow_run(&manifest.build.workflow_run, &policy.source_repository)?;
    Ok(())
}

fn validate_environment(environment: &str) -> Result<()> {
    if matches!(environment, "prod" | "dev") {
        Ok(())
    } else {
        Err(policy_error(format!(
            "unsupported attestation environment '{environment}'"
        )))
    }
}

fn validate_release_tag(tag: &str) -> Result<()> {
    let Some(version) = tag.strip_prefix('v') else {
        return Err(policy_error(format!(
            "release tag '{tag}' is not a stable vMAJOR.MINOR.PATCH tag"
        )));
    };
    let parts = version.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        || parts
            .iter()
            .any(|part| part.len() > 1 && part.starts_with('0'))
    {
        return Err(policy_error(format!(
            "release tag '{tag}' is not a stable vMAJOR.MINOR.PATCH tag"
        )));
    }
    Ok(())
}

fn validate_workflow_path(path: &str) -> Result<()> {
    validate_nonempty("policy.workflow.path", path)?;
    if path.starts_with('/')
        || path.contains('\\')
        || path.split('/').any(|component| component == "..")
        || !path.starts_with(".github/workflows/")
        || !(path.ends_with(".yml") || path.ends_with(".yaml"))
    {
        return Err(policy_error(format!(
            "invalid GitHub Actions workflow path '{path}'"
        )));
    }
    Ok(())
}

fn validate_artifact_name(name: &str) -> Result<()> {
    validate_nonempty("release.artifact.name", name)?;
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(policy_error(format!(
            "release artifact name '{name}' must be a file name"
        )));
    }
    Ok(())
}

fn validate_workflow_run(value: &str, repository: &str) -> Result<()> {
    let url = reqwest::Url::parse(value)
        .map_err(|error| policy_error(format!("invalid build workflowRun URL: {error}")))?;
    let expected_prefix = format!("/{repository}/actions/runs/");
    let run_id = url
        .path()
        .strip_prefix(&expected_prefix)
        .unwrap_or_default();
    let run_parts = run_id.split('/').collect::<Vec<_>>();
    let valid_run_path = matches!(
        run_parts.as_slice(),
        [run, "attempts", attempt]
            if is_positive_decimal(run) && is_positive_decimal(attempt)
    );
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !valid_run_path
    {
        return Err(policy_error(format!(
            "build workflowRun must be an exact GitHub Actions run-attempt URL for '{repository}'"
        )));
    }
    Ok(())
}

fn is_positive_decimal(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) && !value.starts_with('0')
}

fn validate_nonempty(field: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.trim() != value {
        Err(policy_error(format!("{field} must be a non-empty string")))
    } else {
        Ok(())
    }
}

fn validate_hex(field: &str, value: &str, expected_len: usize) -> Result<()> {
    if value.len() != expected_len
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(policy_error(format!(
            "{field} must be exactly {expected_len} lowercase hexadecimal characters"
        )));
    }
    Ok(())
}

fn decode_pcr(field: &str, value: &str) -> Result<[u8; SHA384_BYTES_LEN]> {
    validate_hex(field, value, SHA384_HEX_LEN)?;
    let bytes = hex::decode(value).map_err(|error| policy_error(format!("{field}: {error}")))?;
    if bytes.iter().all(|byte| *byte == 0) {
        return Err(policy_error(format!("{field} must not be all zeroes")));
    }
    bytes.try_into().map_err(|_| {
        policy_error(format!(
            "{field} must decode to exactly {SHA384_BYTES_LEN} bytes"
        ))
    })
}

fn attestation_pcr(document: &AttestationDocument, index: usize) -> Result<[u8; SHA384_BYTES_LEN]> {
    let value = document
        .pcrs
        .get(&index)
        .ok_or_else(|| Error::AttestationVerificationFailed(format!("PCR{index} missing")))?;
    value.as_slice().try_into().map_err(|_| {
        Error::AttestationVerificationFailed(format!(
            "PCR{index} must be exactly {SHA384_BYTES_LEN} bytes"
        ))
    })
}

fn policy_error(message: impl Into<String>) -> Error {
    Error::TrustedReleasePolicy(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn snapshot(releases: &str) -> String {
        let releases: serde_json::Value = serde_json::from_str(releases).unwrap();
        let mut value = serde_json::json!({
            "schema": SNAPSHOT_SCHEMA,
            "policy": {
                "oidcIssuer": EXPECTED_OIDC_ISSUER,
                "sourceRepository": EXPECTED_SOURCE_REPOSITORY,
                "sourceRepositoryId": EXPECTED_SOURCE_REPOSITORY_ID,
                "sourceRepositoryOwnerId": EXPECTED_SOURCE_REPOSITORY_OWNER_ID,
                "workflow": {
                    "path": EXPECTED_WORKFLOW_PATH,
                    "name": EXPECTED_WORKFLOW_NAME,
                    "trigger": EXPECTED_WORKFLOW_TRIGGER,
                    "environment": EXPECTED_WORKFLOW_ENVIRONMENT
                }
            },
            "releases": releases
        });
        let snapshot_id = hex::encode(Sha256::digest(canonical_json_bytes(&value).unwrap()));
        value.as_object_mut().unwrap().insert(
            "snapshotId".to_string(),
            serde_json::Value::String(snapshot_id),
        );
        String::from_utf8(canonical_json_bytes(&value).unwrap()).unwrap()
    }

    fn release(tag: &str, environment: &str, values: [u8; 3]) -> String {
        let mut value: serde_json::Value = serde_json::from_str(&format!(
            r#"{{
  "manifestSha256": "{sha256}",
  "bundleSha256": "{sha256}",
  "signer": {{
    "oidcIssuer": "{EXPECTED_OIDC_ISSUER}",
    "identity": "https://github.com/{EXPECTED_SOURCE_REPOSITORY}/{EXPECTED_WORKFLOW_PATH}@refs/tags/{tag}"
  }},
  "transparencyLog": {{
    "logIndex": "42",
    "logId": "{sha256}"
  }},
  "manifest": {{
    "schema": "{MANIFEST_SCHEMA}",
    "environment": "{environment}",
    "source": {{
      "repository": "{EXPECTED_SOURCE_REPOSITORY}",
      "repositoryId": {EXPECTED_SOURCE_REPOSITORY_ID},
      "ownerId": {EXPECTED_SOURCE_REPOSITORY_OWNER_ID},
      "ref": "refs/tags/{tag}",
      "commit": "{commit}"
    }},
    "release": {{ "tag": "{tag}" }},
    "artifact": {{
      "name": "opensecret-{tag}-{environment}.eif",
      "mediaType": "{EXPECTED_EIF_MEDIA_TYPE}",
      "sha256": "{sha256}",
      "size": 123
    }},
    "measurements": {{
      "algorithm": "sha384",
      "requiredPcrs": [0, 1, 2],
      "pcrs": {{
        "0": "{pcr0}",
        "1": "{pcr1}",
        "2": "{pcr2}"
      }}
    }},
    "build": {{
      "system": "nix",
      "flakeLockSha256": "{sha256}",
      "derivation": "eif-{environment}",
      "workflowRun": "https://github.com/{EXPECTED_SOURCE_REPOSITORY}/actions/runs/123456789/attempts/1"
    }}
  }}
}}"#,
            sha256 = "b".repeat(SHA256_HEX_LEN),
            commit = "c".repeat(40),
            pcr0 = hex::encode([values[0]; SHA384_BYTES_LEN]),
            pcr1 = hex::encode([values[1]; SHA384_BYTES_LEN]),
            pcr2 = hex::encode([values[2]; SHA384_BYTES_LEN]),
        ))
        .unwrap();
        let manifest_sha256 = hex::encode(Sha256::digest(
            canonical_json_bytes(&value["manifest"]).unwrap(),
        ));
        value["manifestSha256"] = serde_json::Value::String(manifest_sha256);
        String::from_utf8(canonical_json_bytes(&value).unwrap()).unwrap()
    }

    fn rehash_release(release: String) -> String {
        let mut value: serde_json::Value = serde_json::from_str(&release).unwrap();
        let manifest_sha256 = hex::encode(Sha256::digest(
            canonical_json_bytes(&value["manifest"]).unwrap(),
        ));
        value["manifestSha256"] = serde_json::Value::String(manifest_sha256);
        String::from_utf8(canonical_json_bytes(&value).unwrap()).unwrap()
    }

    fn document(values: [u8; 3]) -> AttestationDocument {
        AttestationDocument {
            module_id: "test".to_string(),
            timestamp: 0,
            digest: "SHA384".to_string(),
            pcrs: HashMap::from([
                (0, vec![values[0]; SHA384_BYTES_LEN]),
                (1, vec![values[1]; SHA384_BYTES_LEN]),
                (2, vec![values[2]; SHA384_BYTES_LEN]),
            ]),
            certificate: Vec::new(),
            cabundle: Vec::new(),
            public_key: None,
            user_data: None,
            nonce: None,
        }
    }

    #[test]
    fn accepts_complete_tuple_from_one_release() {
        let policy = TrustedReleasePolicy::from_snapshot_json(
            &snapshot(&format!("[{}]", release("v1.2.3", "prod", [1, 2, 3]))),
            "prod",
        )
        .unwrap();

        policy.verify_attestation(&document([1, 2, 3])).unwrap();
    }

    #[test]
    fn rejects_pcrs_mixed_across_releases() {
        let releases = format!(
            "[{},{}]",
            release("v1.2.3", "prod", [1, 2, 3]),
            release("v1.2.4", "prod", [4, 5, 6])
        );
        let policy =
            TrustedReleasePolicy::from_snapshot_json(&snapshot(&releases), "prod").unwrap();

        let error = policy.verify_attestation(&document([1, 5, 3])).unwrap_err();
        assert!(matches!(
            error,
            Error::AttestationVerificationFailed(message)
                if message.contains("PCR0/PCR1/PCR2 tuple")
        ));
    }

    #[test]
    fn binds_releases_to_selected_environment() {
        let releases = format!(
            "[{},{}]",
            release("v1.2.3", "prod", [1, 2, 3]),
            release("v1.2.3", "dev", [4, 5, 6])
        );
        let policy =
            TrustedReleasePolicy::from_snapshot_json(&snapshot(&releases), "prod").unwrap();

        let error = policy.verify_attestation(&document([4, 5, 6])).unwrap_err();
        assert!(matches!(error, Error::AttestationVerificationFailed(_)));
    }

    #[test]
    fn empty_environment_fails_with_unreleased_policy_error() {
        let policy = TrustedReleasePolicy::from_snapshot_json(&snapshot("[]"), "prod").unwrap();

        let error = policy.verify_attestation(&document([1, 2, 3])).unwrap_err();
        assert!(matches!(
            error,
            Error::UnreleasedAttestationPolicy { environment } if environment == "prod"
        ));
    }

    #[test]
    fn rejects_missing_or_wrong_length_required_pcr() {
        let policy = TrustedReleasePolicy::from_snapshot_json(
            &snapshot(&format!("[{}]", release("v1.2.3", "prod", [1, 2, 3]))),
            "prod",
        )
        .unwrap();
        let mut missing = document([1, 2, 3]);
        missing.pcrs.remove(&1);
        assert!(matches!(
            policy.verify_attestation(&missing),
            Err(Error::AttestationVerificationFailed(message)) if message == "PCR1 missing"
        ));

        let mut short = document([1, 2, 3]);
        short.pcrs.insert(2, vec![3; SHA384_BYTES_LEN - 1]);
        assert!(matches!(
            policy.verify_attestation(&short),
            Err(Error::AttestationVerificationFailed(message))
                if message.contains("PCR2 must be exactly")
        ));
    }

    #[test]
    fn rejects_unstable_tag_and_cross_record_ref() {
        let unstable = release("v1.2.3-rc.1", "prod", [1, 2, 3]);
        assert!(matches!(
            TrustedReleasePolicy::from_snapshot_json(&snapshot(&format!("[{unstable}]")), "prod"),
            Err(Error::TrustedReleasePolicy(message)) if message.contains("stable")
        ));

        let wrong_ref = rehash_release(
            release("v1.2.3", "prod", [1, 2, 3]).replace("refs/tags/v1.2.3", "refs/tags/v9.9.9"),
        );
        assert!(matches!(
            TrustedReleasePolicy::from_snapshot_json(&snapshot(&format!("[{wrong_ref}]")), "prod"),
            Err(Error::TrustedReleasePolicy(message)) if message.contains("does not match tag")
        ));
    }

    #[test]
    fn rejects_all_zero_release_measurement() {
        let zero_pcr = rehash_release(release("v1.2.3", "prod", [1, 2, 3]).replace(
            &hex::encode([1; SHA384_BYTES_LEN]),
            &hex::encode([0; SHA384_BYTES_LEN]),
        ));

        assert!(matches!(
            TrustedReleasePolicy::from_snapshot_json(&snapshot(&format!("[{zero_pcr}]")), "prod"),
            Err(Error::TrustedReleasePolicy(message)) if message.contains("must not be all zeroes")
        ));
    }

    #[test]
    fn accepts_exact_github_run_attempt_urls_only() {
        validate_workflow_run(
            "https://github.com/OpenSecretCloud/opensecret/actions/runs/123/attempts/2",
            EXPECTED_SOURCE_REPOSITORY,
        )
        .unwrap();
        for invalid in [
            "https://github.com/OpenSecretCloud/opensecret/actions/runs/123",
            "https://github.com/OpenSecretCloud/opensecret/actions/runs/123/jobs/2",
            "https://github.com/OpenSecretCloud/opensecret/actions/runs/123/attempts/0",
            "https://github.com/OpenSecretCloud/opensecret/actions/runs/123?attempt=2",
            "https://example.com/OpenSecretCloud/opensecret/actions/runs/123",
        ] {
            assert!(validate_workflow_run(invalid, EXPECTED_SOURCE_REPOSITORY).is_err());
        }
    }
}
