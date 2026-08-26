use crate::error::{Error, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::io::Cursor;

pub(crate) type Value = ciborium::value::Value;

pub(crate) fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    ciborium::de::from_reader(Cursor::new(bytes)).map_err(|e| Error::Cbor(e.to_string()))
}

pub(crate) fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes).map_err(|e| Error::Cbor(e.to_string()))?;
    Ok(bytes)
}
