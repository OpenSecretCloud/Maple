use axum::body::Bytes;
use serde::{Deserialize, Serialize};

use super::{
    crypto::RECORD_TAG_BYTES,
    envelope::{EnvelopeError, LogicalHeader},
};

pub(crate) const MAX_RESPONSE_CHUNK_BYTES: usize = 64 * 1024;
pub(crate) const MAX_RESPONSE_METADATA_BYTES: usize = 64 * 1024;
pub(crate) const MAX_RESPONSE_ERROR_CODE_BYTES: usize = 64;
pub(crate) const MAX_RESPONSE_RECORD_PLAINTEXT_BYTES: usize = 1 + MAX_RESPONSE_CHUNK_BYTES;
pub(crate) const MAX_RESPONSE_RECORD_CIPHERTEXT_BYTES: usize =
    MAX_RESPONSE_RECORD_PLAINTEXT_BYTES + RECORD_TAG_BYTES;
pub(crate) const CIPHERTEXT_LENGTH_BYTES: usize = 4;

const START_TAG: u8 = 1;
const CHUNK_TAG: u8 = 2;
const END_TAG: u8 = 3;
const ERROR_TAG: u8 = 4;
const MAX_RESPONSE_HEADER_COUNT: usize = 32;

#[derive(Debug, thiserror::Error)]
pub(crate) enum FramingError {
    #[error("transport-v2 response record is truncated")]
    Truncated,
    #[error("transport-v2 response record is too large")]
    RecordTooLarge,
    #[error("transport-v2 response record tag is invalid")]
    InvalidTag,
    #[error("transport-v2 response metadata is invalid")]
    InvalidMetadata(#[source] serde_json::Error),
    #[error("transport-v2 response status is invalid")]
    InvalidStatus,
    #[error("transport-v2 response has too many headers")]
    TooManyHeaders,
    #[error("transport-v2 response header is invalid")]
    InvalidHeader(#[source] EnvelopeError),
    #[error("transport-v2 response error code is invalid")]
    InvalidErrorCode,
    #[error("transport-v2 response terminal record has trailing bytes")]
    TrailingBytes,
    #[error("transport-v2 ciphertext frame length is invalid")]
    InvalidCiphertextLength,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResponseStart {
    status: u16,
    headers: Vec<LogicalHeader>,
}

impl ResponseStart {
    pub(crate) fn new(status: u16, headers: Vec<LogicalHeader>) -> Result<Self, FramingError> {
        let start = Self { status, headers };
        start.validate()?;
        Ok(start)
    }

    pub(crate) const fn status(&self) -> u16 {
        self.status
    }

    pub(crate) fn headers(&self) -> &[LogicalHeader] {
        &self.headers
    }

    fn validate(&self) -> Result<(), FramingError> {
        if !(200..=599).contains(&self.status) {
            return Err(FramingError::InvalidStatus);
        }
        if self.headers.len() > MAX_RESPONSE_HEADER_COUNT {
            return Err(FramingError::TooManyHeaders);
        }
        for header in &self.headers {
            LogicalHeader::new(header.name().to_owned(), header.value().to_owned())
                .map_err(FramingError::InvalidHeader)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ResponseFailure {
    code: String,
}

pub(crate) enum ResponseRecord {
    Start(ResponseStart),
    Chunk(Bytes),
    End,
    Error { code: String },
}

impl ResponseRecord {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, FramingError> {
        match self {
            Self::Start(start) => {
                start.validate()?;
                encode_metadata(START_TAG, start)
            }
            Self::Chunk(bytes) => {
                if bytes.len() > MAX_RESPONSE_CHUNK_BYTES {
                    return Err(FramingError::RecordTooLarge);
                }
                let mut encoded = Vec::with_capacity(1 + bytes.len());
                encoded.push(CHUNK_TAG);
                encoded.extend_from_slice(bytes);
                Ok(encoded)
            }
            Self::End => Ok(vec![END_TAG]),
            Self::Error { code } => {
                validate_error_code(code)?;
                encode_metadata(ERROR_TAG, &ResponseFailure { code: code.clone() })
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn decode(encoded: &[u8]) -> Result<Self, FramingError> {
        let (&tag, payload) = encoded.split_first().ok_or(FramingError::Truncated)?;
        match tag {
            START_TAG => {
                let start: ResponseStart = decode_metadata(payload)?;
                start.validate()?;
                Ok(Self::Start(start))
            }
            CHUNK_TAG => {
                if payload.len() > MAX_RESPONSE_CHUNK_BYTES {
                    return Err(FramingError::RecordTooLarge);
                }
                Ok(Self::Chunk(Bytes::copy_from_slice(payload)))
            }
            END_TAG if payload.is_empty() => Ok(Self::End),
            END_TAG => Err(FramingError::TrailingBytes),
            ERROR_TAG => {
                let failure: ResponseFailure = decode_metadata(payload)?;
                validate_error_code(&failure.code)?;
                Ok(Self::Error { code: failure.code })
            }
            _ => Err(FramingError::InvalidTag),
        }
    }
}

pub(crate) fn frame_ciphertext(ciphertext: &[u8]) -> Result<Vec<u8>, FramingError> {
    if ciphertext.len() < RECORD_TAG_BYTES
        || ciphertext.len() > MAX_RESPONSE_RECORD_CIPHERTEXT_BYTES
    {
        return Err(FramingError::InvalidCiphertextLength);
    }
    let length =
        u32::try_from(ciphertext.len()).map_err(|_| FramingError::InvalidCiphertextLength)?;
    let mut framed = Vec::with_capacity(CIPHERTEXT_LENGTH_BYTES + ciphertext.len());
    framed.extend_from_slice(&length.to_be_bytes());
    framed.extend_from_slice(ciphertext);
    Ok(framed)
}

fn encode_metadata<T: Serialize>(tag: u8, value: &T) -> Result<Vec<u8>, FramingError> {
    let metadata = serde_json::to_vec(value).map_err(FramingError::InvalidMetadata)?;
    if metadata.len() > MAX_RESPONSE_METADATA_BYTES
        || metadata.len() + 1 > MAX_RESPONSE_RECORD_PLAINTEXT_BYTES
    {
        return Err(FramingError::RecordTooLarge);
    }
    let mut encoded = Vec::with_capacity(1 + metadata.len());
    encoded.push(tag);
    encoded.extend_from_slice(&metadata);
    Ok(encoded)
}

#[cfg(test)]
fn decode_metadata<T: for<'de> Deserialize<'de>>(encoded: &[u8]) -> Result<T, FramingError> {
    if encoded.len() > MAX_RESPONSE_METADATA_BYTES {
        return Err(FramingError::RecordTooLarge);
    }
    serde_json::from_slice(encoded).map_err(FramingError::InvalidMetadata)
}

fn validate_error_code(code: &str) -> Result<(), FramingError> {
    if code.is_empty()
        || code.len() > MAX_RESPONSE_ERROR_CODE_BYTES
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(FramingError::InvalidErrorCode);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_records_round_trip_without_encoding_body_bytes() {
        let records = [
            ResponseRecord::Start(
                ResponseStart::new(
                    200,
                    vec![LogicalHeader::new(
                        "content-type".to_string(),
                        "text/event-stream".to_string(),
                    )
                    .unwrap()],
                )
                .unwrap(),
            ),
            ResponseRecord::Chunk(Bytes::from_static(b"data: hello\n\n")),
            ResponseRecord::Chunk(Bytes::new()),
            ResponseRecord::End,
            ResponseRecord::Error {
                code: "application_stream_failed".to_string(),
            },
        ];

        for record in records {
            let encoded = record.encode().unwrap();
            let decoded = ResponseRecord::decode(&encoded).unwrap();
            match (record, decoded) {
                (ResponseRecord::Start(expected), ResponseRecord::Start(actual)) => {
                    assert_eq!(actual, expected);
                }
                (ResponseRecord::Chunk(expected), ResponseRecord::Chunk(actual)) => {
                    assert_eq!(actual, expected);
                }
                (ResponseRecord::End, ResponseRecord::End) => {}
                (
                    ResponseRecord::Error { code: expected },
                    ResponseRecord::Error { code: actual },
                ) => assert_eq!(actual, expected),
                _ => panic!("response record kind changed during round trip"),
            }
        }
    }

    #[test]
    fn record_and_outer_frame_limits_are_local_not_aggregate() {
        let maximum = ResponseRecord::Chunk(Bytes::from(vec![0; MAX_RESPONSE_CHUNK_BYTES]))
            .encode()
            .unwrap();
        assert_eq!(maximum.len(), MAX_RESPONSE_RECORD_PLAINTEXT_BYTES);
        assert!(matches!(
            ResponseRecord::Chunk(Bytes::from(vec![0; MAX_RESPONSE_CHUNK_BYTES + 1])).encode(),
            Err(FramingError::RecordTooLarge)
        ));

        let ciphertext = vec![0; MAX_RESPONSE_RECORD_CIPHERTEXT_BYTES];
        let framed = frame_ciphertext(&ciphertext).unwrap();
        assert_eq!(
            u32::from_be_bytes(framed[..4].try_into().unwrap()) as usize,
            ciphertext.len()
        );
        assert_eq!(&framed[4..], ciphertext);
    }

    #[test]
    fn metadata_and_terminal_shapes_are_strict() {
        assert!(ResponseStart::new(199, vec![]).is_err());
        assert!(ResponseRecord::Error {
            code: "Not Portable".to_string()
        }
        .encode()
        .is_err());
        assert!(matches!(
            ResponseRecord::decode(&[END_TAG, 0]),
            Err(FramingError::TrailingBytes)
        ));
        assert!(
            ResponseRecord::decode(b"\x01{\"status\":200,\"headers\":[],\"extra\":1}").is_err()
        );
    }

    #[test]
    fn repeated_response_headers_round_trip_in_order() {
        let headers = vec![
            LogicalHeader::new("x-repeat".into(), "first".into()).unwrap(),
            LogicalHeader::new("x-repeat".into(), "second".into()).unwrap(),
        ];
        let encoded = ResponseRecord::Start(ResponseStart::new(200, headers.clone()).unwrap())
            .encode()
            .unwrap();
        let decoded = ResponseRecord::decode(&encoded).unwrap();

        let ResponseRecord::Start(start) = decoded else {
            panic!("response start changed record kind");
        };
        assert_eq!(start.headers(), headers);
    }
}
