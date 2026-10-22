//! Persistence errors deliberately contain neither SQL nor stored values.

use std::{fmt, io};

use serde::{Deserialize, Serialize};

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageReason {
    Database,
    Schema,
    Generation,
    Synchronization,
}

/// A valid or unclassified store that this process must leave untouched.
/// These reasons never authorize repair, replacement, deletion or setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    NewerVersion,
    IncompatibleSchema,
    UnsupportedFilesystem,
    UnsafePath,
    UnsupportedOperationFormat,
    Unavailable,
}

impl RefusalReason {
    pub fn message(self) -> &'static str {
        match self {
            Self::NewerVersion => {
                "This local store requires a newer Maple version. Update Maple and reopen this store."
            }
            Self::IncompatibleSchema => {
                "This Maple version cannot open this local store schema. Use a compatible Maple build and keep the store folder intact."
            }
            Self::UnsupportedFilesystem => {
                "This local store needs a local disk. Close Maple, keep the original folder, and copy this store to a private local folder before restarting with that data location."
            }
            Self::UnsafePath => {
                "This local store is not in a private, safe folder. Close Maple and use a private local copy with the correct ownership and permissions."
            }
            Self::UnsupportedOperationFormat => {
                "This Maple version cannot resume this local store operation. Keep the store folder intact and reopen it with a compatible Maple build."
            }
            Self::Unavailable => {
                "Maple could not open this local store. Check disk space and access permissions, then restart Maple."
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    Locked,
    NeedsSetup,
    Busy,
    DiskFull,
    Damaged { reason: DamageReason },
    Refused { reason: RefusalReason },
    UnsupportedOperationFormat,
    NotFound,
    Conflict,
    EpochChanged,
    InvalidInput,
    InvalidData,
    UnsafePath,
    UnsupportedFilesystem,
    SchemaMismatch,
    NewerVersion { found: i64, supported: i64 },
    SchemaEpoch { found: i64, supported: i64 },
    Database { code: i32, extended_code: i32 },
    Io { kind: io::ErrorKind },
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locked => formatter.write_str("Maple's local store is locked"),
            Self::NeedsSetup => formatter.write_str("Maple's local store needs setup"),
            Self::Busy => formatter.write_str("Maple's local store is busy"),
            Self::DiskFull => formatter.write_str("Maple could not save because the disk is full"),
            Self::Damaged { .. } => formatter.write_str("Maple's local store is damaged"),
            Self::Refused { reason } => formatter.write_str(reason.message()),
            Self::UnsupportedOperationFormat => {
                formatter.write_str(RefusalReason::UnsupportedOperationFormat.message())
            }
            Self::NotFound => formatter.write_str("The local record was not found"),
            Self::Conflict => formatter.write_str("The local record conflicts with saved data"),
            Self::EpochChanged => {
                formatter.write_str("The local history changed; reload its latest page")
            }
            Self::InvalidInput => formatter.write_str("The local store request is invalid"),
            Self::InvalidData => formatter.write_str("The local store contains invalid data"),
            Self::UnsafePath => formatter.write_str("The local store path is not private and safe"),
            Self::UnsupportedFilesystem => formatter.write_str(
                "The local store needs a local filesystem; set XDG_DATA_HOME to a local directory",
            ),
            Self::SchemaMismatch => {
                formatter.write_str("The local store schema does not match Maple")
            }
            Self::NewerVersion { .. } => {
                formatter.write_str("The local store was written by a newer Maple")
            }
            Self::SchemaEpoch { .. } => {
                formatter.write_str("The local store uses a schema this Maple version cannot open")
            }
            Self::Database { .. } => formatter.write_str("The local database operation failed"),
            Self::Io { .. } => formatter.write_str("The local store file operation failed"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::StorageFull {
            Self::DiskFull
        } else {
            Self::Io { kind: error.kind() }
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        use rusqlite::ErrorCode;
        match error {
            rusqlite::Error::QueryReturnedNoRows => Self::NotFound,
            rusqlite::Error::SqliteFailure(failure, _) => match failure.code {
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => Self::Busy,
                ErrorCode::DiskFull => Self::DiskFull,
                ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => Self::Damaged {
                    reason: DamageReason::Database,
                },
                ErrorCode::ConstraintViolation => Self::Conflict,
                _ => Self::Database {
                    code: failure.extended_code & 0xff,
                    extended_code: failure.extended_code,
                },
            },
            // SqlInputError can own the entire statement, and SQLCipher errors can
            // contain sensitive tokens. Never retain, source-chain or format it.
            _ => Self::InvalidData,
        }
    }
}

impl From<StoreError> for io::Error {
    fn from(error: StoreError) -> Self {
        let kind = match error {
            StoreError::Busy => io::ErrorKind::WouldBlock,
            StoreError::DiskFull => io::ErrorKind::StorageFull,
            StoreError::NotFound => io::ErrorKind::NotFound,
            StoreError::InvalidInput => io::ErrorKind::InvalidInput,
            StoreError::Conflict => io::ErrorKind::AlreadyExists,
            StoreError::Locked | StoreError::UnsafePath => io::ErrorKind::PermissionDenied,
            _ => io::ErrorKind::Other,
        };
        Self::new(kind, error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_full_has_one_safe_category_across_sqlite_and_io() {
        let sqlite = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL),
            Some("private statement and values".into()),
        );
        assert_eq!(StoreError::from(sqlite), StoreError::DiskFull);
        let io_error = io::Error::new(io::ErrorKind::StorageFull, "private path");
        assert_eq!(StoreError::from(io_error), StoreError::DiskFull);
        let io_error: io::Error = StoreError::DiskFull.into();
        assert_eq!(io_error.kind(), io::ErrorKind::StorageFull);
        assert!(!io_error.to_string().contains("private"));
    }

    #[test]
    fn database_errors_never_carry_sql_or_values() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        let error = connection
            .execute_batch("SELECT secret_password FROM missing_secret_table")
            .unwrap_err();
        let error = StoreError::from(error);
        for rendered in [format!("{error}"), format!("{error:?}")] {
            assert!(!rendered.contains("secret"));
            assert!(!rendered.contains("SELECT"));
        }
        assert!(std::error::Error::source(&error).is_none());
    }
}
