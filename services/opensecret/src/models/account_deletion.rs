use crate::models::schema::account_deletion_requests;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum AccountDeletionError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),

    #[error("Account deletion request is already deleted")]
    RequestAlreadyDeleted,

    #[error("Account deletion request has expired")]
    RequestExpired,
}

#[derive(Queryable, Identifiable, AsChangeset, Serialize, Deserialize, Clone, Debug)]
#[diesel(table_name = account_deletion_requests)]
pub struct AccountDeletionRequest {
    pub id: i32,
    pub user_id: Uuid,
    pub project_id: i32,
    pub hashed_secret: String,
    pub encrypted_code: Vec<u8>,
    pub expiration_time: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub is_deleted: bool,
}

impl AccountDeletionRequest {
    pub fn get_by_user_id_and_code(
        conn: &mut PgConnection,
        lookup_user_id: Uuid,
        lookup_encrypted_code: &[u8],
    ) -> Result<Option<AccountDeletionRequest>, AccountDeletionError> {
        account_deletion_requests::table
            .filter(account_deletion_requests::user_id.eq(lookup_user_id))
            .filter(account_deletion_requests::encrypted_code.eq(lookup_encrypted_code))
            .filter(account_deletion_requests::is_deleted.eq(false))
            .filter(account_deletion_requests::expiration_time.gt(Utc::now()))
            .first::<AccountDeletionRequest>(conn)
            .optional()
            .map_err(AccountDeletionError::DatabaseError)
    }

    pub fn mark_as_deleted(&self, conn: &mut PgConnection) -> Result<(), AccountDeletionError> {
        diesel::update(account_deletion_requests::table)
            .filter(account_deletion_requests::id.eq(self.id))
            .set((
                account_deletion_requests::is_deleted.eq(true),
                account_deletion_requests::completed_at.eq(Utc::now()),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(AccountDeletionError::DatabaseError)
    }

    pub fn is_expired(&self) -> Result<(), AccountDeletionError> {
        if Utc::now() > self.expiration_time {
            Err(AccountDeletionError::RequestExpired)
        } else {
            Ok(())
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = account_deletion_requests)]
pub struct NewAccountDeletionRequest {
    pub user_id: Uuid,
    pub project_id: i32,
    pub hashed_secret: String,
    pub encrypted_code: Vec<u8>,
    pub expiration_time: DateTime<Utc>,
}

impl NewAccountDeletionRequest {
    pub fn new(
        user_id: Uuid,
        project_id: i32,
        hashed_secret: String,
        encrypted_code: Vec<u8>,
        expiration_hours: i64,
    ) -> Self {
        NewAccountDeletionRequest {
            user_id,
            project_id,
            hashed_secret,
            encrypted_code,
            expiration_time: Utc::now() + chrono::Duration::hours(expiration_hours),
        }
    }

    pub fn insert(
        &self,
        conn: &mut PgConnection,
    ) -> Result<AccountDeletionRequest, AccountDeletionError> {
        diesel::insert_into(account_deletion_requests::table)
            .values(self)
            .get_result::<AccountDeletionRequest>(conn)
            .map_err(AccountDeletionError::DatabaseError)
    }
}
