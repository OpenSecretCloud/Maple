//! Device authority is local and separate from the refresh-token cache. Cloud
//! discovery never creates a grant, and a missing grant is never reconstructed.
use iroh::{EndpointId, SecretKey};
use maple_agent::private_file::write_private_json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use zeroize::Zeroize;

const MAX_PEERS: usize = 64;

pub(super) fn default_name() -> &'static str {
    static NAME: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        default_name_from(&hostname::get().unwrap_or_default().to_string_lossy())
    });
    &NAME
}

fn default_name_from(hostname: &str) -> String {
    let mut name = String::new();
    for ch in hostname
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(64)
    {
        if name.len() + ch.len_utf8() > 128 {
            break;
        }
        name.push(ch);
    }
    if name.trim().is_empty() {
        if cfg!(target_os = "macos") {
            "Mac".into()
        } else if cfg!(target_os = "windows") {
            "Windows PC".into()
        } else {
            "Linux PC".into()
        }
    } else {
        name.trim().to_string()
    }
}

pub(crate) fn validate_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err("Enter a computer name without control characters.".into());
    }
    if name.len() > 128 || name.chars().count() > 64 {
        return Err("Use a shorter computer name.".into());
    }
    Ok(name)
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    pub name: String,
    pub grant: [u8; 16],
    /// Set only on B after confirming A's proof.
    pub controls_this_computer: bool,
    /// Set only on A after verifying B's proof and durable grant acknowledgement.
    pub can_connect: bool,
    #[serde(default)]
    pub pending_confirmation: Option<[u8; 16]>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    scope: String,
    secret: [u8; 32],
    enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    peers: BTreeMap<String, Peer>,
    #[serde(default)]
    attempted_windows: BTreeMap<String, Vec<[u8; 16]>>,
}
impl Drop for Record {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

pub struct Store {
    path: PathBuf,
    record: Record,
}

fn identity_path(
    root: &Path,
    api: &str,
    project: &str,
    channel: &str,
    account: &str,
) -> (String, PathBuf) {
    // Length-delimited namespace, independent of build and auth tokens.
    let mut hash = Sha256::new();
    for part in ["maple.remote.identity.v1", api, project, channel, account] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    let scope = format!("{:x}", hash.finalize());
    let path = root.join("remote").join(format!("{scope}.json"));
    (scope, path)
}

impl Store {
    pub fn exists(root: &Path, api: &str, project: &str, channel: &str, account: &str) -> bool {
        identity_path(root, api, project, channel, account)
            .1
            .exists()
    }
    pub fn open(
        root: &Path,
        api: &str,
        project: &str,
        channel: &str,
        account: &str,
    ) -> Result<Self, String> {
        let (scope, path) = identity_path(root, api, project, channel, account);
        let record: Record = match std::fs::read(&path) {
            Ok(mut bytes) => {
                let result = serde_json::from_slice(&bytes);
                bytes.zeroize();
                result.map_err(
                    |_| "Remote identity file is invalid; restore it before using remote access.",
                )?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let record = Record {
                    version: 1,
                    scope: scope.clone(),
                    secret: SecretKey::generate().to_bytes(),
                    enabled: false,
                    name: None,
                    peers: BTreeMap::new(),
                    attempted_windows: BTreeMap::new(),
                };
                write_private_json(&path, &record).map_err(|_| "Could not save remote identity")?;
                record
            }
            Err(_) => return Err("Could not read remote identity".into()),
        };
        if record.version != 1
            || record.scope != scope
            || record.peers.len() > MAX_PEERS
            || record.attempted_windows.len() > MAX_PEERS
        {
            return Err("Remote identity does not match this account and profile".into());
        }
        if let Some(name) = &record.name {
            validate_name(name)?;
        }
        for (key, peer) in &record.peers {
            if key
                .parse::<EndpointId>()
                .map_err(|_| "Invalid saved remote device identity")?
                .to_string()
                != *key
                || peer.name.len() > 128
                || peer.name.chars().any(char::is_control)
            {
                return Err("Invalid saved remote device".into());
            }
        }
        for (key, windows) in &record.attempted_windows {
            if key
                .parse::<EndpointId>()
                .map_err(|_| "Invalid saved pairing history")?
                .to_string()
                != *key
                || windows.len() > 4096
            {
                return Err("Invalid saved pairing history".into());
            }
        }
        Ok(Self { path, record })
    }

    pub fn key(&self) -> SecretKey {
        SecretKey::from_bytes(&self.record.secret)
    }
    pub fn enabled(&self) -> bool {
        self.record.enabled
    }
    pub fn name(&self) -> &str {
        self.record
            .name
            .as_deref()
            .unwrap_or_else(|| default_name())
    }
    pub fn set_name(&mut self, name: &str) -> Result<(), String> {
        let name = validate_name(name)?.to_owned();
        self.update(|record| {
            record.name = Some(name);
            Ok(())
        })
    }
    /// Discovery supplies display metadata only. It cannot create a peer or
    /// change its grant, direction, or pending confirmation.
    pub fn refresh_peer_names<'a>(
        &mut self,
        names: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<(), String> {
        let changes: Vec<_> = names
            .into_iter()
            .filter(|(key, name)| {
                !name.trim().is_empty()
                    && name.len() <= 128
                    && !name.chars().any(char::is_control)
                    && self.record.peers.get(*key).is_some_and(|p| p.name != *name)
            })
            .collect();
        if changes.is_empty() {
            return Ok(());
        }
        self.update(|record| {
            for (key, name) in changes {
                if let Some(peer) = record.peers.get_mut(key) {
                    peer.name = name.into();
                }
            }
            Ok(())
        })
    }
    pub fn peers(&self) -> &BTreeMap<String, Peer> {
        &self.record.peers
    }
    pub fn permits(&self, peer: EndpointId) -> bool {
        self.record.enabled
            && self
                .record
                .peers
                .get(&peer.to_string())
                .is_some_and(|grant| grant.controls_this_computer)
    }
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), String> {
        self.update(|record| {
            record.enabled = enabled;
            Ok(())
        })
    }
    pub fn grant(&mut self, key: EndpointId, grant: Peer) -> Result<(), String> {
        if grant.name.len() > 128 || grant.name.chars().any(char::is_control) {
            return Err("Remote device name is invalid".into());
        }
        self.update(|record| {
            if !record.peers.contains_key(&key.to_string()) && record.peers.len() >= MAX_PEERS {
                return Err("Remove an old paired device before adding another".into());
            }
            let mut grant = grant;
            if let Some(previous) = record.peers.get(&key.to_string()) {
                // Pairing the opposite direction does not revoke an existing
                // role. The incoming grant ID belongs to this execution host.
                grant.can_connect |= previous.can_connect;
                grant.pending_confirmation = previous.pending_confirmation;
            }
            record.peers.insert(key.to_string(), grant);
            Ok(())
        })
    }
    pub fn remember_host_proof(
        &mut self,
        key: EndpointId,
        window: [u8; 16],
        name: String,
    ) -> Result<(), String> {
        self.update(|record| {
            if !record.peers.contains_key(&key.to_string()) && record.peers.len() >= MAX_PEERS {
                return Err("Too many paired computers".into());
            }
            let peer = record.peers.entry(key.to_string()).or_insert(Peer {
                name: name.clone(),
                grant: [0; 16],
                controls_this_computer: false,
                can_connect: false,
                pending_confirmation: None,
            });
            peer.name = name;
            peer.pending_confirmation = Some(window);
            Ok(())
        })
    }
    pub fn confirm_host(&mut self, key: EndpointId, window: [u8; 16]) -> Result<(), String> {
        self.update(|record| {
            let peer = record
                .peers
                .get_mut(&key.to_string())
                .ok_or("Pairing was removed locally")?;
            if peer.pending_confirmation != Some(window) {
                return Err("Pairing confirmation is no longer pending".into());
            }
            peer.pending_confirmation = None;
            peer.can_connect = true;
            Ok(())
        })
    }
    /// Persist before sending any code-derived proof. A restart cannot retry W.
    pub fn reserve_attempt(&mut self, key: EndpointId, window: [u8; 16]) -> Result<(), String> {
        self.update(|record| {
            if !record.attempted_windows.contains_key(&key.to_string()) && record.attempted_windows.len() >= MAX_PEERS {
                return Err("Remote pairing attempt history is full".into());
            }
            let windows = record.attempted_windows.entry(key.to_string()).or_default();
            if windows.contains(&window) { return Err("This pairing window has already been attempted. Open a new window on the other computer.".into()); }
            // Never evict an attempt that a malicious discovery service could
            // replay. Fail closed at a generous per-device lifetime bound.
            if windows.len() >= 4096 { return Err("Remote pairing attempt history is full".into()); }
            windows.push(window);
            Ok(())
        })
    }
    pub fn revoke(&mut self, key: EndpointId) -> Result<(), String> {
        self.update(|record| {
            record.peers.remove(&key.to_string());
            Ok(())
        })
    }
    fn update(
        &mut self,
        edit: impl FnOnce(&mut Record) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut next = self.record.clone();
        edit(&mut next)?;
        write_private_json(&self.path, &next)
            .map_err(|_| "Could not save remote device settings")?;
        self.record = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn computer_names_upgrade_old_records_and_survive_restart_without_changing_identity() {
        let root = tempfile::tempdir().unwrap();
        let open = || Store::open(root.path(), "local", "p", "c", "a").unwrap();
        let mut store = open();
        let key = store.key().public();
        // An unset name is omitted, matching records from before names existed.
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&store.path).unwrap()).unwrap();
        assert!(record.get("name").is_none());
        assert!(validate_name(open().name()).is_ok());
        store.set_name("  Anthony’s Mac  ").unwrap();
        for invalid in ["", "bad\nname", &"x".repeat(65), &"🦀".repeat(33)] {
            assert!(store.set_name(invalid).is_err());
        }
        drop(store);
        let store = open();
        assert_eq!(store.name(), "Anthony’s Mac");
        assert_eq!(store.key().public(), key);
        assert!(!store.enabled());
        assert!(store.peers().is_empty());
        let other = Store::open(root.path(), "local", "p", "c", "b").unwrap();
        assert!(other.record.name.is_none());
        assert_eq!(
            default_name_from("  Anthony-Mac.local  "),
            "Anthony-Mac.local"
        );
        for hostname in ["", "\n\t", &"🦀".repeat(70)] {
            assert!(validate_name(&default_name_from(hostname)).is_ok());
        }
    }

    #[test]
    fn discovered_names_never_change_or_restore_pairing_authority() {
        let root = tempfile::tempdir().unwrap();
        let open = || Store::open(root.path(), "local", "p", "c", "a").unwrap();
        let mut store = open();
        let peer = SecretKey::generate().public();
        let unknown = SecretKey::generate().public().to_string();
        let key = peer.to_string();
        let mut expected = Peer {
            name: "Maple computer".into(),
            grant: [1; 16],
            controls_this_computer: true,
            can_connect: false,
            pending_confirmation: Some([2; 16]),
        };
        store.grant(peer, expected.clone()).unwrap();
        store
            .refresh_peer_names([(key.as_str(), "Office Mac"), (&unknown, "Unknown")])
            .unwrap();
        expected.name = "Office Mac".into();
        assert!(open().peers()[&key] == expected);
        assert_eq!(store.peers().len(), 1);
        assert!(!store.enabled());
        store
            .refresh_peer_names([(key.as_str(), "bad\nname")])
            .unwrap();
        assert!(store.peers()[&key] == expected);
        store.revoke(peer).unwrap();
        store
            .refresh_peer_names([(key.as_str(), "Office Mac")])
            .unwrap();
        assert!(open().peers().is_empty());
    }

    #[test]
    fn identities_and_grants_survive_restart_but_not_scope_changes() {
        let root = tempfile::tempdir().unwrap();
        let open =
            |account| Store::open(root.path(), "local", "project", "unpackaged", account).unwrap();
        let mut a = open("a");
        let key = a.key().public();
        let peer = SecretKey::generate().public();
        a.set_enabled(true).unwrap();
        a.grant(
            peer,
            Peer {
                name: "Test".into(),
                grant: [1; 16],
                controls_this_computer: true,
                can_connect: false,
                pending_confirmation: None,
            },
        )
        .unwrap();
        drop(a);
        let mut a = open("a");
        assert_eq!(a.key().public(), key);
        assert!(a.permits(peer));
        assert_ne!(open("b").key().public(), key);
        assert!(!open("b").permits(peer));
        assert_ne!(
            Store::open(root.path(), "other", "project", "unpackaged", "a")
                .unwrap()
                .key()
                .public(),
            key
        );
        a.revoke(peer).unwrap();
        drop(a);
        assert!(!open("a").permits(peer));
    }

    #[test]
    fn lost_ack_and_reverse_pairing_preserve_roles_but_revocation_wins() {
        let root = tempfile::tempdir().unwrap();
        let open = || Store::open(root.path(), "local", "p", "c", "a").unwrap();
        let peer = SecretKey::generate().public();
        let mut store = open();
        store.set_enabled(true).unwrap();
        store.reserve_attempt(peer, [1; 16]).unwrap();
        store
            .remember_host_proof(peer, [1; 16], "B".into())
            .unwrap();
        assert!(!store.peers()[&peer.to_string()].can_connect);
        drop(store);
        let mut store = open();
        assert!(store.reserve_attempt(peer, [1; 16]).is_err());
        store.confirm_host(peer, [1; 16]).unwrap();
        store
            .grant(
                peer,
                Peer {
                    name: "B".into(),
                    grant: [2; 16],
                    controls_this_computer: true,
                    can_connect: false,
                    pending_confirmation: None,
                },
            )
            .unwrap();
        assert!(store.permits(peer));
        assert!(store.peers()[&peer.to_string()].can_connect);
        store
            .remember_host_proof(peer, [3; 16], "B".into())
            .unwrap();
        assert!(store.permits(peer));
        assert_eq!(store.peers()[&peer.to_string()].grant, [2; 16]);
        store.revoke(peer).unwrap();
        assert!(store.confirm_host(peer, [3; 16]).is_err());
        assert!(store.reserve_attempt(peer, [1; 16]).is_err());
        assert!(!open().permits(peer));
    }

    #[test]
    fn malformed_identity_is_not_silently_replaced() {
        let root = tempfile::tempdir().unwrap();
        let a = Store::open(root.path(), "local", "p", "c", "a").unwrap();
        std::fs::write(&a.path, "broken").unwrap();
        assert!(Store::open(root.path(), "local", "p", "c", "a").is_err());
        assert_eq!(std::fs::read_to_string(&a.path).unwrap(), "broken");
    }
}
