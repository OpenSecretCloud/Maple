use std::{mem, sync::Arc};

use uuid::Uuid;
use zeroize::Zeroizing;

use super::{
    crypto::{decode_canonical_base64, DirectionalKeys, MIN_RECORD_LEN},
    envelope::{
        EnvelopeLimits, HeaderField, RequestId, StreamRecord, MAX_STREAM_CHUNK_BYTES,
        MAX_STREAM_ERROR_BYTES,
    },
    session::SessionUsage,
    Result, TransportV2Error,
};

// Valid start records are bounded by 64 headers and 64 KiB of decoded
// aggregate header bytes; valid chunk and error records are smaller. This
// ceiling therefore accepts every protocol-valid record while preventing a
// malicious carrier from buffering anywhere near the 50 MiB unary ceiling.
const MAX_STREAM_PLAINTEXT_BYTES: usize = 128 * 1024;
const MAX_STREAM_ENCRYPTED_BYTES: usize = MAX_STREAM_PLAINTEXT_BYTES + MIN_RECORD_LEN;
const MAX_STREAM_BASE64_BYTES: usize = 4 * MAX_STREAM_ENCRYPTED_BYTES.div_ceil(3);
const MAX_STREAM_CARRIER_FRAME_BYTES: usize = b"data: ".len() + MAX_STREAM_BASE64_BYTES + 2;
const MAX_LOGICAL_STREAM_BYTES: usize = EnvelopeLimits::RESPONSE.logical_body_bytes;

#[derive(Eq, PartialEq)]
pub(super) enum StreamEvent {
    Start {
        status: u16,
        headers: Vec<HeaderField>,
    },
    Chunk(Vec<u8>),
    End,
    Error {
        status: u16,
        body: Vec<u8>,
    },
}

impl std::fmt::Debug for StreamEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start { status, headers } => formatter
                .debug_struct("Start")
                .field("status", status)
                .field("header_count", &headers.len())
                .finish(),
            Self::Chunk(body) => formatter
                .debug_struct("Chunk")
                .field("body_bytes", &body.len())
                .finish(),
            Self::End => formatter.write_str("End"),
            Self::Error { status, body } => formatter
                .debug_struct("Error")
                .field("status", status)
                .field("body_bytes", &body.len())
                .finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum StreamState {
    AwaitingStart,
    Open { next_sequence: u64 },
    Terminal,
    Failed,
}

/// Incremental decoder for the exact authenticated outer SSE carrier.
///
/// It owns the response keys and request binding from the admitted request;
/// callers cannot substitute an outer session identifier. `finish` must
/// succeed before the outer carrier is considered complete.
pub(super) struct StreamDecoder {
    session_id: Uuid,
    request_id: RequestId,
    keys: Arc<DirectionalKeys>,
    usage: Arc<SessionUsage>,
    state: StreamState,
    carrier_buffer: Vec<u8>,
    logical_chunk_bytes: usize,
    logical_chunk_limit: usize,
}

impl StreamDecoder {
    pub(super) fn new(
        session_id: Uuid,
        request_id: RequestId,
        keys: Arc<DirectionalKeys>,
        usage: Arc<SessionUsage>,
    ) -> Self {
        Self {
            session_id,
            request_id,
            keys,
            usage,
            state: StreamState::AwaitingStart,
            carrier_buffer: Vec::new(),
            logical_chunk_bytes: 0,
            logical_chunk_limit: MAX_LOGICAL_STREAM_BYTES,
        }
    }

    #[cfg(test)]
    pub(super) fn new_with_logical_limit(
        session_id: Uuid,
        request_id: RequestId,
        keys: Arc<DirectionalKeys>,
        logical_chunk_limit: usize,
    ) -> Self {
        Self {
            session_id,
            request_id,
            keys,
            usage: Arc::new(SessionUsage::new(usize::MAX, usize::MAX)),
            state: StreamState::AwaitingStart,
            carrier_buffer: Vec::new(),
            logical_chunk_bytes: 0,
            logical_chunk_limit,
        }
    }

    /// Add arbitrary HTTP-body bytes. Frames may be split across calls or
    /// coalesced in one call, but each completed frame must be exactly
    /// `data: <canonical-base64>\n\n`.
    pub(super) fn push(&mut self, input: &[u8]) -> Result<Vec<StreamEvent>> {
        if self.state == StreamState::Failed {
            return Err(TransportV2Error::InvalidStreamRecord);
        }
        if self.state == StreamState::Terminal && !input.is_empty() {
            self.state = StreamState::Failed;
            return Err(TransportV2Error::StreamAlreadyTerminal);
        }

        let mut events = Vec::new();
        for byte in input {
            if *byte == b'\r' {
                self.state = StreamState::Failed;
                return Err(TransportV2Error::InvalidStreamFraming);
            }
            if self.state == StreamState::Terminal {
                self.state = StreamState::Failed;
                return Err(TransportV2Error::StreamAlreadyTerminal);
            }
            if self.carrier_buffer.len() == MAX_STREAM_CARRIER_FRAME_BYTES {
                self.state = StreamState::Failed;
                return Err(TransportV2Error::LimitExceeded {
                    field: "stream carrier frame",
                    limit: MAX_STREAM_CARRIER_FRAME_BYTES,
                });
            }

            self.carrier_buffer.push(*byte);
            if self.carrier_buffer.ends_with(b"\n\n") {
                let frame = mem::take(&mut self.carrier_buffer);
                match self.decode_frame(&frame) {
                    Ok(event) => events.push(event),
                    Err(error) => {
                        self.state = StreamState::Failed;
                        return Err(error);
                    }
                }
            }
        }
        Ok(events)
    }

    /// Validate authenticated terminal delivery and an exact carrier boundary.
    pub(super) fn finish(mut self) -> Result<()> {
        if !self.carrier_buffer.is_empty() || self.state != StreamState::Terminal {
            self.state = StreamState::Failed;
            return Err(TransportV2Error::TruncatedStream);
        }
        Ok(())
    }

    fn decode_frame(&mut self, frame: &[u8]) -> Result<StreamEvent> {
        let payload = frame
            .strip_prefix(b"data: ")
            .and_then(|frame| frame.strip_suffix(b"\n\n"))
            .ok_or(TransportV2Error::InvalidStreamFraming)?;
        if payload.is_empty() || payload.iter().any(|byte| matches!(byte, b'\r' | b'\n')) {
            return Err(TransportV2Error::InvalidStreamFraming);
        }
        let encoded =
            std::str::from_utf8(payload).map_err(|_| TransportV2Error::InvalidStreamFraming)?;
        let encrypted = decode_canonical_base64(encoded, MAX_STREAM_ENCRYPTED_BYTES)?;

        let expected_sequence = match self.state {
            StreamState::AwaitingStart => 0,
            StreamState::Open { next_sequence } => next_sequence,
            StreamState::Terminal => return Err(TransportV2Error::StreamAlreadyTerminal),
            StreamState::Failed => return Err(TransportV2Error::InvalidStreamRecord),
        };
        let plaintext = Zeroizing::new(self.keys.decrypt_stream_response_record(
            &self.session_id,
            &self.request_id,
            expected_sequence,
            &encrypted,
        )?);
        if plaintext.len() > MAX_STREAM_PLAINTEXT_BYTES {
            return Err(TransportV2Error::LimitExceeded {
                field: "stream record",
                limit: MAX_STREAM_PLAINTEXT_BYTES,
            });
        }
        let record = StreamRecord::from_json_slice(&plaintext, &EnvelopeLimits::RESPONSE)?;
        if record.request_id() != &self.request_id || record.sequence() != expected_sequence {
            return Err(TransportV2Error::BindingMismatch);
        }
        if matches!(&record, StreamRecord::Chunk { .. }) {
            // Start and terminal capacity were reserved atomically when the
            // request was prepared. Only application chunks charge additional
            // response capacity as they arrive.
            self.usage.reserve_stream_chunk()?;
        }

        match (&self.state, record) {
            (
                StreamState::AwaitingStart,
                StreamRecord::Start {
                    status, headers, ..
                },
            ) => {
                self.state = StreamState::Open { next_sequence: 1 };
                Ok(StreamEvent::Start { status, headers })
            }
            (StreamState::Open { next_sequence }, StreamRecord::Chunk { body_base64, .. }) => {
                debug_assert!(body_base64.len() <= MAX_STREAM_CHUNK_BYTES);
                let logical_chunk_bytes = self
                    .logical_chunk_bytes
                    .checked_add(body_base64.len())
                    .ok_or(TransportV2Error::LimitExceeded {
                    field: "logical stream",
                    limit: self.logical_chunk_limit,
                })?;
                if logical_chunk_bytes > self.logical_chunk_limit {
                    return Err(TransportV2Error::LimitExceeded {
                        field: "logical stream",
                        limit: self.logical_chunk_limit,
                    });
                }
                let next_sequence = next_sequence
                    .checked_add(1)
                    .ok_or(TransportV2Error::InvalidStreamRecord)?;
                self.logical_chunk_bytes = logical_chunk_bytes;
                self.state = StreamState::Open { next_sequence };
                Ok(StreamEvent::Chunk(body_base64.into_bytes()))
            }
            (StreamState::Open { .. }, StreamRecord::End { .. }) => {
                self.state = StreamState::Terminal;
                Ok(StreamEvent::End)
            }
            (
                StreamState::Open { .. },
                StreamRecord::Error {
                    status,
                    body_base64,
                    ..
                },
            ) => {
                debug_assert!(body_base64.len() <= MAX_STREAM_ERROR_BYTES);
                self.state = StreamState::Terminal;
                Ok(StreamEvent::Error {
                    status,
                    body: body_base64.into_bytes(),
                })
            }
            _ => Err(TransportV2Error::InvalidStreamRecord),
        }
    }
}

impl std::fmt::Debug for StreamDecoder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StreamDecoder")
            .field("session_id", &self.session_id)
            .field("request_id", &self.request_id)
            .field("keys", &"[REDACTED]")
            .field("usage", &"[BOUND]")
            .field("state", &self.state)
            .field("carrier_buffer_bytes", &self.carrier_buffer.len())
            .field("logical_chunk_bytes", &self.logical_chunk_bytes)
            .field("logical_chunk_limit", &self.logical_chunk_limit)
            .finish()
    }
}

#[cfg(test)]
pub(super) const fn max_stream_carrier_frame_bytes_for_test() -> usize {
    MAX_STREAM_CARRIER_FRAME_BYTES
}
