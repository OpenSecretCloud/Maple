//! Public metadata may be reconstructed only from a qualified read-only store.

use super::*;

impl Store {
    pub(super) fn repair_generation_from_readonly(
        &self,
        key: Option<&LockedKey>,
    ) -> LifecycleResult<Generation> {
        // An interrupted operation may already own another reserved generation.
        // Never select one by opening SQLite or overwrite a competing marker.
        for path in [
            self.paths.setup(),
            self.paths.root().join("unclassified-reset.json"),
            self.paths.root().join("db.tmp"),
            self.paths.root().join("db.new"),
            self.paths.root().join("db.old"),
        ] {
            if path_present(&path)? {
                return Err(LifecycleError::Generation);
            }
        }
        verify_generation_contents(&self.paths.db())?;
        let database = salvage::open_readonly_source(&self.paths.database(), key)?;
        if database.pragma_query_value::<i64, _>(None, "application_id", |row| row.get(0))?
            != schema::APPLICATION_ID
            || database.pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))?
                != schema::VERSION
            || database.query_row::<i64, _, _>(
                "SELECT value FROM store_meta WHERE key='schema_epoch'",
                [],
                |row| row.get(0),
            )? != schema::SCHEMA_EPOCH
        {
            return Err(StoreError::SchemaMismatch.into());
        }
        schema::validate(&database)?;
        let id: Vec<u8> = database.query_row(
            "SELECT value FROM store_meta WHERE key='store_id'",
            [],
            |row| row.get(0),
        )?;
        let id: [u8; 16] = id.try_into().map_err(|_| LifecycleError::Generation)?;
        let operation: Option<Vec<u8>> = database
            .query_row(
                "SELECT value FROM store_meta WHERE key='setup_operation_id'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let operation = operation
            .map(|bytes| <[u8; 16]>::try_from(bytes).map(|id| encode_id(&id)))
            .transpose()
            .map_err(|_| LifecycleError::Generation)?;
        let expected = Generation::new(id, operation);
        database
            .close()
            .map_err(|(_, error)| StoreError::from(error))?;
        match read_generation(&self.paths.db()) {
            Ok(Some(actual)) if actual != expected => return Err(LifecycleError::Generation),
            Ok(Some(_)) => return Ok(expected),
            Ok(None) | Err(LifecycleError::Generation) => {}
            Err(error) => return Err(error),
        }
        write_json(&self.paths.generation(), &expected)?;
        Ok(expected)
    }
}
