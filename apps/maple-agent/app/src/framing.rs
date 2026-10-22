//! Bound serialized frames before allocating or writing a whole response.

use serde::Serialize;
use std::io::{self, Write};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EncodeError {
    TooLarge,
    Invalid,
}

struct Buffer {
    bytes: Zeroizing<Vec<u8>>,
    limit: usize,
    exceeded: bool,
}

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(next) = self.bytes.len().checked_add(bytes.len()) else {
            self.exceeded = true;
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        };
        if next > self.limit {
            self.exceeded = true;
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        if next > self.bytes.capacity() {
            let capacity = self
                .bytes
                .capacity()
                .saturating_mul(2)
                .max(next)
                .min(self.limit);
            let additional = capacity.saturating_sub(self.bytes.len());
            self.bytes
                .try_reserve_exact(additional)
                .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn encode(
    value: &impl Serialize,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, EncodeError> {
    let mut buffer = Buffer {
        bytes: Zeroizing::new(Vec::new()),
        limit,
        exceeded: false,
    };
    if serde_json::to_writer(&mut buffer, value).is_err() {
        return Err(if buffer.exceeded {
            EncodeError::TooLarge
        } else {
            EncodeError::Invalid
        });
    }
    Ok(buffer.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_payloads_are_capped_during_serialization() {
        let value = "\u{0000}".repeat(1024 * 1024);
        assert_eq!(encode(&value, 4096), Err(EncodeError::TooLarge));
        let mut buffer = Buffer {
            bytes: Zeroizing::new(Vec::new()),
            limit: 4096,
            exceeded: false,
        };
        assert!(serde_json::to_writer(&mut buffer, &value).is_err());
        assert!(buffer.bytes.len() <= 4096);
        assert!(buffer.bytes.capacity() <= 4096);
    }

    #[test]
    fn boundary_counts_serialized_utf8_and_envelope_bytes() {
        let value = serde_json::json!({"text":"a\"é"});
        let expected = serde_json::to_vec(&value).unwrap();
        assert_eq!(&*encode(&value, expected.len()).unwrap(), &expected);
        assert_eq!(
            encode(&value, expected.len() - 1),
            Err(EncodeError::TooLarge)
        );
    }
}
