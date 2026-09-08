use crate::models::schema::app_data_migrations;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppDataMigrationError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
}

#[derive(Queryable, Identifiable, Clone, Debug)]
#[diesel(primary_key(name))]
#[diesel(table_name = app_data_migrations)]
pub struct AppDataMigration {
    pub name: String,
    pub completed_at: DateTime<Utc>,
}

impl AppDataMigration {
    pub fn get(
        conn: &mut PgConnection,
        lookup_name: &str,
    ) -> Result<Option<Self>, AppDataMigrationError> {
        app_data_migrations::table
            .filter(app_data_migrations::name.eq(lookup_name))
            .first::<Self>(conn)
            .optional()
            .map_err(AppDataMigrationError::DatabaseError)
    }

    pub fn exists(
        conn: &mut PgConnection,
        lookup_name: &str,
    ) -> Result<bool, AppDataMigrationError> {
        Ok(Self::get(conn, lookup_name)?.is_some())
    }
}

#[derive(Insertable)]
#[diesel(table_name = app_data_migrations)]
pub struct NewAppDataMigration {
    pub name: String,
}

impl NewAppDataMigration {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn insert(
        &self,
        conn: &mut PgConnection,
    ) -> Result<AppDataMigration, AppDataMigrationError> {
        diesel::insert_into(app_data_migrations::table)
            .values(self)
            .get_result::<AppDataMigration>(conn)
            .map_err(AppDataMigrationError::DatabaseError)
    }
}
