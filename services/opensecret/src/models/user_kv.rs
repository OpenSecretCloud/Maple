use crate::models::schema::user_kv;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum UserKVError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
}

#[derive(Queryable, Identifiable, AsChangeset, Serialize, Deserialize, Clone, Debug)]
#[diesel(table_name = user_kv)]
pub struct UserKV {
    pub id: i64,
    pub user_id: Uuid,
    pub key_enc: Vec<u8>,
    pub value_enc: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl UserKV {
    pub fn get_by_id(
        conn: &mut PgConnection,
        lookup_id: i64,
    ) -> Result<Option<UserKV>, UserKVError> {
        user_kv::table
            .filter(user_kv::id.eq(lookup_id))
            .first::<UserKV>(conn)
            .optional()
            .map_err(UserKVError::DatabaseError)
    }

    pub fn get_by_user_and_key(
        conn: &mut PgConnection,
        lookup_user_id: Uuid,
        lookup_key: &Vec<u8>,
    ) -> Result<Option<UserKV>, UserKVError> {
        user_kv::table
            .filter(user_kv::user_id.eq(lookup_user_id))
            .filter(user_kv::key_enc.eq(lookup_key))
            .first::<UserKV>(conn)
            .optional()
            .map_err(UserKVError::DatabaseError)
    }

    pub fn get_all_for_user(
        conn: &mut PgConnection,
        lookup_user_id: Uuid,
    ) -> Result<Vec<UserKV>, UserKVError> {
        user_kv::table
            .filter(user_kv::user_id.eq(lookup_user_id))
            .load::<UserKV>(conn)
            .map_err(UserKVError::DatabaseError)
    }

    pub fn update(&self, conn: &mut PgConnection) -> Result<(), UserKVError> {
        diesel::update(user_kv::table)
            .filter(user_kv::id.eq(self.id))
            .set(self)
            .execute(conn)
            .map(|_| ())
            .map_err(UserKVError::DatabaseError)
    }

    pub fn delete(&self, conn: &mut PgConnection) -> Result<(), UserKVError> {
        diesel::delete(user_kv::table)
            .filter(user_kv::id.eq(self.id))
            .execute(conn)
            .map(|_| ())
            .map_err(UserKVError::DatabaseError)
    }

    pub fn delete_all_for_user(
        conn: &mut PgConnection,
        lookup_user_id: Uuid,
    ) -> Result<(), UserKVError> {
        diesel::delete(user_kv::table)
            .filter(user_kv::user_id.eq(lookup_user_id))
            .execute(conn)
            .map(|_| ())
            .map_err(UserKVError::DatabaseError)
    }
}

#[derive(Insertable)]
#[diesel(table_name = user_kv)]
pub struct NewUserKV {
    pub user_id: Uuid,
    pub key_enc: Vec<u8>,
    pub value_enc: Vec<u8>,
}

impl NewUserKV {
    pub fn new(user_id: Uuid, key_enc: Vec<u8>, value_enc: Vec<u8>) -> Self {
        NewUserKV {
            user_id,
            key_enc,
            value_enc,
        }
    }

    pub fn insert(&self, conn: &mut PgConnection) -> Result<UserKV, UserKVError> {
        diesel::insert_into(user_kv::table)
            .values(self)
            .on_conflict((user_kv::user_id, user_kv::key_enc))
            .do_update()
            .set(user_kv::value_enc.eq(self.value_enc.clone()))
            .get_result::<UserKV>(conn)
            .map_err(UserKVError::DatabaseError)
    }
}
