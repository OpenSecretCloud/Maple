use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::{Result, TransportV2Error};

const BUNDLE_VERSION: u8 = 2;
const MAX_BASE_URL_BYTES: usize = 4 * 1024;
const MAX_DESCRIPTOR_BYTES: usize = 16 * 1024;
const MAX_BUNDLE_JSON_BYTES: usize = 64 * 1024;
const TOKEN_ISSUER: &str = "urn:opensecret:transport-v2";
const USER_ACCESS_AUDIENCE: &str = "urn:opensecret:internal:transport-v2:user:access-descriptor";
const USER_RESUMPTION_AUDIENCE: &str = "urn:opensecret:internal:transport-v2:user:resumption";

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct AuthBundle {
    version: u8,
    api_origin: String,
    access_token: String,
    refresh_token: String,
    cache_namespace_root_base64: String,
}

#[derive(Deserialize)]
struct TransportV2TokenClaims {
    iss: String,
    aud: String,
    tv: u8,
    tk: String,
    pk: String,
    sub: String,
    exp: u64,
}

pub(crate) struct DecodedAuthBundle {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) cache_namespace_root: [u8; 32],
}

impl Drop for DecodedAuthBundle {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.cache_namespace_root.zeroize();
    }
}

pub(crate) fn encode_auth_bundle(
    api_origin: &str,
    access_token: &str,
    refresh_token: &str,
    cache_namespace_root: &[u8; 32],
) -> Result<String> {
    validate_fields(api_origin, access_token, refresh_token)?;
    validate_v2_user_token_pair(access_token, refresh_token)?;
    let bundle = AuthBundle {
        version: BUNDLE_VERSION,
        api_origin: api_origin.to_owned(),
        access_token: access_token.to_owned(),
        refresh_token: refresh_token.to_owned(),
        cache_namespace_root_base64: STANDARD.encode(cache_namespace_root),
    };
    let json =
        Zeroizing::new(serde_json::to_vec(&bundle).map_err(|_| TransportV2Error::InvalidJson)?);
    if json.len() > MAX_BUNDLE_JSON_BYTES {
        return Err(TransportV2Error::LimitExceeded {
            field: "auth bundle",
            limit: MAX_BUNDLE_JSON_BYTES,
        });
    }
    Ok(URL_SAFE_NO_PAD.encode(json.as_slice()))
}

pub(crate) fn decode_auth_bundle(
    encoded: &str,
    expected_api_origin: &str,
) -> Result<DecodedAuthBundle> {
    if encoded.is_empty() || encoded.contains('=') {
        return Err(TransportV2Error::InvalidEncoding);
    }
    let json = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| TransportV2Error::InvalidEncoding)?,
    );
    if json.len() > MAX_BUNDLE_JSON_BYTES || URL_SAFE_NO_PAD.encode(json.as_slice()) != encoded {
        return Err(TransportV2Error::InvalidEncoding);
    }
    let mut bundle: AuthBundle =
        serde_json::from_slice(&json).map_err(|_| TransportV2Error::InvalidJson)?;
    if bundle.version != BUNDLE_VERSION || bundle.api_origin != expected_api_origin {
        return Err(TransportV2Error::BindingMismatch);
    }
    validate_fields(
        &bundle.api_origin,
        &bundle.access_token,
        &bundle.refresh_token,
    )?;
    validate_v2_user_token_pair(&bundle.access_token, &bundle.refresh_token)?;
    let root = Zeroizing::new(
        STANDARD
            .decode(&bundle.cache_namespace_root_base64)
            .map_err(|_| TransportV2Error::InvalidEncoding)?,
    );
    if root.len() != 32 || STANDARD.encode(root.as_slice()) != bundle.cache_namespace_root_base64 {
        return Err(TransportV2Error::InvalidEncoding);
    }
    let mut cache_namespace_root = [0_u8; 32];
    cache_namespace_root.copy_from_slice(root.as_slice());
    if encode_auth_bundle(
        &bundle.api_origin,
        &bundle.access_token,
        &bundle.refresh_token,
        &cache_namespace_root,
    )? != encoded
    {
        return Err(TransportV2Error::InvalidEncoding);
    }
    Ok(DecodedAuthBundle {
        access_token: std::mem::take(&mut bundle.access_token),
        refresh_token: std::mem::take(&mut bundle.refresh_token),
        cache_namespace_root,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidatedUserTokenPair {
    pub(crate) principal: String,
    pub(crate) access_expires_at_unix_seconds: u64,
}

pub(crate) fn validate_v2_user_token_pair(
    access_token: &str,
    refresh_token: &str,
) -> Result<ValidatedUserTokenPair> {
    let (access_subject, access_expires_at_unix_seconds) =
        token_subject(access_token, USER_ACCESS_AUDIENCE, "access_descriptor")?;
    let (resumption_subject, _) =
        token_subject(refresh_token, USER_RESUMPTION_AUDIENCE, "resumption")?;
    if access_subject != resumption_subject {
        return Err(TransportV2Error::BindingMismatch);
    }
    Ok(ValidatedUserTokenPair {
        principal: access_subject,
        access_expires_at_unix_seconds,
    })
}

fn token_subject(
    token: &str,
    expected_audience: &str,
    expected_kind: &str,
) -> Result<(String, u64)> {
    if token.is_empty() || token.len() > MAX_DESCRIPTOR_BYTES {
        return Err(TransportV2Error::InvalidRequest);
    }
    let mut parts = token.split('.');
    let Some(header) = parts.next() else {
        return Err(TransportV2Error::InvalidEncoding);
    };
    let Some(payload) = parts.next() else {
        return Err(TransportV2Error::InvalidEncoding);
    };
    let Some(signature) = parts.next() else {
        return Err(TransportV2Error::InvalidEncoding);
    };
    if parts.next().is_some() || header.is_empty() || payload.is_empty() || signature.is_empty() {
        return Err(TransportV2Error::InvalidEncoding);
    }
    let decoded = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| TransportV2Error::InvalidEncoding)?,
    );
    if decoded.len() > MAX_DESCRIPTOR_BYTES || URL_SAFE_NO_PAD.encode(decoded.as_slice()) != payload
    {
        return Err(TransportV2Error::InvalidEncoding);
    }
    let claims: TransportV2TokenClaims =
        serde_json::from_slice(&decoded).map_err(|_| TransportV2Error::InvalidJson)?;
    if claims.iss != TOKEN_ISSUER
        || claims.aud != expected_audience
        || claims.tv != BUNDLE_VERSION
        || claims.tk != expected_kind
        || claims.pk != "user"
        || claims.sub.is_empty()
        || claims.exp == 0
    {
        return Err(TransportV2Error::InvalidRequest);
    }
    Ok((claims.sub, claims.exp))
}

fn validate_fields(api_origin: &str, access_token: &str, refresh_token: &str) -> Result<()> {
    if api_origin.is_empty() || api_origin.len() > MAX_BASE_URL_BYTES {
        return Err(TransportV2Error::InvalidRequest);
    }
    for token in [access_token, refresh_token] {
        if token.is_empty() || token.len() > MAX_DESCRIPTOR_BYTES {
            return Err(TransportV2Error::InvalidRequest);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor_with_expiry(audience: &str, kind: &str, subject: &str, expiry: u64) -> String {
        let claims = serde_json::json!({
            "iss": TOKEN_ISSUER,
            "aud": audience,
            "tv": 2,
            "tk": kind,
            "pk": "user",
            "sub": subject,
            "exp": expiry,
        });
        format!(
            "e30.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    fn descriptor(audience: &str, kind: &str, subject: &str) -> String {
        descriptor_with_expiry(audience, kind, subject, 2_000_000_000_u64)
    }

    fn descriptors(subject: &str) -> (String, String) {
        (
            descriptor(USER_ACCESS_AUDIENCE, "access_descriptor", subject),
            descriptor(USER_RESUMPTION_AUDIENCE, "resumption", subject),
        )
    }

    #[test]
    fn bundle_round_trip_is_canonical_and_origin_bound() {
        let root = [7_u8; 32];
        let (access, refresh) = descriptors("user-1");
        let encoded =
            encode_auth_bundle("https://example.com/api", &access, &refresh, &root).unwrap();
        assert!(!encoded.contains('='));
        let decoded = decode_auth_bundle(&encoded, "https://example.com/api").unwrap();
        assert_eq!(decoded.access_token, access);
        assert_eq!(decoded.refresh_token, refresh);
        assert_eq!(decoded.cache_namespace_root, root);
        assert!(matches!(
            decode_auth_bundle(&encoded, "https://other.example"),
            Err(TransportV2Error::BindingMismatch)
        ));
    }

    #[test]
    fn bundle_rejects_unknown_duplicate_and_noncanonical_fields() {
        let root = STANDARD.encode([9_u8; 32]);
        let (access, refresh) = descriptors("user-1");
        for json in [
            format!(
                r#"{{"version":2,"api_origin":"https://example.com","access_token":"{access}","refresh_token":"{refresh}","cache_namespace_root_base64":"{root}","extra":true}}"#
            ),
            format!(
                r#"{{"version":2,"version":2,"api_origin":"https://example.com","access_token":"{access}","refresh_token":"{refresh}","cache_namespace_root_base64":"{root}"}}"#
            ),
            format!(
                r#"{{"api_origin":"https://example.com","version":2,"access_token":"{access}","refresh_token":"{refresh}","cache_namespace_root_base64":"{root}"}}"#
            ),
        ] {
            let encoded = URL_SAFE_NO_PAD.encode(json.as_bytes());
            assert!(decode_auth_bundle(&encoded, "https://example.com").is_err());
        }

        let canonical =
            encode_auth_bundle("https://example.com", &access, &refresh, &[9_u8; 32]).unwrap();
        assert!(decode_auth_bundle(&format!("{canonical}="), "https://example.com").is_err());
    }

    #[test]
    fn descriptors_are_kind_audience_and_principal_bound() {
        let (access, refresh) = descriptors("user-1");
        assert_eq!(
            validate_v2_user_token_pair(&access, &refresh).unwrap(),
            ValidatedUserTokenPair {
                principal: "user-1".to_string(),
                access_expires_at_unix_seconds: 2_000_000_000,
            }
        );
        let (_, other_refresh) = descriptors("user-2");
        assert!(validate_v2_user_token_pair(&access, &other_refresh).is_err());
        assert!(validate_v2_user_token_pair(&refresh, &access).is_err());
        assert!(validate_v2_user_token_pair("legacy", "legacy").is_err());
    }

    #[test]
    fn validation_returns_the_access_descriptor_deadline() {
        let access = descriptor_with_expiry(
            USER_ACCESS_AUDIENCE,
            "access_descriptor",
            "user-1",
            1_900_000_123,
        );
        let refresh = descriptor_with_expiry(
            USER_RESUMPTION_AUDIENCE,
            "resumption",
            "user-1",
            2_100_000_456,
        );
        assert_eq!(
            validate_v2_user_token_pair(&access, &refresh).unwrap(),
            ValidatedUserTokenPair {
                principal: "user-1".to_string(),
                access_expires_at_unix_seconds: 1_900_000_123,
            }
        );
    }
}
