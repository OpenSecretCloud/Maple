use std::{fmt, str::FromStr};

use axum::{
    body::Bytes,
    http::{HeaderName, HeaderValue, Method, Uri},
};
use rand_core::{OsRng, RngCore};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroize;

use crate::provider_cache::CacheNamespaceRoot;

pub(crate) const VERSION: u8 = 2;
pub(crate) const REQUEST_ID_BYTES: usize = 16;
pub(crate) const METADATA_LENGTH_BYTES: usize = 4;
pub(crate) const MAX_METADATA_BYTES: usize = 128 * 1024;
pub(crate) const MAX_BODY_BYTES: usize = 50 * 1024 * 1024;
pub(crate) const MAX_ENCODED_REQUEST_BYTES: usize =
    METADATA_LENGTH_BYTES + MAX_METADATA_BYTES + MAX_BODY_BYTES;
pub(crate) const MAX_CREDENTIAL_BYTES: usize = 16 * 1024;
pub(crate) const MAX_METHOD_BYTES: usize = 32;
pub(crate) const MAX_TARGET_BYTES: usize = 16 * 1024;
pub(crate) const MAX_HEADER_COUNT: usize = 64;

#[derive(Debug, thiserror::Error)]
pub(crate) enum EnvelopeError {
    #[error("transport-v2 request envelope is truncated")]
    Truncated,
    #[error("transport-v2 request envelope exceeds its byte limit")]
    EnvelopeTooLarge,
    #[error("transport-v2 request metadata exceeds its byte limit")]
    MetadataTooLarge,
    #[error("transport-v2 request body exceeds its byte limit")]
    BodyTooLarge,
    #[error("transport-v2 request metadata is invalid")]
    InvalidMetadata(#[source] serde_json::Error),
    #[error("transport-v2 request version must be exactly 2")]
    InvalidVersion,
    #[error("transport-v2 request body presence does not match its metadata")]
    BodyPresenceMismatch,
    #[error("transport-v2 credential is invalid")]
    InvalidCredential,
    #[error("transport-v2 method is invalid")]
    InvalidMethod,
    #[error("transport-v2 relative target is invalid")]
    InvalidTarget,
    #[error("transport-v2 request has too many logical headers")]
    TooManyHeaders,
    #[error("transport-v2 logical header name is invalid")]
    InvalidHeaderName,
    #[error("transport-v2 logical header value is invalid")]
    InvalidHeaderValue,
    #[error("transport-v2 logical header is controlled by the gateway")]
    GatewayControlledHeader,
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct RequestId([u8; REQUEST_ID_BYTES]);

impl RequestId {
    pub(crate) const fn from_bytes(bytes: [u8; REQUEST_ID_BYTES]) -> Self {
        Self(bytes)
    }

    pub(crate) fn random() -> Self {
        let mut bytes = [0; REQUEST_ID_BYTES];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub(crate) const fn as_bytes(&self) -> &[u8; REQUEST_ID_BYTES] {
        &self.0
    }
}

impl fmt::Debug for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RequestId(")?;
        formatter.write_str(&hex::encode(self.0))?;
        formatter.write_str(")")
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl Serialize for RequestId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RequestIdVisitor;

        impl de::Visitor<'_> for RequestIdVisitor {
            type Value = RequestId;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("exactly 32 lowercase hexadecimal characters")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let encoded = value.as_bytes();
                if encoded.len() != REQUEST_ID_BYTES * 2
                    || !encoded
                        .iter()
                        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
                {
                    return Err(E::custom("request ID is not canonical lowercase hex"));
                }
                let mut decoded = [0; REQUEST_ID_BYTES];
                hex::decode_to_slice(encoded, &mut decoded)
                    .map_err(|_| E::custom("request ID is not valid hex"))?;
                Ok(RequestId(decoded))
            }
        }

        deserializer.deserialize_str(RequestIdVisitor)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CredentialKind {
    Bearer,
    ApiKey,
    Resumption,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Credential {
    kind: CredentialKind,
    value: String,
}

impl Credential {
    pub(crate) fn new(kind: CredentialKind, value: String) -> Result<Self, EnvelopeError> {
        let credential = Self { kind, value };
        validate_credential(&credential)?;
        Ok(credential)
    }

    pub(crate) const fn kind(&self) -> CredentialKind {
        self.kind
    }

    pub(crate) fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credential")
            .field("kind", &self.kind)
            .field("value_bytes", &self.value.len())
            .finish()
    }
}

impl Drop for Credential {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LogicalHeader {
    name: String,
    value: String,
}

impl LogicalHeader {
    pub(crate) fn new(name: String, value: String) -> Result<Self, EnvelopeError> {
        let header = Self { name, value };
        validate_header(&header)?;
        Ok(header)
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for LogicalHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LogicalHeader")
            .field("name", &self.name)
            .field("value_bytes", &self.value.len())
            .finish()
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RequestMetadata {
    version: u8,
    credential: Option<Credential>,
    cache_namespace_root: Option<CacheNamespaceRoot>,
    method: String,
    target: String,
    headers: Vec<LogicalHeader>,
    body_present: bool,
}

pub(crate) struct RequestEnvelope {
    request_id: RequestId,
    credential: Option<Credential>,
    cache_namespace_root: Option<CacheNamespaceRoot>,
    method: String,
    target: String,
    headers: Vec<LogicalHeader>,
    body: Option<Bytes>,
}

pub(crate) struct RequestEnvelopeParts {
    pub(crate) credential: Option<Credential>,
    pub(crate) cache_namespace_root: Option<CacheNamespaceRoot>,
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) headers: Vec<LogicalHeader>,
    pub(crate) body: Option<Bytes>,
}

impl fmt::Debug for RequestEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestEnvelope")
            .field("request_id", &self.request_id)
            .field("credential", &self.credential)
            .field(
                "cache_namespace_root_present",
                &self.cache_namespace_root.is_some(),
            )
            .field("method", &self.method)
            .field("target_bytes", &self.target.len())
            .field("header_count", &self.headers.len())
            .field("body_bytes", &self.body.as_ref().map(Bytes::len))
            .finish()
    }
}

impl RequestEnvelope {
    pub(crate) fn new(
        request_id: RequestId,
        credential: Option<Credential>,
        cache_namespace_root: Option<CacheNamespaceRoot>,
        method: String,
        target: String,
        headers: Vec<LogicalHeader>,
        body: Option<Vec<u8>>,
    ) -> Result<Self, EnvelopeError> {
        Self::from_parts(
            request_id,
            credential,
            cache_namespace_root,
            method,
            target,
            headers,
            body.map(Bytes::from),
        )
    }

    fn from_parts(
        request_id: RequestId,
        credential: Option<Credential>,
        cache_namespace_root: Option<CacheNamespaceRoot>,
        method: String,
        target: String,
        headers: Vec<LogicalHeader>,
        body: Option<Bytes>,
    ) -> Result<Self, EnvelopeError> {
        let envelope = Self {
            request_id,
            credential,
            cache_namespace_root,
            method,
            target,
            headers,
            body,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub(crate) const fn request_id(&self) -> RequestId {
        self.request_id
    }

    pub(crate) fn credential(&self) -> Option<&Credential> {
        self.credential.as_ref()
    }

    pub(crate) fn cache_namespace_root(&self) -> Option<&CacheNamespaceRoot> {
        self.cache_namespace_root.as_ref()
    }

    pub(crate) fn method(&self) -> &str {
        &self.method
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn headers(&self) -> &[LogicalHeader] {
        &self.headers
    }

    pub(crate) fn body(&self) -> Option<&[u8]> {
        self.body.as_deref()
    }

    pub(crate) fn into_body(mut self) -> Option<Bytes> {
        self.body.take()
    }

    pub(crate) fn into_parts(mut self) -> RequestEnvelopeParts {
        RequestEnvelopeParts {
            credential: self.credential.take(),
            cache_namespace_root: self.cache_namespace_root.take(),
            method: std::mem::take(&mut self.method),
            target: std::mem::take(&mut self.target),
            headers: std::mem::take(&mut self.headers),
            body: self.body.take(),
        }
    }

    /// Encodes bounded JSON metadata followed by the raw body bytes. The raw
    /// tail avoids base64 expansion and a second large decoded-body allocation.
    pub(crate) fn encode(&self) -> Result<Vec<u8>, EnvelopeError> {
        self.validate()?;
        let metadata = RequestMetadata {
            version: VERSION,
            credential: self.credential.clone(),
            cache_namespace_root: self.cache_namespace_root.clone(),
            method: self.method.clone(),
            target: self.target.clone(),
            headers: self.headers.clone(),
            body_present: self.body.is_some(),
        };
        let encoded_metadata =
            serde_json::to_vec(&metadata).map_err(EnvelopeError::InvalidMetadata)?;
        if encoded_metadata.len() > MAX_METADATA_BYTES {
            return Err(EnvelopeError::MetadataTooLarge);
        }
        let body = self.body.as_deref().unwrap_or_default();
        validate_body_len(body.len())?;

        let mut encoded =
            Vec::with_capacity(METADATA_LENGTH_BYTES + encoded_metadata.len() + body.len());
        encoded.extend_from_slice(&(encoded_metadata.len() as u32).to_be_bytes());
        encoded.extend_from_slice(&encoded_metadata);
        encoded.extend_from_slice(body);
        Ok(encoded)
    }

    pub(crate) fn decode(request_id: RequestId, encoded: &[u8]) -> Result<Self, EnvelopeError> {
        Self::decode_owned(request_id, encoded.to_vec())
    }

    /// Decodes a decrypted record while retaining its allocation behind a
    /// reference-counted byte slice. The logical body therefore does not need
    /// a second maximum-sized copy.
    pub(crate) fn decode_owned(
        request_id: RequestId,
        encoded: Vec<u8>,
    ) -> Result<Self, EnvelopeError> {
        Self::decode_bytes(request_id, Bytes::from(encoded))
    }

    fn decode_bytes(request_id: RequestId, encoded: Bytes) -> Result<Self, EnvelopeError> {
        if encoded.len() < METADATA_LENGTH_BYTES {
            return Err(EnvelopeError::Truncated);
        }
        if encoded.len() > MAX_ENCODED_REQUEST_BYTES {
            return Err(EnvelopeError::EnvelopeTooLarge);
        }

        let metadata_len = u32::from_be_bytes(
            encoded[..METADATA_LENGTH_BYTES]
                .try_into()
                .map_err(|_| EnvelopeError::Truncated)?,
        ) as usize;
        if metadata_len > MAX_METADATA_BYTES {
            return Err(EnvelopeError::MetadataTooLarge);
        }
        let metadata_end = METADATA_LENGTH_BYTES
            .checked_add(metadata_len)
            .ok_or(EnvelopeError::EnvelopeTooLarge)?;
        if metadata_end > encoded.len() {
            return Err(EnvelopeError::Truncated);
        }

        let metadata: RequestMetadata =
            serde_json::from_slice(&encoded[METADATA_LENGTH_BYTES..metadata_end])
                .map_err(EnvelopeError::InvalidMetadata)?;
        if metadata.version != VERSION {
            return Err(EnvelopeError::InvalidVersion);
        }

        let body_bytes = encoded.slice(metadata_end..);
        validate_body_len(body_bytes.len())?;
        let body = match (metadata.body_present, body_bytes.is_empty()) {
            (true, _) => Some(body_bytes),
            (false, true) => None,
            (false, false) => return Err(EnvelopeError::BodyPresenceMismatch),
        };

        Self::from_parts(
            request_id,
            metadata.credential,
            metadata.cache_namespace_root,
            metadata.method,
            metadata.target,
            metadata.headers,
            body,
        )
    }

    fn validate(&self) -> Result<(), EnvelopeError> {
        if let Some(credential) = &self.credential {
            validate_credential(credential)?;
        }
        validate_method(&self.method)?;
        validate_target(&self.target)?;
        validate_headers(&self.headers)?;
        validate_body_len(self.body.as_ref().map_or(0, Bytes::len))
    }
}

fn validate_credential(credential: &Credential) -> Result<(), EnvelopeError> {
    let value = credential.value.as_bytes();
    if value.is_empty()
        || value.len() > MAX_CREDENTIAL_BYTES
        || !value.iter().all(|byte| matches!(byte, 0x21..=0x7e))
    {
        return Err(EnvelopeError::InvalidCredential);
    }
    Ok(())
}

fn validate_method(method: &str) -> Result<(), EnvelopeError> {
    if method.is_empty()
        || method.len() > MAX_METHOD_BYTES
        || Method::from_bytes(method.as_bytes()).is_err()
    {
        return Err(EnvelopeError::InvalidMethod);
    }
    Ok(())
}

fn validate_target(target: &str) -> Result<(), EnvelopeError> {
    if target.is_empty()
        || target.len() > MAX_TARGET_BYTES
        || !target.starts_with('/')
        || target.starts_with("//")
        || target.contains(['#', '\\'])
    {
        return Err(EnvelopeError::InvalidTarget);
    }
    let uri = Uri::from_str(target).map_err(|_| EnvelopeError::InvalidTarget)?;
    if uri.scheme().is_some()
        || uri.authority().is_some()
        || uri
            .path_and_query()
            .is_none_or(|path_and_query| path_and_query.as_str() != target)
    {
        return Err(EnvelopeError::InvalidTarget);
    }
    Ok(())
}

fn validate_headers(headers: &[LogicalHeader]) -> Result<(), EnvelopeError> {
    if headers.len() > MAX_HEADER_COUNT {
        return Err(EnvelopeError::TooManyHeaders);
    }

    for header in headers {
        validate_header(header)?;
    }
    Ok(())
}

fn validate_header(header: &LogicalHeader) -> Result<(), EnvelopeError> {
    let parsed_name = HeaderName::from_bytes(header.name.as_bytes())
        .map_err(|_| EnvelopeError::InvalidHeaderName)?;
    if parsed_name.as_str() != header.name {
        return Err(EnvelopeError::InvalidHeaderName);
    }
    if is_gateway_controlled_header(parsed_name.as_str()) {
        return Err(EnvelopeError::GatewayControlledHeader);
    }
    HeaderValue::from_str(&header.value).map_err(|_| EnvelopeError::InvalidHeaderValue)?;
    Ok(())
}

fn is_gateway_controlled_header(name: &str) -> bool {
    matches!(
        name,
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "keep-alive"
            | "te"
            | "trailer"
            | "upgrade"
            | "forwarded"
            | "via"
            | "x-forwarded-for"
            | "x-forwarded-host"
            | "x-forwarded-proto"
            | "x-opensecret-routing-key"
            | "x-session-id"
    )
}

fn validate_body_len(body_len: usize) -> Result<(), EnvelopeError> {
    if body_len > MAX_BODY_BYTES {
        Err(EnvelopeError::BodyTooLarge)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(body: Option<Vec<u8>>) -> RequestEnvelope {
        RequestEnvelope::new(
            RequestId::from_bytes([0x11; 16]),
            Some(
                Credential::new(CredentialKind::Bearer, "header.payload.signature".into()).unwrap(),
            ),
            Some(CacheNamespaceRoot::from_bytes([0x22; 32])),
            "POST".into(),
            "/v1/chat/completions?trace=1".into(),
            vec![LogicalHeader::new("content-type".into(), "application/json".into()).unwrap()],
            body,
        )
        .unwrap()
    }

    fn wire(metadata: &str, body: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
        encoded.extend_from_slice(metadata.as_bytes());
        encoded.extend_from_slice(body);
        encoded
    }

    fn decode(encoded: &[u8]) -> Result<RequestEnvelope, EnvelopeError> {
        RequestEnvelope::decode(RequestId::from_bytes([0x11; 16]), encoded)
    }

    #[test]
    fn raw_body_round_trip_has_no_base64_expansion() {
        let body = vec![0, 1, 2, 3, 0xfe, 0xff];
        let envelope = sample(Some(body.clone()));
        let encoded = envelope.encode().unwrap();
        assert!(encoded.ends_with(&body));

        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.request_id(), envelope.request_id());
        assert_eq!(decoded.method(), "POST");
        assert_eq!(decoded.target(), "/v1/chat/completions?trace=1");
        assert_eq!(decoded.body(), Some(body.as_slice()));
        assert_eq!(decoded.credential().unwrap().kind(), CredentialKind::Bearer);
        assert_eq!(
            decoded.credential().unwrap().value(),
            "header.payload.signature"
        );
        assert!(decoded.cache_namespace_root().is_some());
        assert!(!format!("{decoded:?}").contains("IiIi"));
    }

    #[test]
    fn absent_and_present_empty_bodies_remain_distinct() {
        let absent = decode(&sample(None).encode().unwrap()).unwrap();
        let empty = decode(&sample(Some(Vec::new())).encode().unwrap()).unwrap();
        assert_eq!(absent.body(), None);
        assert_eq!(empty.body(), Some([].as_slice()));
    }

    #[test]
    fn strict_metadata_rejects_unknown_duplicate_and_wrong_version_fields() {
        let base = r#"{"version":2,"credential":null,"cache_namespace_root":null,"method":"GET","target":"/health-check","headers":[],"body_present":false}"#;
        assert!(decode(&wire(base, &[])).is_ok());

        let unknown = base.replace(
            "\"body_present\":false",
            "\"body_present\":false,\"extra\":1",
        );
        assert!(matches!(
            decode(&wire(&unknown, &[])),
            Err(EnvelopeError::InvalidMetadata(_))
        ));

        let duplicate = base.replacen("\"version\":2", "\"version\":2,\"version\":2", 1);
        assert!(matches!(
            decode(&wire(&duplicate, &[])),
            Err(EnvelopeError::InvalidMetadata(_))
        ));

        let wrong_version = base.replacen("\"version\":2", "\"version\":1", 1);
        assert!(matches!(
            decode(&wire(&wrong_version, &[])),
            Err(EnvelopeError::InvalidVersion)
        ));
    }

    #[test]
    fn body_presence_and_prefix_are_strict() {
        let no_body = r#"{"version":2,"credential":null,"cache_namespace_root":null,"method":"GET","target":"/health-check","headers":[],"body_present":false}"#;
        assert!(matches!(
            decode(&wire(no_body, b"unexpected")),
            Err(EnvelopeError::BodyPresenceMismatch)
        ));
        assert!(matches!(decode(&[0, 0, 0]), Err(EnvelopeError::Truncated)));

        let mut length_past_end = Vec::from(100_u32.to_be_bytes());
        length_past_end.extend_from_slice(b"{}");
        assert!(matches!(
            decode(&length_past_end),
            Err(EnvelopeError::Truncated)
        ));
    }

    #[test]
    fn only_relative_targets_and_non_gateway_headers_are_admitted() {
        for target in [
            "https://example.com/private",
            "relative",
            "//example.com/private",
            "/safe#ignored",
            "/safe\\ignored",
        ] {
            assert!(matches!(
                RequestEnvelope::new(
                    RequestId::random(),
                    None,
                    None,
                    "GET".into(),
                    target.into(),
                    vec![],
                    None,
                ),
                Err(EnvelopeError::InvalidTarget)
            ));
        }

        for name in [
            "authorization",
            "cookie",
            "host",
            "transfer-encoding",
            "x-forwarded-for",
            "x-opensecret-routing-key",
            "x-session-id",
        ] {
            assert!(matches!(
                LogicalHeader::new(name.into(), "value".into()),
                Err(EnvelopeError::GatewayControlledHeader)
            ));
        }
        assert!(LogicalHeader::new("openai-beta".into(), "responses=v1".into()).is_ok());
    }

    #[test]
    fn malformed_credentials_methods_and_headers_are_rejected() {
        assert!(Credential::new(CredentialKind::Bearer, String::new()).is_err());
        assert!(Credential::new(CredentialKind::ApiKey, "contains space".into()).is_err());
        assert!(matches!(
            RequestEnvelope::new(
                RequestId::random(),
                None,
                None,
                "not a method".into(),
                "/v1/models".into(),
                vec![],
                None,
            ),
            Err(EnvelopeError::InvalidMethod)
        ));
        assert!(LogicalHeader::new("Content-Type".into(), "application/json".into()).is_err());
        assert!(LogicalHeader::new("x-value".into(), "line\nbreak".into()).is_err());
    }

    #[test]
    fn repeated_logical_headers_round_trip_in_order() {
        let headers = vec![
            LogicalHeader::new("x-repeat".into(), "first".into()).unwrap(),
            LogicalHeader::new("x-repeat".into(), "second".into()).unwrap(),
        ];
        let envelope = RequestEnvelope::new(
            RequestId::from_bytes([0x55; 16]),
            None,
            None,
            "GET".into(),
            "/v1/models".into(),
            headers.clone(),
            None,
        )
        .unwrap();

        let decoded =
            RequestEnvelope::decode(envelope.request_id(), &envelope.encode().unwrap()).unwrap();
        assert_eq!(decoded.headers(), headers);
    }

    #[test]
    fn all_limits_are_explicit_and_checked_without_preallocating_their_maxima() {
        assert_eq!(MAX_METADATA_BYTES, 128 * 1024);
        assert_eq!(MAX_BODY_BYTES, 50 * 1024 * 1024);
        assert_eq!(MAX_ENCODED_REQUEST_BYTES, 52_559_876);
        assert_eq!(MAX_CREDENTIAL_BYTES, 16 * 1024);
        assert_eq!(MAX_METHOD_BYTES, 32);
        assert_eq!(MAX_TARGET_BYTES, 16 * 1024);
        assert_eq!(MAX_HEADER_COUNT, 64);
        assert!(matches!(
            validate_body_len(MAX_BODY_BYTES + 1),
            Err(EnvelopeError::BodyTooLarge)
        ));
    }

    #[test]
    fn request_ids_are_full_width_and_randomly_generated() {
        let first = RequestId::random();
        let second = RequestId::random();
        assert_eq!(first.as_bytes().len(), REQUEST_ID_BYTES);
        assert_ne!(first, second);
    }
}
