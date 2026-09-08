use secp256k1::{Message, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::encrypt::generate_random;
use crate::Error;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SigningAlgorithm {
    Schnorr,
    Ecdsa,
}

#[derive(Debug)]
pub struct SignMessageResponse {
    pub signature: SignatureType,
    pub message_hash: [u8; 32],
}

#[derive(Debug)]
pub enum SignatureType {
    Schnorr(secp256k1::schnorr::Signature),
    Ecdsa(secp256k1::ecdsa::Signature),
}

impl std::fmt::Display for SignatureType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignatureType::Schnorr(sig) => write!(f, "{}", sig),
            SignatureType::Ecdsa(sig) => write!(f, "{}", sig),
        }
    }
}

pub fn sign_message(
    secret_key: &SecretKey,
    message_bytes: &[u8],
    algorithm: SigningAlgorithm,
) -> Result<SignMessageResponse, Error> {
    let secp = Secp256k1::new();

    // Hash the message
    let mut hasher = Sha256::new();
    hasher.update(message_bytes);
    let message_hash = hasher.finalize();
    let message_hash_array: [u8; 32] = message_hash.into();

    // Create secp256k1 message from hash
    let message = Message::from_digest_slice(&message_hash)
        .map_err(|e| Error::SigningError(format!("Failed to create message from digest: {}", e)))?;

    // Sign with the specified algorithm
    let signature = match algorithm {
        SigningAlgorithm::Schnorr => {
            let keypair = secret_key.keypair(&secp);
            let random_bytes = generate_random::<32>();
            SignatureType::Schnorr(secp.sign_schnorr_with_aux_rand(
                &message,
                &keypair,
                &random_bytes,
            ))
        }
        SigningAlgorithm::Ecdsa => SignatureType::Ecdsa(secp.sign_ecdsa(&message, secret_key)),
    };

    Ok(SignMessageResponse {
        signature,
        message_hash: message_hash_array,
    })
}
