//! CHECK expressions owned by the accepted schema versions.
//!
//! SQLite discards CHECK expression trees while loading a read-only btree, so
//! its integrity_check cannot validate them. Keep the actual checker read-only
//! and scan these fixed predicates instead. Tests compare this catalog with
//! the complete trusted migration schema; no SQL is taken from the checked DB.

use rusqlite::{Connection, OptionalExtension};

use super::{FindingKind, IntegrityFinding, IntegrityTable, MAX_FINDINGS, finding};
use crate::store::schema;

struct Constraints {
    table: &'static str,
    finding: IntegrityTable,
    // The first schema version containing each predicate.
    checks: &'static [(i64, &'static str)],
}

const CONSTRAINTS: &[Constraints] = &[
    Constraints {
        table: "accounts",
        finding: IntegrityTable::Accounts,
        checks: &[(1, "deletion_phase IN ('pending_server', 'confirmed')")],
    },
    Constraints {
        table: "credentials",
        finding: IntegrityTable::Credentials,
        checks: &[(1, "slot = 1")],
    },
    Constraints {
        table: "app_settings",
        finding: IntegrityTable::AppSettings,
        checks: &[(1, "json_valid(value)")],
    },
    Constraints {
        table: "account_settings",
        finding: IntegrityTable::AccountSettings,
        checks: &[(1, "json_valid(value)")],
    },
    Constraints {
        table: "projects",
        finding: IntegrityTable::Projects,
        checks: &[
            (1, "trusted IN (0, 1)"),
            (1, "json_valid(mcp_defaults)"),
            (2, "name_revision >= 0"),
        ],
    },
    Constraints {
        table: "mcp_servers",
        finding: IntegrityTable::McpServers,
        checks: &[(1, "json_valid(definition)")],
    },
    Constraints {
        table: "mcp_server_secrets",
        finding: IntegrityTable::McpServerSecrets,
        checks: &[(1, "kind IN ('env', 'header')"), (1, "length(target) = 32")],
    },
    Constraints {
        table: "tasks",
        finding: IntegrityTable::Tasks,
        checks: &[
            (1, "kind IN ('desktop', 'acp')"),
            (1, "state IN ('active', 'settled', 'archived')"),
            (1, "provisional IN (0, 1)"),
            (1, "json_valid(settings)"),
            (1, "pi_header IS NULL OR json_valid(pi_header)"),
            (1, "context_base IS NULL OR json_valid(context_base)"),
        ],
    },
    Constraints {
        table: "blobs",
        finding: IntegrityTable::Blobs,
        checks: &[(1, "length(sha256) = 32")],
    },
    Constraints {
        table: "send_receipts",
        finding: IntegrityTable::SendReceipts,
        checks: &[
            (1, "result IS NULL OR json_valid(result)"),
            (1, "interrupted IN (0, 1)"),
        ],
    },
    Constraints {
        table: "remote_identities",
        finding: IntegrityTable::RemoteIdentities,
        checks: &[(1, "length(secret) = 32"), (1, "json_valid(record)")],
    },
];

pub(super) fn collect(
    connection: &Connection,
    findings: &mut Vec<IntegrityFinding>,
    mut stopped: impl FnMut() -> bool,
) -> rusqlite::Result<()> {
    if !connection.is_readonly("main")? {
        return Ok(());
    }
    let version: i64 =
        connection.pragma_query_value(Some("main"), "user_version", |row| row.get(0))?;
    if !(1..=schema::VERSION).contains(&version) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    for table in CONSTRAINTS {
        if findings.len() >= MAX_FINDINGS {
            break;
        }
        if stopped() {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_INTERRUPT),
                None,
            ));
        }
        // NOT(NULL) is NULL, matching SQLite CHECK's accepted NULL result.
        // OR preserves independent constraints when one accepts NULL and
        // another is false. All identifiers and expressions are static here.
        let predicate = table
            .checks
            .iter()
            .filter(|(since, _)| *since <= version)
            .map(|(_, expression)| format!("NOT ({expression})"))
            .collect::<Vec<_>>()
            .join(" OR ");
        let sql = format!(
            "SELECT 1 FROM main.\"{}\" NOT INDEXED WHERE {predicate} LIMIT 1",
            table.table,
        );
        if connection
            .query_row(&sql, [], |row| row.get::<_, i64>(0))
            .optional()?
            .is_some()
        {
            let mut found = finding(FindingKind::Constraint);
            found.table = Some(table.finding);
            findings.push(found);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "integrity_constraints_tests.rs"]
mod tests;
