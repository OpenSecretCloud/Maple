use crate::models::schema::user_seed_wrappings;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::upsert::excluded;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum UserSeedWrappingError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
}

#[derive(Queryable, Identifiable, Clone, Debug)]
#[diesel(table_name = user_seed_wrappings)]
pub struct UserSeedWrapping {
    pub id: i64,
    pub user_id: Uuid,
    pub credential_kind: String,
    pub credential_lookup_hash: Vec<u8>,
    pub wrapping_version: i16,
    pub seed_enc: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl UserSeedWrapping {
    pub fn get_for_user_and_kind(
        conn: &mut PgConnection,
        lookup_user_id: Uuid,
        lookup_credential_kind: &str,
    ) -> Result<Vec<Self>, UserSeedWrappingError> {
        user_seed_wrappings::table
            .filter(user_seed_wrappings::user_id.eq(lookup_user_id))
            .filter(user_seed_wrappings::credential_kind.eq(lookup_credential_kind))
            .order(user_seed_wrappings::id.asc())
            .load::<Self>(conn)
            .map_err(UserSeedWrappingError::DatabaseError)
    }

    pub fn get_by_credential(
        conn: &mut PgConnection,
        lookup_user_id: Uuid,
        lookup_credential_kind: &str,
        lookup_credential_hash: &[u8],
        lookup_wrapping_version: i16,
    ) -> Result<Option<Self>, UserSeedWrappingError> {
        user_seed_wrappings::table
            .filter(user_seed_wrappings::user_id.eq(lookup_user_id))
            .filter(user_seed_wrappings::credential_kind.eq(lookup_credential_kind))
            .filter(user_seed_wrappings::credential_lookup_hash.eq(lookup_credential_hash))
            .filter(user_seed_wrappings::wrapping_version.eq(lookup_wrapping_version))
            .first::<Self>(conn)
            .optional()
            .map_err(UserSeedWrappingError::DatabaseError)
    }

    pub fn delete_for_user(
        conn: &mut PgConnection,
        lookup_user_id: Uuid,
    ) -> Result<usize, UserSeedWrappingError> {
        diesel::delete(
            user_seed_wrappings::table.filter(user_seed_wrappings::user_id.eq(lookup_user_id)),
        )
        .execute(conn)
        .map_err(UserSeedWrappingError::DatabaseError)
    }

    pub fn delete_for_user_and_kind(
        conn: &mut PgConnection,
        lookup_user_id: Uuid,
        lookup_credential_kind: &str,
    ) -> Result<usize, UserSeedWrappingError> {
        diesel::delete(
            user_seed_wrappings::table
                .filter(user_seed_wrappings::user_id.eq(lookup_user_id))
                .filter(user_seed_wrappings::credential_kind.eq(lookup_credential_kind)),
        )
        .execute(conn)
        .map_err(UserSeedWrappingError::DatabaseError)
    }
}

#[derive(Insertable, Clone, Debug)]
#[diesel(table_name = user_seed_wrappings)]
pub struct NewUserSeedWrapping {
    pub user_id: Uuid,
    pub credential_kind: String,
    pub credential_lookup_hash: Vec<u8>,
    pub wrapping_version: i16,
    pub seed_enc: Vec<u8>,
}

impl NewUserSeedWrapping {
    pub fn new(
        user_id: Uuid,
        credential_kind: impl Into<String>,
        credential_lookup_hash: Vec<u8>,
        wrapping_version: i16,
        seed_enc: Vec<u8>,
    ) -> Self {
        Self {
            user_id,
            credential_kind: credential_kind.into(),
            credential_lookup_hash,
            wrapping_version,
            seed_enc,
        }
    }

    pub fn insert(
        &self,
        conn: &mut PgConnection,
    ) -> Result<UserSeedWrapping, UserSeedWrappingError> {
        diesel::insert_into(user_seed_wrappings::table)
            .values(self)
            .get_result::<UserSeedWrapping>(conn)
            .map_err(UserSeedWrappingError::DatabaseError)
    }

    pub fn upsert_by_credential(
        &self,
        conn: &mut PgConnection,
    ) -> Result<UserSeedWrapping, UserSeedWrappingError> {
        diesel::insert_into(user_seed_wrappings::table)
            .values(self)
            .on_conflict((
                user_seed_wrappings::user_id,
                user_seed_wrappings::credential_kind,
                user_seed_wrappings::credential_lookup_hash,
                user_seed_wrappings::wrapping_version,
            ))
            .do_update()
            .set((
                user_seed_wrappings::seed_enc.eq(excluded(user_seed_wrappings::seed_enc)),
                user_seed_wrappings::updated_at.eq(diesel::dsl::now),
            ))
            .get_result::<UserSeedWrapping>(conn)
            .map_err(UserSeedWrappingError::DatabaseError)
    }
}
