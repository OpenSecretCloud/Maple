use std::fmt;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use p256::elliptic_curve::rand_core::{OsRng, RngCore};
#[cfg(test)]
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::{Result, TransportV2Error};

const KIB: usize = 1024;
const MIB: usize = 1024 * 1024;

// The raw AEAD carrier is one ChaCha20-Poly1305 record: the encrypted JSON
// envelope plus a 12-byte nonce and 16-byte authentication tag.
pub(super) const MAX_OUTER_REQUEST_BYTES: usize = 67 * MIB + 28;
pub(super) const MAX_OUTER_RESPONSE_BYTES: usize = 50 * MIB + 28;
pub(super) const MAX_KEY_EXCHANGE_BYTES: usize = 4 * KIB;
pub(super) const MAX_STREAM_CHUNK_BYTES: usize = 64 * KIB;
pub(super) const MAX_STREAM_ERROR_BYTES: usize = 16 * KIB;

const KV_ITEM_PATH_PREFIX: &str = "/protected/kv/";
const API_KEY_ITEM_PATH_PREFIX: &str = "/protected/api-keys/";
const VERIFY_EMAIL_PATH_PREFIX: &str = "/verify-email/";
const PLATFORM_VERIFY_EMAIL_PATH_PREFIX: &str = "/platform/verify-email/";
const PLATFORM_ORG_PATH_PREFIX: &str = "/platform/orgs/";
const PLATFORM_ACCEPT_INVITE_PATH_PREFIX: &str = "/platform/accept_invite/";
const CONVERSATION_PROJECT_ITEM_PATH_PREFIX: &str = "/v1/conversation-projects/";
const CONVERSATION_ITEM_PATH_PREFIX: &str = "/v1/conversations/";
const INSTRUCTION_ITEM_PATH_PREFIX: &str = "/v1/instructions/";
const RESPONSE_ITEM_PATH_PREFIX: &str = "/v1/responses/";

/// Resource ceilings for one decrypted transport-v2 envelope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct EnvelopeLimits {
    pub(super) envelope_bytes: usize,
    pub(super) logical_body_bytes: usize,
    pub(super) path_bytes: usize,
    pub(super) query_bytes: usize,
    pub(super) header_count: usize,
    pub(super) header_name_bytes: usize,
    pub(super) header_value_bytes: usize,
    pub(super) aggregate_header_bytes: usize,
    pub(super) credential_bytes: usize,
}

impl EnvelopeLimits {
    pub(super) const DEFAULT: Self = Self {
        envelope_bytes: 67 * MIB,
        logical_body_bytes: 50 * MIB,
        path_bytes: 4096,
        query_bytes: 8192,
        header_count: 64,
        header_name_bytes: 128,
        header_value_bytes: 16 * KIB,
        aggregate_header_bytes: 64 * KIB,
        credential_bytes: 16 * KIB,
    };

    pub(super) const RESPONSE: Self = Self {
        envelope_bytes: 50 * MIB,
        logical_body_bytes: 28 * MIB,
        path_bytes: 4096,
        query_bytes: 8192,
        header_count: 64,
        header_name_bytes: 128,
        header_value_bytes: 16 * KIB,
        aggregate_header_bytes: 64 * KIB,
        credential_bytes: 16 * KIB,
    };
}

impl Default for EnvelopeLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The protocol version has no invalid in-memory representation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Version2;

impl Version2 {
    pub(super) const VALUE: u8 = 2;
}

impl Serialize for Version2 {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(Self::VALUE)
    }
}

impl<'de> Deserialize<'de> for Version2 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if u8::deserialize(deserializer)? == Self::VALUE {
            Ok(Self)
        } else {
            Err(de::Error::custom("transport version must be exactly 2"))
        }
    }
}

/// A full 128-bit, per-session replay identifier.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct RequestId([u8; 16]);

impl RequestId {
    pub(super) fn random() -> Result<Self> {
        let mut bytes = [0_u8; 16];
        OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| TransportV2Error::RandomnessUnavailable)?;
        Ok(Self(bytes))
    }

    #[cfg(test)]
    pub(super) const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub(super) const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Debug for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl Serialize for RequestId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RequestIdVisitor;

        impl de::Visitor<'_> for RequestIdVisitor {
            type Value = RequestId;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("exactly 32 lowercase hexadecimal characters")
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                parse_request_id(value).ok_or_else(|| E::custom("non-canonical request ID"))
            }
        }

        deserializer.deserialize_str(RequestIdVisitor)
    }
}

fn parse_request_id(value: &str) -> Option<RequestId> {
    let encoded = value.as_bytes();
    if encoded.len() != 32
        || !encoded
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return None;
    }

    let mut decoded = [0_u8; 16];
    for (destination, pair) in decoded.iter_mut().zip(encoded.chunks_exact(2)) {
        *destination = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Some(RequestId(decoded))
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Exact bytes represented on the wire as padded standard base64.
#[derive(Clone, Eq, PartialEq, Zeroize, ZeroizeOnDrop)]
pub(crate) struct EncodedBytes(Vec<u8>);

impl EncodedBytes {
    pub(super) fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    pub(super) fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub(super) fn into_bytes(mut self) -> Vec<u8> {
        std::mem::take(&mut self.0)
    }

    pub(super) fn len(&self) -> usize {
        self.0.len()
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for EncodedBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EncodedBytes")
            .field("len", &self.0.len())
            .finish_non_exhaustive()
    }
}

impl Serialize for EncodedBytes {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = Zeroizing::new(STANDARD.encode(self.0.as_slice()));
        serializer.serialize_str(encoded.as_str())
    }
}

impl<'de> Deserialize<'de> for EncodedBytes {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct EncodedBytesVisitor;

        impl de::Visitor<'_> for EncodedBytesVisitor {
            type Value = EncodedBytes;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("canonical padded standard base64")
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                let mut bytes = Zeroizing::new(
                    STANDARD
                        .decode(value)
                        .map_err(|_| E::custom("invalid standard base64"))?,
                );
                let canonical = Zeroizing::new(STANDARD.encode(bytes.as_slice()));
                if canonical.as_str() != value {
                    return Err(E::custom("non-canonical standard base64"));
                }
                Ok(EncodedBytes(std::mem::take(&mut *bytes)))
            }
        }

        deserializer.deserialize_str(EncodedBytesVisitor)
    }
}

/// Stable client-generated provider-cache namespace root.
#[derive(Eq, PartialEq, Zeroize, ZeroizeOnDrop)]
pub(crate) struct CacheNamespaceRoot([u8; 32]);

impl CacheNamespaceRoot {
    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl fmt::Debug for CacheNamespaceRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CacheNamespaceRoot([REDACTED])")
    }
}

impl Serialize for CacheNamespaceRoot {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = Zeroizing::new(STANDARD.encode(self.0.as_slice()));
        serializer.serialize_str(encoded.as_str())
    }
}

impl<'de> Deserialize<'de> for CacheNamespaceRoot {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = EncodedBytes::deserialize(deserializer)?;
        if encoded.len() != 32 {
            return Err(de::Error::custom(
                "cache namespace root must contain exactly 32 bytes",
            ));
        }
        let mut root = Self([0_u8; 32]);
        root.0.copy_from_slice(encoded.as_slice());
        Ok(root)
    }
}

/// Authentication material permitted only during an anonymous transition.
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Credential {
    ApiKey { value_base64: EncodedBytes },
    Resumption { value_base64: EncodedBytes },
}

impl Credential {
    pub(crate) fn api_key(bytes: impl Into<Vec<u8>>) -> Self {
        Self::ApiKey {
            value_base64: EncodedBytes::from_bytes(bytes),
        }
    }

    pub(crate) fn resumption(bytes: impl Into<Vec<u8>>) -> Self {
        Self::Resumption {
            value_base64: EncodedBytes::from_bytes(bytes),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::ApiKey { value_base64 } | Self::Resumption { value_base64 } => value_base64.len(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseMode {
    Unary,
    Stream,
    Auto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum LogicalMethod {
    #[serde(rename = "GET")]
    Get,
    #[serde(rename = "POST")]
    Post,
    #[serde(rename = "PUT")]
    Put,
    #[serde(rename = "PATCH")]
    Patch,
    #[serde(rename = "DELETE")]
    Delete,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HeaderField {
    pub(super) name: String,
    pub(super) value_base64: EncodedBytes,
}

impl HeaderField {
    pub(crate) fn new(name: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            value_base64: EncodedBytes::from_bytes(value),
        }
    }

    pub(super) fn value(&self) -> &[u8] {
        self.value_base64.as_slice()
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LogicalRequest {
    pub(super) method: LogicalMethod,
    pub(super) path: String,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub(super) query: Option<String>,
    pub(super) headers: Vec<HeaderField>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub(super) body_base64: Option<EncodedBytes>,
}

impl LogicalRequest {
    pub(crate) fn new(
        method: LogicalMethod,
        path: impl Into<String>,
        query: Option<String>,
        headers: Vec<HeaderField>,
        body: Option<Vec<u8>>,
    ) -> Self {
        Self {
            method,
            path: path.into(),
            query,
            headers,
            body_base64: body.map(EncodedBytes::from_bytes),
        }
    }

    pub(super) fn validate(&self, limits: &EnvelopeLimits) -> Result<()> {
        validate_path(self.method, &self.path, limits)?;
        if let Some(query) = self.query.as_deref() {
            validate_query(query, limits)?;
        }
        validate_headers(&self.headers, limits)?;
        if let Some(body) = &self.body_base64 {
            check_limit(body.len(), limits.logical_body_bytes, "logical body")?;
        }
        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RequestEnvelope {
    pub(super) version: Version2,
    pub(super) request_id: RequestId,
    pub(super) response_mode: ResponseMode,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub(super) credential: Option<Credential>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub(super) cache_namespace_root_base64: Option<CacheNamespaceRoot>,
    pub(super) request: LogicalRequest,
}

impl RequestEnvelope {
    #[cfg(test)]
    pub(super) fn from_json_slice(input: &[u8], limits: &EnvelopeLimits) -> Result<Self> {
        check_limit(input.len(), limits.envelope_bytes, "envelope")?;
        let envelope: Self =
            serde_json::from_slice(input).map_err(|_| TransportV2Error::InvalidJson)?;
        envelope.validate(limits)?;
        Ok(envelope)
    }

    pub(super) fn to_json_vec(&self, limits: &EnvelopeLimits) -> Result<Vec<u8>> {
        self.validate(limits)?;
        let encoded = serde_json::to_vec(self).map_err(|_| TransportV2Error::InvalidJson)?;
        check_limit(encoded.len(), limits.envelope_bytes, "envelope")?;
        Ok(encoded)
    }

    pub(super) fn validate(&self, limits: &EnvelopeLimits) -> Result<()> {
        self.request.validate(limits)?;
        if let Some(credential) = &self.credential {
            check_limit(credential.len(), limits.credential_bytes, "credential")?;
        }
        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UnaryResponseEnvelope {
    pub(super) version: Version2,
    pub(super) request_id: RequestId,
    pub(super) status: u16,
    pub(super) headers: Vec<HeaderField>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub(super) body_base64: Option<EncodedBytes>,
}

impl UnaryResponseEnvelope {
    pub(super) fn from_json_slice(input: &[u8], limits: &EnvelopeLimits) -> Result<Self> {
        check_limit(input.len(), limits.envelope_bytes, "envelope")?;
        let envelope: Self =
            serde_json::from_slice(input).map_err(|_| TransportV2Error::InvalidJson)?;
        envelope.validate(limits)?;
        Ok(envelope)
    }

    pub(super) fn validate(&self, limits: &EnvelopeLimits) -> Result<()> {
        if !(100..=599).contains(&self.status) {
            return Err(TransportV2Error::InvalidResponse);
        }
        validate_headers(&self.headers, limits)?;
        if let Some(body) = &self.body_base64 {
            check_limit(body.len(), limits.logical_body_bytes, "logical body")?;
        }
        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum StreamRecord {
    Start {
        version: Version2,
        request_id: RequestId,
        sequence: u64,
        status: u16,
        headers: Vec<HeaderField>,
    },
    Chunk {
        version: Version2,
        request_id: RequestId,
        sequence: u64,
        body_base64: EncodedBytes,
    },
    End {
        version: Version2,
        request_id: RequestId,
        sequence: u64,
    },
    Error {
        version: Version2,
        request_id: RequestId,
        sequence: u64,
        status: u16,
        body_base64: EncodedBytes,
    },
}

impl StreamRecord {
    pub(super) fn from_json_slice(input: &[u8], limits: &EnvelopeLimits) -> Result<Self> {
        check_limit(input.len(), limits.envelope_bytes, "envelope")?;
        let record: Self =
            serde_json::from_slice(input).map_err(|_| TransportV2Error::InvalidJson)?;
        record.validate(limits)?;
        Ok(record)
    }

    pub(super) fn validate(&self, limits: &EnvelopeLimits) -> Result<()> {
        match self {
            Self::Start {
                sequence,
                status,
                headers,
                ..
            } => {
                if *sequence != 0 || !(200..=299).contains(status) {
                    return Err(TransportV2Error::InvalidStreamRecord);
                }
                validate_headers(headers, limits)
            }
            Self::Chunk {
                sequence,
                body_base64,
                ..
            } => {
                validate_non_initial_sequence(*sequence)?;
                check_limit(body_base64.len(), MAX_STREAM_CHUNK_BYTES, "stream chunk")
            }
            Self::End { sequence, .. } => validate_non_initial_sequence(*sequence),
            Self::Error {
                sequence,
                status,
                body_base64,
                ..
            } => {
                validate_non_initial_sequence(*sequence)?;
                if !(400..=599).contains(status) {
                    return Err(TransportV2Error::InvalidStreamRecord);
                }
                check_limit(body_base64.len(), MAX_STREAM_ERROR_BYTES, "stream error")
            }
        }
    }

    pub(super) const fn request_id(&self) -> &RequestId {
        match self {
            Self::Start { request_id, .. }
            | Self::Chunk { request_id, .. }
            | Self::End { request_id, .. }
            | Self::Error { request_id, .. } => request_id,
        }
    }

    pub(super) const fn sequence(&self) -> u64 {
        match self {
            Self::Start { sequence, .. }
            | Self::Chunk { sequence, .. }
            | Self::End { sequence, .. }
            | Self::Error { sequence, .. } => *sequence,
        }
    }
}

fn deserialize_required_nullable<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

pub(super) fn check_limit(actual: usize, limit: usize, field: &'static str) -> Result<()> {
    if actual > limit {
        Err(TransportV2Error::LimitExceeded { field, limit })
    } else {
        Ok(())
    }
}

fn validate_non_initial_sequence(sequence: u64) -> Result<()> {
    if sequence == 0 {
        Err(TransportV2Error::InvalidStreamRecord)
    } else {
        Ok(())
    }
}

fn validate_path(method: LogicalMethod, path: &str, limits: &EnvelopeLimits) -> Result<()> {
    check_limit(path.len(), limits.path_bytes, "path")?;
    if matches!(
        method,
        LogicalMethod::Get | LogicalMethod::Put | LogicalMethod::Delete
    ) {
        if let Some(segment) = path.strip_prefix(KV_ITEM_PATH_PREFIX) {
            decode_canonical_opaque_segment(segment)?;
            return Ok(());
        }
    }
    if method == LogicalMethod::Delete {
        if let Some(segment) = path.strip_prefix(API_KEY_ITEM_PATH_PREFIX) {
            let decoded = decode_canonical_opaque_segment(segment)?;
            if decoded.len() > 50
                || decoded.starts_with(' ')
                || decoded.ends_with(' ')
                || !decoded
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_'))
            {
                return Err(TransportV2Error::InvalidRequest);
            }
            return Ok(());
        }
    }
    if validate_canonical_uuid_path(method, path)? {
        return Ok(());
    }
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('?')
        || path.contains('#')
        || path.contains('\\')
    {
        return Err(TransportV2Error::InvalidRequest);
    }

    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let decoded =
                    decode_percent_triplet(bytes, index).ok_or(TransportV2Error::InvalidRequest)?;
                if matches!(decoded, b'/' | b'\\') {
                    return Err(TransportV2Error::InvalidRequest);
                }
                index += 3;
            }
            byte if is_path_character(byte) => index += 1,
            _ => return Err(TransportV2Error::InvalidRequest),
        }
    }

    for segment in path.split('/') {
        if is_dot_segment(segment)? {
            return Err(TransportV2Error::InvalidRequest);
        }
    }
    Ok(())
}

/// Encode one opaque UTF-8 final path segment exactly as the released Rust SDK
/// does: ASCII alphanumeric bytes remain literal and every other byte becomes
/// one uppercase `%HH` triplet.
#[cfg(test)]
pub(super) fn encode_canonical_opaque_path_segment(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

fn decode_canonical_opaque_segment(segment: &str) -> Result<Zeroizing<String>> {
    if segment.is_empty() {
        return Err(TransportV2Error::InvalidRequest);
    }

    let encoded = segment.as_bytes();
    let mut decoded = Zeroizing::new(Vec::with_capacity(encoded.len()));
    let mut index = 0;
    while index < encoded.len() {
        let byte = encoded[index];
        if byte.is_ascii_alphanumeric() {
            decoded.push(byte);
            index += 1;
            continue;
        }
        if byte != b'%' {
            return Err(TransportV2Error::InvalidRequest);
        }
        let high = *encoded
            .get(index + 1)
            .ok_or(TransportV2Error::InvalidRequest)?;
        let low = *encoded
            .get(index + 2)
            .ok_or(TransportV2Error::InvalidRequest)?;
        let decoded_byte = (canonical_uri_hex_nibble(high)? << 4) | canonical_uri_hex_nibble(low)?;
        if decoded_byte.is_ascii_alphanumeric() {
            return Err(TransportV2Error::InvalidRequest);
        }
        decoded.push(decoded_byte);
        index += 3;
    }

    match String::from_utf8(std::mem::take(&mut *decoded)) {
        Ok(value) => Ok(Zeroizing::new(value)),
        Err(error) => {
            let mut bytes = error.into_bytes();
            bytes.zeroize();
            Err(TransportV2Error::InvalidRequest)
        }
    }
}

fn canonical_uri_hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(TransportV2Error::InvalidRequest),
    }
}

fn validate_canonical_uuid_path(method: LogicalMethod, path: &str) -> Result<bool> {
    if method == LogicalMethod::Get {
        if let Some(segment) = path.strip_prefix(VERIFY_EMAIL_PATH_PREFIX) {
            decode_canonical_uuid_segment(segment)?;
            return Ok(true);
        }
        if let Some(segment) = path.strip_prefix(PLATFORM_VERIFY_EMAIL_PATH_PREFIX) {
            decode_canonical_uuid_segment(segment)?;
            return Ok(true);
        }
    }
    if validate_canonical_platform_resource_path(method, path)? {
        return Ok(true);
    }
    if validate_canonical_conversation_project_path(method, path)? {
        return Ok(true);
    }
    if validate_canonical_conversation_path(method, path)? {
        return Ok(true);
    }
    if validate_canonical_instruction_path(method, path)? {
        return Ok(true);
    }
    validate_canonical_response_path(method, path)
}

fn validate_canonical_platform_resource_path(method: LogicalMethod, path: &str) -> Result<bool> {
    if let Some(code) = path.strip_prefix(PLATFORM_ACCEPT_INVITE_PATH_PREFIX) {
        if method != LogicalMethod::Post {
            return Ok(false);
        }
        decode_canonical_uuid_segment(code)?;
        return Ok(true);
    }

    let Some(suffix) = path.strip_prefix(PLATFORM_ORG_PATH_PREFIX) else {
        return Ok(false);
    };
    let mut segments = suffix.split('/');
    decode_canonical_uuid_segment(segments.next().unwrap_or_default())?;
    let remainder = segments.collect::<Vec<_>>();

    match remainder.as_slice() {
        [] if method == LogicalMethod::Delete => Ok(true),
        ["projects"] if matches!(method, LogicalMethod::Get | LogicalMethod::Post) => Ok(true),
        ["projects", project]
            if matches!(
                method,
                LogicalMethod::Get | LogicalMethod::Patch | LogicalMethod::Delete
            ) =>
        {
            decode_canonical_uuid_segment(project)?;
            Ok(true)
        }
        ["projects", project, "secrets"]
            if matches!(method, LogicalMethod::Get | LogicalMethod::Post) =>
        {
            decode_canonical_uuid_segment(project)?;
            Ok(true)
        }
        ["projects", project, "secrets", key_name] if method == LogicalMethod::Delete => {
            decode_canonical_uuid_segment(project)?;
            if key_name.is_empty()
                || key_name.len() > 50
                || !key_name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(TransportV2Error::InvalidRequest);
            }
            Ok(true)
        }
        ["projects", project, "settings", "email"]
            if matches!(method, LogicalMethod::Get | LogicalMethod::Put) =>
        {
            decode_canonical_uuid_segment(project)?;
            Ok(true)
        }
        ["projects", project, "settings", "oauth"]
            if matches!(method, LogicalMethod::Get | LogicalMethod::Put) =>
        {
            decode_canonical_uuid_segment(project)?;
            Ok(true)
        }
        ["memberships"] if method == LogicalMethod::Get => Ok(true),
        ["memberships", user] if matches!(method, LogicalMethod::Patch | LogicalMethod::Delete) => {
            decode_canonical_uuid_segment(user)?;
            Ok(true)
        }
        ["invites"] if matches!(method, LogicalMethod::Get | LogicalMethod::Post) => Ok(true),
        ["invites", invite] if matches!(method, LogicalMethod::Get | LogicalMethod::Delete) => {
            decode_canonical_uuid_segment(invite)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn validate_canonical_conversation_project_path(method: LogicalMethod, path: &str) -> Result<bool> {
    if !matches!(
        method,
        LogicalMethod::Get | LogicalMethod::Post | LogicalMethod::Delete
    ) {
        return Ok(false);
    }
    let Some(segment) = path.strip_prefix(CONVERSATION_PROJECT_ITEM_PATH_PREFIX) else {
        return Ok(false);
    };
    decode_canonical_uuid_segment(segment)?;
    Ok(true)
}

fn validate_canonical_instruction_path(method: LogicalMethod, path: &str) -> Result<bool> {
    let Some(segment) = path.strip_prefix(INSTRUCTION_ITEM_PATH_PREFIX) else {
        return Ok(false);
    };
    let segment = if let Some(segment) = segment.strip_suffix("/set-default") {
        if method != LogicalMethod::Post {
            return Ok(false);
        }
        segment
    } else {
        if !matches!(
            method,
            LogicalMethod::Get | LogicalMethod::Post | LogicalMethod::Delete
        ) {
            return Ok(false);
        }
        segment
    };
    decode_canonical_uuid_segment(segment)?;
    Ok(true)
}

fn validate_canonical_conversation_path(method: LogicalMethod, path: &str) -> Result<bool> {
    let Some(suffix) = path.strip_prefix(CONVERSATION_ITEM_PATH_PREFIX) else {
        return Ok(false);
    };
    if matches!(suffix, "batch-delete" | "batch-update-project") {
        return Ok(false);
    }

    if method == LogicalMethod::Get {
        if let Some((conversation, item)) = suffix.split_once("/items/") {
            if item.contains('/') {
                return Err(TransportV2Error::InvalidRequest);
            }
            decode_canonical_uuid_segment(conversation)?;
            decode_canonical_uuid_segment(item)?;
            return Ok(true);
        }
        if let Some(conversation) = suffix.strip_suffix("/items") {
            decode_canonical_uuid_segment(conversation)?;
            return Ok(true);
        }
    }

    if !matches!(
        method,
        LogicalMethod::Get | LogicalMethod::Post | LogicalMethod::Delete
    ) {
        return Ok(false);
    }
    decode_canonical_uuid_segment(suffix)?;
    Ok(true)
}

fn validate_canonical_response_path(method: LogicalMethod, path: &str) -> Result<bool> {
    let Some(suffix) = path.strip_prefix(RESPONSE_ITEM_PATH_PREFIX) else {
        return Ok(false);
    };
    if let Some(response) = suffix.strip_suffix("/cancel") {
        if method != LogicalMethod::Post {
            return Ok(false);
        }
        decode_canonical_uuid_segment(response)?;
        return Ok(true);
    }
    if !matches!(method, LogicalMethod::Get | LogicalMethod::Delete) {
        return Ok(false);
    }
    decode_canonical_uuid_segment(suffix)?;
    Ok(true)
}

fn decode_canonical_uuid_segment(segment: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(segment).map_err(|_| TransportV2Error::InvalidRequest)?;
    if id.hyphenated().to_string() != segment {
        return Err(TransportV2Error::InvalidRequest);
    }
    Ok(id)
}

fn validate_query(query: &str, limits: &EnvelopeLimits) -> Result<()> {
    check_limit(query.len(), limits.query_bytes, "query")?;
    if query.starts_with('?') || query.starts_with('#') || query.contains('#') {
        return Err(TransportV2Error::InvalidRequest);
    }

    let bytes = query.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                decode_percent_triplet(bytes, index).ok_or(TransportV2Error::InvalidRequest)?;
                index += 3;
            }
            byte if is_query_character(byte) => index += 1,
            _ => return Err(TransportV2Error::InvalidRequest),
        }
    }
    Ok(())
}

fn decode_percent_triplet(bytes: &[u8], percent_index: usize) -> Option<u8> {
    let high = *bytes.get(percent_index + 1)?;
    let low = *bytes.get(percent_index + 2)?;
    Some((uri_hex_nibble(high)? << 4) | uri_hex_nibble(low)?)
}

fn uri_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn is_dot_segment(segment: &str) -> Result<bool> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            decoded.push(
                decode_percent_triplet(bytes, index).ok_or(TransportV2Error::InvalidRequest)?,
            );
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Ok(matches!(decoded.as_slice(), b"." | b".."))
}

fn is_path_character(byte: u8) -> bool {
    byte == b'/' || is_uri_pchar(byte)
}

fn is_query_character(byte: u8) -> bool {
    matches!(byte, b'/' | b'?') || is_uri_pchar(byte)
}

fn is_uri_pchar(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
        )
}

fn validate_headers(headers: &[HeaderField], limits: &EnvelopeLimits) -> Result<()> {
    if headers.len() > limits.header_count {
        return Err(TransportV2Error::LimitExceeded {
            field: "header count",
            limit: limits.header_count,
        });
    }

    let mut aggregate_bytes = 0_usize;
    for header in headers {
        check_limit(header.name.len(), limits.header_name_bytes, "header name")?;
        if header.name.is_empty() || !header.name.bytes().all(is_lowercase_http_token) {
            return Err(TransportV2Error::InvalidRequest);
        }

        check_limit(
            header.value_base64.len(),
            limits.header_value_bytes,
            "header value",
        )?;
        if header
            .value_base64
            .as_slice()
            .iter()
            .any(|byte| matches!(byte, b'\r' | b'\n' | 0))
        {
            return Err(TransportV2Error::InvalidRequest);
        }

        aggregate_bytes = aggregate_bytes
            .checked_add(header.name.len())
            .and_then(|total| total.checked_add(header.value_base64.len()))
            .ok_or(TransportV2Error::LimitExceeded {
                field: "aggregate headers",
                limit: limits.aggregate_header_bytes,
            })?;
        check_limit(
            aggregate_bytes,
            limits.aggregate_header_bytes,
            "aggregate headers",
        )?;
    }
    Ok(())
}

fn is_lowercase_http_token(byte: u8) -> bool {
    byte.is_ascii_lowercase()
        || byte.is_ascii_digit()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}
