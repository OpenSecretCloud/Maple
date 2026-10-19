//! Enrollment records are discovery/admission hints, never device grants.
//! Expected account/project/deployment come from authenticated request context;
//! copying another account's valid row and its lookup columns cannot authorize it.
use serde::{de::DeserializeOwned, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::encrypt::{decrypt_aead_v1, derive_key, encrypt_aead_v1, CanonicalBytes, EncryptError};

const KEY_DOMAIN: &[u8] = b"os.remote-enrollment.aead-key.v1";
const AAD_DOMAIN: &str = "os.remote-enrollment.record.v1";

pub(crate) struct Binding<'a> {
    pub account: Uuid,
    pub project: i32,
    pub deployment: &'a str,
    pub kind: &'a str,
    pub id: &'a str,
}

impl Binding<'_> {
    fn aad(&self) -> Vec<u8> {
        let mut aad = CanonicalBytes::new(AAD_DOMAIN);
        aad.append_i16(1)
            .append_uuid(self.account)
            .append_i32(self.project)
            .append_str(self.deployment)
            .append_str(self.kind)
            .append_str(self.id);
        aad.into_bytes()
    }

    pub fn seal<T: Serialize>(&self, root: &[u8], value: &T) -> Result<Vec<u8>, EncryptError> {
        let key = Zeroizing::new(derive_key(root, KEY_DOMAIN)?);
        let body =
            Zeroizing::new(serde_json::to_vec(value).map_err(|_| EncryptError::FailedToDecrypt)?);
        encrypt_aead_v1(&key, &body, &self.aad())
    }

    pub fn open<T: DeserializeOwned>(
        &self,
        root: &[u8],
        encrypted: &[u8],
    ) -> Result<T, EncryptError> {
        let key = Zeroizing::new(derive_key(root, KEY_DOMAIN)?);
        let body = Zeroizing::new(decrypt_aead_v1(&key, encrypted, &self.aad())?);
        serde_json::from_slice(&body).map_err(|_| EncryptError::FailedToDecrypt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn binding() -> Binding<'static> {
        Binding {
            account: Uuid::from_u128(1),
            project: 7,
            deployment: "local",
            kind: "window",
            id: "endpoint:window",
        }
    }

    #[test]
    fn valid_foreign_rows_and_swapped_context_are_rejected() {
        let root = [42; 32];
        let expected = binding();
        let value = json!({"endpoint": "host", "window": "nonce", "expires": 100});
        let ciphertext = expected.seal(&root, &value).unwrap();
        assert_eq!(expected.open::<Value>(&root, &ciphertext).unwrap(), value);
        for field in 0..5 {
            let mut foreign = binding();
            match field {
                0 => foreign.account = Uuid::from_u128(2),
                1 => foreign.project = 8,
                2 => foreign.deployment = "prod",
                3 => foreign.kind = "device",
                _ => foreign.id = "other-endpoint:other-window",
            }
            // The attacker has a fully valid foreign row, including all its
            // metadata. The reader still supplies the authenticated identity
            // and requested context rather than trusting those columns.
            let foreign_ciphertext = foreign.seal(&root, &value).unwrap();
            assert!(expected.open::<Value>(&root, &foreign_ciphertext).is_err());
            assert!(foreign.open::<Value>(&root, &ciphertext).is_err());
        }
        let mut changed = ciphertext;
        *changed.last_mut().unwrap() ^= 1;
        assert!(expected.open::<Value>(&root, &changed).is_err());
    }
}
