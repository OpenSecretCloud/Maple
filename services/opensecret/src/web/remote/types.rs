//! Native desktop discovery and pairing admission over authenticated Transport V2.
//! These records do not authorize control: devices keep their own pairing grants.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteRegistration {
    pub account: Uuid,
    pub project: Uuid,
    pub deployment: String,
    pub channel: String,
    pub endpoint: String,
    pub name: String,
    pub addresses: RemoteAddresses,
    pub accepting_connections: bool,
    pub window: Option<RemoteWindowInit>,
}

/// Account-private bootstrap hints; they are not published in public DNS.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteAddresses {
    pub relays: Vec<String>,
    pub direct: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteWindowInit {
    pub id: Uuid,
    pub remaining_seconds: u32,
}

impl RemoteRegistration {
    /// Fixed schema serialization, prefixed by its purpose. Field order is part
    /// of version 1 and covered by a fixture in the service/client tests.
    pub fn proof_message(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut bytes = b"maple.remote.registration.v1\0".to_vec();
        bytes.extend(serde_json::to_vec(self)?);
        Ok(bytes)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterRemoteDevice {
    pub registration: RemoteRegistration,
    pub signature: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteDevice {
    pub channel: String,
    pub endpoint: String,
    pub name: String,
    pub addresses: RemoteAddresses,
    pub accepting_connections: bool,
    pub window: Option<RemoteWindow>,
    pub expires_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteWindow {
    pub id: Uuid,
    pub expires_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteAdmissionRequest {
    pub controller: String,
    pub execution: String,
    pub window: Uuid,
}

/// Deliberately not Debug: an admission handle must not enter application logs.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteAdmission {
    pub handle: String,
    pub expires_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRemoteAdmission {
    pub handle: String,
    pub controller: String,
    pub execution: String,
    pub window: Uuid,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteAdmissionCheck {
    pub valid: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteDeviceList {
    pub devices: Vec<RemoteDevice>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proof_schema_matches_shared_version_one_fixture() {
        // Keep this golden vector inside the standalone service package too.
        let fixture = include_str!("registration_v1.json").trim_end();
        let registration: RemoteRegistration = serde_json::from_str(fixture).unwrap();
        let mut expected = b"maple.remote.registration.v1\0".to_vec();
        expected.extend_from_slice(fixture.as_bytes());
        assert_eq!(registration.proof_message().unwrap(), expected);
    }
}
