//! Lossless, size-checked payload encoding shared by source entries and rows.

use std::io::Read;

use super::{StoreError, StoreResult};

const COMPRESS_AT: usize = 8 * 1024;
// A corrupt compressed value must not allocate an unbounded amount of memory.
pub(super) const MAX_DECODED_BYTES: usize = 256 * 1024 * 1024;

pub(super) fn encode(bytes: &[u8]) -> StoreResult<(i64, Vec<u8>)> {
    if bytes.len() > MAX_DECODED_BYTES {
        return Err(StoreError::InvalidInput);
    }
    if bytes.len() < COMPRESS_AT {
        return Ok((0, bytes.to_vec()));
    }
    zstd::stream::encode_all(bytes, 3)
        .map(|encoded| (1, encoded))
        .map_err(|_| StoreError::InvalidData)
}

pub(super) fn decode(codec: i64, bytes: &[u8]) -> StoreResult<Vec<u8>> {
    match codec {
        0 if bytes.len() <= MAX_DECODED_BYTES => Ok(bytes.to_vec()),
        1 => {
            let decoder =
                zstd::stream::read::Decoder::new(bytes).map_err(|_| StoreError::InvalidData)?;
            let mut decoded = Vec::new();
            decoder
                .take(MAX_DECODED_BYTES as u64 + 1)
                .read_to_end(&mut decoded)
                .map_err(|_| StoreError::InvalidData)?;
            if decoded.len() > MAX_DECODED_BYTES {
                return Err(StoreError::InvalidData);
            }
            Ok(decoded)
        }
        _ => Err(StoreError::InvalidData),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_and_binary_round_trip_are_lossless() {
        for size in [0, COMPRESS_AT - 1, COMPRESS_AT, 128 * 1024] {
            let original = (0..size).map(|i| (i % 251) as u8).collect::<Vec<_>>();
            let (codec, encoded) = encode(&original).unwrap();
            assert_eq!(codec, i64::from(size >= COMPRESS_AT));
            assert_eq!(decode(codec, &encoded).unwrap(), original);
        }
    }

    #[test]
    fn invalid_codecs_and_truncated_frames_fail_closed() {
        assert_eq!(decode(2, b"secret"), Err(StoreError::InvalidData));
        assert_eq!(decode(1, b"not a frame"), Err(StoreError::InvalidData));
        let (_, encoded) = encode(&vec![42; COMPRESS_AT]).unwrap();
        assert_eq!(
            decode(1, &encoded[..encoded.len() / 2]),
            Err(StoreError::InvalidData)
        );
    }
}
