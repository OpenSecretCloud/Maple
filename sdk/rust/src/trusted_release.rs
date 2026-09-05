//! Dynamically refreshed trust policy for OpenSecret Nitro enclave releases.
//!
//! The SDK bootstraps from an embedded TUF root, then refreshes signed metadata
//! and targets from `attestations.trymaple.ai`. TUF selects the currently active
//! releases; each selected manifest is independently verified as a portable
//! Sigstore bundle. Only the complete PCR0/PCR1/PCR2 tuple from a fully verified
//! release can authorize an attestation.

use crate::{
    attestation::AttestationDocument,
    error::{Error, Result},
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use fs2::FileExt;
use futures::StreamExt;
use reqwest::{redirect::Policy as RedirectPolicy, Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sigstore_tuf::{
    transport::FetchFuture, MetadataStore, Repository, StoreRepository, Updater, UpdaterConfig,
};
use sigstore_verify::{
    trust_root::TrustedRoot as SigstoreTrustedRoot,
    types::{Bundle, HashAlgorithm, SignatureContent},
    VerificationPolicy, Verifier,
};
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashSet},
    fmt,
    fs::{File, OpenOptions},
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex, OnceLock},
    time::Duration,
};
use tokio::sync::{watch, Mutex};

const REPOSITORY_URL: &str = "https://attestations.trymaple.ai/tuf/";
const CHANNEL_SCHEMA: &str = "https://attestations.trymaple.ai/schemas/channel/v1";
const MANIFEST_SCHEMA: &str = "https://attestations.trymaple.ai/schemas/nitro-eif-release/v1";
const COMPONENT: &str = "opensecret-backend";
const EIF_MEDIA_TYPE: &str = "application/vnd.aws.nitro.eif";
const CACHE_SCHEMA: &str = "https://attestations.trymaple.ai/schemas/sdk-tuf-cache/v4";
const UNPUBLISHED_ROOT_SCHEMA: &str =
    "https://attestations.trymaple.ai/schemas/unpublished-tuf-root/v1";
const SHA256_HEX_LEN: usize = 64;
const SHA384_HEX_LEN: usize = 96;
const SHA384_BYTES_LEN: usize = 48;
const MAX_ACTIVE_RELEASES: usize = 2;
const MAX_ROOT_BYTES: u64 = 64 * 1024;
const MAX_TIMESTAMP_BYTES: u64 = 32 * 1024;
const MAX_SNAPSHOT_BYTES: u64 = 128 * 1024;
const MAX_TARGETS_METADATA_BYTES: u64 = 256 * 1024;
const MAX_CHANNEL_BYTES: usize = 128 * 1024;
const MAX_SIGSTORE_ROOT_BYTES: usize = 512 * 1024;
const MAX_MANIFEST_BYTES: usize = 128 * 1024;
const MAX_BUNDLE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 128;
const MAX_AUTHORITY_KEYS: usize = 128;
const MAX_ROOT_TRANSITIONS: u64 = 32;
const MAX_TIMESTAMP_VALIDITY_HOURS: i64 = 48;
const TUF_UNAVAILABLE_PREFIX: &str = "repository unavailable: ";
const TUF_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

const EMBEDDED_TUF_ROOT: &[u8] = include_bytes!("../assets/attestations_tuf_root.generated.json");

static PRODUCTION_MANAGER: OnceLock<Arc<TrustedReleaseManager>> = OnceLock::new();
static DEVELOPMENT_MANAGER: OnceLock<Arc<TrustedReleaseManager>> = OnceLock::new();
static REPOSITORY_MEMORY_STATES: OnceLock<StdMutex<BTreeMap<String, Arc<RepositoryMemoryState>>>> =
    OnceLock::new();

/// TUF channel selected for an enclave attestation.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AttestationEnvironment {
    /// Public production enclave channel.
    #[serde(rename = "prod")]
    Production,
    /// Explicit development enclave channel.
    #[serde(rename = "dev")]
    Development,
}

impl AttestationEnvironment {
    /// Wire value used in TUF target paths and signed documents.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Production => "prod",
            Self::Development => "dev",
        }
    }
}

impl fmt::Display for AttestationEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Explicit configuration for a TUF-backed release-policy source.
#[derive(Clone, Debug)]
pub struct TrustedReleaseConfig {
    environment: AttestationEnvironment,
    repository_url: String,
    tuf_root: Arc<[u8]>,
    cache_path: Option<PathBuf>,
}

impl TrustedReleaseConfig {
    /// Creates a policy source. The repository URL must be HTTPS and is
    /// expected to contain `metadata/` and `targets/` below it.
    pub fn new(
        environment: AttestationEnvironment,
        repository_url: impl Into<String>,
        tuf_root_json: impl Into<Vec<u8>>,
    ) -> Result<Self> {
        let repository_url = repository_url.into();
        let repository_url = validate_repository_base(&repository_url, false)?.to_string();
        let cache_path = Some(default_cache_path(&repository_url)?);
        Ok(Self {
            environment,
            repository_url,
            tuf_root: Arc::from(tuf_root_json.into()),
            cache_path,
        })
    }

    /// Creates a policy source with an application-owned durable state path.
    ///
    /// Mobile hosts should use this constructor because Rust cannot discover
    /// an Android or iOS application sandbox without a platform context. The
    /// path must live in durable application data, not an OS cache directory.
    pub fn new_with_cache_path(
        environment: AttestationEnvironment,
        repository_url: impl Into<String>,
        tuf_root_json: impl Into<Vec<u8>>,
        cache_path: impl Into<PathBuf>,
    ) -> Result<Self> {
        let repository_url = repository_url.into();
        let repository_url = validate_repository_base(&repository_url, false)?.to_string();
        Ok(Self {
            environment,
            repository_url,
            tuf_root: Arc::from(tuf_root_json.into()),
            cache_path: Some(cache_path.into()),
        })
    }

    /// Overrides the persistent cache file used for last-known-good metadata.
    pub fn with_cache_path(mut self, cache_path: impl Into<PathBuf>) -> Self {
        self.cache_path = Some(cache_path.into());
        self
    }

    /// Disables cross-process persistence for a custom API origin. The manager
    /// still refreshes and verifies before every attestation handshake.
    /// Official API origins reject managers without durable rollback state.
    pub fn without_persistent_cache(mut self) -> Self {
        self.cache_path = None;
        self
    }

    /// Selected release channel.
    pub const fn environment(&self) -> AttestationEnvironment {
        self.environment
    }
}

/// A fully verified, immutable set of active enclave PCR tuples.
#[derive(Clone, Debug)]
pub struct TrustedReleasePolicy {
    environment: AttestationEnvironment,
    sequence: u64,
    policy_id: String,
    repository_high_water: RepositoryHighWater,
    valid_until: jiff::Timestamp,
    releases: Vec<TrustedRelease>,
}

#[derive(Clone, Debug)]
struct TrustedRelease {
    version: String,
    pcr0: [u8; SHA384_BYTES_LEN],
    pcr1: [u8; SHA384_BYTES_LEN],
    pcr2: [u8; SHA384_BYTES_LEN],
}

#[cfg(test)]
type TestReleaseTuple<'a> = (&'a str, [u8; 48], [u8; 48], [u8; 48]);

impl TrustedReleasePolicy {
    /// The environment this policy is permitted to authorize.
    pub fn environment(&self) -> &str {
        self.environment.as_str()
    }

    /// Monotonic channel sequence authenticated by TUF.
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// SHA-256 of the exact authenticated channel bytes.
    pub fn policy_id(&self) -> &str {
        &self.policy_id
    }

    /// Verifies PCR0, PCR1, and PCR2 as one indivisible release tuple.
    pub fn verify_attestation(&self, document: &AttestationDocument) -> Result<()> {
        self.verify_attestation_at(document, jiff::Timestamp::now())
    }

    fn verify_attestation_at(
        &self,
        document: &AttestationDocument,
        now: jiff::Timestamp,
    ) -> Result<()> {
        if now >= self.valid_until {
            return Err(policy_error(
                "authenticated attestation policy metadata expired before PCR authorization",
            ));
        }
        if self.releases.is_empty() {
            return Err(Error::UnreleasedAttestationPolicy {
                environment: self.environment.to_string(),
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

        let versions = self
            .releases
            .iter()
            .map(|release| release.version.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Err(Error::AttestationVerificationFailed(format!(
            "PCR0/PCR1/PCR2 tuple is not active in authenticated channel {} for environment '{}' (active releases: {})",
            self.policy_id, self.environment, versions
        )))
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        environment: AttestationEnvironment,
        sequence: u64,
        releases: Vec<TestReleaseTuple<'_>>,
    ) -> Self {
        Self {
            environment,
            sequence,
            policy_id: "test-policy".to_string(),
            repository_high_water: RepositoryHighWater::for_test(),
            valid_until: jiff::Timestamp::MAX,
            releases: releases
                .into_iter()
                .map(|(version, pcr0, pcr1, pcr2)| TrustedRelease {
                    version: version.to_string(),
                    pcr0,
                    pcr1,
                    pcr2,
                })
                .collect(),
        }
    }
}

/// Single-flight dynamic release-policy manager shared by SDK clients.
pub struct TrustedReleaseManager {
    config: TrustedReleaseConfig,
    repository: HttpTufRepository,
    refresh_coordinator: Arc<Mutex<RefreshCoordinator>>,
    memory_state: Arc<RepositoryMemoryState>,
    #[cfg(test)]
    fixed_policy: Option<TrustedReleasePolicy>,
}

#[derive(Clone, Default)]
struct MemoryHighWater {
    repository: Option<RepositoryHighWater>,
    channels: BTreeMap<AttestationEnvironment, CacheHighWater>,
}

#[derive(Clone, Default)]
struct ProcessSecurityState {
    high_water: MemoryHighWater,
    root_history: BTreeMap<String, Vec<u8>>,
}

#[derive(Default)]
struct RepositoryMemoryState {
    state: StdMutex<ProcessSecurityState>,
}

#[derive(Default)]
struct RefreshCoordinator {
    next_id: u64,
    in_flight: Option<(u64, watch::Receiver<Option<SharedRefreshResult>>)>,
}

type SharedRefreshResult = std::result::Result<TrustedReleasePolicy, SharedRefreshError>;

#[derive(Clone, Debug)]
enum SharedRefreshError {
    Network(String),
    Policy(String),
    Unreleased(String),
    Other(String),
}

impl SharedRefreshError {
    fn from_error(error: &Error) -> Self {
        match error {
            Error::TrustedReleaseNetwork(message) => Self::Network(message.clone()),
            Error::TrustedReleasePolicy(message) => Self::Policy(message.clone()),
            Error::UnreleasedAttestationPolicy { environment } => {
                Self::Unreleased(environment.clone())
            }
            other => Self::Other(other.to_string()),
        }
    }

    fn into_error(self) -> Error {
        match self {
            Self::Network(message) => Error::TrustedReleaseNetwork(message),
            Self::Policy(message) => Error::TrustedReleasePolicy(message),
            Self::Unreleased(environment) => Error::UnreleasedAttestationPolicy { environment },
            Self::Other(message) => Error::Other(message),
        }
    }
}

impl fmt::Debug for TrustedReleaseManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TrustedReleaseManager")
            .field("environment", &self.config.environment)
            .field("repository_url", &self.config.repository_url)
            .field("cache_path", &self.config.cache_path)
            .finish_non_exhaustive()
    }
}

impl TrustedReleaseManager {
    /// Creates a manager for an explicit TUF repository and bootstrap root.
    pub fn new(config: TrustedReleaseConfig) -> Result<Self> {
        let repository = HttpTufRepository::new(&config.repository_url, false)?;
        let memory_state = repository_memory_state(&config);
        Ok(Self {
            config,
            repository,
            refresh_coordinator: Arc::new(Mutex::new(RefreshCoordinator::default())),
            memory_state,
            #[cfg(test)]
            fixed_policy: None,
        })
    }

    /// Returns the process-wide manager for the official production or
    /// development channel.
    pub fn official(environment: AttestationEnvironment) -> Result<Arc<Self>> {
        validate_official_embedded_root(EMBEDDED_TUF_ROOT)?;
        let slot = match environment {
            AttestationEnvironment::Production => &PRODUCTION_MANAGER,
            AttestationEnvironment::Development => &DEVELOPMENT_MANAGER,
        };
        if let Some(manager) = slot.get() {
            return Ok(Arc::clone(manager));
        }

        let config =
            TrustedReleaseConfig::new(environment, REPOSITORY_URL, EMBEDDED_TUF_ROOT.to_vec())?;
        let candidate = Arc::new(Self::new(config)?);
        Ok(Arc::clone(slot.get_or_init(|| candidate)))
    }

    /// Creates an official-repository manager with a host-owned durable state
    /// path while retaining the SDK's embedded TUF bootstrap root.
    ///
    /// This is intended for Android and iOS hosts, which must obtain their app
    /// data directory from the platform and pass a file path below it.
    pub fn official_with_cache_path(
        environment: AttestationEnvironment,
        cache_path: impl Into<PathBuf>,
    ) -> Result<Arc<Self>> {
        validate_official_embedded_root(EMBEDDED_TUF_ROOT)?;
        let config = TrustedReleaseConfig::new_with_cache_path(
            environment,
            REPOSITORY_URL,
            EMBEDDED_TUF_ROOT.to_vec(),
            cache_path,
        )?;
        Ok(Arc::new(Self::new(config)?))
    }

    /// Selected release channel.
    pub const fn environment(&self) -> AttestationEnvironment {
        self.config.environment
    }

    pub(crate) fn validate_official_trust_domain(
        &self,
        expected_environment: AttestationEnvironment,
    ) -> Result<()> {
        if self.config.environment != expected_environment {
            return Err(Error::Configuration(format!(
                "Attestation environment '{}' is not allowed for this official origin; expected '{}'",
                self.config.environment,
                expected_environment.as_str()
            )));
        }
        if self.config.repository_url != REPOSITORY_URL {
            return Err(Error::Configuration(
                "Official API origins require the SDK's canonical attestation repository"
                    .to_string(),
            ));
        }
        if self.config.tuf_root.as_ref() != EMBEDDED_TUF_ROOT {
            return Err(Error::Configuration(
                "Official API origins require the SDK's embedded TUF bootstrap root".to_string(),
            ));
        }
        if self.config.cache_path.is_none() {
            return Err(Error::Configuration(
                "Official API origins require persistent attestation rollback state".to_string(),
            ));
        }
        Ok(())
    }

    fn clone_for_refresh(&self) -> Self {
        Self {
            config: self.config.clone(),
            repository: self.repository.clone(),
            refresh_coordinator: Arc::clone(&self.refresh_coordinator),
            memory_state: Arc::clone(&self.memory_state),
            #[cfg(test)]
            fixed_policy: self.fixed_policy.clone(),
        }
    }

    /// Refreshes TUF and verifies every active Sigstore bundle. Network failure
    /// falls back only to a complete cached repository that still passes TUF's
    /// signature, rollback, hash, and expiration checks at the current time.
    pub async fn refresh_policy(&self) -> Result<TrustedReleasePolicy> {
        let worker = self.clone_for_refresh();
        coalesce_refresh(Arc::clone(&self.refresh_coordinator), move || async move {
            worker.refresh_policy_inner().await
        })
        .await
    }

    /// Confirms, without network I/O, that a previously refreshed policy has
    /// not been superseded by a newer repository or channel floor observed by
    /// this process or persisted by another process.
    pub(crate) async fn assert_policy_current(&self, policy: &TrustedReleasePolicy) -> Result<()> {
        if policy.environment != self.config.environment {
            return Err(policy_error(
                "held attestation policy belongs to a different environment",
            ));
        }
        let cache_guard = self.acquire_cache_lock().await?;
        let cached = self.load_cache().await?;
        let memory = self
            .memory_state
            .state
            .lock()
            .expect("repository memory-state mutex poisoned")
            .clone();
        let latest = merge_loaded_security_high_water_states(
            cached.repository_high_water.as_ref(),
            cached.channel_high_water,
            memory.high_water.repository.as_ref(),
            &memory.high_water.channels,
            &cached.entries,
            &memory.root_history,
        )?;
        enforce_repository_high_water(&policy.repository_high_water, latest.repository.as_ref())?;
        enforce_high_water(policy, latest.channels.get(&policy.environment))?;
        drop(cache_guard);
        Ok(())
    }

    async fn refresh_policy_inner(&self) -> Result<TrustedReleasePolicy> {
        self.refresh_policy_inner_with_verifier(&PortableBundleVerifier)
            .await
    }

    async fn refresh_policy_inner_with_verifier(
        &self,
        bundle_verifier: &dyn BundleVerifier,
    ) -> Result<TrustedReleasePolicy> {
        #[cfg(test)]
        if let Some(policy) = &self.fixed_policy {
            return Ok(policy.clone());
        }

        if is_unpublished_root(&self.config.tuf_root) {
            return Err(Error::UnreleasedAttestationPolicy {
                environment: self.config.environment.to_string(),
            });
        }

        // Hold an advisory process lock across read/refresh/write. Without it,
        // a slower process could atomically overwrite a newer verified TUF
        // generation with an older one after racing from the same cache.
        let mut cache_guard = self.acquire_cache_lock().await?;
        let cached = self.load_cache().await?;
        let cached_repository_high_water = cached.repository_high_water;
        let cached_channel_high_water = cached.channel_high_water;
        let online_store = Arc::new(SnapshotStore::from_entries(cached.entries));
        let now = jiff::Timestamp::now();
        let online_result = resolve_policy(
            self.repository.clone(),
            Arc::clone(&online_store),
            &self.config.tuf_root,
            self.config.environment,
            now,
            bundle_verifier,
        )
        .await;
        let online_result = match online_result {
            Ok(mut policy) => {
                let memory_high_water = self
                    .memory_state
                    .state
                    .lock()
                    .expect("repository memory-state mutex poisoned")
                    .clone();
                let advanced = align_security_high_water_states_to_observed(
                    cached_repository_high_water.as_ref(),
                    cached_channel_high_water.clone(),
                    memory_high_water.high_water.repository.as_ref(),
                    &memory_high_water.high_water.channels,
                    &policy.repository_high_water,
                    &online_store.entries(),
                );
                let advanced = match advanced {
                    Ok(advanced) => advanced,
                    Err(error) => return Err(error),
                };
                let channel_high_water = advanced.channels;
                let effective_prior_repository = advanced.repository;
                let validation = (|| -> Result<()> {
                    enforce_repository_high_water(
                        &policy.repository_high_water,
                        effective_prior_repository.as_ref(),
                    )?;
                    let prior_channel =
                        channel_high_water
                            .get(&self.config.environment)
                            .filter(|floor| {
                                !safely_replaces_authority(
                                    &floor.authority,
                                    &policy.repository_high_water.targets_authority,
                                )
                            });
                    enforce_high_water(&policy, prior_channel)
                })();
                match validation {
                    Ok(()) => {
                        let merged = merge_repository_observation(
                            effective_prior_repository.as_ref(),
                            &policy.repository_high_water,
                        );
                        if let Some(error) = merged.error {
                            Err(RefreshFailure::Security(error))
                        } else {
                            policy.repository_high_water = merged.high_water;
                            Ok((policy, channel_high_water))
                        }
                    }
                    Err(error) => Err(RefreshFailure::Security(error)),
                }
            }
            Err(error) => Err(error),
        };

        let result = match online_result {
            Ok((policy, mut channel_high_water)) => {
                let high_water = CacheHighWater::from_policy(&policy);
                let entries = online_store.entries();
                let root_history =
                    root_history_for_process_state(&entries, &policy.repository_high_water)?;
                retain_channel_floors_for_authority(
                    &mut channel_high_water,
                    &policy.repository_high_water.targets_authority,
                )?;
                if let Some(merged) = merge_high_water(
                    channel_high_water.get(&self.config.environment),
                    Some(&high_water),
                )? {
                    channel_high_water.insert(self.config.environment, merged);
                }
                // Install process-wide monotonic floors before the fallible
                // disk write. Production and development managers share this
                // state because they consume one TUF repository and cache.
                {
                    let mut memory = self
                        .memory_state
                        .state
                        .lock()
                        .expect("repository memory-state mutex poisoned");
                    // The state loaded above already merged this shared memory
                    // under the cross-process cache lock. Install the complete
                    // authenticated generation atomically before disk I/O, so
                    // a failed write cannot make this process regress.
                    memory.high_water.channels = channel_high_water.clone();
                    memory.high_water.repository = Some(policy.repository_high_water.clone());
                    memory.root_history = root_history;
                }
                if let Some(cache_path) = self.config.cache_path.clone() {
                    let repository_id = repository_id(&self.config.repository_url);
                    let repository_high_water = policy.repository_high_water.clone();
                    let persisted_channel_high_water = channel_high_water.clone();
                    persist_cache_while_locked(
                        &mut cache_guard,
                        cache_path,
                        repository_id,
                        repository_high_water,
                        persisted_channel_high_water,
                        entries,
                        "cache",
                    )
                    .await?;
                }
                Ok(policy)
            }
            Err(refresh_failure) => {
                // Even a refresh that cannot activate a policy may have
                // authenticated newer root or top-level metadata, or a newer
                // channel sequence. Journal those observations before any
                // fallback or error escapes so a restart cannot replay an
                // older, still-unexpired generation.
                let refresh_failure_message = refresh_failure.error_ref().to_string();
                let recorded = self
                    .record_authenticated_observation(
                        Arc::clone(&online_store),
                        now,
                        cached_repository_high_water.as_ref(),
                        cached_channel_high_water,
                        &mut cache_guard,
                    )
                    .await
                    .map_err(|journal_error| {
                        policy_error(format!(
                            "attestation policy refresh failed ({refresh_failure_message}); authenticated observation journal update failed: {journal_error}"
                        ))
                    })?;
                if let Some(observation_error) = recorded.observation_error {
                    return Err(observation_error);
                }
                let journal = recorded.cache;

                match refresh_failure {
                    RefreshFailure::Unavailable(online_error) => {
                        let offline_store = Arc::new(SnapshotStore::from_entries(journal.entries));
                        match resolve_policy(
                            StoreRepository::new(Arc::clone(&offline_store)),
                            offline_store,
                            &self.config.tuf_root,
                            self.config.environment,
                            now,
                            bundle_verifier,
                        )
                        .await
                        {
                            Ok(policy) => {
                                enforce_repository_high_water(
                                    &policy.repository_high_water,
                                    journal.repository_high_water.as_ref(),
                                )?;
                                let prior_channel = journal
                                    .channel_high_water
                                    .get(&self.config.environment)
                                    .filter(|floor| {
                                        !safely_replaces_authority(
                                            &floor.authority,
                                            &policy.repository_high_water.targets_authority,
                                        )
                                    });
                                enforce_high_water(&policy, prior_channel)?;
                                tracing::warn!(
                                    %online_error,
                                    environment = %self.config.environment,
                                    "using still-valid authenticated attestation policy cache"
                                );
                                Ok(policy)
                            }
                            Err(offline_error) => {
                                tracing::warn!(
                                    %online_error,
                                    cached_error = %offline_error.into_error(),
                                    "online TUF refresh failed and cached policy was unusable"
                                );
                                Err(online_error)
                            }
                        }
                    }
                    RefreshFailure::UnavailableAfterChannel(error)
                    | RefreshFailure::Security(error) => Err(error),
                }
            }
        };
        drop(cache_guard);
        result
    }

    async fn acquire_cache_lock(&self) -> Result<Option<File>> {
        let Some(path) = self.config.cache_path.clone() else {
            return Ok(None);
        };
        match tokio::task::spawn_blocking(move || lock_cache(&path)).await {
            Ok(Ok(file)) => Ok(Some(file)),
            Ok(Err(error)) => Err(policy_error(format!(
                "attestation policy cache locking failed: {error}"
            ))),
            Err(error) => Err(policy_error(format!(
                "attestation policy cache lock task failed: {error}"
            ))),
        }
    }

    async fn load_cache(&self) -> Result<CachedRepository> {
        let Some(path) = self.config.cache_path.clone() else {
            return Ok(CachedRepository::default());
        };
        let repository_id = repository_id(&self.config.repository_url);
        let result = tokio::task::spawn_blocking(move || read_cache(&path, &repository_id)).await;
        match result {
            Err(error) => Err(policy_error(format!(
                "attestation policy cache read task failed: {error}"
            ))),
            Ok(Ok(cached)) => {
                validate_cached_root_span(&self.config.tuf_root, &cached)?;
                Ok(cached)
            }
            Ok(Err(error)) => Err(policy_error(format!(
                "attestation policy cache read failed: {error}"
            ))),
        }
    }

    async fn record_authenticated_observation(
        &self,
        store: Arc<SnapshotStore>,
        now: jiff::Timestamp,
        cached_repository_high_water: Option<&RepositoryHighWater>,
        cached_channel_high_water: BTreeMap<AttestationEnvironment, CacheHighWater>,
        cache_guard: &mut Option<File>,
    ) -> Result<RecordedObservation> {
        let observation =
            capture_authenticated_observation(store, &self.config.tuf_root, now).await?;
        let root_history = root_history_for_process_state(
            &observation.entries,
            &observation.repository_high_water,
        )?;
        let (repository_high_water, channel_high_water, observation_error) = {
            let mut memory = self
                .memory_state
                .state
                .lock()
                .expect("repository memory-state mutex poisoned");
            // Merge the disk-derived and process-wide floors as one state. A
            // channel sequence is meaningful only in the targets-authority
            // epoch that authenticated it.
            let base = align_security_high_water_states_to_observed(
                cached_repository_high_water,
                cached_channel_high_water,
                memory.high_water.repository.as_ref(),
                &memory.high_water.channels,
                &observation.repository_high_water,
                &observation.entries,
            )?;
            let repository_merge = merge_repository_observation(
                base.repository.as_ref(),
                &observation.repository_high_water,
            );
            let mut channels = base.channels;
            retain_channel_floors_for_authority(
                &mut channels,
                &repository_merge.high_water.targets_authority,
            )?;
            let mut observation_error = observation.error;
            if let Some(error) = repository_merge.error {
                observation_error.get_or_insert(error);
            }
            if repository_merge.accepted_through_targets {
                for (environment, candidate) in observation.channel_high_water {
                    match enforce_channel_high_water(&candidate, channels.get(&environment)) {
                        Ok(()) => {
                            if let Some(merged) =
                                merge_high_water(channels.get(&environment), Some(&candidate))?
                            {
                                channels.insert(environment, merged);
                            }
                        }
                        Err(error) => {
                            // Never activate a lower/equivocated channel, but
                            // still journal independently safe repository/root
                            // advancement.
                            observation_error.get_or_insert(error);
                        }
                    }
                }
            }
            let repository_high_water = repository_merge.high_water;
            memory.high_water.repository = Some(repository_high_water.clone());
            memory.high_water.channels = channels.clone();
            memory.root_history = root_history;
            (repository_high_water, channels, observation_error)
        };

        if let Some(cache_path) = self.config.cache_path.clone() {
            let repository_id = repository_id(&self.config.repository_url);
            let persisted_repository_high_water = repository_high_water.clone();
            let persisted_channel_high_water = channel_high_water.clone();
            let entries = observation.entries.clone();
            persist_cache_while_locked(
                cache_guard,
                cache_path,
                repository_id,
                persisted_repository_high_water,
                persisted_channel_high_water,
                entries,
                "journal cache",
            )
            .await?;
        }

        Ok(RecordedObservation {
            cache: CachedRepository {
                repository_high_water: Some(repository_high_water),
                channel_high_water,
                entries: observation.entries,
            },
            observation_error,
        })
    }

    #[cfg(test)]
    pub(crate) fn fixed_for_test(policy: TrustedReleasePolicy) -> Arc<Self> {
        let config = TrustedReleaseConfig {
            environment: policy.environment,
            repository_url: "https://attestations.invalid/tuf/".to_string(),
            tuf_root: Arc::from(EMBEDDED_TUF_ROOT),
            cache_path: None,
        };
        Arc::new(Self {
            repository: HttpTufRepository::new(&config.repository_url, false).unwrap(),
            config,
            refresh_coordinator: Arc::new(Mutex::new(RefreshCoordinator::default())),
            memory_state: Arc::new(RepositoryMemoryState::default()),
            fixed_policy: Some(policy),
        })
    }

    #[cfg(test)]
    pub(crate) fn install_policy_floor_for_test(&self, policy: &TrustedReleasePolicy) {
        let mut memory = self
            .memory_state
            .state
            .lock()
            .expect("repository memory-state mutex poisoned");
        memory.high_water.repository = Some(policy.repository_high_water.clone());
        memory
            .high_water
            .channels
            .insert(policy.environment, CacheHighWater::from_policy(policy));
    }
}

async fn coalesce_refresh<F, Fut>(
    coordinator: Arc<Mutex<RefreshCoordinator>>,
    operation: F,
) -> Result<TrustedReleasePolicy>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<TrustedReleasePolicy>> + Send + 'static,
{
    let (id, mut receiver, sender) = {
        let mut state = coordinator.lock().await;
        if let Some((id, receiver)) = &state.in_flight {
            (*id, receiver.clone(), None)
        } else {
            state.next_id = state.next_id.wrapping_add(1);
            let id = state.next_id;
            let (sender, receiver) = watch::channel(None);
            state.in_flight = Some((id, receiver.clone()));
            (id, receiver, Some(sender))
        }
    };

    if let Some(sender) = sender {
        let worker_coordinator = Arc::clone(&coordinator);
        tokio::spawn(async move {
            let result = operation().await;
            let shared = match result {
                Ok(policy) => Ok(policy),
                Err(error) => Err(SharedRefreshError::from_error(&error)),
            };
            let _ = sender.send(Some(shared));
            let mut state = worker_coordinator.lock().await;
            if state
                .in_flight
                .as_ref()
                .is_some_and(|(active, _)| *active == id)
            {
                state.in_flight = None;
            }
        });
    }

    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result.map_err(SharedRefreshError::into_error);
        }
        if receiver.changed().await.is_err() {
            let mut state = coordinator.lock().await;
            if state
                .in_flight
                .as_ref()
                .is_some_and(|(active, _)| *active == id)
            {
                state.in_flight = None;
            }
            return Err(policy_error(
                "attestation policy refresh worker ended without a result",
            ));
        }
    }
}

#[derive(Clone, Debug)]
struct HttpTufRepository {
    metadata_base: Url,
    targets_base: Url,
    client: Client,
}

impl HttpTufRepository {
    fn new(repository_url: &str, allow_test_loopback_http: bool) -> Result<Self> {
        Self::new_with_timeout(
            repository_url,
            allow_test_loopback_http,
            TUF_REQUEST_TIMEOUT,
        )
    }

    fn new_with_timeout(
        repository_url: &str,
        allow_test_loopback_http: bool,
        request_timeout: Duration,
    ) -> Result<Self> {
        let base = validate_repository_base(repository_url, allow_test_loopback_http)?;
        let metadata_base = base
            .join("metadata/")
            .map_err(|error| policy_error(format!("invalid TUF metadata URL: {error}")))?;
        let targets_base = base
            .join("targets/")
            .map_err(|error| policy_error(format!("invalid TUF targets URL: {error}")))?;
        let client = Client::builder()
            .redirect(RedirectPolicy::none())
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            // Reqwest's request deadline spans connection, response headers,
            // and the complete streamed body. The read timeout alone only
            // rejects an idle stream and would permit an indefinite slow drip.
            .timeout(request_timeout)
            .user_agent(concat!("opensecret-rust-sdk/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| policy_error(format!("could not build TUF HTTP client: {error}")))?;
        Ok(Self {
            metadata_base,
            targets_base,
            client,
        })
    }

    async fn bounded_get(
        &self,
        url: Url,
        max_length: u64,
    ) -> sigstore_tuf::Result<Option<Vec<u8>>> {
        let response = self.client.get(url.clone()).send().await.map_err(|error| {
            sigstore_tuf::Error::Transport(format!(
                "{TUF_UNAVAILABLE_PREFIX}GET {url} failed: {error}"
            ))
        })?;
        // TUF's missing-version sentinel is exact: only a 404 proves that the
        // next root does not exist. Treating 403 as absence would let a mirror
        // hide a published root rotation behind an authorization failure.
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            let status = response.status();
            let unavailable = status.is_server_error()
                || matches!(
                    status,
                    StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS
                );
            let prefix = if unavailable {
                TUF_UNAVAILABLE_PREFIX
            } else {
                ""
            };
            return Err(sigstore_tuf::Error::Transport(format!(
                "{prefix}GET {url} returned status {status}"
            )));
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_length)
        {
            return Err(sigstore_tuf::Error::Transport(format!(
                "GET {url} exceeds maximum response length {max_length}"
            )));
        }

        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                sigstore_tuf::Error::Transport(format!(
                    "{TUF_UNAVAILABLE_PREFIX}reading {url} failed: {error}"
                ))
            })?;
            if body.len() as u64 + chunk.len() as u64 > max_length {
                return Err(sigstore_tuf::Error::Transport(format!(
                    "GET {url} exceeds maximum response length {max_length}"
                )));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Some(body))
    }

    fn url(base: &Url, relative: &str) -> sigstore_tuf::Result<Url> {
        validate_repository_path(relative).map_err(|error| {
            sigstore_tuf::Error::Transport(format!("invalid repository path: {error}"))
        })?;
        base.join(relative)
            .map_err(|error| sigstore_tuf::Error::Transport(format!("invalid URL: {error}")))
    }
}

impl Repository for HttpTufRepository {
    fn fetch_metadata<'a>(&'a self, name: &'a str, max_length: u64) -> FetchFuture<'a> {
        Box::pin(async move {
            let url = Self::url(&self.metadata_base, name)?;
            self.bounded_get(url, max_length).await
        })
    }

    fn fetch_target<'a>(&'a self, path: &'a str, max_length: u64) -> FetchFuture<'a> {
        Box::pin(async move {
            let url = Self::url(&self.targets_base, path)?;
            self.bounded_get(url, max_length).await
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum RootCeilingProbeState {
    #[default]
    NotObserved,
    ConfirmedAbsent,
    Present,
    Failed,
}

#[derive(Clone)]
struct RootCeilingRepository<R> {
    inner: R,
    sentinel_name: Option<String>,
    probe_state: Arc<StdMutex<RootCeilingProbeState>>,
}

impl<R> RootCeilingRepository<R> {
    fn new(inner: R, bootstrap_version: u64) -> Self {
        let sentinel_name = bootstrap_version
            .saturating_add(MAX_ROOT_TRANSITIONS)
            .checked_add(1)
            .map(|version| format!("{version}.root.json"));
        Self {
            inner,
            sentinel_name,
            probe_state: Arc::new(StdMutex::new(RootCeilingProbeState::NotObserved)),
        }
    }

    fn probe_state(&self) -> Arc<StdMutex<RootCeilingProbeState>> {
        Arc::clone(&self.probe_state)
    }
}

impl<R> Repository for RootCeilingRepository<R>
where
    R: Repository + 'static,
{
    fn fetch_metadata<'a>(&'a self, name: &'a str, max_length: u64) -> FetchFuture<'a> {
        let is_ceiling_sentinel = self
            .sentinel_name
            .as_deref()
            .is_some_and(|sentinel| sentinel == name);
        let probe_state = Arc::clone(&self.probe_state);
        Box::pin(async move {
            let result = self.inner.fetch_metadata(name, max_length).await;
            if is_ceiling_sentinel {
                let state = match &result {
                    Ok(None) => RootCeilingProbeState::ConfirmedAbsent,
                    Ok(Some(_)) => RootCeilingProbeState::Present,
                    Err(_) => RootCeilingProbeState::Failed,
                };
                *probe_state
                    .lock()
                    .expect("root-ceiling probe mutex poisoned") = state;
            }
            result
        })
    }

    fn fetch_target<'a>(&'a self, path: &'a str, max_length: u64) -> FetchFuture<'a> {
        self.inner.fetch_target(path, max_length)
    }
}

#[derive(Debug, Default)]
struct SnapshotStore {
    entries: StdMutex<BTreeMap<String, Vec<u8>>>,
}

impl SnapshotStore {
    fn from_entries(entries: BTreeMap<String, Vec<u8>>) -> Self {
        Self {
            entries: StdMutex::new(entries),
        }
    }

    fn entries(&self) -> BTreeMap<String, Vec<u8>> {
        self.entries.lock().expect("cache mutex poisoned").clone()
    }

    fn replace_entries(&self, entries: BTreeMap<String, Vec<u8>>) {
        *self.entries.lock().expect("cache mutex poisoned") = entries;
    }
}

impl MetadataStore for SnapshotStore {
    fn load(&self, name: &str) -> Option<Vec<u8>> {
        self.entries
            .lock()
            .expect("cache mutex poisoned")
            .get(name)
            .cloned()
    }

    fn store(&self, name: &str, bytes: &[u8]) -> sigstore_tuf::Result<()> {
        validate_store_name(name)?;
        self.entries
            .lock()
            .expect("cache mutex poisoned")
            .insert(name.to_string(), bytes.to_vec());
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CacheFile {
    schema: String,
    repository_id: String,
    repository_high_water: RepositoryHighWater,
    channel_high_water: BTreeMap<AttestationEnvironment, CacheHighWater>,
    entries: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetadataHighWater {
    version: u64,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    authority: Option<AuthorityProvenance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    referenced_authority: Option<AuthorityProvenance>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RepositoryHighWater {
    root: MetadataHighWater,
    root_authority: RoleAuthority,
    timestamp_authority: RoleAuthority,
    snapshot_authority: RoleAuthority,
    targets_authority: RoleAuthority,
    authority_history: AuthorityHistory,
    timestamp: Option<MetadataHighWater>,
    snapshot_descriptor: Option<MetadataHighWater>,
    snapshot: Option<MetadataHighWater>,
    targets_descriptor: Option<MetadataHighWater>,
    targets: Option<MetadataHighWater>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RoleAuthority {
    threshold: usize,
    key_fingerprints: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthorityProvenance {
    key_fingerprints: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthorityHistory {
    root: Vec<String>,
    timestamp: Vec<String>,
    snapshot: Vec<String>,
    targets: Vec<String>,
}

impl AuthorityHistory {
    fn from_authorities(
        root: &RoleAuthority,
        timestamp: &RoleAuthority,
        snapshot: &RoleAuthority,
        targets: &RoleAuthority,
    ) -> Self {
        Self {
            root: root.key_fingerprints.clone(),
            timestamp: timestamp.key_fingerprints.clone(),
            snapshot: snapshot.key_fingerprints.clone(),
            targets: targets.key_fingerprints.clone(),
        }
    }
}

impl From<&RoleAuthority> for AuthorityProvenance {
    fn from(authority: &RoleAuthority) -> Self {
        Self {
            key_fingerprints: authority.key_fingerprints.clone(),
        }
    }
}

#[cfg(test)]
impl RepositoryHighWater {
    fn for_test() -> Self {
        let mark = MetadataHighWater {
            version: 1,
            sha256: "a".repeat(SHA256_HEX_LEN),
            authority: None,
            referenced_authority: None,
        };
        let role_mark = |authority: &RoleAuthority| MetadataHighWater {
            authority: Some(authority.into()),
            ..mark.clone()
        };
        let descriptor_mark =
            |authority: &RoleAuthority, referenced_authority: &RoleAuthority| MetadataHighWater {
                authority: Some(authority.into()),
                referenced_authority: Some(referenced_authority.into()),
                ..mark.clone()
            };
        let timestamp_authority = RoleAuthority::for_test('b');
        let snapshot_authority = RoleAuthority::for_test('c');
        let targets_authority = RoleAuthority::for_test('d');
        let root_authority = RoleAuthority::for_test('e');
        Self {
            root: mark.clone(),
            root_authority: root_authority.clone(),
            timestamp_authority: timestamp_authority.clone(),
            snapshot_authority: snapshot_authority.clone(),
            targets_authority: targets_authority.clone(),
            authority_history: AuthorityHistory::from_authorities(
                &root_authority,
                &timestamp_authority,
                &snapshot_authority,
                &targets_authority,
            ),
            timestamp: Some(role_mark(&timestamp_authority)),
            snapshot_descriptor: Some(descriptor_mark(&timestamp_authority, &snapshot_authority)),
            snapshot: Some(role_mark(&snapshot_authority)),
            targets_descriptor: Some(descriptor_mark(&snapshot_authority, &targets_authority)),
            targets: Some(role_mark(&targets_authority)),
        }
    }
}

#[cfg(test)]
impl RoleAuthority {
    fn for_test(byte: char) -> Self {
        Self {
            threshold: 1,
            key_fingerprints: vec![byte.to_string().repeat(SHA256_HEX_LEN)],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CacheHighWater {
    sequence: u64,
    policy_id: String,
    authority: AuthorityProvenance,
}

impl CacheHighWater {
    fn from_policy(policy: &TrustedReleasePolicy) -> Self {
        Self {
            sequence: policy.sequence,
            policy_id: policy.policy_id.clone(),
            authority: (&policy.repository_high_water.targets_authority).into(),
        }
    }
}

#[cfg(test)]
impl CacheHighWater {
    fn for_test(sequence: u64, policy_byte: char, authority_byte: char) -> Self {
        Self {
            sequence,
            policy_id: policy_byte.to_string().repeat(SHA256_HEX_LEN),
            authority: (&RoleAuthority::for_test(authority_byte)).into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CachedRepository {
    repository_high_water: Option<RepositoryHighWater>,
    channel_high_water: BTreeMap<AttestationEnvironment, CacheHighWater>,
    entries: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug)]
struct RecordedObservation {
    cache: CachedRepository,
    observation_error: Option<Error>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Channel {
    schema: String,
    environment: AttestationEnvironment,
    sequence: u64,
    sigstore_trusted_root_target: TargetReference,
    active: Vec<ActiveRelease>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TargetReference {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActiveRelease {
    manifest_target: String,
    manifest_sha256: String,
    bundle_target: String,
    bundle_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleaseManifest {
    schema: String,
    component: String,
    environment: AttestationEnvironment,
    release: ManifestRelease,
    source: ManifestSource,
    artifact: ManifestArtifact,
    measurements: ManifestMeasurements,
    build: ManifestBuild,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestRelease {
    version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestSource {
    uri: String,
    path: String,
    r#ref: String,
    revision: ManifestRevision,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestRevision {
    algorithm: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestArtifact {
    name: String,
    media_type: String,
    size: u64,
    digests: ManifestDigests,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestDigests {
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestMeasurements {
    algorithm: String,
    required_pcrs: [u8; 3],
    pcrs: ManifestPcrs,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestPcrs {
    #[serde(rename = "0")]
    pcr0: String,
    #[serde(rename = "1")]
    pcr1: String,
    #[serde(rename = "2")]
    pcr2: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestBuild {
    system: String,
    builder_id: String,
    derivation: String,
    flake_lock_sha256: String,
    run_uri: String,
}

trait BundleVerifier: Send + Sync {
    fn verify(
        &self,
        manifest_bytes: &[u8],
        bundle_bytes: &[u8],
        trusted_root_bytes: &[u8],
    ) -> Result<()>;
}

#[derive(Debug)]
enum RefreshFailure {
    Unavailable(Error),
    UnavailableAfterChannel(Error),
    Security(Error),
}

impl RefreshFailure {
    fn error_ref(&self) -> &Error {
        match self {
            Self::Unavailable(error)
            | Self::UnavailableAfterChannel(error)
            | Self::Security(error) => error,
        }
    }

    fn into_error(self) -> Error {
        match self {
            Self::Unavailable(error)
            | Self::UnavailableAfterChannel(error)
            | Self::Security(error) => error,
        }
    }
}

impl From<Error> for RefreshFailure {
    fn from(error: Error) -> Self {
        Self::Security(error)
    }
}

fn classify_tuf_error(context: &str, error: sigstore_tuf::Error) -> RefreshFailure {
    let unavailable = matches!(
        &error,
        sigstore_tuf::Error::Transport(message)
            if message.starts_with(TUF_UNAVAILABLE_PREFIX) || message.ends_with("not found")
    );
    if unavailable {
        RefreshFailure::Unavailable(Error::TrustedReleaseNetwork(format!("{context}: {error}")))
    } else {
        RefreshFailure::Security(policy_error(format!("{context}: {error}")))
    }
}

fn prevent_fallback_after_channel(error: RefreshFailure) -> RefreshFailure {
    match error {
        RefreshFailure::Unavailable(error) => RefreshFailure::UnavailableAfterChannel(error),
        other => other,
    }
}

struct PortableBundleVerifier;

fn parse_portable_bundle(bundle_json: &str) -> Result<Bundle> {
    let mut value: Value = serde_json::from_str(bundle_json)
        .map_err(|error| policy_error(format!("invalid Sigstore bundle: {error}")))?;
    if value
        .pointer("/messageSignature/messageDigest/algorithm")
        .is_some_and(|algorithm| algorithm.as_str() != Some("SHA2_256"))
    {
        return Err(policy_error(
            "Sigstore messageDigest algorithm must be exactly SHA2_256",
        ));
    }
    let entries = value
        .pointer_mut("/verificationMaterial/tlogEntries")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            policy_error("Sigstore bundle v0.3 must contain exactly one transparency-log entry")
        })?;
    if entries.len() != 1 {
        return Err(policy_error(
            "Sigstore bundle v0.3 must contain exactly one transparency-log entry",
        ));
    }
    // ProtoJSON parsers accept an int64 encoded as either a decimal string or
    // JSON number, and `null` is equivalent to an unset scalar. Canonical Rekor
    // v2 output omits its zero-valued integratedTime, while the pinned Sigstore
    // type rejects the two equivalent explicit forms. Normalize only v2 null
    // and numeric zero before parsing. TUF has already authenticated the exact
    // original bundle bytes.
    let entry = entries[0]
        .as_object_mut()
        .ok_or_else(|| policy_error("Sigstore transparency-log entry must be a JSON object"))?;
    let kind_version = entry.get("kindVersion");
    let is_hashedrekord_v2 = kind_version
        .and_then(|value| value.get("kind"))
        .and_then(Value::as_str)
        == Some("hashedrekord")
        && kind_version
            .and_then(|value| value.get("version"))
            .and_then(Value::as_str)
            == Some("0.0.2");
    let normalize_integrated_time = match entry.get("integratedTime") {
        Some(Value::Null) if is_hashedrekord_v2 => true,
        Some(Value::Number(number)) if is_hashedrekord_v2 => {
            if number.as_i64() != Some(0) {
                return Err(policy_error(
                    "Rekor v2 transparency-log entry integratedTime must be absent, null, or zero",
                ));
            }
            true
        }
        _ => false,
    };
    let normalized = if normalize_integrated_time {
        entry.remove("integratedTime");
        Cow::Owned(
            serde_json::to_string(&value)
                .map_err(|error| policy_error(format!("invalid Sigstore bundle: {error}")))?,
        )
    } else {
        Cow::Borrowed(bundle_json)
    };

    Bundle::from_json(&normalized)
        .map_err(|error| policy_error(format!("invalid Sigstore bundle: {error}")))
}

fn validate_portable_bundle_profile(bundle: &Bundle) -> Result<()> {
    if bundle.media_type != "application/vnd.dev.sigstore.bundle.v0.3+json" {
        return Err(policy_error(
            "Sigstore bundle mediaType must be application/vnd.dev.sigstore.bundle.v0.3+json",
        ));
    }
    let SignatureContent::MessageSignature(signature) = &bundle.content else {
        return Err(policy_error(
            "Sigstore bundle must contain a messageSignature",
        ));
    };
    let digest = signature
        .message_digest
        .as_ref()
        .ok_or_else(|| policy_error("Sigstore messageSignature must contain a messageDigest"))?;
    if digest.algorithm != HashAlgorithm::Sha2256 {
        return Err(policy_error(
            "Sigstore messageDigest algorithm must be exactly SHA2_256",
        ));
    }
    if digest.digest.as_bytes().len() != 32 {
        return Err(policy_error(
            "Sigstore SHA2_256 messageDigest must contain exactly 32 bytes",
        ));
    }
    let [entry] = bundle.verification_material.tlog_entries.as_slice() else {
        return Err(policy_error(
            "Sigstore bundle v0.3 must contain exactly one transparency-log entry",
        ));
    };
    if entry.kind_version.kind != "hashedrekord" {
        return Err(policy_error(
            "Sigstore bundle transparency-log entry must be hashedrekord",
        ));
    }
    match entry.kind_version.version.as_str() {
        "0.0.1" => {
            if entry.integrated_time <= 0 {
                return Err(policy_error(
                    "Rekor v1 transparency-log entry integratedTime must be positive",
                ));
            }
            if entry.inclusion_promise.is_none() {
                return Err(policy_error(
                    "Rekor v1 transparency-log entry must contain an inclusion promise",
                ));
            }
        }
        "0.0.2" => {
            if entry.integrated_time != 0 {
                return Err(policy_error(
                    "Rekor v2 transparency-log entry integratedTime must be absent, null, or zero",
                ));
            }
        }
        version => {
            return Err(policy_error(format!(
                "unsupported hashedrekord version '{version}' in Sigstore bundle"
            )));
        }
    }
    if bundle
        .verification_material
        .timestamp_verification_data
        .rfc3161_timestamps
        .is_empty()
    {
        return Err(policy_error(
            "Sigstore bundle must contain an RFC3161 timestamp",
        ));
    }
    if entry
        .inclusion_proof
        .as_ref()
        .is_none_or(|proof| proof.checkpoint.is_empty())
    {
        return Err(policy_error(
            "Sigstore transparency-log entry must contain a Merkle inclusion proof and signed checkpoint",
        ));
    }
    Ok(())
}

impl BundleVerifier for PortableBundleVerifier {
    fn verify(
        &self,
        manifest_bytes: &[u8],
        bundle_bytes: &[u8],
        trusted_root_bytes: &[u8],
    ) -> Result<()> {
        let trusted_root_json = std::str::from_utf8(trusted_root_bytes).map_err(|error| {
            policy_error(format!("Sigstore trusted root is not UTF-8: {error}"))
        })?;
        let trusted_root = SigstoreTrustedRoot::from_json(trusted_root_json)
            .map_err(|error| policy_error(format!("invalid Sigstore trusted root: {error}")))?;
        let bundle_json = std::str::from_utf8(bundle_bytes)
            .map_err(|error| policy_error(format!("Sigstore bundle is not UTF-8: {error}")))?;
        let bundle = parse_portable_bundle(bundle_json)?;
        validate_portable_bundle_profile(&bundle)?;
        // TUF has already authorized the exact manifest, bundle, and trusted
        // root bytes. Sigstore therefore supplies cryptographic provenance and
        // transparency, while builder admission remains a promotion-time
        // policy. Deliberately leave identity and issuer unconstrained here so
        // repository, workflow, and CI-provider migrations do not brick
        // already released clients.
        Verifier::new(&trusted_root)
            .verify(manifest_bytes, &bundle, &VerificationPolicy::default())
            .map_err(|error| policy_error(format!("Sigstore verification failed: {error}")))?;
        Ok(())
    }
}

async fn resolve_policy<R: Repository + 'static>(
    repository: R,
    store: Arc<SnapshotStore>,
    root_bytes: &[u8],
    environment: AttestationEnvironment,
    now: jiff::Timestamp,
    bundle_verifier: &dyn BundleVerifier,
) -> std::result::Result<TrustedReleasePolicy, RefreshFailure> {
    resolve_policy_with_final_time(
        repository,
        store,
        root_bytes,
        environment,
        now,
        bundle_verifier,
        jiff::Timestamp::now,
    )
    .await
}

async fn resolve_policy_with_final_time<R, F>(
    repository: R,
    store: Arc<SnapshotStore>,
    root_bytes: &[u8],
    environment: AttestationEnvironment,
    now: jiff::Timestamp,
    bundle_verifier: &dyn BundleVerifier,
    final_now: F,
) -> std::result::Result<TrustedReleasePolicy, RefreshFailure>
where
    R: Repository + 'static,
    F: FnOnce() -> jiff::Timestamp,
{
    let config = tuf_updater_config();
    let store_handle = Arc::clone(&store);
    let bootstrap = sigstore_tuf::TrustedMetadataSet::from_root(root_bytes).map_err(|error| {
        RefreshFailure::Security(policy_error(format!("invalid embedded TUF root: {error}")))
    })?;
    let repository = RootCeilingRepository::new(repository, bootstrap.root().version);
    let root_ceiling_probe = repository.probe_state();
    let mut updater = Updater::new(repository, root_bytes)
        .map_err(|error| {
            RefreshFailure::Security(policy_error(format!("invalid embedded TUF root: {error}")))
        })?
        .with_config(config)
        .with_store(store);
    if let Err(error) = updater.refresh(now).await {
        let failure = classify_tuf_error("TUF metadata refresh failed", error);
        let ceiling_probe_failed = *root_ceiling_probe
            .lock()
            .expect("root-ceiling probe mutex poisoned")
            == RootCeilingProbeState::Failed;
        return Err(if ceiling_probe_failed {
            prevent_fallback_after_channel(failure)
        } else {
            failure
        });
    }
    enforce_timestamp_window(&updater, now)?;

    let channel_path = format!("channels/{}.json", environment.as_str());
    let channel_bytes = get_target(&mut updater, &channel_path, now)
        .await
        .map_err(prevent_fallback_after_channel)?;
    enforce_target_size("channel", &channel_bytes, MAX_CHANNEL_BYTES)?;
    let channel: Channel = parse_json("channel", &channel_bytes)?;
    validate_channel(&channel, environment)?;
    let policy_id = sha256_hex(&channel_bytes);

    if channel.active.is_empty() {
        let valid_until = authorizing_metadata_valid_until(&updater, final_now())?;
        let repository_high_water =
            repository_high_water_from_updater(&updater, &store_handle, root_bytes)?;
        prune_repository_cache(&updater, &store_handle, environment, &channel)?;
        return Ok(TrustedReleasePolicy {
            environment,
            sequence: channel.sequence,
            policy_id,
            repository_high_water,
            valid_until,
            releases: Vec::new(),
        });
    }

    let sigstore_root_bytes = get_bound_target(
        &mut updater,
        &channel.sigstore_trusted_root_target.path,
        &channel.sigstore_trusted_root_target.sha256,
        MAX_SIGSTORE_ROOT_BYTES,
        now,
    )
    .await
    .map_err(prevent_fallback_after_channel)?;

    let mut releases = Vec::with_capacity(channel.active.len());
    let mut versions = HashSet::new();
    let mut tuples = HashSet::new();
    for active in &channel.active {
        let version = validate_release_targets(active, environment)?;
        if !versions.insert(version.clone()) {
            return Err(policy_error(format!(
                "channel contains duplicate active release '{version}'"
            ))
            .into());
        }

        let manifest_bytes = get_bound_target(
            &mut updater,
            &active.manifest_target,
            &active.manifest_sha256,
            MAX_MANIFEST_BYTES,
            now,
        )
        .await
        .map_err(prevent_fallback_after_channel)?;
        let bundle_bytes = get_bound_target(
            &mut updater,
            &active.bundle_target,
            &active.bundle_sha256,
            MAX_BUNDLE_BYTES as usize,
            now,
        )
        .await
        .map_err(prevent_fallback_after_channel)?;

        // Parse for the release/PCR contract, but verify the Sigstore
        // signature over the exact raw bytes rather than a reserialization.
        let manifest: ReleaseManifest = parse_json("release manifest", &manifest_bytes)?;
        validate_manifest(&manifest, &version, environment)?;
        bundle_verifier.verify(&manifest_bytes, &bundle_bytes, &sigstore_root_bytes)?;

        let pcr0 = decode_pcr("measurements.pcrs.0", &manifest.measurements.pcrs.pcr0)?;
        let pcr1 = decode_pcr("measurements.pcrs.1", &manifest.measurements.pcrs.pcr1)?;
        let pcr2 = decode_pcr("measurements.pcrs.2", &manifest.measurements.pcrs.pcr2)?;
        if !tuples.insert((pcr0, pcr1, pcr2)) {
            return Err(
                policy_error("two active releases contain the same PCR0/PCR1/PCR2 tuple").into(),
            );
        }
        releases.push(TrustedRelease {
            version,
            pcr0,
            pcr1,
            pcr2,
        });
    }

    // Network, target download, parsing, and Sigstore verification may span a
    // metadata-expiry boundary. Re-check every top-level role against a fresh
    // clock reading immediately before the PCR authorization can escape.
    let valid_until = authorizing_metadata_valid_until(&updater, final_now())?;
    let repository_high_water =
        repository_high_water_from_updater(&updater, &store_handle, root_bytes)?;
    prune_repository_cache(&updater, &store_handle, environment, &channel)?;

    Ok(TrustedReleasePolicy {
        environment,
        sequence: channel.sequence,
        policy_id,
        repository_high_water,
        valid_until,
        releases,
    })
}

fn tuf_updater_config() -> UpdaterConfig {
    UpdaterConfig {
        root_max_length: MAX_ROOT_BYTES,
        timestamp_max_length: MAX_TIMESTAMP_BYTES,
        snapshot_max_length: MAX_SNAPSHOT_BYTES,
        targets_max_length: MAX_TARGETS_METADATA_BYTES,
        target_max_length: MAX_BUNDLE_BYTES,
        // sigstore-tuf treats this as the number of fetch iterations, including
        // the final missing-next-root sentinel. Thirty-three iterations permit
        // root 1 through root 33 (32 rotations) and fail if root 34 exists.
        max_root_rotations: MAX_ROOT_TRANSITIONS + 1,
        max_delegations: 16,
    }
}

fn authorizing_metadata_valid_until(
    updater: &Updater,
    now: jiff::Timestamp,
) -> Result<jiff::Timestamp> {
    let trusted = updater.trusted();
    let mut valid_until = validate_role_current("root", &trusted.root().expires, now)?;
    let timestamp = trusted
        .timestamp()
        .ok_or_else(|| policy_error("TUF refresh did not produce timestamp metadata"))?;
    valid_until = valid_until.min(validate_role_current("timestamp", &timestamp.expires, now)?);
    let snapshot = trusted
        .snapshot()
        .ok_or_else(|| policy_error("TUF refresh did not produce snapshot metadata"))?;
    valid_until = valid_until.min(validate_role_current("snapshot", &snapshot.expires, now)?);
    let targets = trusted
        .targets()
        .ok_or_else(|| policy_error("TUF refresh did not produce targets metadata"))?;
    Ok(valid_until.min(validate_role_current("targets", &targets.expires, now)?))
}

fn validate_role_current(
    role: &str,
    expires: &str,
    now: jiff::Timestamp,
) -> Result<jiff::Timestamp> {
    let expires = expires
        .parse::<jiff::Timestamp>()
        .map_err(|error| policy_error(format!("invalid TUF {role} expiry: {error}")))?;
    if expires <= now {
        return Err(policy_error(format!(
            "TUF {role} metadata expired during attestation policy refresh"
        )));
    }
    Ok(expires)
}

fn repository_high_water_from_updater(
    updater: &Updater,
    store: &SnapshotStore,
    bootstrap_root: &[u8],
) -> Result<RepositoryHighWater> {
    let trusted = updater.trusted();
    let root_chain = authenticated_root_authority_history(
        bootstrap_root,
        &store.entries(),
        trusted.root().version,
        trusted.root_bytes(),
    )?;
    if let Some(error) = root_chain.error {
        return Err(error);
    }
    let authority_history = root_chain.repository.authority_history;
    let authorities = root_role_authorities(trusted.root())?;
    let timestamp = trusted
        .timestamp()
        .ok_or_else(|| policy_error("TUF refresh did not produce timestamp metadata"))?;
    let snapshot = trusted
        .snapshot()
        .ok_or_else(|| policy_error("TUF refresh did not produce snapshot metadata"))?;
    let targets = trusted
        .targets()
        .ok_or_else(|| policy_error("TUF refresh did not produce targets metadata"))?;
    Ok(RepositoryHighWater {
        root: MetadataHighWater {
            version: trusted.root().version,
            sha256: signed_metadata_sha256("root", trusted.root_bytes())?,
            authority: None,
            referenced_authority: None,
        },
        root_authority: authorities.root.clone(),
        timestamp_authority: authorities.timestamp.clone(),
        snapshot_authority: authorities.snapshot.clone(),
        targets_authority: authorities.targets.clone(),
        authority_history,
        timestamp: Some(metadata_high_water(
            store,
            "timestamp.json",
            timestamp.version,
            &authorities.timestamp,
        )?),
        snapshot_descriptor: Some(metadata_descriptor_high_water(
            "snapshot",
            timestamp
                .snapshot_meta()
                .ok_or_else(|| policy_error("TUF timestamp metadata does not pin snapshot.json"))?,
            &authorities.timestamp,
            &authorities.snapshot,
        )?),
        snapshot: Some(metadata_high_water(
            store,
            "snapshot.json",
            snapshot.version,
            &authorities.snapshot,
        )?),
        targets_descriptor: Some(metadata_descriptor_high_water(
            "targets",
            snapshot
                .meta
                .get("targets.json")
                .ok_or_else(|| policy_error("TUF snapshot metadata does not pin targets.json"))?,
            &authorities.snapshot,
            &authorities.targets,
        )?),
        targets: Some(metadata_high_water(
            store,
            "targets.json",
            targets.version,
            &authorities.targets,
        )?),
    })
}

struct AuthenticatedObservation {
    repository_high_water: RepositoryHighWater,
    channel_high_water: BTreeMap<AttestationEnvironment, CacheHighWater>,
    entries: BTreeMap<String, Vec<u8>>,
    error: Option<Error>,
}

async fn capture_authenticated_observation(
    store: Arc<SnapshotStore>,
    root_bytes: &[u8],
    now: jiff::Timestamp,
) -> Result<AuthenticatedObservation> {
    let store_handle = Arc::clone(&store);
    let mut updater = Updater::new(StoreRepository::new(Arc::clone(&store)), root_bytes)
        .map_err(|error| policy_error(format!("invalid embedded TUF root: {error}")))?
        .with_config(tuf_updater_config())
        .with_store(store);

    // A downstream failure is expected for a partial observation. The trusted
    // set still contains every role that was successfully authenticated before
    // the failure, which is precisely the monotonic journal we must retain.
    let _ = updater.refresh(now).await;
    let root_chain = authenticated_root_authority_history(
        root_bytes,
        &store_handle.entries(),
        updater.trusted().root().version,
        updater.trusted().root_bytes(),
    )?;
    if let Some(error) = root_chain.error {
        let entries = retain_root_only_observation_prefix(&store_handle, &root_chain.repository)?;
        return Ok(AuthenticatedObservation {
            repository_high_water: root_chain.repository,
            channel_high_water: BTreeMap::new(),
            entries,
            error: Some(error),
        });
    }
    let repository_high_water = partial_repository_high_water(
        &updater,
        &store_handle,
        root_chain.repository.authority_history,
    )?;
    let channel_high_water = prune_observation_cache(&updater, &store_handle)?;
    Ok(AuthenticatedObservation {
        repository_high_water,
        channel_high_water,
        entries: store_handle.entries(),
        error: None,
    })
}

fn partial_repository_high_water(
    updater: &Updater,
    store: &SnapshotStore,
    authority_history: AuthorityHistory,
) -> Result<RepositoryHighWater> {
    let trusted = updater.trusted();
    let authorities = root_role_authorities(trusted.root())?;
    let timestamp = trusted
        .timestamp()
        .map(|metadata| {
            metadata_high_water(
                store,
                "timestamp.json",
                metadata.version,
                &authorities.timestamp,
            )
        })
        .transpose()?;
    let snapshot_descriptor = trusted
        .timestamp()
        .and_then(|metadata| metadata.snapshot_meta())
        .map(|descriptor| {
            metadata_descriptor_high_water(
                "snapshot",
                descriptor,
                &authorities.timestamp,
                &authorities.snapshot,
            )
        })
        .transpose()?;
    let snapshot = trusted
        .snapshot()
        .map(|metadata| {
            metadata_high_water(
                store,
                "snapshot.json",
                metadata.version,
                &authorities.snapshot,
            )
        })
        .transpose()?;
    let targets_descriptor = trusted
        .snapshot()
        .and_then(|metadata| metadata.meta.get("targets.json"))
        .map(|descriptor| {
            metadata_descriptor_high_water(
                "targets",
                descriptor,
                &authorities.snapshot,
                &authorities.targets,
            )
        })
        .transpose()?;
    let targets = trusted
        .targets()
        .map(|metadata| {
            metadata_high_water(
                store,
                "targets.json",
                metadata.version,
                &authorities.targets,
            )
        })
        .transpose()?;
    Ok(RepositoryHighWater {
        root: MetadataHighWater {
            version: trusted.root().version,
            sha256: signed_metadata_sha256("root", trusted.root_bytes())?,
            authority: None,
            referenced_authority: None,
        },
        authority_history,
        root_authority: authorities.root,
        timestamp_authority: authorities.timestamp,
        snapshot_authority: authorities.snapshot,
        targets_authority: authorities.targets,
        timestamp,
        snapshot_descriptor,
        snapshot,
        targets_descriptor,
        targets,
    })
}

struct RootRoleAuthorities {
    root: RoleAuthority,
    timestamp: RoleAuthority,
    snapshot: RoleAuthority,
    targets: RoleAuthority,
}

fn root_role_authorities(root: &sigstore_tuf::Root) -> Result<RootRoleAuthorities> {
    // sigstore-tuf 0.11 counts distinct declared key IDs toward a threshold.
    // Reject duplicate aliases for the same normalized key material on every
    // top-level role, including root itself, before trusting online-role epoch
    // descriptors derived from this root.
    let root_authority = root_role_authority(root, "root")?;
    let authorities = RootRoleAuthorities {
        root: root_authority.clone(),
        timestamp: root_role_authority(root, "timestamp")?,
        snapshot: root_role_authority(root, "snapshot")?,
        targets: root_role_authority(root, "targets")?,
    };

    // The offline root authority is the recovery boundary for every online
    // role. Reusing any normalized root key material for timestamp, snapshot,
    // or targets would let compromise of routine publishing credentials also
    // authorize root rotation. Online roles may intentionally share custody in
    // the initial 1-of-1 deployment, but none may intersect the root role.
    let online_key_material = authorities
        .timestamp
        .key_fingerprints
        .iter()
        .chain(&authorities.snapshot.key_fingerprints)
        .chain(&authorities.targets.key_fingerprints)
        .collect::<HashSet<_>>();
    if let Some(fingerprint) = root_authority
        .key_fingerprints
        .iter()
        .find(|fingerprint| online_key_material.contains(fingerprint))
    {
        return Err(policy_error(format!(
            "TUF root role key material must be disjoint from all online roles (shared fingerprint {fingerprint})"
        )));
    }

    Ok(authorities)
}

fn root_role_authority(root: &sigstore_tuf::Root, role_name: &str) -> Result<RoleAuthority> {
    let role = root
        .role(role_name)
        .ok_or_else(|| policy_error(format!("TUF root is missing role '{role_name}'")))?;
    let mut key_fingerprints = Vec::with_capacity(role.keyids.len());
    for key_id in &role.keyids {
        let key = root.keys.get(key_id).ok_or_else(|| {
            policy_error(format!(
                "TUF root role '{role_name}' references unknown key '{key_id}'"
            ))
        })?;
        let verification_key = key
            .verification_key()
            .map_err(|error| policy_error(format!("invalid TUF {role_name} key: {error}")))?;
        // Fingerprint normalized public-key material, not the TUF key ID or
        // signing-scheme label. Declared IDs are opaque and the same RSA/EC key
        // can be re-declared under another compatible scheme; compromise
        // recovery must still recognize it as the same authority.
        key_fingerprints.push(key_custody_fingerprint(
            &key.scheme,
            verification_key.as_bytes(),
        )?);
    }
    key_fingerprints.sort();
    if key_fingerprints.windows(2).any(|keys| keys[0] == keys[1]) {
        return Err(policy_error(format!(
            "TUF root role '{role_name}' authorizes duplicate aliases for the same key material"
        )));
    }
    let authority = RoleAuthority {
        threshold: role.threshold,
        key_fingerprints,
    };
    validate_role_authority(role_name, &authority)?;
    Ok(authority)
}

fn key_custody_fingerprint(scheme: &str, key_bytes: &[u8]) -> Result<String> {
    let family = match scheme {
        "ecdsa-sha2-nistp256" => b"ecdsa-p256".as_slice(),
        "ecdsa-sha2-nistp384" => b"ecdsa-p384".as_slice(),
        "ed25519" => b"ed25519".as_slice(),
        "rsassa-pss-sha256" | "rsassa-pss-sha384" | "rsassa-pss-sha512" => b"rsa".as_slice(),
        scheme => {
            return Err(policy_error(format!(
                "unsupported TUF key scheme '{scheme}' while fingerprinting authority"
            )))
        }
    };
    let mut digest = Sha256::new();
    digest.update(b"opensecret-tuf-key-custody-v1\0");
    digest.update(family);
    digest.update([0]);
    digest.update(key_bytes);
    Ok(hex::encode(digest.finalize()))
}

fn metadata_high_water(
    store: &SnapshotStore,
    name: &str,
    version: u64,
    authority: &RoleAuthority,
) -> Result<MetadataHighWater> {
    let bytes = store
        .load(name)
        .ok_or_else(|| policy_error(format!("verified TUF cache is missing {name}")))?;
    Ok(MetadataHighWater {
        version,
        sha256: signed_metadata_sha256(name, &bytes)?,
        authority: Some(authority.into()),
        referenced_authority: None,
    })
}

fn metadata_descriptor_high_water(
    role: &str,
    descriptor: &sigstore_tuf::MetaFile,
    authority: &RoleAuthority,
    referenced_authority: &RoleAuthority,
) -> Result<MetadataHighWater> {
    let value = serde_json::to_value(descriptor).map_err(|error| {
        policy_error(format!("invalid TUF {role} descriptor metadata: {error}"))
    })?;
    let canonical = sigstore_tuf::canonical_json::to_canonical_bytes(&value).map_err(|error| {
        policy_error(format!("invalid TUF {role} descriptor metadata: {error}"))
    })?;
    Ok(MetadataHighWater {
        version: descriptor.version,
        sha256: sha256_hex(&canonical),
        authority: Some(authority.into()),
        referenced_authority: Some(referenced_authority.into()),
    })
}

fn signed_metadata_sha256(role: &str, envelope_bytes: &[u8]) -> Result<String> {
    let envelope: Value = serde_json::from_slice(envelope_bytes)
        .map_err(|error| policy_error(format!("invalid TUF {role} envelope JSON: {error}")))?;
    let signed = envelope.get("signed").ok_or_else(|| {
        policy_error(format!(
            "invalid TUF {role} envelope: missing signed payload"
        ))
    })?;
    let canonical = sigstore_tuf::canonical_json::to_canonical_bytes(signed)
        .map_err(|error| policy_error(format!("invalid TUF {role} signed metadata: {error}")))?;
    Ok(sha256_hex(&canonical))
}

fn prune_repository_cache(
    updater: &Updater,
    store: &SnapshotStore,
    current_environment: AttestationEnvironment,
    current_channel: &Channel,
) -> Result<()> {
    let existing = store.entries();
    let mut retained = BTreeMap::new();
    let trusted_root_version = updater.trusted().root().version;

    for (name, bytes) in &existing {
        let Some(version) = name
            .strip_prefix("root_history/")
            .and_then(|name| name.strip_suffix(".root.json"))
            .and_then(|version| version.parse::<u64>().ok())
        else {
            continue;
        };
        if version <= trusted_root_version {
            retained.insert(name.clone(), bytes.clone());
        }
    }
    retained.insert(
        "root.json".to_string(),
        updater.trusted().root_bytes().to_vec(),
    );
    for name in ["timestamp.json", "snapshot.json", "targets.json"] {
        let bytes = existing
            .get(name)
            .ok_or_else(|| policy_error(format!("verified TUF cache is missing {name}")))?;
        retained.insert(name.to_string(), bytes.clone());
    }

    retain_complete_channel(updater, current_environment, current_channel, &mut retained)?;
    for environment in [
        AttestationEnvironment::Production,
        AttestationEnvironment::Development,
    ] {
        if environment == current_environment {
            continue;
        }
        let channel_path = format!("channels/{}.json", environment.as_str());
        let Some(channel_bytes) = cached_top_level_target(updater, &channel_path) else {
            continue;
        };
        let Ok(channel) = parse_json::<Channel>("channel", &channel_bytes) else {
            continue;
        };
        if validate_channel(&channel, environment).is_err() {
            continue;
        }
        let mut candidate = BTreeMap::new();
        if retain_complete_channel(updater, environment, &channel, &mut candidate).is_ok() {
            retained.extend(candidate);
        }
    }

    if retained.len() > MAX_CACHE_ENTRIES
        || retained.values().map(Vec::len).sum::<usize>() as u64 > MAX_CACHE_BYTES
    {
        return Err(policy_error(
            "minimum verified TUF cache generation exceeds cache bounds",
        ));
    }
    store.replace_entries(retained);
    Ok(())
}

fn prune_observation_cache(
    updater: &Updater,
    store: &SnapshotStore,
) -> Result<BTreeMap<AttestationEnvironment, CacheHighWater>> {
    let existing = store.entries();
    let mut retained = retain_observed_metadata(updater, &existing)?;
    let mut channel_high_water = BTreeMap::new();
    if updater.trusted().targets().is_some() {
        let targets_authority = root_role_authority(updater.trusted().root(), "targets")?;
        for environment in [
            AttestationEnvironment::Production,
            AttestationEnvironment::Development,
        ] {
            let channel_path = format!("channels/{}.json", environment.as_str());
            let Some(channel_bytes) = cached_top_level_target(updater, &channel_path) else {
                continue;
            };
            // A target may satisfy TUF's repository-wide 2 MiB cap while
            // exceeding this channel schema's tighter 128 KiB cap. It cannot
            // produce a channel floor, but it must not prevent already
            // authenticated root/top-level metadata from being journaled.
            if enforce_target_size("channel", &channel_bytes, MAX_CHANNEL_BYTES).is_err() {
                continue;
            }
            let Ok(channel) = parse_json::<Channel>("channel", &channel_bytes) else {
                continue;
            };
            if validate_channel(&channel, environment).is_err() {
                continue;
            }
            channel_high_water.insert(
                environment,
                CacheHighWater {
                    sequence: channel.sequence,
                    policy_id: sha256_hex(&channel_bytes),
                    authority: (&targets_authority).into(),
                },
            );
            let mut candidate = BTreeMap::new();
            if retain_complete_channel(updater, environment, &channel, &mut candidate).is_ok() {
                retained.extend(candidate);
            } else {
                retained.insert(format!("targets/{channel_path}"), channel_bytes);
            }
        }
    }
    if retained.len() > MAX_CACHE_ENTRIES
        || retained.values().map(Vec::len).sum::<usize>() as u64 > MAX_CACHE_BYTES
    {
        return Err(policy_error(
            "minimum authenticated TUF observation exceeds cache bounds",
        ));
    }
    store.replace_entries(retained);
    Ok(channel_high_water)
}

fn retain_observed_metadata(
    updater: &Updater,
    existing: &BTreeMap<String, Vec<u8>>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut retained = BTreeMap::new();
    let trusted = updater.trusted();
    for (name, bytes) in existing {
        let Some(version) = name
            .strip_prefix("root_history/")
            .and_then(|name| name.strip_suffix(".root.json"))
            .and_then(|version| version.parse::<u64>().ok())
        else {
            continue;
        };
        if version <= trusted.root().version {
            retained.insert(name.clone(), bytes.clone());
        }
    }
    retained.insert("root.json".to_string(), trusted.root_bytes().to_vec());
    for (name, present) in [
        ("timestamp.json", trusted.timestamp().is_some()),
        ("snapshot.json", trusted.snapshot().is_some()),
        ("targets.json", trusted.targets().is_some()),
    ] {
        if present {
            let bytes = existing
                .get(name)
                .ok_or_else(|| policy_error(format!("verified TUF cache is missing {name}")))?;
            retained.insert(name.to_string(), bytes.clone());
        }
    }
    Ok(retained)
}

fn retain_root_only_observation_prefix(
    store: &SnapshotStore,
    repository: &RepositoryHighWater,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let existing = store.entries();
    let mut retained = BTreeMap::new();
    for (name, bytes) in &existing {
        let Some(version) = name
            .strip_prefix("root_history/")
            .and_then(|name| name.strip_suffix(".root.json"))
            .and_then(|version| version.parse::<u64>().ok())
        else {
            continue;
        };
        if version <= repository.root.version {
            retained.insert(name.clone(), bytes.clone());
        }
    }
    let anchor_name = format!("root_history/{}.root.json", repository.root.version);
    let anchor_bytes = retained.get(&anchor_name).ok_or_else(|| {
        policy_error(format!(
            "authenticated TUF root prefix is missing root version {}",
            repository.root.version
        ))
    })?;
    let anchor = root_transition_high_water(anchor_bytes)?;
    if anchor.root != repository.root
        || anchor.root_authority != repository.root_authority
        || anchor.timestamp_authority != repository.timestamp_authority
        || anchor.snapshot_authority != repository.snapshot_authority
        || anchor.targets_authority != repository.targets_authority
    {
        return Err(policy_error(
            "authenticated TUF root prefix does not match its repository floor",
        ));
    }
    retained.insert("root.json".to_string(), anchor_bytes.clone());
    if retained.len() > MAX_CACHE_ENTRIES
        || retained.values().map(Vec::len).sum::<usize>() as u64 > MAX_CACHE_BYTES
    {
        return Err(policy_error(
            "minimum authenticated TUF root prefix exceeds cache bounds",
        ));
    }
    store.replace_entries(retained.clone());
    Ok(retained)
}

fn retain_complete_channel(
    updater: &Updater,
    environment: AttestationEnvironment,
    channel: &Channel,
    retained: &mut BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    let channel_path = format!("channels/{}.json", environment.as_str());
    retain_cached_target(updater, &channel_path, None, MAX_CHANNEL_BYTES, retained)?;
    if channel.active.is_empty() {
        return Ok(());
    }
    retain_cached_target(
        updater,
        &channel.sigstore_trusted_root_target.path,
        Some(&channel.sigstore_trusted_root_target.sha256),
        MAX_SIGSTORE_ROOT_BYTES,
        retained,
    )?;
    for active in &channel.active {
        validate_release_targets(active, environment)?;
        retain_cached_target(
            updater,
            &active.manifest_target,
            Some(&active.manifest_sha256),
            MAX_MANIFEST_BYTES,
            retained,
        )?;
        retain_cached_target(
            updater,
            &active.bundle_target,
            Some(&active.bundle_sha256),
            MAX_BUNDLE_BYTES as usize,
            retained,
        )?;
    }
    Ok(())
}

fn retain_cached_target(
    updater: &Updater,
    path: &str,
    expected_sha256: Option<&str>,
    max_length: usize,
    retained: &mut BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    let bytes = cached_top_level_target(updater, path).ok_or_else(|| {
        policy_error(format!(
            "verified TUF cache is missing current target '{path}'"
        ))
    })?;
    enforce_target_size(path, &bytes, max_length)?;
    if let Some(expected_sha256) = expected_sha256 {
        validate_hex("target sha256", expected_sha256, SHA256_HEX_LEN)?;
        if sha256_hex(&bytes) != expected_sha256 {
            return Err(policy_error(format!(
                "cached target '{path}' does not match its channel SHA-256"
            )));
        }
    }
    retained.insert(format!("targets/{path}"), bytes);
    Ok(())
}

fn cached_top_level_target(updater: &Updater, path: &str) -> Option<Vec<u8>> {
    let target = updater.find_target(path)?;
    updater.find_cached_target(target, path)
}

async fn get_target(
    updater: &mut Updater,
    path: &str,
    now: jiff::Timestamp,
) -> std::result::Result<Vec<u8>, RefreshFailure> {
    validate_target_path(path)?;
    if updater.find_target(path).is_none() {
        return Err(policy_error(format!(
            "required target '{path}' must be authorized by top-level targets metadata"
        ))
        .into());
    }
    updater.get_target(path, now).await.map_err(|error| {
        classify_tuf_error(&format!("TUF target '{path}' failed verification"), error)
    })
}

async fn get_bound_target(
    updater: &mut Updater,
    path: &str,
    expected_sha256: &str,
    max_length: usize,
    now: jiff::Timestamp,
) -> std::result::Result<Vec<u8>, RefreshFailure> {
    validate_hex("target sha256", expected_sha256, SHA256_HEX_LEN)?;
    let bytes = get_target(updater, path, now).await?;
    enforce_target_size(path, &bytes, max_length)?;
    let actual = sha256_hex(&bytes);
    if actual != expected_sha256 {
        return Err(policy_error(format!(
            "channel SHA-256 for target '{path}' is '{expected_sha256}', got '{actual}'"
        ))
        .into());
    }
    Ok(bytes)
}

fn enforce_target_size(label: &str, bytes: &[u8], max_length: usize) -> Result<()> {
    if bytes.len() > max_length {
        return Err(policy_error(format!(
            "authenticated {label} exceeds maximum length {max_length}"
        )));
    }
    Ok(())
}

fn enforce_timestamp_window(updater: &Updater, now: jiff::Timestamp) -> Result<()> {
    let timestamp = updater
        .trusted()
        .timestamp()
        .ok_or_else(|| policy_error("TUF refresh did not produce timestamp metadata"))?;
    validate_timestamp_window(&timestamp.expires, now)
}

fn validate_timestamp_window(expires: &str, now: jiff::Timestamp) -> Result<()> {
    let expires = expires
        .parse::<jiff::Timestamp>()
        .map_err(|error| policy_error(format!("invalid TUF timestamp expiry: {error}")))?;
    if expires < now {
        return Err(policy_error("TUF timestamp metadata has expired"));
    }
    if expires.duration_since(now) > jiff::SignedDuration::from_hours(MAX_TIMESTAMP_VALIDITY_HOURS)
    {
        return Err(policy_error(format!(
            "TUF timestamp validity exceeds the {MAX_TIMESTAMP_VALIDITY_HOURS}-hour last-known-good window"
        )));
    }
    Ok(())
}

fn validate_channel(channel: &Channel, environment: AttestationEnvironment) -> Result<()> {
    if channel.schema != CHANNEL_SCHEMA {
        return Err(policy_error(format!(
            "unsupported channel schema '{}'",
            channel.schema
        )));
    }
    if channel.environment != environment {
        return Err(policy_error(format!(
            "channel environment '{}' does not match requested '{}'",
            channel.environment, environment
        )));
    }
    if channel.sequence == 0 {
        return Err(policy_error("channel sequence must be greater than zero"));
    }
    if channel.active.len() > MAX_ACTIVE_RELEASES {
        return Err(policy_error(format!(
            "channel may contain at most {MAX_ACTIVE_RELEASES} active releases"
        )));
    }
    if channel.sigstore_trusted_root_target.path != "sigstore/trusted_root.json" {
        return Err(policy_error(
            "sigstoreTrustedRootTarget.path must be 'sigstore/trusted_root.json'",
        ));
    }
    validate_hex(
        "sigstoreTrustedRootTarget.sha256",
        &channel.sigstore_trusted_root_target.sha256,
        SHA256_HEX_LEN,
    )?;
    Ok(())
}

fn validate_release_targets(
    active: &ActiveRelease,
    environment: AttestationEnvironment,
) -> Result<String> {
    validate_hex("manifestSha256", &active.manifest_sha256, SHA256_HEX_LEN)?;
    validate_hex("bundleSha256", &active.bundle_sha256, SHA256_HEX_LEN)?;
    let manifest_version =
        release_version_from_target(&active.manifest_target, environment, "manifest.json")?;
    let bundle_version =
        release_version_from_target(&active.bundle_target, environment, "manifest.sigstore.json")?;
    if manifest_version != bundle_version {
        return Err(policy_error(
            "manifestTarget and bundleTarget identify different releases",
        ));
    }
    Ok(manifest_version)
}

fn release_version_from_target(
    path: &str,
    environment: AttestationEnvironment,
    file: &str,
) -> Result<String> {
    validate_target_path(path)?;
    let parts = path.split('/').collect::<Vec<_>>();
    if parts.len() != 4
        || parts[0] != "releases"
        || parts[2] != environment.as_str()
        || parts[3] != file
    {
        return Err(policy_error(format!(
            "release target '{path}' does not belong to the '{}' channel",
            environment
        )));
    }
    validate_version(parts[1])?;
    Ok(parts[1].to_string())
}

fn validate_manifest(
    manifest: &ReleaseManifest,
    version: &str,
    environment: AttestationEnvironment,
) -> Result<()> {
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(policy_error(format!(
            "unsupported manifest schema '{}'",
            manifest.schema
        )));
    }
    if manifest.component != COMPONENT {
        return Err(policy_error(format!(
            "manifest component must be '{COMPONENT}'"
        )));
    }
    if manifest.environment != environment {
        return Err(policy_error(format!(
            "manifest environment '{}' does not match channel '{}'",
            manifest.environment, environment
        )));
    }
    validate_version(&manifest.release.version)?;
    if manifest.release.version != version {
        return Err(policy_error(format!(
            "manifest release version '{}' does not match target path '{version}'",
            manifest.release.version
        )));
    }

    let expected_ref = format!("refs/tags/v{version}");
    if manifest.source.r#ref != expected_ref {
        return Err(policy_error(format!(
            "manifest source ref must be '{expected_ref}'"
        )));
    }
    if manifest.source.revision.algorithm != "git-sha1" {
        return Err(policy_error(
            "manifest source revision algorithm must be 'git-sha1'",
        ));
    }
    validate_hex(
        "source.revision.digest",
        &manifest.source.revision.digest,
        40,
    )?;
    validate_source_path(&manifest.source.path)?;
    validate_https_url("source.uri", &manifest.source.uri)?;

    validate_file_name("artifact.name", &manifest.artifact.name)?;
    if manifest.artifact.media_type != EIF_MEDIA_TYPE {
        return Err(policy_error(format!(
            "manifest artifact mediaType must be '{EIF_MEDIA_TYPE}'"
        )));
    }
    if manifest.artifact.size == 0 {
        return Err(policy_error(
            "manifest artifact size must be greater than zero",
        ));
    }
    validate_hex(
        "artifact.digests.sha256",
        &manifest.artifact.digests.sha256,
        SHA256_HEX_LEN,
    )?;
    if manifest.measurements.algorithm != "sha384" {
        return Err(policy_error(
            "manifest measurements algorithm must be 'sha384'",
        ));
    }
    if manifest.measurements.required_pcrs != [0, 1, 2] {
        return Err(policy_error(
            "manifest requiredPcrs must be exactly [0, 1, 2]",
        ));
    }
    decode_pcr("measurements.pcrs.0", &manifest.measurements.pcrs.pcr0)?;
    decode_pcr("measurements.pcrs.1", &manifest.measurements.pcrs.pcr1)?;
    decode_pcr("measurements.pcrs.2", &manifest.measurements.pcrs.pcr2)?;

    if manifest.build.system != "nix" {
        return Err(policy_error("manifest build system must be 'nix'"));
    }
    validate_identifier("build.builderId", &manifest.build.builder_id)?;
    validate_nonempty("build.derivation", &manifest.build.derivation)?;
    validate_hex(
        "build.flakeLockSha256",
        &manifest.build.flake_lock_sha256,
        SHA256_HEX_LEN,
    )?;
    validate_https_url("build.runUri", &manifest.build.run_uri)?;
    Ok(())
}

fn parse_json<T: for<'de> Deserialize<'de>>(label: &str, bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes)
        .map_err(|error| policy_error(format!("invalid {label} JSON: {error}")))
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn validate_version(version: &str) -> Result<()> {
    let parts = version.split('.').collect::<Vec<_>>();
    if version.starts_with('v')
        || parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
    {
        return Err(policy_error(format!(
            "release version '{version}' must be stable MAJOR.MINOR.PATCH without a leading v"
        )));
    }
    Ok(())
}

fn validate_identifier(field: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(policy_error(format!("{field} is not a valid identifier")));
    }
    Ok(())
}

fn validate_nonempty(field: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.trim() != value || value.len() > 4_096 {
        return Err(policy_error(format!(
            "{field} must be a non-empty, trimmed string"
        )));
    }
    Ok(())
}

fn validate_hex(field: &str, value: &str, length: usize) -> Result<()> {
    if value.len() != length
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(policy_error(format!(
            "{field} must be exactly {length} lowercase hexadecimal characters"
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
    bytes
        .try_into()
        .map_err(|_| policy_error(format!("{field} must decode to 48 bytes")))
}

fn attestation_pcr(document: &AttestationDocument, index: usize) -> Result<[u8; 48]> {
    document
        .pcrs
        .get(&index)
        .ok_or_else(|| Error::AttestationVerificationFailed(format!("PCR{index} missing")))?
        .as_slice()
        .try_into()
        .map_err(|_| {
            Error::AttestationVerificationFailed(format!("PCR{index} must be exactly 48 bytes"))
        })
}

fn validate_file_name(field: &str, value: &str) -> Result<()> {
    validate_nonempty(field, value)?;
    if matches!(value, "." | "..") || value.contains('/') || value.contains('\\') {
        return Err(policy_error(format!("{field} must be a file name")));
    }
    Ok(())
}

fn validate_source_path(value: &str) -> Result<()> {
    validate_nonempty("source.path", value)?;
    if value != "."
        && (value.starts_with('/')
            || value.contains('\\')
            || value.split('/').any(|part| matches!(part, "." | ".." | "")))
    {
        return Err(policy_error(
            "source.path must be '.' or a safe repository-relative path",
        ));
    }
    Ok(())
}

fn validate_https_url(field: &str, value: &str) -> Result<Url> {
    let url =
        Url::parse(value).map_err(|error| policy_error(format!("invalid {field} URL: {error}")))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(policy_error(format!(
            "{field} must be an HTTPS URL without credentials, query, or fragment"
        )));
    }
    Ok(url)
}

fn validate_target_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('?')
        || path.contains('#')
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(policy_error(format!("unsafe TUF target path '{path}'")));
    }
    Ok(())
}

fn validate_repository_path(path: &str) -> std::result::Result<(), &'static str> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('?')
        || path.contains('#')
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        Err("unsafe relative path")
    } else {
        Ok(())
    }
}

fn validate_repository_base(value: &str, allow_test_loopback_http: bool) -> Result<Url> {
    let with_slash = if value.ends_with('/') {
        value.to_string()
    } else {
        format!("{value}/")
    };
    let url = Url::parse(&with_slash)
        .map_err(|error| policy_error(format!("invalid TUF repository URL: {error}")))?;
    let loopback_http = allow_test_loopback_http
        && url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
    if (url.scheme() != "https" && !loopback_http)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(policy_error(
            "TUF repository must be an HTTPS URL without credentials, query, or fragment",
        ));
    }
    Ok(url)
}

fn validate_store_name(name: &str) -> sigstore_tuf::Result<()> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(sigstore_tuf::Error::Malformed(format!(
            "unsafe cache name '{name}'"
        )));
    }
    Ok(())
}

fn is_unpublished_root(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|value| {
            value
                .get("schema")
                .and_then(|schema| schema.as_str())
                .map(str::to_owned)
        })
        .as_deref()
        == Some(UNPUBLISHED_ROOT_SCHEMA)
}

fn validate_official_embedded_root(bytes: &[u8]) -> Result<()> {
    // Keep staging builds fail-closed at refresh time while the generated
    // production bootstrap has not yet replaced the explicit placeholder.
    if is_unpublished_root(bytes) {
        return Ok(());
    }
    let trusted = sigstore_tuf::TrustedMetadataSet::from_root(bytes)
        .map_err(|error| policy_error(format!("invalid official embedded TUF root: {error}")))?;
    if trusted.root().version != 1 {
        return Err(policy_error(format!(
            "official embedded TUF root signed version must be exactly 1; found {}",
            trusted.root().version
        )));
    }
    Ok(())
}

fn validate_cached_root_span(bootstrap_root: &[u8], cached: &CachedRepository) -> Result<()> {
    let Some(repository) = &cached.repository_high_water else {
        return Ok(());
    };
    let bootstrap = root_transition_high_water(bootstrap_root)?;
    let maximum_version = bootstrap.root.version.saturating_add(MAX_ROOT_TRANSITIONS);
    if repository.root.version > maximum_version {
        return Err(policy_error(format!(
            "cached TUF root version {} exceeds the supported {MAX_ROOT_TRANSITIONS} transitions from bootstrap version {}",
            repository.root.version, bootstrap.root.version
        )));
    }
    Ok(())
}

fn merge_high_water(
    persisted: Option<&CacheHighWater>,
    in_memory: Option<&CacheHighWater>,
) -> Result<Option<CacheHighWater>> {
    match (persisted, in_memory) {
        (None, None) => Ok(None),
        (Some(value), None) | (None, Some(value)) => Ok(Some(value.clone())),
        (Some(persisted), Some(in_memory)) => {
            if persisted.sequence == in_memory.sequence
                && persisted.policy_id != in_memory.policy_id
            {
                return Err(policy_error(
                    "conflicting channel policy IDs exist at the same high-water sequence",
                ));
            }
            Ok(Some(if persisted.sequence == in_memory.sequence {
                CacheHighWater {
                    sequence: persisted.sequence,
                    policy_id: persisted.policy_id.clone(),
                    authority: merge_authority_provenance(
                        &persisted.authority,
                        &in_memory.authority,
                    )?,
                }
            } else if persisted.sequence > in_memory.sequence {
                persisted.clone()
            } else {
                in_memory.clone()
            }))
        }
    }
}

fn merge_channel_high_water_maps(
    mut left: BTreeMap<AttestationEnvironment, CacheHighWater>,
    right: &BTreeMap<AttestationEnvironment, CacheHighWater>,
) -> Result<BTreeMap<AttestationEnvironment, CacheHighWater>> {
    for (environment, right_mark) in right {
        if let Some(merged) = merge_high_water(left.get(environment), Some(right_mark))? {
            left.insert(*environment, merged);
        }
    }
    Ok(left)
}

fn retain_channel_floors_for_authority(
    channels: &mut BTreeMap<AttestationEnvironment, CacheHighWater>,
    candidate: &RoleAuthority,
) -> Result<()> {
    let mut safely_replaced = Vec::new();
    for (environment, floor) in channels.iter_mut() {
        if safely_replaces_authority(&floor.authority, candidate) {
            safely_replaced.push(*environment);
            continue;
        }
        // Signatures are detachable from a TUF envelope. When a root keeps
        // enough old targets keys to authorize replay while adding keys, an
        // equal channel may have hidden signatures from any newly authorized
        // key. Conservatively bind the retained floor to the full current
        // authority. Staged overlap is therefore not a recovery mechanism;
        // recovery requires a replacement the old authority cannot satisfy.
        floor.authority = union_authority_provenance(&floor.authority, candidate)?;
    }
    for environment in safely_replaced {
        channels.remove(&environment);
    }
    Ok(())
}

fn merge_loaded_security_high_water_states(
    cached_repository: Option<&RepositoryHighWater>,
    cached_channels: BTreeMap<AttestationEnvironment, CacheHighWater>,
    memory_repository: Option<&RepositoryHighWater>,
    memory_channels: &BTreeMap<AttestationEnvironment, CacheHighWater>,
    cached_entries: &BTreeMap<String, Vec<u8>>,
    memory_root_history: &BTreeMap<String, Vec<u8>>,
) -> Result<MemoryHighWater> {
    match (cached_repository, memory_repository) {
        (Some(cached), Some(memory)) if cached.root.version > memory.root.version => {
            // A process-local floor may be newer than the last successful disk
            // write, while another process later writes a different forward
            // root chain. Anchor and replay that exact cached chain from the
            // in-memory root before combining any child/channel floors.
            let advanced_memory = advance_security_floors_through_root_history(
                Some(memory),
                memory_channels.clone(),
                cached,
                cached_entries,
            )?;
            merge_security_high_water_states(
                Some(cached),
                cached_channels,
                advanced_memory.repository.as_ref(),
                &advanced_memory.channels,
            )
        }
        (Some(cached), Some(memory)) if memory.root.version > cached.root.version => {
            let advanced_cache = advance_security_floors_through_root_history(
                Some(cached),
                cached_channels,
                memory,
                memory_root_history,
            )?;
            merge_security_high_water_states(
                advanced_cache.repository.as_ref(),
                advanced_cache.channels,
                Some(memory),
                memory_channels,
            )
        }
        _ => merge_security_high_water_states(
            cached_repository,
            cached_channels,
            memory_repository,
            memory_channels,
        ),
    }
}

fn merge_security_high_water_states(
    left_repository: Option<&RepositoryHighWater>,
    left_channels: BTreeMap<AttestationEnvironment, CacheHighWater>,
    right_repository: Option<&RepositoryHighWater>,
    right_channels: &BTreeMap<AttestationEnvironment, CacheHighWater>,
) -> Result<MemoryHighWater> {
    match (left_repository, right_repository) {
        (None, None) => {
            if !left_channels.is_empty() || !right_channels.is_empty() {
                return Err(policy_error(
                    "channel high-water state exists without a TUF repository epoch",
                ));
            }
            Ok(MemoryHighWater::default())
        }
        (Some(repository), None) => {
            if !right_channels.is_empty() {
                return Err(policy_error(
                    "channel high-water state exists without a TUF repository epoch",
                ));
            }
            Ok(MemoryHighWater {
                repository: Some(repository.clone()),
                channels: left_channels,
            })
        }
        (None, Some(repository)) => {
            if !left_channels.is_empty() {
                return Err(policy_error(
                    "channel high-water state exists without a TUF repository epoch",
                ));
            }
            Ok(MemoryHighWater {
                repository: Some(repository.clone()),
                channels: right_channels.clone(),
            })
        }
        (Some(left), Some(right)) => {
            let repository = merge_repository_high_waters(Some(left), Some(right))?
                .expect("two repository high-water marks must merge to one");
            let channels = if left.root.version < right.root.version {
                let mut older_channels = left_channels;
                retain_channel_floors_for_authority(&mut older_channels, &right.targets_authority)?;
                merge_channel_high_water_maps(older_channels, right_channels)?
            } else if right.root.version < left.root.version {
                let mut older_channels = right_channels.clone();
                retain_channel_floors_for_authority(&mut older_channels, &left.targets_authority)?;
                merge_channel_high_water_maps(older_channels, &left_channels)?
            } else {
                return Ok(MemoryHighWater {
                    repository: Some(repository),
                    channels: merge_channel_high_water_maps(left_channels, right_channels)?,
                });
            };
            Ok(MemoryHighWater {
                repository: Some(repository),
                channels,
            })
        }
    }
}

fn advance_security_floors_through_root_history(
    prior_repository: Option<&RepositoryHighWater>,
    mut channels: BTreeMap<AttestationEnvironment, CacheHighWater>,
    observed_repository: &RepositoryHighWater,
    entries: &BTreeMap<String, Vec<u8>>,
) -> Result<MemoryHighWater> {
    validate_authenticated_root_history(entries, observed_repository.root.version)?;
    let Some(prior_repository) = prior_repository else {
        if !channels.is_empty() {
            return Err(policy_error(
                "channel high-water state exists without a TUF repository epoch",
            ));
        }
        return Ok(MemoryHighWater {
            repository: None,
            channels,
        });
    };
    if observed_repository.root.version < prior_repository.root.version {
        return Err(policy_error(format!(
            "TUF root metadata rollback: previously accepted version {}, received {}",
            prior_repository.root.version, observed_repository.root.version
        )));
    }
    if observed_repository.root.version == prior_repository.root.version {
        enforce_metadata_high_water(
            "root",
            Some(&observed_repository.root),
            Some(&prior_repository.root),
        )?;
        authority_resets(prior_repository, observed_repository)?;
        return Ok(MemoryHighWater {
            repository: Some(prior_repository.clone()),
            channels,
        });
    }

    let anchor_name = format!("root_history/{}.root.json", prior_repository.root.version);
    let anchor_bytes = entries.get(&anchor_name).ok_or_else(|| {
        policy_error(format!(
            "verified TUF cache is missing root-history anchor {}",
            prior_repository.root.version
        ))
    })?;
    let anchor = root_transition_high_water(anchor_bytes)?;
    if anchor.root != prior_repository.root
        || anchor.root_authority != prior_repository.root_authority
        || anchor.timestamp_authority != prior_repository.timestamp_authority
        || anchor.snapshot_authority != prior_repository.snapshot_authority
        || anchor.targets_authority != prior_repository.targets_authority
    {
        return Err(policy_error(format!(
            "authenticated TUF root history forks from in-memory root version {}",
            prior_repository.root.version
        )));
    }
    let mut trusted_chain = sigstore_tuf::TrustedMetadataSet::from_root(anchor_bytes)
        .map_err(|error| policy_error(format!("invalid TUF root-history anchor: {error}")))?;

    let mut repository = prior_repository.clone();
    for version in (prior_repository.root.version + 1)..=observed_repository.root.version {
        let name = format!("root_history/{version}.root.json");
        let bytes = entries.get(&name).ok_or_else(|| {
            policy_error(format!(
                "verified TUF cache is missing authenticated root transition {version}"
            ))
        })?;
        let transition = root_transition_high_water(bytes)?;
        if transition.root.version != version {
            return Err(policy_error(format!(
                "TUF root history entry {version} contains version {}",
                transition.root.version
            )));
        }
        trusted_chain.update_root(bytes).map_err(|error| {
            policy_error(format!(
                "TUF root transition {version} is not authenticated by the preceding root: {error}"
            ))
        })?;
        let merged = merge_repository_observation(Some(&repository), &transition);
        if let Some(error) = merged.error {
            return Err(error);
        }
        repository = merged.high_water;
        retain_channel_floors_for_authority(&mut channels, &repository.targets_authority)?;
    }

    if repository.root != observed_repository.root
        || repository.root_authority != observed_repository.root_authority
        || repository.timestamp_authority != observed_repository.timestamp_authority
        || repository.snapshot_authority != observed_repository.snapshot_authority
        || repository.targets_authority != observed_repository.targets_authority
    {
        return Err(policy_error(
            "authenticated TUF root history does not match the final trusted root",
        ));
    }
    Ok(MemoryHighWater {
        repository: Some(repository),
        channels,
    })
}

fn root_history_for_process_state(
    entries: &BTreeMap<String, Vec<u8>>,
    repository: &RepositoryHighWater,
) -> Result<BTreeMap<String, Vec<u8>>> {
    validate_authenticated_root_history(entries, repository.root.version)?;
    let mut history = BTreeMap::new();
    for (name, bytes) in entries {
        let Some(version) = name
            .strip_prefix("root_history/")
            .and_then(|name| name.strip_suffix(".root.json"))
            .and_then(|version| version.parse::<u64>().ok())
        else {
            continue;
        };
        if version <= repository.root.version {
            history.insert(name.clone(), bytes.clone());
        }
    }
    let anchor_name = format!("root_history/{}.root.json", repository.root.version);
    let anchor = history.get(&anchor_name).ok_or_else(|| {
        policy_error(format!(
            "verified TUF state is missing root-history anchor {}",
            repository.root.version
        ))
    })?;
    let anchor = root_transition_high_water(anchor)?;
    if anchor.root != repository.root
        || anchor.root_authority != repository.root_authority
        || anchor.timestamp_authority != repository.timestamp_authority
        || anchor.snapshot_authority != repository.snapshot_authority
        || anchor.targets_authority != repository.targets_authority
    {
        return Err(policy_error(
            "verified TUF state root-history anchor does not match its repository floor",
        ));
    }
    Ok(history)
}

fn align_security_high_water_states_to_observed(
    left_repository: Option<&RepositoryHighWater>,
    left_channels: BTreeMap<AttestationEnvironment, CacheHighWater>,
    right_repository: Option<&RepositoryHighWater>,
    right_channels: &BTreeMap<AttestationEnvironment, CacheHighWater>,
    observed_repository: &RepositoryHighWater,
    entries: &BTreeMap<String, Vec<u8>>,
) -> Result<MemoryHighWater> {
    let left = advance_security_floors_through_root_history(
        left_repository,
        left_channels,
        observed_repository,
        entries,
    )?;
    let right = advance_security_floors_through_root_history(
        right_repository,
        right_channels.clone(),
        observed_repository,
        entries,
    )?;
    merge_security_high_water_states(
        left.repository.as_ref(),
        left.channels,
        right.repository.as_ref(),
        &right.channels,
    )
}

fn validate_authenticated_root_history(
    entries: &BTreeMap<String, Vec<u8>>,
    final_version: u64,
) -> Result<()> {
    for (name, bytes) in entries {
        let Some(version) = name
            .strip_prefix("root_history/")
            .and_then(|name| name.strip_suffix(".root.json"))
            .and_then(|version| version.parse::<u64>().ok())
        else {
            continue;
        };
        if version > final_version {
            continue;
        }
        let root = root_transition_high_water(bytes)?;
        if root.root.version != version {
            return Err(policy_error(format!(
                "TUF root history entry {version} contains version {}",
                root.root.version
            )));
        }
    }
    Ok(())
}

struct AuthenticatedRootChain {
    repository: RepositoryHighWater,
    error: Option<Error>,
}

fn authenticated_root_authority_history(
    bootstrap_root: &[u8],
    entries: &BTreeMap<String, Vec<u8>>,
    final_version: u64,
    final_root: &[u8],
) -> Result<AuthenticatedRootChain> {
    // A first-ever refresh has no persisted repository floor to advance, but
    // it can still traverse several authenticated roots. Replay that exact
    // chain from the embedded bootstrap so online keys retired by an
    // intermediate root cannot be reintroduced after the first cache write.
    let mut repository = root_transition_high_water(bootstrap_root)?;
    let bootstrap_version = repository.root.version;
    if final_version < repository.root.version {
        return Err(policy_error(format!(
            "final TUF root version {final_version} predates embedded bootstrap version {}",
            repository.root.version
        )));
    }

    let anchor_name = format!("root_history/{}.root.json", repository.root.version);
    let anchor_bytes = entries.get(&anchor_name).ok_or_else(|| {
        policy_error(format!(
            "verified TUF cache is missing embedded root-history anchor {}",
            repository.root.version
        ))
    })?;
    let anchor = root_transition_high_water(anchor_bytes)?;
    if anchor.root != repository.root
        || anchor.root_authority != repository.root_authority
        || anchor.timestamp_authority != repository.timestamp_authority
        || anchor.snapshot_authority != repository.snapshot_authority
        || anchor.targets_authority != repository.targets_authority
    {
        return Err(policy_error(format!(
            "verified TUF root history forks from embedded bootstrap version {}",
            repository.root.version
        )));
    }

    let maximum_version = bootstrap_version.saturating_add(MAX_ROOT_TRANSITIONS);
    let accepted_final_version = final_version.min(maximum_version);
    let mut trusted_chain = sigstore_tuf::TrustedMetadataSet::from_root(bootstrap_root)
        .map_err(|error| policy_error(format!("invalid embedded TUF root: {error}")))?;
    if accepted_final_version > repository.root.version {
        let first = repository.root.version.checked_add(1).ok_or_else(|| {
            policy_error("embedded TUF root version cannot advance beyond u64::MAX")
        })?;
        for version in first..=accepted_final_version {
            let name = format!("root_history/{version}.root.json");
            let Some(bytes) = entries.get(&name) else {
                return Ok(AuthenticatedRootChain {
                    repository,
                    error: Some(policy_error(format!(
                        "verified TUF cache is missing authenticated root transition {version}"
                    ))),
                });
            };
            let transition = match root_transition_high_water(bytes) {
                Ok(transition) => transition,
                Err(error) => {
                    return Ok(AuthenticatedRootChain {
                        repository,
                        error: Some(error),
                    })
                }
            };
            if transition.root.version != version {
                return Ok(AuthenticatedRootChain {
                    repository,
                    error: Some(policy_error(format!(
                        "TUF root history entry {version} contains version {}",
                        transition.root.version
                    ))),
                });
            }
            if let Err(error) = trusted_chain.update_root(bytes) {
                return Ok(AuthenticatedRootChain {
                    repository,
                    error: Some(policy_error(format!(
                        "TUF root transition {version} is not authenticated by the preceding root: {error}"
                    ))),
                });
            }
            let merged = merge_repository_observation(Some(&repository), &transition);
            if let Some(error) = merged.error {
                return Ok(AuthenticatedRootChain {
                    repository,
                    error: Some(error),
                });
            }
            repository = merged.high_water;
        }
    }

    if final_version > maximum_version {
        return Ok(AuthenticatedRootChain {
            repository,
            error: Some(policy_error(format!(
                "TUF root chain exceeds the supported {MAX_ROOT_TRANSITIONS} transitions from bootstrap version {}",
                bootstrap_version
            ))),
        });
    }

    let expected = root_transition_high_water(final_root)?;
    if repository.root != expected.root
        || repository.root_authority != expected.root_authority
        || repository.timestamp_authority != expected.timestamp_authority
        || repository.snapshot_authority != expected.snapshot_authority
        || repository.targets_authority != expected.targets_authority
        || trusted_chain.root().version != final_version
    {
        return Err(policy_error(
            "authenticated TUF root history does not match the final trusted root",
        ));
    }
    Ok(AuthenticatedRootChain {
        repository,
        error: None,
    })
}

fn root_transition_high_water(root_bytes: &[u8]) -> Result<RepositoryHighWater> {
    let trusted = sigstore_tuf::TrustedMetadataSet::from_root(root_bytes)
        .map_err(|error| policy_error(format!("invalid authenticated TUF root: {error}")))?;
    let authorities = root_role_authorities(trusted.root())?;
    Ok(RepositoryHighWater {
        root: MetadataHighWater {
            version: trusted.root().version,
            sha256: signed_metadata_sha256("root", trusted.root_bytes())?,
            authority: None,
            referenced_authority: None,
        },
        authority_history: AuthorityHistory::from_authorities(
            &authorities.root,
            &authorities.timestamp,
            &authorities.snapshot,
            &authorities.targets,
        ),
        root_authority: authorities.root,
        timestamp_authority: authorities.timestamp,
        snapshot_authority: authorities.snapshot,
        targets_authority: authorities.targets,
        timestamp: None,
        snapshot_descriptor: None,
        snapshot: None,
        targets_descriptor: None,
        targets: None,
    })
}

fn enforce_high_water(
    candidate: &TrustedReleasePolicy,
    high_water: Option<&CacheHighWater>,
) -> Result<()> {
    enforce_channel_high_water(&CacheHighWater::from_policy(candidate), high_water)
}

fn enforce_channel_high_water(
    candidate: &CacheHighWater,
    high_water: Option<&CacheHighWater>,
) -> Result<()> {
    let Some(high_water) = high_water else {
        return Ok(());
    };
    if candidate.sequence < high_water.sequence {
        return Err(policy_error(format!(
            "channel sequence rollback: previously accepted {}, received {}",
            high_water.sequence, candidate.sequence
        )));
    }
    if candidate.sequence == high_water.sequence && candidate.policy_id != high_water.policy_id {
        return Err(policy_error(format!(
            "channel changed without incrementing sequence {}",
            candidate.sequence
        )));
    }
    Ok(())
}

fn enforce_repository_high_water(
    candidate: &RepositoryHighWater,
    high_water: Option<&RepositoryHighWater>,
) -> Result<()> {
    let Some(high_water) = high_water else {
        return Ok(());
    };
    enforce_metadata_high_water("root", Some(&candidate.root), Some(&high_water.root))?;
    let resets = authority_resets(high_water, candidate)?;
    if !resets.timestamp {
        enforce_metadata_high_water(
            "timestamp",
            candidate.timestamp.as_ref(),
            high_water.timestamp.as_ref(),
        )?;
    }
    if !resets.snapshot_descriptor {
        enforce_metadata_high_water(
            "snapshot descriptor",
            candidate.snapshot_descriptor.as_ref(),
            high_water.snapshot_descriptor.as_ref(),
        )?;
    }
    if !resets.snapshot {
        enforce_metadata_high_water(
            "snapshot",
            candidate.snapshot.as_ref(),
            high_water.snapshot.as_ref(),
        )?;
    }
    if !resets.targets_descriptor {
        enforce_metadata_high_water(
            "targets descriptor",
            candidate.targets_descriptor.as_ref(),
            high_water.targets_descriptor.as_ref(),
        )?;
    }
    if !resets.targets {
        enforce_metadata_high_water(
            "targets",
            candidate.targets.as_ref(),
            high_water.targets.as_ref(),
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy, Default)]
struct AuthorityResets {
    timestamp: bool,
    snapshot_descriptor: bool,
    snapshot: bool,
    targets_descriptor: bool,
    targets: bool,
}

fn authority_resets(
    prior: &RepositoryHighWater,
    candidate: &RepositoryHighWater,
) -> Result<AuthorityResets> {
    if candidate.root.version < prior.root.version {
        return Err(policy_error(format!(
            "TUF root metadata rollback: previously accepted version {}, received {}",
            prior.root.version, candidate.root.version
        )));
    }
    if candidate.root.version == prior.root.version {
        if candidate.root.sha256 != prior.root.sha256 {
            return Err(policy_error(format!(
                "TUF root metadata changed without incrementing version {}",
                candidate.root.version
            )));
        }
        if candidate.root_authority != prior.root_authority
            || candidate.timestamp_authority != prior.timestamp_authority
            || candidate.snapshot_authority != prior.snapshot_authority
            || candidate.targets_authority != prior.targets_authority
        {
            return Err(policy_error(
                "TUF role authority changed without a new root version",
            ));
        }
        return Ok(AuthorityResets::default());
    }

    // An authority descriptor changing is not sufficient to clear a rollback
    // floor. During an overlap rotation, old keys may remain authorized by the
    // new root and can replay the very metadata that established that floor.
    // Reset only when the old key set cannot satisfy the candidate threshold.
    let timestamp_changed = role_floor_is_safely_replaced(
        "timestamp",
        prior.timestamp.as_ref(),
        &candidate.timestamp_authority,
    )?;
    let snapshot_descriptor_changed = descriptor_floor_is_safely_replaced(
        "snapshot descriptor",
        prior.snapshot_descriptor.as_ref(),
        &candidate.timestamp_authority,
        &candidate.snapshot_authority,
    )?;
    let snapshot_changed = role_floor_is_safely_replaced(
        "snapshot",
        prior.snapshot.as_ref(),
        &candidate.snapshot_authority,
    )?;
    let targets_descriptor_changed = descriptor_floor_is_safely_replaced(
        "targets descriptor",
        prior.targets_descriptor.as_ref(),
        &candidate.snapshot_authority,
        &candidate.targets_authority,
    )?;
    let targets_changed = role_floor_is_safely_replaced(
        "targets",
        prior.targets.as_ref(),
        &candidate.targets_authority,
    )?;
    Ok(AuthorityResets {
        timestamp: timestamp_changed,
        snapshot_descriptor: snapshot_descriptor_changed,
        snapshot: snapshot_changed,
        targets_descriptor: targets_descriptor_changed,
        targets: targets_changed,
    })
}

fn role_floor_is_safely_replaced(
    role: &str,
    floor: Option<&MetadataHighWater>,
    candidate: &RoleAuthority,
) -> Result<bool> {
    let Some(floor) = floor else {
        return Ok(false);
    };
    let authority = floor.authority.as_ref().ok_or_else(|| {
        policy_error(format!(
            "TUF {role} metadata floor is missing authority provenance"
        ))
    })?;
    Ok(safely_replaces_authority(authority, candidate))
}

fn descriptor_floor_is_safely_replaced(
    role: &str,
    floor: Option<&MetadataHighWater>,
    candidate_parent: &RoleAuthority,
    candidate_child: &RoleAuthority,
) -> Result<bool> {
    let Some(floor) = floor else {
        return Ok(false);
    };
    let parent = floor.authority.as_ref().ok_or_else(|| {
        policy_error(format!(
            "TUF {role} floor is missing asserting-authority provenance"
        ))
    })?;
    let child = floor.referenced_authority.as_ref().ok_or_else(|| {
        policy_error(format!(
            "TUF {role} floor is missing referenced-authority provenance"
        ))
    })?;
    Ok(safely_replaces_authority(parent, candidate_parent)
        || safely_replaces_authority(child, candidate_child))
}

fn safely_replaces_authority(prior: &AuthorityProvenance, candidate: &RoleAuthority) -> bool {
    let candidate_keys = candidate
        .key_fingerprints
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let overlap = prior
        .key_fingerprints
        .iter()
        .filter(|key| candidate_keys.contains(key.as_str()))
        .count();
    overlap < candidate.threshold
}

fn union_authority_provenance(
    prior: &AuthorityProvenance,
    candidate: &RoleAuthority,
) -> Result<AuthorityProvenance> {
    let mut key_fingerprints = prior.key_fingerprints.clone();
    key_fingerprints.extend(candidate.key_fingerprints.iter().cloned());
    key_fingerprints.sort();
    key_fingerprints.dedup();
    let provenance = AuthorityProvenance { key_fingerprints };
    validate_authority_provenance("cumulative", &provenance)?;
    Ok(provenance)
}

fn merge_authority_provenance(
    left: &AuthorityProvenance,
    right: &AuthorityProvenance,
) -> Result<AuthorityProvenance> {
    let mut key_fingerprints = left.key_fingerprints.clone();
    key_fingerprints.extend(right.key_fingerprints.iter().cloned());
    key_fingerprints.sort();
    key_fingerprints.dedup();
    let provenance = AuthorityProvenance { key_fingerprints };
    validate_authority_provenance("merged", &provenance)?;
    Ok(provenance)
}

fn merge_authority_histories(
    left: &AuthorityHistory,
    right: &AuthorityHistory,
) -> Result<AuthorityHistory> {
    let history = AuthorityHistory {
        root: merge_authority_key_history("root", &left.root, &right.root)?,
        timestamp: merge_authority_key_history("timestamp", &left.timestamp, &right.timestamp)?,
        snapshot: merge_authority_key_history("snapshot", &left.snapshot, &right.snapshot)?,
        targets: merge_authority_key_history("targets", &left.targets, &right.targets)?,
    };
    validate_authority_custody_classes(&history)?;
    Ok(history)
}

fn advance_authority_history(
    prior: &RepositoryHighWater,
    observed: &RepositoryHighWater,
) -> Result<AuthorityHistory> {
    let prior_global = prior
        .authority_history
        .timestamp
        .iter()
        .chain(&prior.authority_history.snapshot)
        .chain(&prior.authority_history.targets)
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let history = AuthorityHistory {
        root: merge_authority_key_history(
            "root",
            &prior.authority_history.root,
            &observed.authority_history.root,
        )?,
        timestamp: advance_role_key_history(
            "timestamp",
            &prior.timestamp_authority,
            &prior.authority_history.timestamp,
            &prior_global,
            &observed.timestamp_authority,
            &observed.authority_history.timestamp,
        )?,
        snapshot: advance_role_key_history(
            "snapshot",
            &prior.snapshot_authority,
            &prior.authority_history.snapshot,
            &prior_global,
            &observed.snapshot_authority,
            &observed.authority_history.snapshot,
        )?,
        targets: advance_role_key_history(
            "targets",
            &prior.targets_authority,
            &prior.authority_history.targets,
            &prior_global,
            &observed.targets_authority,
            &observed.authority_history.targets,
        )?,
    };
    validate_authority_custody_classes(&history)?;
    Ok(history)
}

fn validate_authority_custody_classes(history: &AuthorityHistory) -> Result<()> {
    let root = history
        .root
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let online = history
        .timestamp
        .iter()
        .chain(&history.snapshot)
        .chain(&history.targets)
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if let Some(fingerprint) = root.intersection(&online).next() {
        return Err(policy_error(format!(
            "TUF key custody class violation: root and online authority histories share key material {fingerprint}"
        )));
    }
    Ok(())
}

fn advance_role_key_history(
    role: &str,
    prior_authority: &RoleAuthority,
    prior_history: &[String],
    prior_global_history: &HashSet<&str>,
    observed_authority: &RoleAuthority,
    observed_history: &[String],
) -> Result<Vec<String>> {
    let prior_current = prior_authority
        .key_fingerprints
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if let Some(reintroduced) = observed_authority.key_fingerprints.iter().find(|key| {
        prior_global_history.contains(key.as_str()) && !prior_current.contains(key.as_str())
    }) {
        return Err(policy_error(format!(
            "TUF {role} authority reauthorizes retired key material {reintroduced}"
        )));
    }
    merge_authority_key_history(role, prior_history, observed_history)
}

fn merge_authority_key_history(
    role: &str,
    left: &[String],
    right: &[String],
) -> Result<Vec<String>> {
    let mut history = left.to_vec();
    history.extend(right.iter().cloned());
    history.sort();
    history.dedup();
    validate_authority_key_history(role, &history)?;
    Ok(history)
}

fn validate_authority_key_history(role: &str, history: &[String]) -> Result<()> {
    if history.is_empty() || history.len() > MAX_AUTHORITY_KEYS {
        return Err(policy_error(format!(
            "TUF {role} authority history must contain between 1 and {MAX_AUTHORITY_KEYS} keys"
        )));
    }
    let mut prior = None;
    for fingerprint in history {
        validate_hex(
            &format!("TUF {role} authority history fingerprint"),
            fingerprint,
            SHA256_HEX_LEN,
        )?;
        if prior.is_some_and(|prior: &String| prior >= fingerprint) {
            return Err(policy_error(format!(
                "TUF {role} authority history fingerprints must be unique and sorted"
            )));
        }
        prior = Some(fingerprint);
    }
    Ok(())
}

fn merge_repository_high_waters(
    left: Option<&RepositoryHighWater>,
    right: Option<&RepositoryHighWater>,
) -> Result<Option<RepositoryHighWater>> {
    match (left, right) {
        (None, None) => Ok(None),
        (Some(value), None) | (None, Some(value)) => Ok(Some(value.clone())),
        (Some(left), Some(right)) if left.root.version == right.root.version => {
            enforce_metadata_high_water("root", Some(&left.root), Some(&right.root))?;
            enforce_metadata_high_water("root", Some(&right.root), Some(&left.root))?;
            authority_resets(left, right)?;
            Ok(Some(RepositoryHighWater {
                root: left.root.clone(),
                root_authority: left.root_authority.clone(),
                timestamp_authority: left.timestamp_authority.clone(),
                snapshot_authority: left.snapshot_authority.clone(),
                targets_authority: left.targets_authority.clone(),
                authority_history: merge_authority_histories(
                    &left.authority_history,
                    &right.authority_history,
                )?,
                timestamp: merge_metadata_high_waters(
                    "timestamp",
                    left.timestamp.as_ref(),
                    right.timestamp.as_ref(),
                )?,
                snapshot_descriptor: merge_metadata_high_waters(
                    "snapshot descriptor",
                    left.snapshot_descriptor.as_ref(),
                    right.snapshot_descriptor.as_ref(),
                )?,
                snapshot: merge_metadata_high_waters(
                    "snapshot",
                    left.snapshot.as_ref(),
                    right.snapshot.as_ref(),
                )?,
                targets_descriptor: merge_metadata_high_waters(
                    "targets descriptor",
                    left.targets_descriptor.as_ref(),
                    right.targets_descriptor.as_ref(),
                )?,
                targets: merge_metadata_high_waters(
                    "targets",
                    left.targets.as_ref(),
                    right.targets.as_ref(),
                )?,
            }))
        }
        (Some(left), Some(right)) => {
            let (prior, observed) = if left.root.version < right.root.version {
                (left, right)
            } else {
                (right, left)
            };
            let merged = merge_repository_observation(Some(prior), observed);
            if let Some(error) = merged.error {
                return Err(error);
            }
            Ok(Some(merged.high_water))
        }
    }
}

fn merge_metadata_high_waters(
    role: &str,
    left: Option<&MetadataHighWater>,
    right: Option<&MetadataHighWater>,
) -> Result<Option<MetadataHighWater>> {
    match (left, right) {
        (None, None) => Ok(None),
        (Some(value), None) | (None, Some(value)) => Ok(Some(value.clone())),
        (Some(left), Some(right)) if left.version == right.version => {
            if left.sha256 != right.sha256 {
                return Err(policy_error(format!(
                    "conflicting TUF {role} metadata hashes exist at high-water version {}",
                    left.version
                )));
            }
            Ok(Some(MetadataHighWater {
                version: left.version,
                sha256: left.sha256.clone(),
                authority: merge_optional_authority_provenance(
                    role,
                    "asserting",
                    left.authority.as_ref(),
                    right.authority.as_ref(),
                )?,
                referenced_authority: merge_optional_authority_provenance(
                    role,
                    "referenced",
                    left.referenced_authority.as_ref(),
                    right.referenced_authority.as_ref(),
                )?,
            }))
        }
        (Some(left), Some(right)) => Ok(Some(if left.version > right.version {
            left.clone()
        } else {
            right.clone()
        })),
    }
}

fn merge_optional_authority_provenance(
    role: &str,
    kind: &str,
    left: Option<&AuthorityProvenance>,
    right: Option<&AuthorityProvenance>,
) -> Result<Option<AuthorityProvenance>> {
    match (left, right) {
        (None, None) => Ok(None),
        (Some(left), Some(right)) => Ok(Some(merge_authority_provenance(left, right)?)),
        _ => Err(policy_error(format!(
            "conflicting TUF {role} {kind}-authority provenance exists at the same high-water version"
        ))),
    }
}

struct RepositoryObservationMerge {
    high_water: RepositoryHighWater,
    error: Option<Error>,
    accepted_through_targets: bool,
}

fn merge_repository_observation(
    prior: Option<&RepositoryHighWater>,
    observed: &RepositoryHighWater,
) -> RepositoryObservationMerge {
    let Some(prior) = prior else {
        return RepositoryObservationMerge {
            high_water: observed.clone(),
            error: None,
            accepted_through_targets: observed.timestamp.is_some()
                && observed.snapshot_descriptor.is_some()
                && observed.snapshot.is_some()
                && observed.targets_descriptor.is_some()
                && observed.targets.is_some(),
        };
    };
    if let Err(error) = enforce_metadata_high_water("root", Some(&observed.root), Some(&prior.root))
    {
        // Child roles authenticated under an obsolete or equivocated root are
        // not safe observations. Retain the entire prior floor so an attacker
        // with old online-role keys cannot fast-forward child versions and
        // permanently block recovery.
        return RepositoryObservationMerge {
            high_water: prior.clone(),
            error: Some(error),
            accepted_through_targets: false,
        };
    }
    let resets = match authority_resets(prior, observed) {
        Ok(resets) => resets,
        Err(error) => {
            return RepositoryObservationMerge {
                high_water: prior.clone(),
                error: Some(error),
                accepted_through_targets: false,
            }
        }
    };
    let authority_history = match advance_authority_history(prior, observed) {
        Ok(history) => history,
        Err(error) => {
            return RepositoryObservationMerge {
                high_water: prior.clone(),
                error: Some(error),
                accepted_through_targets: false,
            }
        }
    };
    let baseline = (|| -> Result<RepositoryHighWater> {
        Ok(RepositoryHighWater {
            root: observed.root.clone(),
            root_authority: observed.root_authority.clone(),
            timestamp_authority: observed.timestamp_authority.clone(),
            snapshot_authority: observed.snapshot_authority.clone(),
            targets_authority: observed.targets_authority.clone(),
            authority_history: authority_history.clone(),
            timestamp: if resets.timestamp {
                None
            } else {
                taint_role_floor(prior.timestamp.as_ref(), &observed.timestamp_authority)?
            },
            snapshot_descriptor: if resets.snapshot_descriptor {
                None
            } else {
                taint_descriptor_floor(
                    prior.snapshot_descriptor.as_ref(),
                    &observed.timestamp_authority,
                    &observed.snapshot_authority,
                )?
            },
            snapshot: if resets.snapshot {
                None
            } else {
                taint_role_floor(prior.snapshot.as_ref(), &observed.snapshot_authority)?
            },
            targets_descriptor: if resets.targets_descriptor {
                None
            } else {
                taint_descriptor_floor(
                    prior.targets_descriptor.as_ref(),
                    &observed.snapshot_authority,
                    &observed.targets_authority,
                )?
            },
            targets: if resets.targets {
                None
            } else {
                taint_role_floor(prior.targets.as_ref(), &observed.targets_authority)?
            },
        })
    })();
    let baseline = match baseline {
        Ok(baseline) => baseline,
        Err(error) => {
            return RepositoryObservationMerge {
                high_water: prior.clone(),
                error: Some(error),
                accepted_through_targets: false,
            }
        }
    };
    let mut error = None;
    let (timestamp, timestamp_accepted) = merge_metadata_observation_chain_link(
        "timestamp",
        baseline.timestamp.as_ref(),
        observed.timestamp.as_ref(),
        &mut error,
    );
    let (snapshot_descriptor, snapshot_descriptor_accepted) =
        merge_metadata_observation_chain_link_if_parent(
            timestamp_accepted,
            "snapshot descriptor",
            baseline.snapshot_descriptor.as_ref(),
            observed.snapshot_descriptor.as_ref(),
            &mut error,
        );
    let (snapshot, snapshot_accepted) = merge_metadata_observation_chain_link_if_parent(
        snapshot_descriptor_accepted,
        "snapshot",
        baseline.snapshot.as_ref(),
        observed.snapshot.as_ref(),
        &mut error,
    );
    let (targets_descriptor, targets_descriptor_accepted) =
        merge_metadata_observation_chain_link_if_parent(
            snapshot_accepted,
            "targets descriptor",
            baseline.targets_descriptor.as_ref(),
            observed.targets_descriptor.as_ref(),
            &mut error,
        );
    let (targets, targets_accepted) = merge_metadata_observation_chain_link_if_parent(
        targets_descriptor_accepted,
        "targets",
        baseline.targets.as_ref(),
        observed.targets.as_ref(),
        &mut error,
    );
    RepositoryObservationMerge {
        high_water: RepositoryHighWater {
            root: observed.root.clone(),
            root_authority: observed.root_authority.clone(),
            timestamp_authority: observed.timestamp_authority.clone(),
            snapshot_authority: observed.snapshot_authority.clone(),
            targets_authority: observed.targets_authority.clone(),
            authority_history,
            timestamp,
            snapshot_descriptor,
            snapshot,
            targets_descriptor,
            targets,
        },
        error,
        accepted_through_targets: targets_accepted,
    }
}

fn taint_role_floor(
    floor: Option<&MetadataHighWater>,
    authority: &RoleAuthority,
) -> Result<Option<MetadataHighWater>> {
    floor
        .cloned()
        .map(|mut floor| {
            floor.authority = floor
                .authority
                .as_ref()
                .map(|prior| union_authority_provenance(prior, authority))
                .transpose()?;
            Ok(floor)
        })
        .transpose()
}

fn taint_descriptor_floor(
    floor: Option<&MetadataHighWater>,
    authority: &RoleAuthority,
    referenced_authority: &RoleAuthority,
) -> Result<Option<MetadataHighWater>> {
    floor
        .cloned()
        .map(|mut floor| {
            floor.authority = floor
                .authority
                .as_ref()
                .map(|prior| union_authority_provenance(prior, authority))
                .transpose()?;
            floor.referenced_authority = floor
                .referenced_authority
                .as_ref()
                .map(|prior| union_authority_provenance(prior, referenced_authority))
                .transpose()?;
            Ok(floor)
        })
        .transpose()
}

fn merge_metadata_observation_chain_link_if_parent(
    parent_accepted: bool,
    role: &str,
    prior: Option<&MetadataHighWater>,
    observed: Option<&MetadataHighWater>,
    error: &mut Option<Error>,
) -> (Option<MetadataHighWater>, bool) {
    if !parent_accepted {
        return (prior.cloned(), false);
    }
    merge_metadata_observation_chain_link(role, prior, observed, error)
}

fn merge_metadata_observation_chain_link(
    role: &str,
    prior: Option<&MetadataHighWater>,
    observed: Option<&MetadataHighWater>,
    error: &mut Option<Error>,
) -> (Option<MetadataHighWater>, bool) {
    match (prior, observed) {
        (None, None) => (None, false),
        (Some(prior), None) => (Some(prior.clone()), false),
        (None, Some(observed)) => (Some(observed.clone()), true),
        (Some(prior), Some(observed)) => {
            match enforce_metadata_high_water(role, Some(observed), Some(prior)) {
                Ok(()) if observed.version == prior.version => {
                    // The baseline was already widened to every authority
                    // that can authenticate the same detachable-signature
                    // payload in the new root epoch. Preserve that conservative
                    // union for an equal semantic payload.
                    (Some(prior.clone()), true)
                }
                Ok(()) => (Some(observed.clone()), true),
                Err(merge_error) => {
                    error.get_or_insert(merge_error);
                    (Some(prior.clone()), false)
                }
            }
        }
    }
}

fn enforce_metadata_high_water(
    role: &str,
    candidate: Option<&MetadataHighWater>,
    prior: Option<&MetadataHighWater>,
) -> Result<()> {
    match (candidate, prior) {
        (_, None) => Ok(()),
        (None, Some(prior)) => Err(policy_error(format!(
            "TUF {role} metadata rollback: previously accepted version {} is missing",
            prior.version
        ))),
        (Some(candidate), Some(prior)) if candidate.version < prior.version => {
            Err(policy_error(format!(
                "TUF {role} metadata rollback: previously accepted version {}, received {}",
                prior.version, candidate.version
            )))
        }
        (Some(candidate), Some(prior))
            if candidate.version == prior.version && candidate.sha256 != prior.sha256 =>
        {
            Err(policy_error(format!(
                "TUF {role} metadata changed without incrementing version {}",
                candidate.version
            )))
        }
        (Some(_), Some(_)) => Ok(()),
    }
}

fn validate_repository_high_water(high_water: &RepositoryHighWater) -> Result<()> {
    validate_metadata_high_water("root", &high_water.root)?;
    if high_water.root.authority.is_some() || high_water.root.referenced_authority.is_some() {
        return Err(policy_error(
            "TUF root high-water mark must not carry online-role authority provenance",
        ));
    }
    validate_role_authority("root", &high_water.root_authority)?;
    validate_role_authority("timestamp", &high_water.timestamp_authority)?;
    validate_role_authority("snapshot", &high_water.snapshot_authority)?;
    validate_role_authority("targets", &high_water.targets_authority)?;
    for (role, current, history) in [
        (
            "root",
            &high_water.root_authority,
            high_water.authority_history.root.as_slice(),
        ),
        (
            "timestamp",
            &high_water.timestamp_authority,
            high_water.authority_history.timestamp.as_slice(),
        ),
        (
            "snapshot",
            &high_water.snapshot_authority,
            high_water.authority_history.snapshot.as_slice(),
        ),
        (
            "targets",
            &high_water.targets_authority,
            high_water.authority_history.targets.as_slice(),
        ),
    ] {
        validate_authority_key_history(role, history)?;
        if !current
            .key_fingerprints
            .iter()
            .all(|key| history.binary_search(key).is_ok())
        {
            return Err(policy_error(format!(
                "TUF {role} authority history does not contain the current authority"
            )));
        }
    }
    validate_authority_custody_classes(&high_water.authority_history)?;
    for (role, mark, current) in [
        (
            "timestamp",
            high_water.timestamp.as_ref(),
            &high_water.timestamp_authority,
        ),
        (
            "snapshot",
            high_water.snapshot.as_ref(),
            &high_water.snapshot_authority,
        ),
        (
            "targets",
            high_water.targets.as_ref(),
            &high_water.targets_authority,
        ),
    ] {
        let Some(mark) = mark else {
            continue;
        };
        validate_metadata_high_water(role, mark)?;
        let authority = mark.authority.as_ref().ok_or_else(|| {
            policy_error(format!(
                "TUF {role} high-water mark is missing authority provenance"
            ))
        })?;
        validate_authority_provenance(role, authority)?;
        validate_provenance_covers_authority(role, authority, current)?;
        if mark.referenced_authority.is_some() {
            return Err(policy_error(format!(
                "TUF {role} envelope high-water mark must not carry referenced-authority provenance"
            )));
        }
    }
    for (role, mark, parent, child) in [
        (
            "snapshot descriptor",
            high_water.snapshot_descriptor.as_ref(),
            &high_water.timestamp_authority,
            &high_water.snapshot_authority,
        ),
        (
            "targets descriptor",
            high_water.targets_descriptor.as_ref(),
            &high_water.snapshot_authority,
            &high_water.targets_authority,
        ),
    ] {
        let Some(mark) = mark else {
            continue;
        };
        validate_metadata_high_water(role, mark)?;
        let authority = mark.authority.as_ref().ok_or_else(|| {
            policy_error(format!(
                "TUF {role} high-water mark is missing asserting-authority provenance"
            ))
        })?;
        let referenced_authority = mark.referenced_authority.as_ref().ok_or_else(|| {
            policy_error(format!(
                "TUF {role} high-water mark is missing referenced-authority provenance"
            ))
        })?;
        validate_authority_provenance(role, authority)?;
        validate_authority_provenance(&format!("{role} referenced role"), referenced_authority)?;
        validate_provenance_covers_authority(role, authority, parent)?;
        validate_provenance_covers_authority(
            &format!("{role} referenced role"),
            referenced_authority,
            child,
        )?;
    }
    Ok(())
}

fn validate_provenance_covers_authority(
    role: &str,
    provenance: &AuthorityProvenance,
    current: &RoleAuthority,
) -> Result<()> {
    if current
        .key_fingerprints
        .iter()
        .all(|key| provenance.key_fingerprints.binary_search(key).is_ok())
    {
        Ok(())
    } else {
        Err(policy_error(format!(
            "TUF {role} floor provenance does not cover the current authority"
        )))
    }
}

fn validate_role_authority(role: &str, authority: &RoleAuthority) -> Result<()> {
    if authority.threshold == 0
        || authority.threshold > authority.key_fingerprints.len()
        || authority.key_fingerprints.len() > MAX_AUTHORITY_KEYS
    {
        return Err(policy_error(format!(
            "TUF {role} authority threshold is invalid"
        )));
    }
    let mut prior = None;
    for fingerprint in &authority.key_fingerprints {
        validate_hex(
            &format!("TUF {role} authority key fingerprint"),
            fingerprint,
            SHA256_HEX_LEN,
        )?;
        if prior.is_some_and(|prior: &String| prior >= fingerprint) {
            return Err(policy_error(format!(
                "TUF {role} authority key fingerprints must be unique and sorted"
            )));
        }
        prior = Some(fingerprint);
    }
    Ok(())
}

fn validate_authority_provenance(role: &str, authority: &AuthorityProvenance) -> Result<()> {
    if authority.key_fingerprints.is_empty()
        || authority.key_fingerprints.len() > MAX_AUTHORITY_KEYS
    {
        return Err(policy_error(format!(
            "TUF {role} authority provenance must contain between 1 and {MAX_AUTHORITY_KEYS} keys"
        )));
    }
    let mut prior = None;
    for fingerprint in &authority.key_fingerprints {
        validate_hex(
            &format!("TUF {role} authority key fingerprint"),
            fingerprint,
            SHA256_HEX_LEN,
        )?;
        if prior.is_some_and(|prior: &String| prior >= fingerprint) {
            return Err(policy_error(format!(
                "TUF {role} authority key fingerprints must be unique and sorted"
            )));
        }
        prior = Some(fingerprint);
    }
    Ok(())
}

fn validate_metadata_high_water(role: &str, mark: &MetadataHighWater) -> Result<()> {
    if mark.version == 0 {
        return Err(policy_error(format!(
            "TUF {role} high-water version must be greater than zero"
        )));
    }
    validate_hex(
        &format!("TUF {role} high-water SHA-256"),
        &mark.sha256,
        SHA256_HEX_LEN,
    )?;
    Ok(())
}

fn repository_id(repository_url: &str) -> String {
    sha256_hex(repository_url.as_bytes())
}

fn repository_memory_state(config: &TrustedReleaseConfig) -> Arc<RepositoryMemoryState> {
    let Some(cache_path) = &config.cache_path else {
        // Explicitly non-persistent managers do not claim shared rollback
        // protection and therefore do not retain a process-global registry
        // entry indefinitely.
        return Arc::new(RepositoryMemoryState::default());
    };
    let path = std::path::absolute(cache_path).unwrap_or_else(|_| cache_path.clone());
    let key = format!(
        "{}:{}",
        repository_id(&config.repository_url),
        path.to_string_lossy()
    );
    let registry = REPOSITORY_MEMORY_STATES.get_or_init(|| StdMutex::new(BTreeMap::new()));
    let mut registry = registry
        .lock()
        .expect("repository memory-state registry mutex poisoned");
    Arc::clone(
        registry
            .entry(key)
            .or_insert_with(|| Arc::new(RepositoryMemoryState::default())),
    )
}

fn default_cache_path(repository_url: &str) -> Result<PathBuf> {
    let repository_id = repository_id(repository_url);
    let file = format!("tuf-{}.json", &repository_id[..16]);
    directories::ProjectDirs::from("ai", "Maple", "OpenSecret")
        .map(|directories| {
            directories
                .data_local_dir()
                .join("attestations")
                .join(&file)
        })
        .ok_or_else(|| {
            policy_error(
                "no durable application-data directory is available for attestation rollback state; mobile hosts must use TrustedReleaseManager::official_with_cache_path or TrustedReleaseConfig::new_with_cache_path",
            )
        })
}

fn lock_cache(path: &Path) -> Result<File> {
    let parent = cache_parent(path)?;
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| policy_error("cache path has no UTF-8 file name"))?;
    let lock_path = parent.join(format!(".{file_name}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)?;
    file.lock_exclusive()?;
    Ok(file)
}

fn read_cache(path: &Path, expected_repository_id: &str) -> Result<CachedRepository> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CachedRepository::default())
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.len() > MAX_CACHE_BYTES {
        return Err(policy_error("attestation policy cache exceeds size limit"));
    }
    let raw = std::fs::read(path)?;
    let cache: CacheFile = serde_json::from_slice(&raw)?;
    if cache.schema != CACHE_SCHEMA || cache.repository_id != expected_repository_id {
        return Err(policy_error("attestation policy cache identity mismatch"));
    }
    validate_hex(
        "attestation policy cache repositoryId",
        &cache.repository_id,
        SHA256_HEX_LEN,
    )?;
    validate_repository_high_water(&cache.repository_high_water)?;
    if cache.channel_high_water.len() > 2 {
        return Err(policy_error(
            "attestation policy cache may have at most two channel high-water marks",
        ));
    }
    for high_water in cache.channel_high_water.values() {
        validate_channel_high_water(high_water)?;
        validate_provenance_covers_authority(
            "channel targets",
            &high_water.authority,
            &cache.repository_high_water.targets_authority,
        )?;
    }
    if cache.entries.len() > MAX_CACHE_ENTRIES {
        return Err(policy_error(
            "attestation policy cache has too many entries",
        ));
    }
    let mut total = 0u64;
    let mut decoded = BTreeMap::new();
    for (name, encoded) in cache.entries {
        validate_store_name(&name)
            .map_err(|error| policy_error(format!("invalid cache entry: {error}")))?;
        let bytes = BASE64
            .decode(encoded)
            .map_err(|error| policy_error(format!("invalid cache encoding: {error}")))?;
        total = total.saturating_add(bytes.len() as u64);
        if total > MAX_CACHE_BYTES {
            return Err(policy_error(
                "attestation policy cache exceeds decoded size limit",
            ));
        }
        decoded.insert(name, bytes);
    }
    Ok(CachedRepository {
        repository_high_water: Some(cache.repository_high_water),
        channel_high_water: cache.channel_high_water,
        entries: decoded,
    })
}

#[allow(clippy::too_many_arguments)]
async fn persist_cache_while_locked(
    cache_guard: &mut Option<File>,
    path: PathBuf,
    repository_id: String,
    repository_high_water: RepositoryHighWater,
    channel_high_water: BTreeMap<AttestationEnvironment, CacheHighWater>,
    entries: BTreeMap<String, Vec<u8>>,
    task_name: &'static str,
) -> Result<()> {
    run_blocking_with_cache_lock(cache_guard, task_name, move || {
        persist_cache(
            &path,
            &repository_id,
            &repository_high_water,
            &channel_high_water,
            &entries,
        )
    })
    .await
}

async fn run_blocking_with_cache_lock<T, F>(
    cache_guard: &mut Option<File>,
    task_name: &'static str,
    operation: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let held_guard = cache_guard.take().ok_or_else(|| {
        policy_error(format!(
            "{task_name} persistence attempted without the cache lock"
        ))
    })?;
    let task = tokio::task::spawn_blocking(move || {
        let result = operation();
        // Return the guard to the caller only after the atomic write finishes.
        // If the async caller is cancelled, Tokio drops this output after the
        // blocking task completes, so the lock still outlives the write.
        (result, held_guard)
    })
    .await
    .map_err(|error| policy_error(format!("{task_name} task failed: {error}")))?;
    let (result, held_guard) = task;
    *cache_guard = Some(held_guard);
    result.map_err(|error| policy_error(format!("{task_name} persistence failed: {error}")))
}

fn persist_cache(
    path: &Path,
    repository_id: &str,
    repository_high_water: &RepositoryHighWater,
    channel_high_water: &BTreeMap<AttestationEnvironment, CacheHighWater>,
    entries: &BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    validate_hex(
        "attestation policy cache repositoryId",
        repository_id,
        SHA256_HEX_LEN,
    )?;
    validate_repository_high_water(repository_high_water)?;
    if channel_high_water.len() > 2 {
        return Err(policy_error(
            "refusing to persist invalid channel high-water marks",
        ));
    }
    for high_water in channel_high_water.values() {
        validate_channel_high_water(high_water)?;
        validate_provenance_covers_authority(
            "channel targets",
            &high_water.authority,
            &repository_high_water.targets_authority,
        )?;
    }
    if entries.len() > MAX_CACHE_ENTRIES {
        return Err(policy_error("refusing to persist oversized policy cache"));
    }
    let encoded = entries
        .iter()
        .map(|(name, bytes)| (name.clone(), BASE64.encode(bytes)))
        .collect();
    let cache = CacheFile {
        schema: CACHE_SCHEMA.to_string(),
        repository_id: repository_id.to_string(),
        repository_high_water: repository_high_water.clone(),
        channel_high_water: channel_high_water.clone(),
        entries: encoded,
    };
    let bytes = serde_json::to_vec(&cache)?;
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return Err(policy_error("refusing to persist oversized policy cache"));
    }
    let parent = cache_parent(path)?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    let persisted = temporary
        .persist(path)
        .map_err(|error| Error::Io(error.error))?;
    persisted.sync_all()?;
    sync_parent_directory(parent)?;
    Ok(())
}

fn validate_channel_high_water(high_water: &CacheHighWater) -> Result<()> {
    if high_water.sequence == 0 {
        return Err(policy_error(
            "attestation policy cache has an invalid channel sequence",
        ));
    }
    validate_hex(
        "attestation policy cache policyId",
        &high_water.policy_id,
        SHA256_HEX_LEN,
    )?;
    validate_authority_provenance("channel targets", &high_water.authority)
}

fn cache_parent(path: &Path) -> Result<&Path> {
    let parent = path
        .parent()
        .ok_or_else(|| policy_error("cache path has no parent directory"))?;
    // `Path::parent` returns an empty path for a bare relative filename. Use
    // the current directory explicitly so temporary-file creation and the
    // durability fsync target the same real directory as the final rename.
    Ok(if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    })
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> Result<()> {
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) -> Result<()> {
    Ok(())
}

fn policy_error(message: impl Into<String>) -> Error {
    Error::TrustedReleasePolicy(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use sigstore_verify::crypto::KeyPair;
    use std::{
        collections::{BTreeSet, HashMap},
        sync::atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::Barrier;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn document(pcr0: u8, pcr1: u8, pcr2: u8) -> AttestationDocument {
        AttestationDocument {
            module_id: "test".to_string(),
            digest: "SHA384".to_string(),
            timestamp: 0,
            pcrs: HashMap::from([
                (0, vec![pcr0; 48]),
                (1, vec![pcr1; 48]),
                (2, vec![pcr2; 48]),
            ]),
            certificate: Vec::new(),
            cabundle: Vec::new(),
            public_key: None,
            user_data: None,
            nonce: None,
        }
    }

    #[derive(Clone, Default)]
    struct MemoryRepository {
        metadata: HashMap<String, Vec<u8>>,
        targets: HashMap<String, Vec<u8>>,
    }

    impl Repository for MemoryRepository {
        fn fetch_metadata<'a>(&'a self, name: &'a str, max_length: u64) -> FetchFuture<'a> {
            let result = match self.metadata.get(name) {
                Some(bytes) if bytes.len() as u64 > max_length => Err(
                    sigstore_tuf::Error::Transport("metadata too large".to_string()),
                ),
                bytes => Ok(bytes.cloned()),
            };
            Box::pin(async move { result })
        }

        fn fetch_target<'a>(&'a self, path: &'a str, max_length: u64) -> FetchFuture<'a> {
            let result = match self.targets.get(path) {
                Some(bytes) if bytes.len() as u64 > max_length => Err(
                    sigstore_tuf::Error::Transport("target too large".to_string()),
                ),
                bytes => Ok(bytes.cloned()),
            };
            Box::pin(async move { result })
        }
    }

    struct FixtureBundleVerifier {
        fail: bool,
    }

    impl BundleVerifier for FixtureBundleVerifier {
        fn verify(
            &self,
            manifest_bytes: &[u8],
            bundle_bytes: &[u8],
            trusted_root_bytes: &[u8],
        ) -> Result<()> {
            if self.fail {
                return Err(policy_error("fixture bundle rejected"));
            }
            assert!(std::str::from_utf8(manifest_bytes)
                .unwrap()
                .contains("opensecret-backend"));
            assert_eq!(bundle_bytes, b"fixture-bundle");
            assert_eq!(trusted_root_bytes, b"fixture-trusted-root");
            Ok(())
        }
    }

    fn tuf_key_entry(key_pair: &KeyPair) -> (String, Value) {
        let public = key_pair.public_key_der().unwrap().to_pem();
        let value = json!({
            "keytype": "ecdsa",
            "scheme": "ecdsa-sha2-nistp256",
            "keyval": { "public": public },
        });
        let key: sigstore_tuf::Key = serde_json::from_value(value.clone()).unwrap();
        (key.key_id().unwrap(), value)
    }

    fn tuf_key_fingerprint(key: &Value) -> String {
        let parsed: sigstore_tuf::Key = serde_json::from_value(key.clone()).unwrap();
        let verification = parsed.verification_key().unwrap();
        key_custody_fingerprint(&parsed.scheme, verification.as_bytes()).unwrap()
    }

    fn tuf_signature(signed: &Value, key_id: &str, key_pair: &KeyPair) -> Value {
        let canonical = sigstore_tuf::canonical_json::to_canonical_bytes(signed).unwrap();
        let signature = key_pair.sign(&canonical).unwrap();
        json!({ "keyid": key_id, "sig": hex::encode(signature.as_bytes()) })
    }

    fn tuf_envelope(signed: Value, key_id: &str, key_pair: &KeyPair) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "signatures": [tuf_signature(&signed, key_id, key_pair)],
            "signed": signed,
        }))
        .unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn tuf_root(
        version: u64,
        root_key_id: &str,
        root_key: &Value,
        root_key_pair: &KeyPair,
        online_key_id: &str,
        online_key: &Value,
    ) -> Vec<u8> {
        tuf_root_with_expiry(
            version,
            root_key_id,
            root_key,
            root_key_pair,
            online_key_id,
            online_key,
            "2027-01-01T00:00:00Z",
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn tuf_root_with_expiry(
        version: u64,
        root_key_id: &str,
        root_key: &Value,
        root_key_pair: &KeyPair,
        online_key_id: &str,
        online_key: &Value,
        expires: &str,
    ) -> Vec<u8> {
        let signed = json!({
            "_type": "root",
            "spec_version": "1.0.0",
            "version": version,
            "expires": expires,
            "consistent_snapshot": true,
            "keys": { root_key_id: root_key, online_key_id: online_key },
            "roles": {
                "root": { "keyids": [root_key_id], "threshold": 1 },
                "timestamp": { "keyids": [online_key_id], "threshold": 1 },
                "snapshot": { "keyids": [online_key_id], "threshold": 1 },
                "targets": { "keyids": [online_key_id], "threshold": 1 },
            },
        });
        tuf_envelope(signed, root_key_id, root_key_pair)
    }

    #[allow(clippy::too_many_arguments)]
    fn tuf_root_with_role_bindings(
        version: u64,
        root_key_id: &str,
        root_key: &Value,
        root_key_pair: &KeyPair,
        online_keys: &[(&str, &Value)],
        timestamp: (&[&str], usize),
        snapshot: (&[&str], usize),
        targets: (&[&str], usize),
    ) -> Vec<u8> {
        let mut keys = serde_json::Map::new();
        keys.insert(root_key_id.to_string(), root_key.clone());
        for (key_id, key) in online_keys {
            keys.insert((*key_id).to_string(), (*key).clone());
        }
        let signed = json!({
            "_type": "root",
            "spec_version": "1.0.0",
            "version": version,
            "expires": "2027-01-01T00:00:00Z",
            "consistent_snapshot": true,
            "keys": Value::Object(keys),
            "roles": {
                "root": { "keyids": [root_key_id], "threshold": 1 },
                "timestamp": { "keyids": timestamp.0, "threshold": timestamp.1 },
                "snapshot": { "keyids": snapshot.0, "threshold": snapshot.1 },
                "targets": { "keyids": targets.0, "threshold": targets.1 },
            },
        });
        tuf_envelope(signed, root_key_id, root_key_pair)
    }

    #[allow(clippy::too_many_arguments)]
    fn tuf_root_with_custom_root_role(
        version: u64,
        keys: &[(&str, &Value)],
        root: (&[&str], usize),
        timestamp: (&[&str], usize),
        snapshot: (&[&str], usize),
        targets: (&[&str], usize),
        signers: &[(&str, &KeyPair)],
    ) -> Vec<u8> {
        let mut key_map = serde_json::Map::new();
        for (key_id, key) in keys {
            key_map.insert((*key_id).to_string(), (*key).clone());
        }
        let signed = json!({
            "_type": "root",
            "spec_version": "1.0.0",
            "version": version,
            "expires": "2027-01-01T00:00:00Z",
            "consistent_snapshot": true,
            "keys": Value::Object(key_map),
            "roles": {
                "root": { "keyids": root.0, "threshold": root.1 },
                "timestamp": { "keyids": timestamp.0, "threshold": timestamp.1 },
                "snapshot": { "keyids": snapshot.0, "threshold": snapshot.1 },
                "targets": { "keyids": targets.0, "threshold": targets.1 },
            },
        });
        let signatures = signers
            .iter()
            .map(|(key_id, key_pair)| tuf_signature(&signed, key_id, key_pair))
            .collect::<Vec<_>>();
        serde_json::to_vec(&json!({
            "signatures": signatures,
            "signed": signed,
        }))
        .unwrap()
    }

    fn repository_floor_from_root(root_bytes: &[u8], version: u64) -> RepositoryHighWater {
        let trusted = sigstore_tuf::TrustedMetadataSet::from_root(root_bytes).unwrap();
        let authorities = root_role_authorities(trusted.root()).unwrap();
        let mark = |byte: char, authority: &RoleAuthority| MetadataHighWater {
            version,
            sha256: byte.to_string().repeat(SHA256_HEX_LEN),
            authority: Some(authority.into()),
            referenced_authority: None,
        };
        let descriptor =
            |byte: char, authority: &RoleAuthority, referenced_authority: &RoleAuthority| {
                MetadataHighWater {
                    version,
                    sha256: byte.to_string().repeat(SHA256_HEX_LEN),
                    authority: Some(authority.into()),
                    referenced_authority: Some(referenced_authority.into()),
                }
            };
        RepositoryHighWater {
            root: MetadataHighWater {
                version: trusted.root().version,
                sha256: signed_metadata_sha256("root", trusted.root_bytes()).unwrap(),
                authority: None,
                referenced_authority: None,
            },
            root_authority: authorities.root.clone(),
            timestamp_authority: authorities.timestamp.clone(),
            snapshot_authority: authorities.snapshot.clone(),
            targets_authority: authorities.targets.clone(),
            authority_history: AuthorityHistory::from_authorities(
                &authorities.root,
                &authorities.timestamp,
                &authorities.snapshot,
                &authorities.targets,
            ),
            timestamp: Some(mark('1', &authorities.timestamp)),
            snapshot_descriptor: Some(descriptor(
                '2',
                &authorities.timestamp,
                &authorities.snapshot,
            )),
            snapshot: Some(mark('3', &authorities.snapshot)),
            targets_descriptor: Some(descriptor('4', &authorities.snapshot, &authorities.targets)),
            targets: Some(mark('5', &authorities.targets)),
        }
    }

    fn metadata_pin(bytes: &[u8], version: u64) -> Value {
        json!({
            "version": version,
            "length": bytes.len(),
            "hashes": { "sha256": sha256_hex(bytes) },
        })
    }

    fn target_pin(bytes: &[u8]) -> Value {
        json!({
            "length": bytes.len(),
            "hashes": { "sha256": sha256_hex(bytes) },
        })
    }

    fn consistent_target_path(path: &str, digest: &str) -> String {
        match path.rsplit_once('/') {
            Some((directory, file)) => format!("{directory}/{digest}.{file}"),
            None => format!("{digest}.{path}"),
        }
    }

    fn build_policy_repository(
        timestamp_expires: &str,
        bad_sigstore_root_digest: bool,
        active_releases: bool,
    ) -> (MemoryRepository, Vec<u8>) {
        let key_pair = KeyPair::generate_ecdsa_p256().unwrap();
        build_policy_repository_generation(
            timestamp_expires,
            bad_sigstore_root_digest,
            active_releases,
            &key_pair,
            1,
            if active_releases { 7 } else { 8 },
            true,
        )
    }

    fn build_policy_repository_generation(
        timestamp_expires: &str,
        bad_sigstore_root_digest: bool,
        active_releases: bool,
        key_pair: &KeyPair,
        metadata_version: u64,
        channel_sequence: u64,
        publish_channel_target: bool,
    ) -> (MemoryRepository, Vec<u8>) {
        build_policy_repository_generation_with_channel_padding(
            timestamp_expires,
            bad_sigstore_root_digest,
            active_releases,
            key_pair,
            metadata_version,
            channel_sequence,
            publish_channel_target,
            0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_policy_repository_generation_with_channel_padding(
        timestamp_expires: &str,
        bad_sigstore_root_digest: bool,
        active_releases: bool,
        key_pair: &KeyPair,
        metadata_version: u64,
        channel_sequence: u64,
        publish_channel_target: bool,
        channel_padding_bytes: usize,
    ) -> (MemoryRepository, Vec<u8>) {
        let (key_id, key) = tuf_key_entry(key_pair);
        let root_key_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_key_id, root_key) = tuf_key_entry(&root_key_pair);
        let manifest_path = "releases/1.2.3/prod/manifest.json";
        let bundle_path = "releases/1.2.3/prod/manifest.sigstore.json";
        let trusted_root_path = "sigstore/trusted_root.json";

        let manifest = serde_json::to_vec(&json!({
            "schema": MANIFEST_SCHEMA,
            "component": COMPONENT,
            "environment": "prod",
            "release": { "version": "1.2.3" },
            "source": {
                "uri": "https://source.example/OpenSecretCloud/opensecret",
                "path": "nix/enclave",
                "ref": "refs/tags/v1.2.3",
                "revision": { "algorithm": "git-sha1", "digest": "a".repeat(40) },
            },
            "artifact": {
                "name": "opensecret-1.2.3-prod.eif",
                "mediaType": EIF_MEDIA_TYPE,
                "size": 42,
                "digests": { "sha256": "b".repeat(64) },
            },
            "measurements": {
                "algorithm": "sha384",
                "requiredPcrs": [0, 1, 2],
                "pcrs": {
                    "0": "01".repeat(48),
                    "1": "02".repeat(48),
                    "2": "03".repeat(48),
                },
            },
            "build": {
                "system": "nix",
                "builderId": "portable-nix-builder",
                "derivation": "eif-prod",
                "flakeLockSha256": "c".repeat(64),
                "runUri": "https://ci.example/runs/1",
            },
        }))
        .unwrap();
        let bundle = b"fixture-bundle".to_vec();
        let trusted_root = b"fixture-trusted-root".to_vec();
        let trusted_root_digest = if bad_sigstore_root_digest {
            "0".repeat(64)
        } else {
            sha256_hex(&trusted_root)
        };
        let active = if active_releases {
            json!([{
                "manifestTarget": manifest_path,
                "manifestSha256": sha256_hex(&manifest),
                "bundleTarget": bundle_path,
                "bundleSha256": sha256_hex(&bundle),
            }])
        } else {
            json!([])
        };
        let mut channel_value = json!({
            "schema": CHANNEL_SCHEMA,
            "environment": "prod",
            "sequence": channel_sequence,
            "sigstoreTrustedRootTarget": { "path": trusted_root_path, "sha256": trusted_root_digest },
            "active": active,
        });
        if channel_padding_bytes > 0 {
            channel_value["padding"] = Value::String("x".repeat(channel_padding_bytes));
        }
        let channel = serde_json::to_vec(&channel_value).unwrap();

        let logical_targets = BTreeMap::from([
            ("channels/prod.json".to_string(), channel),
            (trusted_root_path.to_string(), trusted_root),
            (manifest_path.to_string(), manifest),
            (bundle_path.to_string(), bundle),
        ]);
        let target_entries = logical_targets
            .iter()
            .map(|(path, bytes)| (path.clone(), target_pin(bytes)))
            .collect::<serde_json::Map<_, _>>();
        let targets_signed = json!({
            "_type": "targets",
            "spec_version": "1.0.0",
            "version": metadata_version,
            "expires": "2026-09-01T00:00:00Z",
            "targets": target_entries,
        });
        let targets = tuf_envelope(targets_signed, &key_id, key_pair);
        let snapshot_signed = json!({
            "_type": "snapshot",
            "spec_version": "1.0.0",
            "version": metadata_version,
            "expires": "2026-09-01T00:00:00Z",
            "meta": { "targets.json": metadata_pin(&targets, metadata_version) },
        });
        let snapshot = tuf_envelope(snapshot_signed, &key_id, key_pair);
        let timestamp_signed = json!({
            "_type": "timestamp",
            "spec_version": "1.0.0",
            "version": metadata_version,
            "expires": timestamp_expires,
            "meta": { "snapshot.json": metadata_pin(&snapshot, metadata_version) },
        });
        let timestamp = tuf_envelope(timestamp_signed, &key_id, key_pair);
        let root = tuf_root(1, &root_key_id, &root_key, &root_key_pair, &key_id, &key);

        let mut repository = MemoryRepository::default();
        repository
            .metadata
            .insert("timestamp.json".to_string(), timestamp);
        repository
            .metadata
            .insert(format!("{metadata_version}.snapshot.json"), snapshot);
        repository
            .metadata
            .insert(format!("{metadata_version}.targets.json"), targets);
        for (path, bytes) in logical_targets {
            if path == "channels/prod.json" && !publish_channel_target {
                continue;
            }
            let digest = sha256_hex(&bytes);
            repository
                .targets
                .insert(consistent_target_path(&path, &digest), bytes);
        }
        (repository, root)
    }

    #[test]
    fn pcr_tuple_cannot_be_mixed_between_active_releases() {
        let policy = TrustedReleasePolicy::for_test(
            AttestationEnvironment::Production,
            1,
            vec![
                ("1.0.0", [1; 48], [2; 48], [3; 48]),
                ("1.1.0", [4; 48], [5; 48], [6; 48]),
            ],
        );
        assert!(policy.verify_attestation(&document(1, 2, 3)).is_ok());
        assert!(policy.verify_attestation(&document(4, 5, 6)).is_ok());
        assert!(matches!(
            policy.verify_attestation(&document(1, 5, 6)),
            Err(Error::AttestationVerificationFailed(_))
        ));
    }

    #[test]
    fn pcr_authorization_rechecks_policy_expiry_at_the_exact_boundary() {
        let mut policy = TrustedReleasePolicy::for_test(
            AttestationEnvironment::Production,
            1,
            vec![("1.0.0", [1; 48], [2; 48], [3; 48])],
        );
        policy.valid_until = "2026-08-30T00:00:00Z".parse().unwrap();
        assert!(policy
            .verify_attestation_at(
                &document(1, 2, 3),
                "2026-08-29T23:59:59.999999999Z".parse().unwrap(),
            )
            .is_ok());
        assert!(matches!(
            policy.verify_attestation_at(
                &document(1, 2, 3),
                "2026-08-30T00:00:00Z".parse().unwrap(),
            ),
            Err(Error::TrustedReleasePolicy(_))
        ));
    }

    #[tokio::test]
    async fn concurrent_refresh_waiters_share_one_in_flight_result_only() {
        let coordinator = Arc::new(Mutex::new(RefreshCoordinator::default()));
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(17));
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let coordinator = Arc::clone(&coordinator);
            let calls = Arc::clone(&calls);
            let barrier = Arc::clone(&barrier);
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                coalesce_refresh(coordinator, move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Ok(TrustedReleasePolicy::for_test(
                        AttestationEnvironment::Production,
                        7,
                        vec![("1.2.3", [1; 48], [2; 48], [3; 48])],
                    ))
                })
                .await
                .unwrap()
                .sequence()
            }));
        }
        barrier.wait().await;
        for task in tasks {
            assert_eq!(task.await.unwrap(), 7);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let later_calls = Arc::clone(&calls);
        let later = coalesce_refresh(Arc::clone(&coordinator), move || async move {
            later_calls.fetch_add(1, Ordering::SeqCst);
            Ok(TrustedReleasePolicy::for_test(
                AttestationEnvironment::Production,
                8,
                Vec::new(),
            ))
        })
        .await
        .unwrap();
        assert_eq!(later.sequence(), 8);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn caller_cancellation_does_not_cancel_the_owned_refresh_worker() {
        let coordinator = Arc::new(Mutex::new(RefreshCoordinator::default()));
        let release = Arc::new(tokio::sync::Notify::new());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let held = TrustedReleasePolicy::for_test(
            AttestationEnvironment::Production,
            6,
            vec![("1.2.2", [1; 48], [2; 48], [3; 48])],
        );
        let manager = TrustedReleaseManager::fixed_for_test(held.clone());
        let leader_coordinator = Arc::clone(&coordinator);
        let worker_release = Arc::clone(&release);
        let worker_manager = Arc::clone(&manager);
        let leader = tokio::spawn(async move {
            coalesce_refresh(leader_coordinator, move || async move {
                let _ = started_tx.send(());
                worker_release.notified().await;
                let policy = TrustedReleasePolicy::for_test(
                    AttestationEnvironment::Production,
                    7,
                    vec![("1.2.3", [1; 48], [2; 48], [3; 48])],
                );
                worker_manager.install_policy_floor_for_test(&policy);
                Ok(policy)
            })
            .await
        });
        started_rx.await.unwrap();
        leader.abort();
        assert!(leader.await.unwrap_err().is_cancelled());

        let fallback_calls = Arc::new(AtomicUsize::new(0));
        let waiter_calls = Arc::clone(&fallback_calls);
        let waiter = tokio::spawn(coalesce_refresh(
            Arc::clone(&coordinator),
            move || async move {
                waiter_calls.fetch_add(1, Ordering::SeqCst);
                Ok(TrustedReleasePolicy::for_test(
                    AttestationEnvironment::Production,
                    99,
                    Vec::new(),
                ))
            },
        ));
        release.notify_one();
        assert_eq!(waiter.await.unwrap().unwrap().sequence(), 7);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            manager.assert_policy_current(&held).await,
            Err(Error::TrustedReleasePolicy(_))
        ));
    }

    #[tokio::test]
    async fn held_policy_is_rejected_after_a_concurrent_channel_floor_advances() {
        let held = TrustedReleasePolicy::for_test(
            AttestationEnvironment::Production,
            7,
            vec![("1.2.3", [1; 48], [2; 48], [3; 48])],
        );
        let manager = TrustedReleaseManager::fixed_for_test(held.clone());
        assert!(manager.assert_policy_current(&held).await.is_ok());

        let revoked =
            TrustedReleasePolicy::for_test(AttestationEnvironment::Production, 8, Vec::new());
        manager.install_policy_floor_for_test(&revoked);
        assert!(matches!(
            manager.assert_policy_current(&held).await,
            Err(Error::TrustedReleasePolicy(_))
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_persistence_keeps_the_cross_process_lock_until_write_finishes() {
        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("cache.json");
        let initial_guard = lock_cache(&cache_path).unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();

        let writer = tokio::spawn(async move {
            let mut guard = Some(initial_guard);
            run_blocking_with_cache_lock(&mut guard, "cancellation test", move || {
                let _ = started_tx.send(());
                release_rx.recv().expect("test releases the blocked writer");
                Ok(())
            })
            .await
        });
        started_rx.await.unwrap();
        writer.abort();
        assert!(writer.await.unwrap_err().is_cancelled());

        let contender_path = cache_path.clone();
        let (acquired_tx, mut acquired_rx) = tokio::sync::oneshot::channel();
        let contender = tokio::task::spawn_blocking(move || {
            let guard = lock_cache(&contender_path);
            let _ = acquired_tx.send(());
            guard
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut acquired_rx)
                .await
                .is_err()
        );

        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), &mut acquired_rx)
            .await
            .expect("the contender acquires after the detached write finishes")
            .unwrap();
        let contender_guard = contender.await.unwrap().unwrap();
        drop(contender_guard);
    }

    #[tokio::test]
    async fn signed_tuf_repository_resolves_and_reverifies_offline() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        let (repository, root) = build_policy_repository("2026-08-30T00:00:00Z", false, true);
        let store = Arc::new(SnapshotStore::default());
        let policy = resolve_policy_with_final_time(
            repository,
            Arc::clone(&store),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .unwrap();
        assert_eq!(policy.sequence(), 7);
        assert!(policy
            .verify_attestation_at(&document(1, 2, 3), now)
            .is_ok());

        let offline = resolve_policy_with_final_time(
            StoreRepository::new(Arc::clone(&store)),
            Arc::clone(&store),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .expect("the complete cache must reverify without network access");
        assert_eq!(offline.policy_id(), policy.policy_id());
    }

    #[tokio::test]
    async fn verified_generation_pruning_discards_obsolete_release_entries() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        let (repository, root) = build_policy_repository("2026-08-30T00:00:00Z", false, true);
        let obsolete = (0..MAX_CACHE_ENTRIES)
            .map(|index| {
                (
                    format!("targets/releases/0.0.{index}/prod/obsolete.sigstore.json"),
                    vec![index as u8; 32],
                )
            })
            .collect();
        let store = Arc::new(SnapshotStore::from_entries(obsolete));
        let policy = resolve_policy_with_final_time(
            repository,
            Arc::clone(&store),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .unwrap();

        let retained = store.entries();
        assert!(retained.len() < 16, "cache must stay generation-bounded");
        assert!(!retained.keys().any(|name| name.contains("obsolete")));
        assert!(retained.contains_key("timestamp.json"));
        assert!(retained.contains_key("targets/channels/prod.json"));
        assert!(retained.contains_key("targets/releases/1.2.3/prod/manifest.json"));

        let directory = tempfile::tempdir().unwrap();
        let high_water = BTreeMap::from([(
            AttestationEnvironment::Production,
            CacheHighWater::from_policy(&policy),
        )]);
        persist_cache(
            &directory.path().join("cache.json"),
            &repository_id(REPOSITORY_URL),
            &policy.repository_high_water,
            &high_water,
            &retained,
        )
        .expect("a pruned rollover generation must remain persistable");
    }

    #[tokio::test]
    async fn metadata_is_rechecked_at_the_end_of_a_slow_refresh() {
        let initial_now: jiff::Timestamp = "2026-08-29T23:59:59Z".parse().unwrap();
        let expires = "2026-08-30T00:00:00Z";
        let (repository, root) = build_policy_repository(expires, false, true);
        let before_expiry: jiff::Timestamp = "2026-08-29T23:59:59.999999999Z".parse().unwrap();
        resolve_policy_with_final_time(
            repository.clone(),
            Arc::new(SnapshotStore::default()),
            &root,
            AttestationEnvironment::Production,
            initial_now,
            &FixtureBundleVerifier { fail: false },
            || before_expiry,
        )
        .await
        .expect("metadata remains usable immediately before its expiry instant");

        let error = resolve_policy_with_final_time(
            repository,
            Arc::new(SnapshotStore::default()),
            &root,
            AttestationEnvironment::Production,
            initial_now,
            &FixtureBundleVerifier { fail: false },
            || expires.parse().unwrap(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, RefreshFailure::Security(_)));
    }

    #[tokio::test]
    async fn authenticated_revoke_all_persists_as_an_offline_deny_policy() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        let (mut repository, root) = build_policy_repository("2026-08-30T00:00:00Z", false, false);
        // Once the authenticated channel revokes everything, policy/root and
        // release targets are no longer required. Their unavailability must
        // not trigger fallback to an older active generation.
        repository
            .targets
            .retain(|path, _| path.starts_with("channels/"));
        let store = Arc::new(SnapshotStore::default());
        let policy = resolve_policy_with_final_time(
            repository,
            Arc::clone(&store),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .expect("an authenticated empty channel is a valid deny-all policy");
        assert_eq!(policy.sequence(), 8);
        assert!(matches!(
            policy.verify_attestation_at(&document(1, 2, 3), now),
            Err(Error::UnreleasedAttestationPolicy { .. })
        ));

        let offline = resolve_policy_with_final_time(
            StoreRepository::new(Arc::clone(&store)),
            Arc::clone(&store),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .expect("the cached revoke-all policy must reverify without network access");
        assert_eq!(offline.policy_id(), policy.policy_id());
        assert!(matches!(
            offline.verify_attestation_at(&document(1, 2, 3), now),
            Err(Error::UnreleasedAttestationPolicy { .. })
        ));
    }

    #[tokio::test]
    async fn newer_metadata_with_missing_changed_channel_cannot_fallback() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        let key_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (generation_a, root) = build_policy_repository_generation(
            "2026-08-30T00:00:00Z",
            false,
            true,
            &key_pair,
            1,
            7,
            true,
        );
        let store = Arc::new(SnapshotStore::default());
        let policy_a = resolve_policy_with_final_time(
            generation_a.clone(),
            Arc::clone(&store),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .unwrap();

        let (generation_b, _) = build_policy_repository_generation(
            "2026-08-30T00:00:00Z",
            false,
            true,
            &key_pair,
            2,
            8,
            false,
        );
        let error = resolve_policy_with_final_time(
            generation_b,
            Arc::clone(&store),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            RefreshFailure::UnavailableAfterChannel(Error::TrustedReleaseNetwork(_))
        ));

        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("journal.json");
        let config = TrustedReleaseConfig::new(
            AttestationEnvironment::Production,
            "https://attestations.invalid/tuf/",
            root.clone(),
        )
        .unwrap()
        .with_cache_path(&cache_path);
        let manager = TrustedReleaseManager::new(config).unwrap();
        let mut cache_guard = manager.acquire_cache_lock().await.unwrap();
        let prior_channel = CacheHighWater::from_policy(&policy_a);
        let journal = manager
            .record_authenticated_observation(
                Arc::clone(&store),
                now,
                Some(&policy_a.repository_high_water),
                BTreeMap::from([(AttestationEnvironment::Production, prior_channel.clone())]),
                &mut cache_guard,
            )
            .await
            .expect("the authenticated v2 metadata must be journaled")
            .cache;
        assert_eq!(
            journal
                .repository_high_water
                .as_ref()
                .and_then(|high_water| high_water.targets.as_ref())
                .map(|high_water| high_water.version),
            Some(2)
        );
        assert_eq!(
            journal
                .channel_high_water
                .get(&AttestationEnvironment::Production)
                .map(|high_water| high_water.sequence),
            Some(7),
            "a missing changed channel must not erase the prior sequence floor"
        );

        drop(manager);
        let reloaded = read_cache(
            &cache_path,
            &repository_id("https://attestations.invalid/tuf/"),
        )
        .unwrap();
        let replay_store = Arc::new(SnapshotStore::from_entries(reloaded.entries.clone()));
        let replay = resolve_policy_with_final_time(
            generation_a,
            Arc::clone(&replay_store),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await;
        if let Ok(policy) = replay {
            enforce_repository_high_water(
                &policy.repository_high_water,
                reloaded.repository_high_water.as_ref(),
            )
            .expect_err("a restarted client must reject replay of metadata v1");
        }

        let (generation_b_repaired, _) = build_policy_repository_generation(
            "2026-08-30T00:00:00Z",
            false,
            true,
            &key_pair,
            2,
            8,
            true,
        );
        let repaired_store = Arc::new(SnapshotStore::from_entries(reloaded.entries));
        let repaired = resolve_policy_with_final_time(
            generation_b_repaired,
            repaired_store,
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .expect("the repaired v2 generation must remain recoverable");
        enforce_repository_high_water(
            &repaired.repository_high_water,
            reloaded.repository_high_water.as_ref(),
        )
        .unwrap();
        enforce_high_water(
            &repaired,
            reloaded
                .channel_high_water
                .get(&AttestationEnvironment::Production),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn oversized_authenticated_channel_cannot_erase_new_metadata_floors() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        let key_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (generation_v1, root) = build_policy_repository_generation(
            "2026-08-30T00:00:00Z",
            false,
            true,
            &key_pair,
            1,
            7,
            true,
        );
        let store = Arc::new(SnapshotStore::default());
        let policy_v1 = resolve_policy_with_final_time(
            generation_v1.clone(),
            Arc::clone(&store),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .unwrap();

        let (oversized_v2, _) = build_policy_repository_generation_with_channel_padding(
            "2026-08-30T00:00:00Z",
            false,
            true,
            &key_pair,
            2,
            8,
            true,
            MAX_CHANNEL_BYTES,
        );
        assert!(matches!(
            resolve_policy_with_final_time(
                oversized_v2,
                Arc::clone(&store),
                &root,
                AttestationEnvironment::Production,
                now,
                &FixtureBundleVerifier { fail: false },
                || now,
            )
            .await,
            Err(RefreshFailure::Security(_))
        ));

        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("journal.json");
        let repository_url = "https://oversized.attestations.invalid/tuf/";
        let manager = TrustedReleaseManager::new(
            TrustedReleaseConfig::new(
                AttestationEnvironment::Production,
                repository_url,
                root.clone(),
            )
            .unwrap()
            .with_cache_path(&cache_path),
        )
        .unwrap();
        let mut cache_guard = manager.acquire_cache_lock().await.unwrap();
        let prior_channel = CacheHighWater::from_policy(&policy_v1);
        let journal = manager
            .record_authenticated_observation(
                store,
                now,
                Some(&policy_v1.repository_high_water),
                BTreeMap::from([(AttestationEnvironment::Production, prior_channel.clone())]),
                &mut cache_guard,
            )
            .await
            .expect("oversized channel must not block repository journaling")
            .cache;
        assert_eq!(
            journal
                .repository_high_water
                .as_ref()
                .and_then(|high_water| high_water.targets.as_ref())
                .map(|high_water| high_water.version),
            Some(2)
        );
        assert_eq!(
            journal
                .channel_high_water
                .get(&AttestationEnvironment::Production),
            Some(&prior_channel)
        );

        let reloaded = read_cache(&cache_path, &repository_id(repository_url)).unwrap();
        let replay = resolve_policy_with_final_time(
            generation_v1,
            Arc::new(SnapshotStore::from_entries(reloaded.entries)),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await;
        assert!(
            replay.is_err(),
            "metadata v1 must not replay after observed v2"
        );
    }

    #[tokio::test]
    async fn first_run_partial_metadata_journal_survives_restart_without_a_channel() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        let key_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (generation_v2, root) = build_policy_repository_generation(
            "2026-08-30T00:00:00Z",
            false,
            true,
            &key_pair,
            2,
            8,
            false,
        );
        let store = Arc::new(SnapshotStore::default());
        assert!(matches!(
            resolve_policy_with_final_time(
                generation_v2,
                Arc::clone(&store),
                &root,
                AttestationEnvironment::Production,
                now,
                &FixtureBundleVerifier { fail: false },
                || now,
            )
            .await,
            Err(RefreshFailure::UnavailableAfterChannel(_))
        ));

        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("journal.json");
        let repository_url = "https://attestations.invalid/tuf/";
        let manager = TrustedReleaseManager::new(
            TrustedReleaseConfig::new(
                AttestationEnvironment::Production,
                repository_url,
                root.clone(),
            )
            .unwrap()
            .with_cache_path(&cache_path),
        )
        .unwrap();
        let mut cache_guard = manager.acquire_cache_lock().await.unwrap();
        let journal = manager
            .record_authenticated_observation(store, now, None, BTreeMap::new(), &mut cache_guard)
            .await
            .unwrap()
            .cache;
        assert!(journal.channel_high_water.is_empty());
        assert_eq!(
            journal
                .repository_high_water
                .as_ref()
                .and_then(|high_water| high_water.targets.as_ref())
                .map(|high_water| high_water.version),
            Some(2)
        );

        drop(manager);
        let reloaded = read_cache(&cache_path, &repository_id(repository_url)).unwrap();
        assert!(reloaded.channel_high_water.is_empty());
        let (generation_v1, _) = build_policy_repository_generation(
            "2026-08-30T00:00:00Z",
            false,
            true,
            &key_pair,
            1,
            7,
            true,
        );
        let replay_store = Arc::new(SnapshotStore::from_entries(reloaded.entries));
        let replay = resolve_policy_with_final_time(
            generation_v1,
            replay_store,
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await;
        assert!(
            replay.is_err(),
            "metadata v1 must not replay after v2 was journaled"
        );
    }

    #[tokio::test]
    async fn rejected_channel_rollback_still_journals_new_repository_metadata() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        let key_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (generation_v1, root) = build_policy_repository_generation(
            "2026-08-30T00:00:00Z",
            false,
            true,
            &key_pair,
            1,
            10,
            true,
        );
        let store = Arc::new(SnapshotStore::default());
        let policy_v1 = resolve_policy_with_final_time(
            generation_v1.clone(),
            Arc::clone(&store),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .unwrap();

        let (generation_v2, _) = build_policy_repository_generation(
            "2026-08-30T00:00:00Z",
            false,
            true,
            &key_pair,
            2,
            9,
            true,
        );
        let policy_v2 = resolve_policy_with_final_time(
            generation_v2,
            Arc::clone(&store),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .unwrap();
        let prior_channel = CacheHighWater::from_policy(&policy_v1);
        assert!(enforce_high_water(&policy_v2, Some(&prior_channel)).is_err());

        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("journal.json");
        let repository_url = "https://attestations.invalid/tuf/";
        let manager = TrustedReleaseManager::new(
            TrustedReleaseConfig::new(
                AttestationEnvironment::Production,
                repository_url,
                root.clone(),
            )
            .unwrap()
            .with_cache_path(&cache_path),
        )
        .unwrap();
        let mut cache_guard = manager.acquire_cache_lock().await.unwrap();
        let recorded = manager
            .record_authenticated_observation(
                store,
                now,
                Some(&policy_v1.repository_high_water),
                BTreeMap::from([(AttestationEnvironment::Production, prior_channel.clone())]),
                &mut cache_guard,
            )
            .await
            .unwrap();
        assert!(recorded.observation_error.is_some());
        assert_eq!(
            recorded
                .cache
                .repository_high_water
                .as_ref()
                .and_then(|high_water| high_water.targets.as_ref())
                .map(|high_water| high_water.version),
            Some(2)
        );
        assert_eq!(
            recorded
                .cache
                .channel_high_water
                .get(&AttestationEnvironment::Production)
                .map(|high_water| high_water.sequence),
            Some(10)
        );

        let reloaded = read_cache(&cache_path, &repository_id(repository_url)).unwrap();
        let replay = resolve_policy_with_final_time(
            generation_v1,
            Arc::new(SnapshotStore::from_entries(reloaded.entries)),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await;
        assert!(
            replay.is_err(),
            "repository metadata v1 must not replay after v2"
        );
    }

    #[tokio::test]
    async fn journal_write_failure_retains_authenticated_in_process_floors() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        let (repository, root) = build_policy_repository("2026-08-30T00:00:00Z", false, true);
        let store = Arc::new(SnapshotStore::default());
        let policy = resolve_policy_with_final_time(
            repository,
            Arc::clone(&store),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .unwrap();

        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("not-a-directory");
        std::fs::write(&blocker, b"block directory creation").unwrap();
        let cache_path = blocker.join("journal.json");
        let repository_url = "https://attestations.invalid/tuf/";
        let manager = TrustedReleaseManager::new(
            TrustedReleaseConfig::new(
                AttestationEnvironment::Production,
                repository_url,
                root.clone(),
            )
            .unwrap()
            .with_cache_path(&cache_path),
        )
        .unwrap();
        let development_manager = TrustedReleaseManager::new(
            TrustedReleaseConfig::new(AttestationEnvironment::Development, repository_url, root)
                .unwrap()
                .with_cache_path(&cache_path),
        )
        .unwrap();
        assert!(Arc::ptr_eq(
            &manager.memory_state,
            &development_manager.memory_state
        ));
        let mut cache_guard = None;
        let error = manager
            .record_authenticated_observation(store, now, None, BTreeMap::new(), &mut cache_guard)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::Io(_) | Error::TrustedReleasePolicy(_)
        ));
        assert_eq!(
            development_manager
                .memory_state
                .state
                .lock()
                .unwrap()
                .high_water
                .channels
                .get(&AttestationEnvironment::Production)
                .map(|high_water| high_water.sequence),
            Some(policy.sequence())
        );
        assert_eq!(
            development_manager
                .memory_state
                .state
                .lock()
                .unwrap()
                .high_water
                .repository
                .as_ref()
                .and_then(|high_water| high_water.targets.as_ref())
                .map(|high_water| high_water.version),
            Some(1)
        );
    }

    #[tokio::test]
    async fn expired_verified_root_rotation_is_journaled_and_can_recover_forward() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        let key_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (key_id, key) = tuf_key_entry(&key_pair);
        let root_key_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_key_id, root_key) = tuf_key_entry(&root_key_pair);
        let root_v1 = tuf_root(1, &root_key_id, &root_key, &root_key_pair, &key_id, &key);
        let root_v2 = tuf_root_with_expiry(
            2,
            &root_key_id,
            &root_key,
            &root_key_pair,
            &key_id,
            &key,
            "2026-08-28T00:00:00Z",
        );
        let mut expired_rotation = MemoryRepository::default();
        expired_rotation
            .metadata
            .insert("2.root.json".to_string(), root_v2);
        let store = Arc::new(SnapshotStore::default());
        assert!(matches!(
            resolve_policy_with_final_time(
                expired_rotation,
                Arc::clone(&store),
                &root_v1,
                AttestationEnvironment::Production,
                now,
                &FixtureBundleVerifier { fail: false },
                || now,
            )
            .await,
            Err(RefreshFailure::Security(_))
        ));

        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("journal.json");
        let repository_url = "https://attestations.invalid/tuf/";
        let manager = TrustedReleaseManager::new(
            TrustedReleaseConfig::new(
                AttestationEnvironment::Production,
                repository_url,
                root_v1.clone(),
            )
            .unwrap()
            .with_cache_path(&cache_path),
        )
        .unwrap();
        let mut cache_guard = manager.acquire_cache_lock().await.unwrap();
        let journal = manager
            .record_authenticated_observation(store, now, None, BTreeMap::new(), &mut cache_guard)
            .await
            .unwrap()
            .cache;
        assert_eq!(
            journal.repository_high_water.as_ref().unwrap().root.version,
            2
        );

        let reloaded = read_cache(&cache_path, &repository_id(repository_url)).unwrap();
        let (mut repaired, _) = build_policy_repository_generation(
            "2026-08-30T00:00:00Z",
            false,
            true,
            &key_pair,
            1,
            7,
            true,
        );
        repaired.metadata.insert(
            "3.root.json".to_string(),
            tuf_root(3, &root_key_id, &root_key, &root_key_pair, &key_id, &key),
        );
        let repaired_policy = resolve_policy_with_final_time(
            repaired,
            Arc::new(SnapshotStore::from_entries(reloaded.entries)),
            &root_v1,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .expect("a valid root v3 must recover forward from expired root v2");
        assert_eq!(repaired_policy.repository_high_water.root.version, 3);
        enforce_repository_high_water(
            &repaired_policy.repository_high_water,
            reloaded.repository_high_water.as_ref(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn authenticated_channel_digest_mismatch_is_security_failure() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        let (repository, root) = build_policy_repository("2026-08-30T00:00:00Z", true, true);
        let error = resolve_policy_with_final_time(
            repository,
            Arc::new(SnapshotStore::default()),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, RefreshFailure::Security(_)));
    }

    #[tokio::test]
    async fn expired_tuf_timestamp_is_security_failure() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        let (repository, root) = build_policy_repository("2026-08-28T00:00:00Z", false, true);
        let error = resolve_policy_with_final_time(
            repository,
            Arc::new(SnapshotStore::default()),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, RefreshFailure::Security(_)));
    }

    #[tokio::test]
    async fn rejected_sigstore_bundle_is_security_failure() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        let (repository, root) = build_policy_repository("2026-08-30T00:00:00Z", false, true);
        let error = resolve_policy_with_final_time(
            repository,
            Arc::new(SnapshotStore::default()),
            &root,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: true },
            || now,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, RefreshFailure::Security(_)));
    }

    #[test]
    fn official_placeholder_fails_closed() {
        assert!(is_unpublished_root(EMBEDDED_TUF_ROOT));
        validate_official_embedded_root(EMBEDDED_TUF_ROOT).unwrap();
    }

    #[test]
    fn official_bootstrap_requires_signed_root_version_one_but_custom_roots_remain_flexible() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let online_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (online_id, online_key) = tuf_key_entry(&online_pair);
        let root_v1 = tuf_root(1, &root_id, &root_key, &root_pair, &online_id, &online_key);
        let root_v2 = tuf_root(2, &root_id, &root_key, &root_pair, &online_id, &online_key);

        validate_official_embedded_root(&root_v1).unwrap();
        let error = validate_official_embedded_root(&root_v2).unwrap_err();
        assert!(error
            .to_string()
            .contains("official embedded TUF root signed version must be exactly 1; found 2"));

        let directory = tempfile::tempdir().unwrap();
        let custom = TrustedReleaseConfig::new_with_cache_path(
            AttestationEnvironment::Production,
            "https://attestations.example/tuf/",
            root_v2,
            directory.path().join("custom-v2.json"),
        )
        .unwrap();
        TrustedReleaseManager::new(custom).unwrap();
    }

    #[test]
    fn mobile_host_can_supply_durable_state_path_for_the_official_root() {
        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("attestation-state.json");
        let manager = TrustedReleaseManager::official_with_cache_path(
            AttestationEnvironment::Production,
            &cache_path,
        )
        .unwrap();
        assert_eq!(manager.config.repository_url, REPOSITORY_URL);
        assert_eq!(
            manager.config.cache_path.as_deref(),
            Some(cache_path.as_path())
        );
        assert!(is_unpublished_root(&manager.config.tuf_root));
    }

    #[test]
    fn official_trust_domain_requires_exact_repository_root_environment_and_persistence() {
        let directory = tempfile::tempdir().unwrap();
        let valid = TrustedReleaseManager::official_with_cache_path(
            AttestationEnvironment::Production,
            directory.path().join("valid.json"),
        )
        .unwrap();
        valid
            .validate_official_trust_domain(AttestationEnvironment::Production)
            .unwrap();

        let custom_repository = TrustedReleaseManager::new(
            TrustedReleaseConfig::new_with_cache_path(
                AttestationEnvironment::Production,
                "https://attestations.example/tuf/",
                EMBEDDED_TUF_ROOT.to_vec(),
                directory.path().join("repository.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(custom_repository
            .validate_official_trust_domain(AttestationEnvironment::Production)
            .unwrap_err()
            .to_string()
            .contains("canonical attestation repository"));

        let custom_root = TrustedReleaseManager::new(
            TrustedReleaseConfig::new_with_cache_path(
                AttestationEnvironment::Production,
                REPOSITORY_URL,
                b"custom root".to_vec(),
                directory.path().join("root.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(custom_root
            .validate_official_trust_domain(AttestationEnvironment::Production)
            .unwrap_err()
            .to_string()
            .contains("embedded TUF bootstrap root"));

        let ephemeral = TrustedReleaseManager::new(
            TrustedReleaseConfig::new(
                AttestationEnvironment::Production,
                REPOSITORY_URL,
                EMBEDDED_TUF_ROOT.to_vec(),
            )
            .unwrap()
            .without_persistent_cache(),
        )
        .unwrap();
        assert!(ephemeral
            .validate_official_trust_domain(AttestationEnvironment::Production)
            .unwrap_err()
            .to_string()
            .contains("persistent attestation rollback state"));

        assert!(valid
            .validate_official_trust_domain(AttestationEnvironment::Development)
            .unwrap_err()
            .to_string()
            .contains("expected 'dev'"));
    }

    #[test]
    fn equivalent_repository_urls_share_one_canonical_cache_identity() {
        let without_slash = TrustedReleaseConfig::new(
            AttestationEnvironment::Production,
            "https://EXAMPLE.com/tuf",
            b"{}".to_vec(),
        )
        .unwrap();
        let with_slash = TrustedReleaseConfig::new(
            AttestationEnvironment::Development,
            "https://example.com/tuf/",
            b"{}".to_vec(),
        )
        .unwrap();
        assert_eq!(without_slash.repository_url, "https://example.com/tuf/");
        assert_eq!(without_slash.repository_url, with_slash.repository_url);
        assert_eq!(without_slash.cache_path, with_slash.cache_path);
        assert_eq!(
            repository_id(&without_slash.repository_url),
            repository_id(&with_slash.repository_url)
        );
    }

    #[test]
    fn release_targets_are_environment_isolated() {
        let active = ActiveRelease {
            manifest_target: "releases/1.2.3/dev/manifest.json".to_string(),
            manifest_sha256: "a".repeat(64),
            bundle_target: "releases/1.2.3/dev/manifest.sigstore.json".to_string(),
            bundle_sha256: "b".repeat(64),
        };
        assert!(validate_release_targets(&active, AttestationEnvironment::Development).is_ok());
        assert!(validate_release_targets(&active, AttestationEnvironment::Production).is_err());
    }

    #[test]
    fn manifest_audit_fields_and_urls_match_the_wire_profile() {
        assert!(validate_identifier("builder ID", "builder_1.test").is_ok());
        assert!(validate_identifier("builder ID", "_builder").is_err());
        assert!(validate_identifier("builder ID", &format!("a{}", "b".repeat(256))).is_err());
        assert!(validate_https_url("source.uri", "https://source.example/project").is_ok());
        assert!(validate_https_url("build.runUri", "https://ci.example/runs/1?secret=x").is_err());
        assert!(validate_source_path(".").is_ok());
        assert!(validate_source_path("nix/enclave").is_ok());
        assert!(validate_source_path("nix/./enclave").is_err());
        assert!(validate_source_path("../enclave").is_err());
    }

    #[test]
    fn only_transport_errors_allow_cache_fallback() {
        assert!(matches!(
            classify_tuf_error(
                "refresh",
                sigstore_tuf::Error::Transport(format!("{TUF_UNAVAILABLE_PREFIX}offline"))
            ),
            RefreshFailure::Unavailable(Error::TrustedReleaseNetwork(_))
        ));
        assert!(matches!(
            classify_tuf_error(
                "refresh",
                sigstore_tuf::Error::Transport("timestamp.json not found".to_string())
            ),
            RefreshFailure::Unavailable(Error::TrustedReleaseNetwork(_))
        ));
        assert!(matches!(
            prevent_fallback_after_channel(RefreshFailure::Unavailable(
                Error::TrustedReleaseNetwork("missing channel".to_string())
            )),
            RefreshFailure::UnavailableAfterChannel(Error::TrustedReleaseNetwork(_))
        ));
        for error in [
            sigstore_tuf::Error::Expired {
                role: "timestamp".to_string(),
                expires: "2026-01-01T00:00:00Z".to_string(),
            },
            sigstore_tuf::Error::Rollback {
                role: "targets".to_string(),
                trusted: 2,
                new: 1,
            },
            sigstore_tuf::Error::IntegrityMismatch("tampered".to_string()),
            sigstore_tuf::Error::Transport("response exceeds size limit".to_string()),
            sigstore_tuf::Error::Transport("GET returned status 302".to_string()),
        ] {
            assert!(matches!(
                classify_tuf_error("refresh", error),
                RefreshFailure::Security(_)
            ));
        }
    }

    #[test]
    fn signed_timestamp_is_limited_to_48_hours() {
        let now: jiff::Timestamp = "2026-08-29T00:00:00Z".parse().unwrap();
        assert!(validate_timestamp_window("2026-08-31T00:00:00Z", now).is_ok());
        assert!(validate_timestamp_window("2026-08-31T00:00:01Z", now).is_err());
        assert!(validate_timestamp_window("2026-08-28T23:59:59Z", now).is_err());
    }

    #[tokio::test]
    async fn tuf_root_rotation_limit_allows_root_33_but_rejects_root_34() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let online_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (online_id, online_key) = tuf_key_entry(&online_pair);
        let make_root = |version| {
            tuf_root(
                version,
                &root_id,
                &root_key,
                &root_pair,
                &online_id,
                &online_key,
            )
        };
        let root_v1 = make_root(1);
        let mut through_33 = HashMap::new();
        for version in 2..=33 {
            through_33.insert(format!("{version}.root.json"), make_root(version));
        }

        let now = "2026-08-29T00:00:00Z".parse().unwrap();
        let mut updater = Updater::new(
            MemoryRepository {
                metadata: through_33.clone(),
                targets: HashMap::new(),
            },
            &root_v1,
        )
        .unwrap()
        .with_config(tuf_updater_config());
        let error = updater.refresh(now).await.unwrap_err();
        assert_eq!(updater.trusted().root().version, 33);
        assert!(!error.to_string().contains("maximum root rotations"));

        let root_v34 = make_root(34);
        through_33.insert("34.root.json".to_string(), root_v34.clone());
        let through_34 = through_33.clone();
        let mut updater = Updater::new(
            MemoryRepository {
                metadata: through_33,
                targets: HashMap::new(),
            },
            &root_v1,
        )
        .unwrap()
        .with_config(tuf_updater_config());
        let error = updater.refresh(now).await.unwrap_err();
        assert!(error.to_string().contains("exceeded 33 root rotations"));
        // sigstore-tuf has already adopted root 34 before returning its
        // rotation-limit error. The SDK wrapper must never journal that root.
        assert_eq!(updater.trusted().root().version, 34);

        let mut entries =
            BTreeMap::from([("root_history/1.root.json".to_string(), root_v1.clone())]);
        for version in 2..=34 {
            entries.insert(
                format!("root_history/{version}.root.json"),
                through_34
                    .get(&format!("{version}.root.json"))
                    .unwrap()
                    .clone(),
            );
        }
        let chain =
            authenticated_root_authority_history(&root_v1, &entries, 34, &root_v34).unwrap();
        assert_eq!(chain.repository.root.version, 33);
        assert!(chain
            .error
            .unwrap()
            .to_string()
            .contains("exceeds the supported 32 transitions"));

        let manager_entries = entries.clone();
        let store = Arc::new(SnapshotStore::from_entries(entries));
        let observation = capture_authenticated_observation(Arc::clone(&store), &root_v1, now)
            .await
            .unwrap();
        assert_eq!(observation.repository_high_water.root.version, 33);
        assert!(observation
            .error
            .as_ref()
            .unwrap()
            .to_string()
            .contains("exceeds the supported 32 transitions"));
        assert!(!observation
            .entries
            .contains_key("root_history/34.root.json"));
        assert_eq!(
            root_transition_high_water(observation.entries.get("root.json").unwrap())
                .unwrap()
                .root
                .version,
            33
        );

        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("root-limit.json");
        let repository_id = repository_id(REPOSITORY_URL);
        let manager = TrustedReleaseManager::new(
            TrustedReleaseConfig::new_with_cache_path(
                AttestationEnvironment::Production,
                REPOSITORY_URL,
                root_v1.clone(),
                &cache_path,
            )
            .unwrap(),
        )
        .unwrap();
        let manager_store = Arc::new(SnapshotStore::from_entries(manager_entries));
        let mut cache_guard = manager.acquire_cache_lock().await.unwrap();
        let recorded = manager
            .record_authenticated_observation(
                manager_store,
                now,
                None,
                BTreeMap::new(),
                &mut cache_guard,
            )
            .await
            .unwrap();
        assert!(recorded
            .observation_error
            .unwrap()
            .to_string()
            .contains("exceeds the supported 32 transitions"));
        let restarted = read_cache(&cache_path, &repository_id).unwrap();
        validate_cached_root_span(&root_v1, &restarted).unwrap();
        assert_eq!(
            restarted
                .repository_high_water
                .as_ref()
                .unwrap()
                .root
                .version,
            33
        );
        assert!(!restarted.entries.contains_key("root_history/34.root.json"));

        let poisoned = CachedRepository {
            repository_high_water: Some(root_transition_high_water(&root_v34).unwrap()),
            channel_high_water: BTreeMap::new(),
            entries: BTreeMap::new(),
        };
        assert!(validate_cached_root_span(&root_v1, &poisoned).is_err());

        let poisoned_path = directory.path().join("root-limit-poisoned.json");
        let mut poisoned_entries = BTreeMap::from([
            ("root.json".to_string(), root_v34.clone()),
            ("root_history/1.root.json".to_string(), root_v1.clone()),
        ]);
        for version in 2..=34 {
            poisoned_entries.insert(
                format!("root_history/{version}.root.json"),
                through_34
                    .get(&format!("{version}.root.json"))
                    .unwrap()
                    .clone(),
            );
        }
        persist_cache(
            &poisoned_path,
            &repository_id,
            poisoned.repository_high_water.as_ref().unwrap(),
            &poisoned.channel_high_water,
            &poisoned_entries,
        )
        .unwrap();
        let poisoned_manager = TrustedReleaseManager::new(
            TrustedReleaseConfig::new_with_cache_path(
                AttestationEnvironment::Production,
                REPOSITORY_URL,
                root_v1,
                poisoned_path,
            )
            .unwrap(),
        )
        .unwrap();
        let error = poisoned_manager.load_cache().await.unwrap_err();
        assert!(error
            .to_string()
            .contains("exceeds the supported 32 transitions"));

        let custom_bootstrap = make_root(7);
        let mut custom_entries = BTreeMap::from([(
            "root_history/7.root.json".to_string(),
            custom_bootstrap.clone(),
        )]);
        for version in 8..=40 {
            custom_entries.insert(
                format!("root_history/{version}.root.json"),
                make_root(version),
            );
        }
        let custom_final = custom_entries
            .get("root_history/40.root.json")
            .unwrap()
            .clone();
        let custom_chain = authenticated_root_authority_history(
            &custom_bootstrap,
            &custom_entries,
            40,
            &custom_final,
        )
        .unwrap();
        assert_eq!(custom_chain.repository.root.version, 39);
        assert!(custom_chain
            .error
            .unwrap()
            .to_string()
            .contains("from bootstrap version 7"));
    }

    #[tokio::test]
    async fn root_ceiling_requires_exact_404_before_cached_policy_fallback() {
        // This path exercises the production clock during cached fallback, so
        // keep its timestamp fixture valid relative to the test run instead of
        // tying the assertion to the date on which the test was authored.
        let now = jiff::Timestamp::now();
        let timestamp_expires = (now + jiff::SignedDuration::from_hours(24)).to_string();
        let online_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (mut repository, _) = build_policy_repository_generation(
            &timestamp_expires,
            false,
            true,
            &online_pair,
            1,
            7,
            true,
        );
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let (online_id, online_key) = tuf_key_entry(&online_pair);
        let make_root = |version| {
            tuf_root(
                version,
                &root_id,
                &root_key,
                &root_pair,
                &online_id,
                &online_key,
            )
        };
        let root_v1 = make_root(1);
        for version in 2..=33 {
            repository
                .metadata
                .insert(format!("{version}.root.json"), make_root(version));
        }
        let store = Arc::new(SnapshotStore::default());
        let cached_policy = resolve_policy_with_final_time(
            repository,
            Arc::clone(&store),
            &root_v1,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .expect("root 33 policy fixture must resolve");
        assert_eq!(cached_policy.repository_high_water.root.version, 33);
        let cached_entries = store.entries();
        let cached_channels = BTreeMap::from([(
            AttestationEnvironment::Production,
            CacheHighWater::from_policy(&cached_policy),
        )]);
        let directory = tempfile::tempdir().unwrap();

        let make_manager = |repository_url: &str, cache_path: &Path, timeout: Duration| {
            let config = TrustedReleaseConfig {
                environment: AttestationEnvironment::Production,
                repository_url: repository_url.to_string(),
                tuf_root: Arc::from(root_v1.clone()),
                cache_path: Some(cache_path.to_path_buf()),
            };
            TrustedReleaseManager {
                repository: HttpTufRepository::new_with_timeout(repository_url, true, timeout)
                    .unwrap(),
                config,
                refresh_coordinator: Arc::new(Mutex::new(RefreshCoordinator::default())),
                memory_state: Arc::new(RepositoryMemoryState::default()),
                fixed_policy: None,
            }
        };

        for status in [403, 408, 429, 500] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/tuf/metadata/34.root.json"))
                .respond_with(ResponseTemplate::new(status))
                .mount(&server)
                .await;
            let repository_url = format!("{}/tuf/", server.uri());
            let cache_path = directory
                .path()
                .join(format!("root34-status-{status}.json"));
            persist_cache(
                &cache_path,
                &repository_id(&repository_url),
                &cached_policy.repository_high_water,
                &cached_channels,
                &cached_entries,
            )
            .unwrap();
            let manager = make_manager(&repository_url, &cache_path, TUF_REQUEST_TIMEOUT);
            let error = manager
                .refresh_policy_inner_with_verifier(&FixtureBundleVerifier { fail: false })
                .await
                .expect_err("a non-404 root 34 response must not authorize cached root 33");
            if status == 403 {
                assert!(matches!(error, Error::TrustedReleasePolicy(_)));
            } else {
                assert!(matches!(error, Error::TrustedReleaseNetwork(_)));
            }

            if status == 500 {
                let restarted = make_manager(&repository_url, &cache_path, TUF_REQUEST_TIMEOUT);
                let error = restarted
                    .refresh_policy_inner_with_verifier(&FixtureBundleVerifier { fail: false })
                    .await
                    .expect_err(
                        "restart must not turn an unavailable root 34 probe into cached fallback",
                    );
                assert!(matches!(error, Error::TrustedReleaseNetwork(_)));
                let persisted = read_cache(&cache_path, &repository_id(&repository_url)).unwrap();
                assert_eq!(
                    persisted
                        .repository_high_water
                        .as_ref()
                        .unwrap()
                        .root
                        .version,
                    33
                );
                assert!(!persisted.entries.contains_key("root_history/34.root.json"));
            }
        }

        let timeout_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tuf/metadata/34.root.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(250))
                    .set_body_bytes(make_root(34)),
            )
            .mount(&timeout_server)
            .await;
        let timeout_url = format!("{}/tuf/", timeout_server.uri());
        let timeout_cache = directory.path().join("root34-timeout.json");
        persist_cache(
            &timeout_cache,
            &repository_id(&timeout_url),
            &cached_policy.repository_high_water,
            &cached_channels,
            &cached_entries,
        )
        .unwrap();
        let manager = make_manager(&timeout_url, &timeout_cache, Duration::from_millis(25));
        assert!(matches!(
            manager
                .refresh_policy_inner_with_verifier(&FixtureBundleVerifier { fail: false })
                .await,
            Err(Error::TrustedReleaseNetwork(_))
        ));

        let missing_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tuf/metadata/34.root.json"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&missing_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/tuf/metadata/timestamp.json"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&missing_server)
            .await;
        let missing_url = format!("{}/tuf/", missing_server.uri());
        let missing_cache = directory.path().join("root34-missing.json");
        persist_cache(
            &missing_cache,
            &repository_id(&missing_url),
            &cached_policy.repository_high_water,
            &cached_channels,
            &cached_entries,
        )
        .unwrap();
        let manager = make_manager(&missing_url, &missing_cache, TUF_REQUEST_TIMEOUT);
        let fallback = manager
            .refresh_policy_inner_with_verifier(&FixtureBundleVerifier { fail: false })
            .await
            .expect("exact root 34 404 permits a still-valid cached policy after timestamp 503");
        assert_eq!(fallback.policy_id(), cached_policy.policy_id());
        assert_eq!(fallback.repository_high_water.root.version, 33);
    }

    #[test]
    fn persistent_cache_is_atomic_repository_bound_and_keeps_both_channels() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.json");
        let repository_id = repository_id(REPOSITORY_URL);
        let channel_high_water = BTreeMap::from([
            (
                AttestationEnvironment::Production,
                CacheHighWater::for_test(7, 'a', 'd'),
            ),
            (
                AttestationEnvironment::Development,
                CacheHighWater::for_test(9, 'b', 'd'),
            ),
        ]);
        let entries = BTreeMap::from([
            ("timestamp.json".to_string(), b"timestamp".to_vec()),
            (
                "targets/channels/prod.json".to_string(),
                b"channel".to_vec(),
            ),
        ]);
        let repository_high_water = RepositoryHighWater::for_test();
        persist_cache(
            &path,
            &repository_id,
            &repository_high_water,
            &channel_high_water,
            &entries,
        )
        .unwrap();
        let cached = read_cache(&path, &repository_id).unwrap();
        assert_eq!(cached.entries, entries);
        assert_eq!(
            cached.repository_high_water.as_ref(),
            Some(&repository_high_water)
        );
        assert_eq!(cached.channel_high_water, channel_high_water);
        assert!(read_cache(&path, &"c".repeat(SHA256_HEX_LEN)).is_err());
    }

    #[test]
    fn unshipped_legacy_cache_schema_is_rejected_without_migration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy-cache.json");
        let repository_id = repository_id(REPOSITORY_URL);
        persist_cache(
            &path,
            &repository_id,
            &RepositoryHighWater::for_test(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        let mut cache: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        cache["schema"] =
            Value::String("https://attestations.trymaple.ai/schemas/sdk-tuf-cache/v3".to_string());
        std::fs::write(&path, serde_json::to_vec(&cache).unwrap()).unwrap();

        let error = read_cache(&path, &repository_id).unwrap_err();
        assert!(error
            .to_string()
            .contains("attestation policy cache identity mismatch"));
    }

    #[test]
    fn repository_global_cache_carries_root_history_between_channels() {
        let production = TrustedReleaseConfig::new(
            AttestationEnvironment::Production,
            REPOSITORY_URL,
            EMBEDDED_TUF_ROOT.to_vec(),
        )
        .unwrap();
        let development = TrustedReleaseConfig::new(
            AttestationEnvironment::Development,
            REPOSITORY_URL,
            EMBEDDED_TUF_ROOT.to_vec(),
        )
        .unwrap();
        assert_eq!(production.cache_path, development.cache_path);

        let root_key_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_key_id, root_key) = tuf_key_entry(&root_key_pair);
        let online_key_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (online_key_id, online_key) = tuf_key_entry(&online_key_pair);
        let root_v1 = tuf_root(
            1,
            &root_key_id,
            &root_key,
            &root_key_pair,
            &online_key_id,
            &online_key,
        );
        let root_v2 = tuf_root(
            2,
            &root_key_id,
            &root_key,
            &root_key_pair,
            &online_key_id,
            &online_key,
        );
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shared-cache.json");
        let repository_id = repository_id(REPOSITORY_URL);
        let channel_high_water = BTreeMap::from([(
            AttestationEnvironment::Production,
            CacheHighWater::for_test(7, 'a', 'd'),
        )]);
        let entries = BTreeMap::from([
            ("root.json".to_string(), root_v2.clone()),
            ("root_history/1.root.json".to_string(), root_v1.clone()),
            ("root_history/2.root.json".to_string(), root_v2),
        ]);
        persist_cache(
            &path,
            &repository_id,
            &RepositoryHighWater::for_test(),
            &channel_high_water,
            &entries,
        )
        .unwrap();

        // A development-channel manager reads the same repository cache even
        // though only production has a channel sequence floor so far.
        let cached_for_development = read_cache(&path, &repository_id).unwrap();
        assert!(!cached_for_development
            .channel_high_water
            .contains_key(&AttestationEnvironment::Development));
        let store = Arc::new(SnapshotStore::from_entries(cached_for_development.entries));
        let updater = Updater::new(MemoryRepository::default(), &root_v1)
            .unwrap()
            .with_store(store);
        assert_eq!(updater.trusted().root().version, 2);
    }

    #[test]
    fn channel_high_water_rejects_rollback_and_equal_sequence_mutation() {
        let prior = CacheHighWater::for_test(9, 'a', 'd');
        let rollback = TrustedReleasePolicy {
            environment: AttestationEnvironment::Production,
            sequence: 8,
            policy_id: prior.policy_id.clone(),
            repository_high_water: RepositoryHighWater::for_test(),
            valid_until: jiff::Timestamp::MAX,
            releases: Vec::new(),
        };
        let equivocation = TrustedReleasePolicy {
            environment: AttestationEnvironment::Production,
            sequence: 9,
            policy_id: "b".repeat(SHA256_HEX_LEN),
            repository_high_water: RepositoryHighWater::for_test(),
            valid_until: jiff::Timestamp::MAX,
            releases: Vec::new(),
        };
        let advance = TrustedReleasePolicy {
            environment: AttestationEnvironment::Production,
            sequence: 10,
            policy_id: "c".repeat(SHA256_HEX_LEN),
            repository_high_water: RepositoryHighWater::for_test(),
            valid_until: jiff::Timestamp::MAX,
            releases: Vec::new(),
        };

        assert!(enforce_high_water(&rollback, Some(&prior)).is_err());
        assert!(enforce_high_water(&equivocation, Some(&prior)).is_err());
        assert!(enforce_high_water(&advance, Some(&prior)).is_ok());
    }

    #[test]
    fn channel_high_water_merge_keeps_the_strictest_floor() {
        let older = CacheHighWater::for_test(7, 'a', 'd');
        let newer = CacheHighWater::for_test(8, 'b', 'd');
        assert_eq!(
            merge_high_water(Some(&older), Some(&newer)).unwrap(),
            Some(newer.clone())
        );

        let conflicting = CacheHighWater::for_test(newer.sequence, 'c', 'd');
        assert!(merge_high_water(Some(&newer), Some(&conflicting)).is_err());
    }

    #[test]
    fn repository_metadata_high_water_rejects_rollback_and_equivocation() {
        let prior = RepositoryHighWater::for_test();
        let mut rollback = prior.clone();
        rollback.timestamp.as_mut().unwrap().version = 0;
        assert!(enforce_repository_high_water(&rollback, Some(&prior)).is_err());

        let mut equivocation = prior.clone();
        equivocation.targets.as_mut().unwrap().sha256 = "b".repeat(SHA256_HEX_LEN);
        assert!(enforce_repository_high_water(&equivocation, Some(&prior)).is_err());

        let mut advance = prior.clone();
        advance.timestamp.as_mut().unwrap().version += 1;
        advance.timestamp.as_mut().unwrap().sha256 = "c".repeat(SHA256_HEX_LEN);
        assert!(enforce_repository_high_water(&advance, Some(&prior)).is_ok());
    }

    #[test]
    fn repository_high_water_requires_complete_disjoint_custody_history() {
        let mut missing_root = RepositoryHighWater::for_test();
        missing_root.authority_history.root = RoleAuthority::for_test('f').key_fingerprints;
        let error = validate_repository_high_water(&missing_root).unwrap_err();
        assert!(error
            .to_string()
            .contains("root authority history does not contain the current authority"));

        let mut crossed = RepositoryHighWater::for_test();
        crossed
            .authority_history
            .root
            .extend(crossed.authority_history.timestamp.iter().cloned());
        crossed.authority_history.root.sort();
        crossed.authority_history.root.dedup();
        let error = validate_repository_high_water(&crossed).unwrap_err();
        assert!(error.to_string().contains("key custody class violation"));
    }

    fn root_history_entries(roots: &[&[u8]]) -> BTreeMap<String, Vec<u8>> {
        roots
            .iter()
            .map(|bytes| {
                let root = root_transition_high_water(bytes).unwrap();
                (
                    format!("root_history/{}.root.json", root.root.version),
                    bytes.to_vec(),
                )
            })
            .collect()
    }

    fn verify_root_chain(roots: &[&[u8]]) {
        let (first, rest) = roots.split_first().unwrap();
        let mut trusted = sigstore_tuf::TrustedMetadataSet::from_root(first).unwrap();
        for root in rest {
            trusted.update_root(root).unwrap();
        }
    }

    #[test]
    fn authority_recovery_resets_only_direct_claims_and_dependent_descriptors() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let timestamp_a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (timestamp_a_id, timestamp_a_key) = tuf_key_entry(&timestamp_a_pair);
        let timestamp_b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (timestamp_b_id, timestamp_b_key) = tuf_key_entry(&timestamp_b_pair);
        let snapshot_a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (snapshot_a_id, snapshot_a_key) = tuf_key_entry(&snapshot_a_pair);
        let snapshot_b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (snapshot_b_id, snapshot_b_key) = tuf_key_entry(&snapshot_b_pair);
        let targets_a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (targets_a_id, targets_a_key) = tuf_key_entry(&targets_a_pair);
        let targets_b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (targets_b_id, targets_b_key) = tuf_key_entry(&targets_b_pair);

        let root_v1 = tuf_root_with_role_bindings(
            1,
            &root_id,
            &root_key,
            &root_pair,
            &[
                (&timestamp_a_id, &timestamp_a_key),
                (&snapshot_a_id, &snapshot_a_key),
                (&targets_a_id, &targets_a_key),
            ],
            (&[timestamp_a_id.as_str()], 1),
            (&[snapshot_a_id.as_str()], 1),
            (&[targets_a_id.as_str()], 1),
        );
        let prior = repository_floor_from_root(&root_v1, u64::MAX);
        let channels = BTreeMap::from([
            (
                AttestationEnvironment::Production,
                CacheHighWater {
                    sequence: 7,
                    policy_id: "a".repeat(SHA256_HEX_LEN),
                    authority: (&prior.targets_authority).into(),
                },
            ),
            (
                AttestationEnvironment::Development,
                CacheHighWater {
                    sequence: 9,
                    policy_id: "b".repeat(SHA256_HEX_LEN),
                    authority: (&prior.targets_authority).into(),
                },
            ),
        ]);

        let timestamp_root = tuf_root_with_role_bindings(
            2,
            &root_id,
            &root_key,
            &root_pair,
            &[
                (&timestamp_b_id, &timestamp_b_key),
                (&snapshot_a_id, &snapshot_a_key),
                (&targets_a_id, &targets_a_key),
            ],
            (&[timestamp_b_id.as_str()], 1),
            (&[snapshot_a_id.as_str()], 1),
            (&[targets_a_id.as_str()], 1),
        );
        verify_root_chain(&[&root_v1, &timestamp_root]);
        let observed = root_transition_high_water(&timestamp_root).unwrap();
        let advanced = advance_security_floors_through_root_history(
            Some(&prior),
            channels.clone(),
            &observed,
            &root_history_entries(&[&root_v1, &timestamp_root]),
        )
        .unwrap();
        let timestamp_state = advanced.repository.unwrap();
        assert!(timestamp_state.timestamp.is_none());
        assert!(timestamp_state.snapshot_descriptor.is_none());
        assert!(timestamp_state.snapshot.is_some());
        assert!(timestamp_state.targets_descriptor.is_some());
        assert!(timestamp_state.targets.is_some());
        assert_eq!(advanced.channels, channels);

        let snapshot_root = tuf_root_with_role_bindings(
            2,
            &root_id,
            &root_key,
            &root_pair,
            &[
                (&timestamp_a_id, &timestamp_a_key),
                (&snapshot_b_id, &snapshot_b_key),
                (&targets_a_id, &targets_a_key),
            ],
            (&[timestamp_a_id.as_str()], 1),
            (&[snapshot_b_id.as_str()], 1),
            (&[targets_a_id.as_str()], 1),
        );
        verify_root_chain(&[&root_v1, &snapshot_root]);
        let observed = root_transition_high_water(&snapshot_root).unwrap();
        let advanced = advance_security_floors_through_root_history(
            Some(&prior),
            channels.clone(),
            &observed,
            &root_history_entries(&[&root_v1, &snapshot_root]),
        )
        .unwrap();
        let snapshot_state = advanced.repository.unwrap();
        assert!(snapshot_state.timestamp.is_some());
        assert!(snapshot_state.snapshot_descriptor.is_none());
        assert!(snapshot_state.snapshot.is_none());
        assert!(snapshot_state.targets_descriptor.is_none());
        assert!(snapshot_state.targets.is_some());
        assert_eq!(advanced.channels, channels);

        let targets_root = tuf_root_with_role_bindings(
            2,
            &root_id,
            &root_key,
            &root_pair,
            &[
                (&timestamp_a_id, &timestamp_a_key),
                (&snapshot_a_id, &snapshot_a_key),
                (&targets_b_id, &targets_b_key),
            ],
            (&[timestamp_a_id.as_str()], 1),
            (&[snapshot_a_id.as_str()], 1),
            (&[targets_b_id.as_str()], 1),
        );
        verify_root_chain(&[&root_v1, &targets_root]);
        let observed = root_transition_high_water(&targets_root).unwrap();
        let advanced = advance_security_floors_through_root_history(
            Some(&prior),
            channels,
            &observed,
            &root_history_entries(&[&root_v1, &targets_root]),
        )
        .unwrap();
        let targets_state = advanced.repository.unwrap();
        assert!(targets_state.timestamp.is_some());
        assert!(targets_state.snapshot_descriptor.is_some());
        assert!(targets_state.snapshot.is_some());
        assert!(targets_state.targets_descriptor.is_none());
        assert!(targets_state.targets.is_none());
        assert!(advanced.channels.is_empty());
    }

    #[test]
    fn every_intermediate_root_transition_taints_floor_provenance() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (a_id, a_key) = tuf_key_entry(&a_pair);
        let b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (b_id, b_key) = tuf_key_entry(&b_pair);
        let c_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (c_id, c_key) = tuf_key_entry(&c_pair);

        let make_root = |version: u64, ids: &[&str], keys: &[(&str, &Value)]| {
            tuf_root_with_role_bindings(
                version,
                &root_id,
                &root_key,
                &root_pair,
                keys,
                (ids, 1),
                (ids, 1),
                (ids, 1),
            )
        };
        let root_v1 = make_root(1, &[a_id.as_str()], &[(&a_id, &a_key)]);
        let root_v2 = make_root(
            2,
            &[a_id.as_str(), b_id.as_str()],
            &[(&a_id, &a_key), (&b_id, &b_key)],
        );
        let root_v3 = make_root(
            3,
            &[b_id.as_str(), c_id.as_str()],
            &[(&b_id, &b_key), (&c_id, &c_key)],
        );
        verify_root_chain(&[&root_v1, &root_v2, &root_v3]);

        let prior = repository_floor_from_root(&root_v1, u64::MAX);
        let channels = BTreeMap::from([
            (
                AttestationEnvironment::Production,
                CacheHighWater::from_policy(&TrustedReleasePolicy {
                    environment: AttestationEnvironment::Production,
                    sequence: 7,
                    policy_id: "a".repeat(SHA256_HEX_LEN),
                    repository_high_water: prior.clone(),
                    valid_until: jiff::Timestamp::MAX,
                    releases: Vec::new(),
                }),
            ),
            (
                AttestationEnvironment::Development,
                CacheHighWater::from_policy(&TrustedReleasePolicy {
                    environment: AttestationEnvironment::Development,
                    sequence: 9,
                    policy_id: "b".repeat(SHA256_HEX_LEN),
                    repository_high_water: prior.clone(),
                    valid_until: jiff::Timestamp::MAX,
                    releases: Vec::new(),
                }),
            ),
        ]);
        let observed = root_transition_high_water(&root_v3).unwrap();
        let advanced = advance_security_floors_through_root_history(
            Some(&prior),
            channels,
            &observed,
            &root_history_entries(&[&root_v1, &root_v2, &root_v3]),
        )
        .unwrap();
        let repository = advanced.repository.unwrap();
        let fingerprint = |key: &Value| {
            let parsed: sigstore_tuf::Key = serde_json::from_value(key.clone()).unwrap();
            let verification = parsed.verification_key().unwrap();
            key_custody_fingerprint(&parsed.scheme, verification.as_bytes()).unwrap()
        };
        let expected = BTreeSet::from([
            fingerprint(&a_key),
            fingerprint(&b_key),
            fingerprint(&c_key),
        ]);
        let assert_tainted = |provenance: &AuthorityProvenance| {
            assert_eq!(
                provenance
                    .key_fingerprints
                    .iter()
                    .cloned()
                    .collect::<BTreeSet<_>>(),
                expected
            );
        };
        assert_tainted(
            repository
                .timestamp
                .as_ref()
                .unwrap()
                .authority
                .as_ref()
                .unwrap(),
        );
        assert_tainted(
            repository
                .snapshot_descriptor
                .as_ref()
                .unwrap()
                .authority
                .as_ref()
                .unwrap(),
        );
        assert_tainted(
            repository
                .snapshot_descriptor
                .as_ref()
                .unwrap()
                .referenced_authority
                .as_ref()
                .unwrap(),
        );
        assert_tainted(
            repository
                .snapshot
                .as_ref()
                .unwrap()
                .authority
                .as_ref()
                .unwrap(),
        );
        assert_tainted(
            repository
                .targets_descriptor
                .as_ref()
                .unwrap()
                .authority
                .as_ref()
                .unwrap(),
        );
        assert_tainted(
            repository
                .targets_descriptor
                .as_ref()
                .unwrap()
                .referenced_authority
                .as_ref()
                .unwrap(),
        );
        assert_tainted(
            repository
                .targets
                .as_ref()
                .unwrap()
                .authority
                .as_ref()
                .unwrap(),
        );
        for channel in advanced.channels.values() {
            assert_tainted(&channel.authority);
        }
    }

    #[tokio::test]
    async fn first_complete_refresh_records_intermediate_retired_online_keys() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (a_id, a_key) = tuf_key_entry(&a_pair);
        let b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (b_id, b_key) = tuf_key_entry(&b_pair);
        let c_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (c_id, c_key) = tuf_key_entry(&c_pair);

        let make_root = |version: u64, id: &str, key: &Value| {
            tuf_root_with_role_bindings(
                version,
                &root_id,
                &root_key,
                &root_pair,
                &[(id, key)],
                (&[id], 1),
                (&[id], 1),
                (&[id], 1),
            )
        };
        let root_v1 = make_root(1, &a_id, &a_key);
        let root_v2 = make_root(2, &b_id, &b_key);
        let root_v3 = make_root(3, &c_id, &c_key);
        verify_root_chain(&[&root_v1, &root_v2, &root_v3]);

        let (mut repository, _) = build_policy_repository_generation(
            "2026-08-30T00:00:00Z",
            false,
            true,
            &c_pair,
            1,
            7,
            true,
        );
        repository
            .metadata
            .insert("2.root.json".to_string(), root_v2.clone());
        repository
            .metadata
            .insert("3.root.json".to_string(), root_v3.clone());
        let store = Arc::new(SnapshotStore::default());
        let now = "2026-08-29T00:00:00Z".parse().unwrap();
        let policy = resolve_policy_with_final_time(
            repository,
            Arc::clone(&store),
            &root_v1,
            AttestationEnvironment::Production,
            now,
            &FixtureBundleVerifier { fail: false },
            || now,
        )
        .await
        .expect("the first complete refresh should traverse the root chain");

        let expected = BTreeSet::from([
            tuf_key_fingerprint(&a_key),
            tuf_key_fingerprint(&b_key),
            tuf_key_fingerprint(&c_key),
        ]);
        for history in [
            &policy.repository_high_water.authority_history.timestamp,
            &policy.repository_high_water.authority_history.snapshot,
            &policy.repository_high_water.authority_history.targets,
        ] {
            assert_eq!(history.iter().cloned().collect::<BTreeSet<_>>(), expected);
        }

        let root_v4 = make_root(4, &a_id, &a_key);
        verify_root_chain(&[&root_v1, &root_v2, &root_v3, &root_v4]);
        let mut entries = store.entries();
        entries.insert("root_history/4.root.json".to_string(), root_v4.clone());
        let error = advance_security_floors_through_root_history(
            Some(&policy.repository_high_water),
            BTreeMap::from([(
                AttestationEnvironment::Production,
                CacheHighWater::from_policy(&policy),
            )]),
            &root_transition_high_water(&root_v4).unwrap(),
            &entries,
        )
        .err()
        .expect("a key retired during the first refresh must not be reauthorized");
        assert!(error
            .to_string()
            .contains("reauthorizes retired key material"));
    }

    #[tokio::test]
    async fn first_partial_root_only_refresh_persists_intermediate_retired_keys() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (a_id, a_key) = tuf_key_entry(&a_pair);
        let b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (b_id, b_key) = tuf_key_entry(&b_pair);
        let c_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (c_id, c_key) = tuf_key_entry(&c_pair);
        let make_root = |version: u64, id: &str, key: &Value| {
            tuf_root_with_role_bindings(
                version,
                &root_id,
                &root_key,
                &root_pair,
                &[(id, key)],
                (&[id], 1),
                (&[id], 1),
                (&[id], 1),
            )
        };
        let root_v1 = make_root(1, &a_id, &a_key);
        let root_v2 = make_root(2, &b_id, &b_key);
        let root_v3 = make_root(3, &c_id, &c_key);
        verify_root_chain(&[&root_v1, &root_v2, &root_v3]);

        let repository = MemoryRepository {
            metadata: HashMap::from([
                ("2.root.json".to_string(), root_v2.clone()),
                ("3.root.json".to_string(), root_v3.clone()),
            ]),
            targets: HashMap::new(),
        };
        let store = Arc::new(SnapshotStore::default());
        let mut updater = Updater::new(repository, &root_v1)
            .unwrap()
            .with_config(tuf_updater_config())
            .with_store(Arc::clone(&store));
        let now = "2026-08-29T00:00:00Z".parse().unwrap();
        assert!(updater.refresh(now).await.is_err());
        let root_chain = authenticated_root_authority_history(
            &root_v1,
            &store.entries(),
            updater.trusted().root().version,
            updater.trusted().root_bytes(),
        )
        .unwrap();
        assert!(root_chain.error.is_none());
        let repository_high_water = partial_repository_high_water(
            &updater,
            &store,
            root_chain.repository.authority_history,
        )
        .unwrap();
        assert!(repository_high_water.timestamp.is_none());

        let cache_dir = tempfile::tempdir().unwrap();
        let cache_path = cache_dir.path().join("root-only.json");
        let repository_id = "a".repeat(SHA256_HEX_LEN);
        persist_cache(
            &cache_path,
            &repository_id,
            &repository_high_water,
            &BTreeMap::new(),
            &store.entries(),
        )
        .unwrap();
        let mut restarted = read_cache(&cache_path, &repository_id).unwrap();
        let root_v4 = make_root(4, &a_id, &a_key);
        verify_root_chain(&[&root_v1, &root_v2, &root_v3, &root_v4]);
        restarted
            .entries
            .insert("root_history/4.root.json".to_string(), root_v4.clone());
        let error = advance_security_floors_through_root_history(
            restarted.repository_high_water.as_ref(),
            restarted.channel_high_water,
            &root_transition_high_water(&root_v4).unwrap(),
            &restarted.entries,
        )
        .err()
        .expect("restart must preserve keys retired by a root-only first refresh");
        assert!(error
            .to_string()
            .contains("reauthorizes retired key material"));
    }

    #[test]
    fn first_root_chain_rejects_retired_and_cross_role_key_reuse() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (a_id, a_key) = tuf_key_entry(&a_pair);
        let b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (b_id, b_key) = tuf_key_entry(&b_pair);
        let c_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (c_id, c_key) = tuf_key_entry(&c_pair);
        let d_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (d_id, d_key) = tuf_key_entry(&d_pair);

        let same_role_root = |version: u64, id: &str, key: &Value| {
            tuf_root_with_role_bindings(
                version,
                &root_id,
                &root_key,
                &root_pair,
                &[(id, key)],
                (&[id], 1),
                (&[id], 1),
                (&[id], 1),
            )
        };
        let root_v1 = same_role_root(1, &a_id, &a_key);
        let root_v2 = same_role_root(2, &b_id, &b_key);
        let root_v3 = same_role_root(3, &a_id, &a_key);
        verify_root_chain(&[&root_v1, &root_v2, &root_v3]);
        let root_chain = authenticated_root_authority_history(
            &root_v1,
            &root_history_entries(&[&root_v1, &root_v2, &root_v3]),
            3,
            &root_v3,
        )
        .unwrap();
        let error = root_chain.error.unwrap();
        assert!(error
            .to_string()
            .contains("reauthorizes retired key material"));

        let cross_role_v1 = tuf_root_with_role_bindings(
            1,
            &root_id,
            &root_key,
            &root_pair,
            &[(&a_id, &a_key), (&b_id, &b_key), (&c_id, &c_key)],
            (&[a_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[c_id.as_str()], 1),
        );
        let cross_role_v2 = tuf_root_with_role_bindings(
            2,
            &root_id,
            &root_key,
            &root_pair,
            &[(&d_id, &d_key), (&b_id, &b_key), (&c_id, &c_key)],
            (&[d_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[c_id.as_str()], 1),
        );
        let cross_role_v3 = tuf_root_with_role_bindings(
            3,
            &root_id,
            &root_key,
            &root_pair,
            &[(&d_id, &d_key), (&b_id, &b_key), (&a_id, &a_key)],
            (&[d_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[a_id.as_str()], 1),
        );
        verify_root_chain(&[&cross_role_v1, &cross_role_v2, &cross_role_v3]);
        let root_chain = authenticated_root_authority_history(
            &cross_role_v1,
            &root_history_entries(&[&cross_role_v1, &cross_role_v2, &cross_role_v3]),
            3,
            &cross_role_v3,
        )
        .unwrap();
        let error = root_chain.error.unwrap();
        assert!(error
            .to_string()
            .contains("reauthorizes retired key material"));
    }

    #[tokio::test]
    async fn invalid_first_root_chain_journals_longest_valid_prefix_across_restart() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (a_id, a_key) = tuf_key_entry(&a_pair);
        let b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (b_id, b_key) = tuf_key_entry(&b_pair);
        let c_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (c_id, c_key) = tuf_key_entry(&c_pair);
        let make_root = |version: u64, id: &str, key: &Value| {
            tuf_root_with_role_bindings(
                version,
                &root_id,
                &root_key,
                &root_pair,
                &[(id, key)],
                (&[id], 1),
                (&[id], 1),
                (&[id], 1),
            )
        };
        let root_v1 = make_root(1, &a_id, &a_key);
        let root_v2 = make_root(2, &b_id, &b_key);
        let rejected_root_v3 = make_root(3, &a_id, &a_key);
        verify_root_chain(&[&root_v1, &root_v2, &rejected_root_v3]);

        let store = Arc::new(SnapshotStore::from_entries(root_history_entries(&[
            &root_v1,
            &root_v2,
            &rejected_root_v3,
        ])));
        let now = "2026-08-29T00:00:00Z".parse().unwrap();
        let observation = capture_authenticated_observation(Arc::clone(&store), &root_v1, now)
            .await
            .unwrap();
        assert_eq!(observation.repository_high_water.root.version, 2);
        assert!(observation.repository_high_water.timestamp.is_none());
        assert!(observation.channel_high_water.is_empty());
        assert!(observation
            .error
            .as_ref()
            .unwrap()
            .to_string()
            .contains("reauthorizes retired key material"));
        assert!(observation.entries.contains_key("root_history/2.root.json"));
        assert!(!observation.entries.contains_key("root_history/3.root.json"));
        let retained_root =
            root_transition_high_water(observation.entries.get("root.json").unwrap()).unwrap();
        assert_eq!(retained_root.root.version, 2);

        let cache_dir = tempfile::tempdir().unwrap();
        let cache_path = cache_dir.path().join("valid-prefix.json");
        let repository_id = "a".repeat(SHA256_HEX_LEN);
        persist_cache(
            &cache_path,
            &repository_id,
            &observation.repository_high_water,
            &observation.channel_high_water,
            &observation.entries,
        )
        .unwrap();
        let restarted = read_cache(&cache_path, &repository_id).unwrap();
        let replay_error = enforce_repository_high_water(
            &root_transition_high_water(&root_v1).unwrap(),
            restarted.repository_high_water.as_ref(),
        )
        .unwrap_err();
        assert!(replay_error.to_string().contains("root metadata rollback"));

        let corrected_root_v3 = make_root(3, &c_id, &c_key);
        verify_root_chain(&[&root_v1, &root_v2, &corrected_root_v3]);
        let mut corrected_entries = restarted.entries;
        corrected_entries.insert(
            "root_history/3.root.json".to_string(),
            corrected_root_v3.clone(),
        );
        let recovered = advance_security_floors_through_root_history(
            restarted.repository_high_water.as_ref(),
            restarted.channel_high_water,
            &root_transition_high_water(&corrected_root_v3).unwrap(),
            &corrected_entries,
        )
        .expect("a corrected root at the rejected version should recover");
        assert_eq!(recovered.repository.unwrap().root.version, 3);
    }

    #[test]
    fn root_key_material_must_be_disjoint_from_online_roles() {
        let shared_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (shared_id, shared_key) = tuf_key_entry(&shared_pair);
        let root = tuf_root_with_custom_root_role(
            1,
            &[(&shared_id, &shared_key)],
            (&[shared_id.as_str()], 1),
            (&[shared_id.as_str()], 1),
            (&[shared_id.as_str()], 1),
            (&[shared_id.as_str()], 1),
            &[(&shared_id, &shared_pair)],
        );
        let trusted = sigstore_tuf::TrustedMetadataSet::from_root(&root).unwrap();

        let error = match root_role_authorities(trusted.root()) {
            Err(error) => error,
            Ok(_) => panic!("shared root and online key material was accepted"),
        };
        assert!(error
            .to_string()
            .contains("root role key material must be disjoint"));
    }

    #[test]
    fn root_key_alias_cannot_hide_online_key_reuse() {
        let shared_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, shared_key) = tuf_key_entry(&shared_pair);
        let online_alias = format!("online-alias-{root_id}");
        let root = tuf_root_with_custom_root_role(
            1,
            &[(&root_id, &shared_key), (&online_alias, &shared_key)],
            (&[root_id.as_str()], 1),
            (&[online_alias.as_str()], 1),
            (&[online_alias.as_str()], 1),
            (&[online_alias.as_str()], 1),
            &[(&root_id, &shared_pair)],
        );
        let trusted = sigstore_tuf::TrustedMetadataSet::from_root(&root).unwrap();

        let error = match root_role_authorities(trusted.root()) {
            Err(error) => error,
            Ok(_) => panic!("aliased root key material was accepted online"),
        };
        assert!(error
            .to_string()
            .contains("root role key material must be disjoint"));
    }

    #[test]
    fn online_roles_may_share_key_material_when_root_is_separate() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let online_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (online_id, online_key) = tuf_key_entry(&online_pair);
        let root = tuf_root(1, &root_id, &root_key, &root_pair, &online_id, &online_key);
        let trusted = sigstore_tuf::TrustedMetadataSet::from_root(&root).unwrap();

        let authorities = root_role_authorities(trusted.root()).unwrap();
        assert_eq!(authorities.timestamp, authorities.snapshot);
        assert_eq!(authorities.snapshot, authorities.targets);
    }

    #[tokio::test]
    async fn root_and_online_custody_classes_cannot_cross_across_history_or_restart() {
        let r_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (r_id, r_key) = tuf_key_entry(&r_pair);
        let s_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (s_id, s_key) = tuf_key_entry(&s_pair);
        let a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (a_id, a_key) = tuf_key_entry(&a_pair);
        let b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (b_id, b_key) = tuf_key_entry(&b_pair);

        let root_v1 = tuf_root_with_custom_root_role(
            1,
            &[(&r_id, &r_key), (&a_id, &a_key)],
            (&[r_id.as_str()], 1),
            (&[a_id.as_str()], 1),
            (&[a_id.as_str()], 1),
            (&[a_id.as_str()], 1),
            &[(&r_id, &r_pair)],
        );
        let root_v2 = tuf_root_with_custom_root_role(
            2,
            &[
                (&r_id, &r_key),
                (&s_id, &s_key),
                (&a_id, &a_key),
                (&b_id, &b_key),
            ],
            (&[s_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            &[(&r_id, &r_pair), (&s_id, &s_pair)],
        );
        verify_root_chain(&[&root_v1, &root_v2]);
        let history_v2 = root_history_entries(&[&root_v1, &root_v2]);
        let chain_v2 =
            authenticated_root_authority_history(&root_v1, &history_v2, 2, &root_v2).unwrap();
        assert!(chain_v2.error.is_none());
        assert_eq!(chain_v2.repository.root.version, 2);

        // A direct R/A swap is individually disjoint in root v2 and validly
        // cross-signed, but violates the repository-lifetime custody classes.
        let swapped_v2 = tuf_root_with_custom_root_role(
            2,
            &[(&r_id, &r_key), (&a_id, &a_key)],
            (&[a_id.as_str()], 1),
            (&[r_id.as_str()], 1),
            (&[r_id.as_str()], 1),
            (&[r_id.as_str()], 1),
            &[(&r_id, &r_pair), (&a_id, &a_pair)],
        );
        verify_root_chain(&[&root_v1, &swapped_v2]);
        let swapped = authenticated_root_authority_history(
            &root_v1,
            &root_history_entries(&[&root_v1, &swapped_v2]),
            2,
            &swapped_v2,
        )
        .unwrap();
        assert_eq!(swapped.repository.root.version, 1);
        assert!(swapped
            .error
            .unwrap()
            .to_string()
            .contains("key custody class violation"));

        let swapped_store = Arc::new(SnapshotStore::from_entries(root_history_entries(&[
            &root_v1,
            &swapped_v2,
        ])));
        let observation = capture_authenticated_observation(
            swapped_store,
            &root_v1,
            "2026-08-29T00:00:00Z".parse().unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(observation.repository_high_water.root.version, 1);
        assert!(observation.repository_high_water.timestamp.is_none());
        assert!(observation
            .error
            .unwrap()
            .to_string()
            .contains("key custody class violation"));
        assert!(!observation.entries.contains_key("root_history/2.root.json"));

        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("custody-ledger.json");
        let repository_id = repository_id(REPOSITORY_URL);
        let mut persisted_entries = history_v2;
        persisted_entries.insert("root.json".to_string(), root_v2.clone());
        persist_cache(
            &cache_path,
            &repository_id,
            &chain_v2.repository,
            &BTreeMap::new(),
            &persisted_entries,
        )
        .unwrap();
        let restarted = read_cache(&cache_path, &repository_id).unwrap();

        let root_alias = format!("root-alias-{r_id}");
        let prior_root_becomes_online = tuf_root_with_custom_root_role(
            3,
            &[(&s_id, &s_key), (&root_alias, &r_key), (&b_id, &b_key)],
            (&[s_id.as_str()], 1),
            (&[root_alias.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            &[(&s_id, &s_pair)],
        );
        verify_root_chain(&[&root_v1, &root_v2, &prior_root_becomes_online]);

        let online_alias = format!("online-alias-{a_id}");
        let prior_online_becomes_root = tuf_root_with_custom_root_role(
            3,
            &[(&s_id, &s_key), (&online_alias, &a_key), (&b_id, &b_key)],
            (&[online_alias.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            &[(&s_id, &s_pair), (&online_alias, &a_pair)],
        );
        verify_root_chain(&[&root_v1, &root_v2, &prior_online_becomes_root]);

        for rejected in [prior_root_becomes_online, prior_online_becomes_root] {
            let mut entries = restarted.entries.clone();
            entries.insert("root_history/3.root.json".to_string(), rejected.clone());
            let error = advance_security_floors_through_root_history(
                restarted.repository_high_water.as_ref(),
                restarted.channel_high_water.clone(),
                &root_transition_high_water(&rejected).unwrap(),
                &entries,
            )
            .err()
            .expect("crossing a historical custody class must fail");
            assert!(error.to_string().contains("key custody class violation"));
        }
    }

    #[test]
    fn intermediate_root_with_duplicate_key_material_is_rejected() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (a_id, a_key) = tuf_key_entry(&a_pair);
        let alias_id = format!("alias-{a_id}");
        let b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (b_id, b_key) = tuf_key_entry(&b_pair);
        let root_v1 = tuf_root_with_role_bindings(
            1,
            &root_id,
            &root_key,
            &root_pair,
            &[(&a_id, &a_key)],
            (&[a_id.as_str()], 1),
            (&[a_id.as_str()], 1),
            (&[a_id.as_str()], 1),
        );
        let root_v2 = tuf_root_with_role_bindings(
            2,
            &root_id,
            &root_key,
            &root_pair,
            &[(&a_id, &a_key), (&alias_id, &a_key)],
            (&[a_id.as_str(), alias_id.as_str()], 2),
            (&[a_id.as_str()], 1),
            (&[a_id.as_str()], 1),
        );
        let root_v3 = tuf_root_with_role_bindings(
            3,
            &root_id,
            &root_key,
            &root_pair,
            &[(&b_id, &b_key)],
            (&[b_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[b_id.as_str()], 1),
        );
        verify_root_chain(&[&root_v1, &root_v2, &root_v3]);
        let entries = BTreeMap::from([
            ("root_history/1.root.json".to_string(), root_v1),
            ("root_history/2.root.json".to_string(), root_v2),
            ("root_history/3.root.json".to_string(), root_v3),
        ]);
        let error = validate_authenticated_root_history(&entries, 3).unwrap_err();
        assert!(error.to_string().contains("duplicate aliases"));
    }

    #[test]
    fn intermediate_root_role_alias_threshold_is_rejected_even_if_final_root_is_clean() {
        let root_a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_a_id, root_a_key) = tuf_key_entry(&root_a_pair);
        let root_a_alias_1 = format!("alias-1-{root_a_id}");
        let root_a_alias_2 = format!("alias-2-{root_a_id}");
        let root_b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_b_id, root_b_key) = tuf_key_entry(&root_b_pair);
        let online_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (online_id, online_key) = tuf_key_entry(&online_pair);

        let root_v1 = tuf_root_with_custom_root_role(
            1,
            &[(&root_a_id, &root_a_key), (&online_id, &online_key)],
            (&[root_a_id.as_str()], 1),
            (&[online_id.as_str()], 1),
            (&[online_id.as_str()], 1),
            (&[online_id.as_str()], 1),
            &[(&root_a_id, &root_a_pair)],
        );
        let root_v2 = tuf_root_with_custom_root_role(
            2,
            &[
                (&root_a_id, &root_a_key),
                (&root_a_alias_1, &root_a_key),
                (&root_a_alias_2, &root_a_key),
                (&online_id, &online_key),
            ],
            (&[root_a_alias_1.as_str(), root_a_alias_2.as_str()], 2),
            (&[online_id.as_str()], 1),
            (&[online_id.as_str()], 1),
            (&[online_id.as_str()], 1),
            &[
                (&root_a_id, &root_a_pair),
                (&root_a_alias_1, &root_a_pair),
                (&root_a_alias_2, &root_a_pair),
            ],
        );
        let root_v3 = tuf_root_with_custom_root_role(
            3,
            &[(&root_b_id, &root_b_key), (&online_id, &online_key)],
            (&[root_b_id.as_str()], 1),
            (&[online_id.as_str()], 1),
            (&[online_id.as_str()], 1),
            (&[online_id.as_str()], 1),
            &[
                (&root_a_alias_1, &root_a_pair),
                (&root_a_alias_2, &root_a_pair),
                (&root_b_id, &root_b_pair),
            ],
        );

        // sigstore-tuf 0.11 counts distinct declared key IDs, so it accepts
        // root v2's two aliases as a threshold of two and lets that malformed
        // intermediate root authenticate the otherwise-clean root v3.
        verify_root_chain(&[&root_v1, &root_v2, &root_v3]);
        let entries = BTreeMap::from([
            ("root_history/1.root.json".to_string(), root_v1),
            ("root_history/2.root.json".to_string(), root_v2),
            ("root_history/3.root.json".to_string(), root_v3),
        ]);
        let error = validate_authenticated_root_history(&entries, 3).unwrap_err();
        assert!(error.to_string().contains("role 'root'"));
        assert!(error.to_string().contains("duplicate aliases"));
    }

    #[test]
    fn retired_online_key_material_cannot_be_reauthorized() {
        let mut prior = RepositoryHighWater::for_test();
        let retired = prior.timestamp_authority.clone();
        let replacement = RoleAuthority::for_test('e');
        prior.timestamp_authority = replacement.clone();
        prior.authority_history.timestamp = merge_authority_key_history(
            "timestamp",
            &retired.key_fingerprints,
            &replacement.key_fingerprints,
        )
        .unwrap();
        prior.timestamp = None;
        prior.snapshot_descriptor = None;

        let mut observed = prior.clone();
        observed.root.version += 1;
        observed.root.sha256 = "f".repeat(SHA256_HEX_LEN);
        observed.timestamp_authority = RoleAuthority {
            threshold: 2,
            key_fingerprints: prior.authority_history.timestamp.clone(),
        };
        observed.authority_history = AuthorityHistory::from_authorities(
            &observed.root_authority,
            &observed.timestamp_authority,
            &observed.snapshot_authority,
            &observed.targets_authority,
        );
        let merged = merge_repository_observation(Some(&prior), &observed);
        assert!(merged
            .error
            .unwrap()
            .to_string()
            .contains("reauthorizes retired key material"));
    }

    #[test]
    fn retired_key_cannot_return_in_another_online_role_after_safe_replacement() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (a_id, a_key) = tuf_key_entry(&a_pair);
        let b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (b_id, b_key) = tuf_key_entry(&b_pair);
        let c_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (c_id, c_key) = tuf_key_entry(&c_pair);
        let d_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (d_id, d_key) = tuf_key_entry(&d_pair);

        let root_v1 = tuf_root_with_role_bindings(
            1,
            &root_id,
            &root_key,
            &root_pair,
            &[(&a_id, &a_key), (&b_id, &b_key), (&c_id, &c_key)],
            (&[a_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[c_id.as_str()], 1),
        );
        let root_v2 = tuf_root_with_role_bindings(
            2,
            &root_id,
            &root_key,
            &root_pair,
            &[(&d_id, &d_key), (&b_id, &b_key), (&c_id, &c_key)],
            (&[d_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[c_id.as_str()], 1),
        );
        let root_v3 = tuf_root_with_role_bindings(
            3,
            &root_id,
            &root_key,
            &root_pair,
            &[(&d_id, &d_key), (&b_id, &b_key), (&a_id, &a_key)],
            (&[d_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[a_id.as_str()], 1),
        );
        verify_root_chain(&[&root_v1, &root_v2, &root_v3]);

        let initial = repository_floor_from_root(&root_v1, 7);
        let root_v2_floor = root_transition_high_water(&root_v2).unwrap();
        let advanced = advance_security_floors_through_root_history(
            Some(&initial),
            BTreeMap::new(),
            &root_v2_floor,
            &root_history_entries(&[&root_v1, &root_v2]),
        )
        .unwrap()
        .repository
        .unwrap();
        let root_v3_floor = root_transition_high_water(&root_v3).unwrap();
        let error = advance_security_floors_through_root_history(
            Some(&advanced),
            BTreeMap::new(),
            &root_v3_floor,
            &root_history_entries(&[&root_v1, &root_v2, &root_v3]),
        )
        .err()
        .expect("retired cross-role key must be rejected");
        assert!(error
            .to_string()
            .contains("reauthorizes retired key material"));
    }

    #[test]
    fn root_history_must_anchor_to_the_exact_in_memory_fork() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (a_id, a_key) = tuf_key_entry(&a_pair);
        let b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (b_id, b_key) = tuf_key_entry(&b_pair);
        let c_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (c_id, c_key) = tuf_key_entry(&c_pair);

        let make_root = |version: u64, ids: &[&str], keys: &[(&str, &Value)]| {
            tuf_root_with_role_bindings(
                version,
                &root_id,
                &root_key,
                &root_pair,
                keys,
                (ids, 1),
                (ids, 1),
                (ids, 1),
            )
        };
        let root_v1 = make_root(1, &[a_id.as_str()], &[(&a_id, &a_key)]);
        let root_v2_memory = make_root(
            2,
            &[a_id.as_str(), b_id.as_str()],
            &[(&a_id, &a_key), (&b_id, &b_key)],
        );
        let root_v2_fork = make_root(
            2,
            &[a_id.as_str(), c_id.as_str()],
            &[(&a_id, &a_key), (&c_id, &c_key)],
        );
        let root_v3_fork = make_root(3, &[c_id.as_str()], &[(&c_id, &c_key)]);
        verify_root_chain(&[&root_v1, &root_v2_memory]);
        verify_root_chain(&[&root_v1, &root_v2_fork, &root_v3_fork]);

        let initial = repository_floor_from_root(&root_v1, 7);
        let memory = advance_security_floors_through_root_history(
            Some(&initial),
            BTreeMap::new(),
            &root_transition_high_water(&root_v2_memory).unwrap(),
            &root_history_entries(&[&root_v1, &root_v2_memory]),
        )
        .unwrap()
        .repository
        .unwrap();
        let error = advance_security_floors_through_root_history(
            Some(&memory),
            BTreeMap::new(),
            &root_transition_high_water(&root_v3_fork).unwrap(),
            &root_history_entries(&[&root_v1, &root_v2_fork, &root_v3_fork]),
        )
        .err()
        .expect("root-history fork must be rejected");
        assert!(error
            .to_string()
            .contains("forks from in-memory root version 2"));
    }

    #[test]
    fn root_history_transition_must_be_cross_signed_by_its_predecessor() {
        let root_a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_a_id, root_a_key) = tuf_key_entry(&root_a_pair);
        let root_b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_b_id, root_b_key) = tuf_key_entry(&root_b_pair);
        let online_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (online_id, online_key) = tuf_key_entry(&online_pair);
        let root_v1 = tuf_root(
            1,
            &root_a_id,
            &root_a_key,
            &root_a_pair,
            &online_id,
            &online_key,
        );
        // This root is internally well formed and self-signed, but root v1
        // never authorized root-B, so it is not a valid TUF transition.
        let root_v2 = tuf_root(
            2,
            &root_b_id,
            &root_b_key,
            &root_b_pair,
            &online_id,
            &online_key,
        );
        let error = advance_security_floors_through_root_history(
            Some(&repository_floor_from_root(&root_v1, 7)),
            BTreeMap::new(),
            &root_transition_high_water(&root_v2).unwrap(),
            &root_history_entries(&[&root_v1, &root_v2]),
        )
        .err()
        .expect("self-signed non-transitioning root must be rejected");
        assert!(error
            .to_string()
            .contains("not authenticated by the preceding root"));
    }

    #[test]
    fn root_history_cannot_skip_an_intermediate_version() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let online_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (online_id, online_key) = tuf_key_entry(&online_pair);
        let root_v1 = tuf_root(1, &root_id, &root_key, &root_pair, &online_id, &online_key);
        let root_v3 = tuf_root(3, &root_id, &root_key, &root_pair, &online_id, &online_key);
        let entries = root_history_entries(&[&root_v1, &root_v3]);

        let error = advance_security_floors_through_root_history(
            Some(&repository_floor_from_root(&root_v1, 7)),
            BTreeMap::new(),
            &root_transition_high_water(&root_v3).unwrap(),
            &entries,
        )
        .err()
        .expect("a root chain missing version 2 must fail closed");

        assert!(error
            .to_string()
            .contains("missing authenticated root transition 2"));
    }

    #[test]
    fn memory_root_history_reconciles_newer_disk_child_and_channel_floors() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (a_id, a_key) = tuf_key_entry(&a_pair);
        let b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (b_id, b_key) = tuf_key_entry(&b_pair);
        let make_root = |version: u64, ids: &[&str], keys: &[(&str, &Value)]| {
            tuf_root_with_role_bindings(
                version,
                &root_id,
                &root_key,
                &root_pair,
                keys,
                (ids, 1),
                (ids, 1),
                (ids, 1),
            )
        };
        let root_v1 = make_root(1, &[a_id.as_str()], &[(&a_id, &a_key)]);
        let root_v2 = make_root(
            2,
            &[a_id.as_str(), b_id.as_str()],
            &[(&a_id, &a_key), (&b_id, &b_key)],
        );
        verify_root_chain(&[&root_v1, &root_v2]);

        let cached_repository = repository_floor_from_root(&root_v1, 99);
        let cached_channel = CacheHighWater::from_policy(&TrustedReleasePolicy {
            environment: AttestationEnvironment::Production,
            sequence: 12,
            policy_id: "a".repeat(SHA256_HEX_LEN),
            repository_high_water: cached_repository.clone(),
            valid_until: jiff::Timestamp::MAX,
            releases: Vec::new(),
        });
        let memory_initial = repository_floor_from_root(&root_v1, 7);
        let history = root_history_entries(&[&root_v1, &root_v2]);
        let memory = advance_security_floors_through_root_history(
            Some(&memory_initial),
            BTreeMap::new(),
            &root_transition_high_water(&root_v2).unwrap(),
            &history,
        )
        .unwrap()
        .repository
        .unwrap();

        let merged = merge_loaded_security_high_water_states(
            Some(&cached_repository),
            BTreeMap::from([(AttestationEnvironment::Production, cached_channel)]),
            Some(&memory),
            &BTreeMap::new(),
            &root_history_entries(&[&root_v1]),
            &history,
        )
        .unwrap();
        let repository = merged.repository.unwrap();
        assert_eq!(repository.root.version, 2);
        assert_eq!(repository.timestamp.as_ref().unwrap().version, 99);
        assert_eq!(repository.snapshot.as_ref().unwrap().version, 99);
        assert_eq!(repository.targets.as_ref().unwrap().version, 99);
        assert_eq!(
            merged
                .channels
                .get(&AttestationEnvironment::Production)
                .unwrap()
                .sequence,
            12
        );
        assert_eq!(
            repository
                .targets
                .as_ref()
                .unwrap()
                .authority
                .as_ref()
                .unwrap()
                .key_fingerprints
                .len(),
            2
        );
    }

    #[test]
    fn memory_root_history_reconciliation_rejects_an_older_disk_fork() {
        let root_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (root_id, root_key) = tuf_key_entry(&root_pair);
        let a_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (a_id, a_key) = tuf_key_entry(&a_pair);
        let b_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (b_id, b_key) = tuf_key_entry(&b_pair);
        let root_v1 = tuf_root_with_role_bindings(
            1,
            &root_id,
            &root_key,
            &root_pair,
            &[(&a_id, &a_key)],
            (&[a_id.as_str()], 1),
            (&[a_id.as_str()], 1),
            (&[a_id.as_str()], 1),
        );
        let root_v1_fork = tuf_root_with_role_bindings(
            1,
            &root_id,
            &root_key,
            &root_pair,
            &[(&b_id, &b_key)],
            (&[b_id.as_str()], 1),
            (&[b_id.as_str()], 1),
            (&[b_id.as_str()], 1),
        );
        let root_v2 = tuf_root_with_role_bindings(
            2,
            &root_id,
            &root_key,
            &root_pair,
            &[(&a_id, &a_key), (&b_id, &b_key)],
            (&[a_id.as_str(), b_id.as_str()], 1),
            (&[a_id.as_str(), b_id.as_str()], 1),
            (&[a_id.as_str(), b_id.as_str()], 1),
        );
        verify_root_chain(&[&root_v1, &root_v2]);
        let history = root_history_entries(&[&root_v1, &root_v2]);
        let memory = advance_security_floors_through_root_history(
            Some(&repository_floor_from_root(&root_v1, 7)),
            BTreeMap::new(),
            &root_transition_high_water(&root_v2).unwrap(),
            &history,
        )
        .unwrap()
        .repository
        .unwrap();
        let error = merge_loaded_security_high_water_states(
            Some(&repository_floor_from_root(&root_v1_fork, 99)),
            BTreeMap::new(),
            Some(&memory),
            &BTreeMap::new(),
            &root_history_entries(&[&root_v1_fork]),
            &history,
        )
        .err()
        .expect("forked disk anchor must be rejected");
        assert!(error
            .to_string()
            .contains("forks from in-memory root version 1"));
    }

    #[test]
    fn semantic_metadata_hash_ignores_signature_and_encoding_variants() {
        let key_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (key_id, _) = tuf_key_entry(&key_pair);
        let signed = json!({
            "_type": "timestamp",
            "expires": "2027-01-01T00:00:00Z",
            "meta": {"snapshot.json": {"version": 7}},
            "spec_version": "1.0.0",
            "version": 9,
        });
        let signature = tuf_signature(&signed, &key_id, &key_pair);
        let extra_pair = KeyPair::generate_ecdsa_p256().unwrap();
        let (extra_id, _) = tuf_key_entry(&extra_pair);
        let extra = tuf_signature(&signed, &extra_id, &extra_pair);
        let compact = serde_json::to_vec(&json!({
            "signatures": [signature.clone()],
            "signed": signed.clone(),
        }))
        .unwrap();
        let reordered = serde_json::to_string_pretty(&json!({
            "signed": signed,
            "signatures": [extra, signature],
        }))
        .unwrap();
        assert_eq!(
            signed_metadata_sha256("timestamp", &compact).unwrap(),
            signed_metadata_sha256("timestamp", reordered.as_bytes()).unwrap()
        );
    }

    #[test]
    fn equal_floor_merges_union_provenance_commutatively_and_bound_growth() {
        let left = RepositoryHighWater::for_test();
        let mut right = left.clone();
        right.timestamp.as_mut().unwrap().authority = Some(AuthorityProvenance {
            key_fingerprints: RoleAuthority::for_test('e').key_fingerprints,
        });
        right
            .snapshot_descriptor
            .as_mut()
            .unwrap()
            .referenced_authority = Some(AuthorityProvenance {
            key_fingerprints: RoleAuthority::for_test('f').key_fingerprints,
        });
        let left_right = merge_repository_high_waters(Some(&left), Some(&right))
            .unwrap()
            .unwrap();
        let right_left = merge_repository_high_waters(Some(&right), Some(&left))
            .unwrap()
            .unwrap();
        assert_eq!(left_right, right_left);
        assert_eq!(
            left_right
                .timestamp
                .as_ref()
                .unwrap()
                .authority
                .as_ref()
                .unwrap()
                .key_fingerprints
                .len(),
            2
        );
        assert_eq!(
            left_right
                .snapshot_descriptor
                .as_ref()
                .unwrap()
                .referenced_authority
                .as_ref()
                .unwrap()
                .key_fingerprints
                .len(),
            2
        );

        let left_channel = CacheHighWater::for_test(7, 'a', 'd');
        let right_channel = CacheHighWater::for_test(7, 'a', 'e');
        assert_eq!(
            merge_high_water(Some(&left_channel), Some(&right_channel)).unwrap(),
            merge_high_water(Some(&right_channel), Some(&left_channel)).unwrap()
        );

        let full = AuthorityProvenance {
            key_fingerprints: (0..MAX_AUTHORITY_KEYS)
                .map(|index| format!("{index:064x}"))
                .collect(),
        };
        let extra = RoleAuthority {
            threshold: 1,
            key_fingerprints: vec![format!("{:064x}", MAX_AUTHORITY_KEYS)],
        };
        assert!(union_authority_provenance(&full, &extra).is_err());
    }

    #[test]
    fn authenticated_observation_journals_only_the_longest_accepted_role_chain() {
        let mark = |version: u64, byte: char| MetadataHighWater {
            version,
            sha256: byte.to_string().repeat(SHA256_HEX_LEN),
            authority: None,
            referenced_authority: None,
        };
        let role_mark = |version: u64, byte: char, authority: &RoleAuthority| MetadataHighWater {
            version,
            sha256: byte.to_string().repeat(SHA256_HEX_LEN),
            authority: Some(authority.into()),
            referenced_authority: None,
        };
        let descriptor_mark = |version: u64,
                               byte: char,
                               authority: &RoleAuthority,
                               referenced_authority: &RoleAuthority|
         -> MetadataHighWater {
            MetadataHighWater {
                version,
                sha256: byte.to_string().repeat(SHA256_HEX_LEN),
                authority: Some(authority.into()),
                referenced_authority: Some(referenced_authority.into()),
            }
        };
        let timestamp_authority = RoleAuthority::for_test('1');
        let snapshot_authority = RoleAuthority::for_test('2');
        let targets_authority = RoleAuthority::for_test('3');
        let root_authority = RoleAuthority::for_test('4');
        let prior = RepositoryHighWater {
            root: mark(1, 'a'),
            root_authority: root_authority.clone(),
            timestamp_authority: timestamp_authority.clone(),
            snapshot_authority: snapshot_authority.clone(),
            targets_authority: targets_authority.clone(),
            authority_history: AuthorityHistory::from_authorities(
                &root_authority,
                &timestamp_authority,
                &snapshot_authority,
                &targets_authority,
            ),
            timestamp: Some(role_mark(10, 'b', &timestamp_authority)),
            snapshot_descriptor: Some(descriptor_mark(
                10,
                'e',
                &timestamp_authority,
                &snapshot_authority,
            )),
            snapshot: Some(role_mark(10, 'c', &snapshot_authority)),
            targets_descriptor: Some(descriptor_mark(
                10,
                'e',
                &snapshot_authority,
                &targets_authority,
            )),
            targets: Some(role_mark(10, 'd', &targets_authority)),
        };

        let rejected_root = RepositoryHighWater {
            root: mark(1, 'e'),
            root_authority: prior.root_authority.clone(),
            timestamp_authority: prior.timestamp_authority.clone(),
            snapshot_authority: prior.snapshot_authority.clone(),
            targets_authority: prior.targets_authority.clone(),
            authority_history: prior.authority_history.clone(),
            timestamp: Some(role_mark(u64::MAX, 'f', &timestamp_authority)),
            snapshot_descriptor: Some(descriptor_mark(
                u64::MAX,
                'f',
                &timestamp_authority,
                &snapshot_authority,
            )),
            snapshot: Some(role_mark(u64::MAX, 'f', &snapshot_authority)),
            targets_descriptor: Some(descriptor_mark(
                u64::MAX,
                'f',
                &snapshot_authority,
                &targets_authority,
            )),
            targets: Some(role_mark(u64::MAX, 'f', &targets_authority)),
        };
        let merged = merge_repository_observation(Some(&prior), &rejected_root);
        assert!(merged.error.is_some());
        assert!(!merged.accepted_through_targets);
        assert_eq!(merged.high_water, prior);

        let rejected_snapshot = RepositoryHighWater {
            root: mark(2, 'e'),
            root_authority: prior.root_authority.clone(),
            timestamp_authority: prior.timestamp_authority.clone(),
            snapshot_authority: prior.snapshot_authority.clone(),
            targets_authority: prior.targets_authority.clone(),
            authority_history: prior.authority_history.clone(),
            timestamp: Some(role_mark(11, 'f', &timestamp_authority)),
            snapshot_descriptor: Some(descriptor_mark(
                11,
                'f',
                &timestamp_authority,
                &snapshot_authority,
            )),
            snapshot: Some(role_mark(9, 'a', &snapshot_authority)),
            targets_descriptor: Some(descriptor_mark(
                u64::MAX,
                'f',
                &snapshot_authority,
                &targets_authority,
            )),
            targets: Some(role_mark(u64::MAX, 'f', &targets_authority)),
        };
        let merged = merge_repository_observation(Some(&prior), &rejected_snapshot);
        assert!(merged.error.is_some());
        assert!(!merged.accepted_through_targets);
        assert_eq!(merged.high_water.root, rejected_snapshot.root);
        assert_eq!(merged.high_water.timestamp, rejected_snapshot.timestamp);
        assert_eq!(merged.high_water.snapshot, prior.snapshot);
        assert_eq!(merged.high_water.targets, prior.targets);

        let accepted = RepositoryHighWater {
            root: mark(2, 'e'),
            root_authority: prior.root_authority.clone(),
            timestamp_authority: prior.timestamp_authority.clone(),
            snapshot_authority: prior.snapshot_authority.clone(),
            targets_authority: prior.targets_authority.clone(),
            authority_history: prior.authority_history.clone(),
            timestamp: Some(role_mark(11, 'f', &timestamp_authority)),
            snapshot_descriptor: Some(descriptor_mark(
                11,
                'f',
                &timestamp_authority,
                &snapshot_authority,
            )),
            snapshot: Some(role_mark(11, 'f', &snapshot_authority)),
            targets_descriptor: Some(descriptor_mark(
                11,
                'f',
                &snapshot_authority,
                &targets_authority,
            )),
            targets: Some(role_mark(11, 'f', &targets_authority)),
        };
        let merged = merge_repository_observation(Some(&prior), &accepted);
        assert!(merged.error.is_none());
        assert!(merged.accepted_through_targets);
        assert_eq!(merged.high_water, accepted);
    }

    #[test]
    fn portable_bundle_verification_is_fully_local_without_an_identity_gate() {
        let bundle = include_bytes!("../tests/fixtures/cosign-v3-blob.sigstore.json");
        let trusted_root = sigstore_verify::trust_root::SIGSTORE_PRODUCTION_TRUSTED_ROOT.as_bytes();
        let artifact = b"test content for cosign\n";
        PortableBundleVerifier
            .verify(artifact, bundle, trusted_root)
            .unwrap();

        assert!(PortableBundleVerifier
            .verify(b"tampered", bundle, trusted_root)
            .is_err());

        let mut downgraded: Value = serde_json::from_slice(bundle).unwrap();
        downgraded["mediaType"] =
            Value::String("application/vnd.dev.sigstore.bundle+json;version=0.2".to_string());
        assert!(PortableBundleVerifier
            .verify(
                artifact,
                &serde_json::to_vec(&downgraded).unwrap(),
                trusted_root,
            )
            .is_err());

        let mut multiple_entries: Value = serde_json::from_slice(bundle).unwrap();
        let duplicate = multiple_entries["verificationMaterial"]["tlogEntries"][0].clone();
        multiple_entries["verificationMaterial"]["tlogEntries"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        let error = PortableBundleVerifier
            .verify(
                artifact,
                &serde_json::to_vec(&multiple_entries).unwrap(),
                trusted_root,
            )
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("exactly one transparency-log entry"));

        let mut missing_checkpoint: Value = serde_json::from_slice(bundle).unwrap();
        missing_checkpoint["verificationMaterial"]["tlogEntries"][0]["inclusionProof"]
            .as_object_mut()
            .unwrap()
            .remove("checkpoint");
        let error = PortableBundleVerifier
            .verify(
                artifact,
                &serde_json::to_vec(&missing_checkpoint).unwrap(),
                trusted_root,
            )
            .unwrap_err();
        assert!(error.to_string().contains("signed checkpoint"));
    }

    #[test]
    fn portable_bundle_verifies_official_rekor_v2_with_omitted_integrated_time() {
        let artifact = include_bytes!("../tests/fixtures/rekor-v2-artifact.txt");
        let bundle = include_bytes!("../tests/fixtures/rekor-v2-bundle.sigstore.fixture");
        let trusted_root = include_bytes!("../tests/fixtures/rekor-v2-trusted-root.fixture");
        let value: Value = serde_json::from_slice(bundle).unwrap();
        assert!(value["verificationMaterial"]["tlogEntries"][0]
            .get("integratedTime")
            .is_none());

        PortableBundleVerifier
            .verify(artifact, bundle, trusted_root)
            .unwrap();
    }

    #[test]
    fn portable_bundle_profile_requires_an_exact_sha256_message_digest() {
        let fixture = include_bytes!("../tests/fixtures/cosign-v3-blob.sigstore.json");

        let mut missing: Value = serde_json::from_slice(fixture).unwrap();
        missing["messageSignature"]
            .as_object_mut()
            .unwrap()
            .remove("messageDigest");
        let bundle = parse_portable_bundle(&serde_json::to_string(&missing).unwrap()).unwrap();
        let error = validate_portable_bundle_profile(&bundle).unwrap_err();
        assert!(error.to_string().contains("must contain a messageDigest"));

        for algorithm in ["sha256", "SHA2_512"] {
            let mut wrong_algorithm: Value = serde_json::from_slice(fixture).unwrap();
            wrong_algorithm["messageSignature"]["messageDigest"]["algorithm"] =
                Value::String(algorithm.to_string());
            let error = parse_portable_bundle(&serde_json::to_string(&wrong_algorithm).unwrap())
                .and_then(|bundle| validate_portable_bundle_profile(&bundle))
                .unwrap_err();
            assert!(error
                .to_string()
                .contains("algorithm must be exactly SHA2_256"));
        }

        let mut short_digest: Value = serde_json::from_slice(fixture).unwrap();
        short_digest["messageSignature"]["messageDigest"]["digest"] =
            Value::String(BASE64.encode([0_u8; 31]));
        let bundle = parse_portable_bundle(&serde_json::to_string(&short_digest).unwrap()).unwrap();
        let error = validate_portable_bundle_profile(&bundle).unwrap_err();
        assert!(error.to_string().contains("exactly 32 bytes"));
    }

    #[test]
    fn portable_bundle_profile_enforces_hashedrekord_v1_time_and_promise() {
        let fixture = include_bytes!("../tests/fixtures/cosign-v3-blob.sigstore.json");

        let mut zero_time: Value = serde_json::from_slice(fixture).unwrap();
        zero_time["verificationMaterial"]["tlogEntries"][0]["integratedTime"] =
            Value::String("0".to_string());
        let bundle = parse_portable_bundle(&serde_json::to_string(&zero_time).unwrap()).unwrap();
        let error = validate_portable_bundle_profile(&bundle).unwrap_err();
        assert!(error
            .to_string()
            .contains("integratedTime must be positive"));

        let mut missing_promise: Value = serde_json::from_slice(fixture).unwrap();
        missing_promise["verificationMaterial"]["tlogEntries"][0]
            .as_object_mut()
            .unwrap()
            .remove("inclusionPromise");
        let bundle =
            parse_portable_bundle(&serde_json::to_string(&missing_promise).unwrap()).unwrap();
        let error = validate_portable_bundle_profile(&bundle).unwrap_err();
        assert!(error.to_string().contains("inclusion promise"));

        let mut missing_timestamp: Value = serde_json::from_slice(fixture).unwrap();
        missing_timestamp["verificationMaterial"]["timestampVerificationData"]
            ["rfc3161Timestamps"] = json!([]);
        let bundle =
            parse_portable_bundle(&serde_json::to_string(&missing_timestamp).unwrap()).unwrap();
        let error = validate_portable_bundle_profile(&bundle).unwrap_err();
        assert!(error
            .to_string()
            .contains("must contain an RFC3161 timestamp"));

        let mut wrong_kind: Value = serde_json::from_slice(fixture).unwrap();
        wrong_kind["verificationMaterial"]["tlogEntries"][0]["kindVersion"]["kind"] =
            Value::String("intoto".to_string());
        let bundle = parse_portable_bundle(&serde_json::to_string(&wrong_kind).unwrap()).unwrap();
        let error = validate_portable_bundle_profile(&bundle).unwrap_err();
        assert!(error.to_string().contains("must be hashedrekord"));

        let mut unknown_version: Value = serde_json::from_slice(fixture).unwrap();
        unknown_version["verificationMaterial"]["tlogEntries"][0]["kindVersion"]["version"] =
            Value::String("0.0.3".to_string());
        let bundle =
            parse_portable_bundle(&serde_json::to_string(&unknown_version).unwrap()).unwrap();
        let error = validate_portable_bundle_profile(&bundle).unwrap_err();
        assert!(error
            .to_string()
            .contains("unsupported hashedrekord version '0.0.3'"));
    }

    #[test]
    fn portable_bundle_profile_accepts_zero_equivalent_rekor_v2_time_forms() {
        let fixture = include_bytes!("../tests/fixtures/cosign-v3-blob.sigstore.json");
        for integrated_time in [None, Some(Value::Null), Some(json!(0)), Some(json!("0"))] {
            let mut value: Value = serde_json::from_slice(fixture).unwrap();
            value["verificationMaterial"]["tlogEntries"][0]["kindVersion"]["version"] =
                Value::String("0.0.2".to_string());
            let entry = value["verificationMaterial"]["tlogEntries"][0]
                .as_object_mut()
                .unwrap();
            match integrated_time {
                Some(integrated_time) => {
                    entry.insert("integratedTime".to_string(), integrated_time);
                }
                None => {
                    entry.remove("integratedTime");
                }
            }

            let bundle = parse_portable_bundle(&serde_json::to_string(&value).unwrap()).unwrap();
            assert_eq!(
                bundle.verification_material.tlog_entries[0].integrated_time,
                0
            );
            validate_portable_bundle_profile(&bundle).unwrap();
        }
    }

    #[test]
    fn portable_bundle_profile_rejects_rekor_v2_nonzero_time_or_missing_timestamp() {
        let fixture = include_bytes!("../tests/fixtures/cosign-v3-blob.sigstore.json");
        let mut v2: Value = serde_json::from_slice(fixture).unwrap();
        v2["verificationMaterial"]["tlogEntries"][0]["kindVersion"]["version"] =
            Value::String("0.0.2".to_string());

        for integrated_time in [json!(1), json!("1")] {
            let mut nonzero = v2.clone();
            nonzero["verificationMaterial"]["tlogEntries"][0]["integratedTime"] = integrated_time;
            let error = parse_portable_bundle(&serde_json::to_string(&nonzero).unwrap())
                .and_then(|bundle| validate_portable_bundle_profile(&bundle))
                .unwrap_err();
            assert!(error
                .to_string()
                .contains("integratedTime must be absent, null, or zero"));
        }

        v2["verificationMaterial"]["tlogEntries"][0]
            .as_object_mut()
            .unwrap()
            .remove("integratedTime");
        v2["verificationMaterial"]["timestampVerificationData"]["rfc3161Timestamps"] = json!([]);
        let bundle = parse_portable_bundle(&serde_json::to_string(&v2).unwrap()).unwrap();
        let error = validate_portable_bundle_profile(&bundle).unwrap_err();
        assert!(error
            .to_string()
            .contains("must contain an RFC3161 timestamp"));
    }

    #[tokio::test]
    async fn repository_does_not_follow_redirects() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tuf/metadata/timestamp.json"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", "https://github.com/"),
            )
            .mount(&server)
            .await;
        let repository = HttpTufRepository::new(&format!("{}/tuf/", server.uri()), true).unwrap();
        let error = repository
            .fetch_metadata("timestamp.json", 1024)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("302"));
    }

    #[tokio::test]
    async fn repository_enforces_stream_size_bound() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tuf/metadata/timestamp.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0; 32]))
            .mount(&server)
            .await;
        let repository = HttpTufRepository::new(&format!("{}/tuf/", server.uri()), true).unwrap();
        let error = repository
            .fetch_metadata("timestamp.json", 8)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("maximum response length"));
    }

    #[tokio::test]
    async fn repository_total_deadline_bounds_a_delayed_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tuf/metadata/timestamp.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(250))
                    .set_body_bytes(b"eventual response"),
            )
            .mount(&server)
            .await;
        let repository = HttpTufRepository::new_with_timeout(
            &format!("{}/tuf/", server.uri()),
            true,
            Duration::from_millis(25),
        )
        .unwrap();
        let started = tokio::time::Instant::now();
        let error = repository
            .fetch_metadata("timestamp.json", 1024)
            .await
            .unwrap_err();
        assert!(error.to_string().contains(TUF_UNAVAILABLE_PREFIX));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
