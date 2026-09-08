use crate::encrypt::{
    decrypt_key_deterministic, decrypt_with_key, encrypt_key_deterministic, encrypt_with_key,
};
use crate::{
    aws_credentials::AwsCredentialManager,
    models::user_kv::{NewUserKV, UserKV, UserKVError},
};
use diesel::prelude::*;
use secp256k1::SecretKey;
use serde::Serialize;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error};
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("Key not found: {0}")]
    KeyNotFound(String),
    #[error("Unauthorized access")]
    Unauthorized,
    #[error("Decryption error")]
    DecryptionError,
    #[error("Database error: {0}")]
    DatabaseError(#[from] UserKVError),
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug, Clone, Serialize)]
pub struct KVPair {
    pub key: String,
    pub value: String,
    pub created_at: i64,
    pub updated_at: i64,
}

// Update the get function
pub fn get(
    pool: &diesel::r2d2::Pool<diesel::r2d2::ConnectionManager<PgConnection>>,
    user_id: Uuid,
    key: &str,
    user_secret_key: &SecretKey,
) -> StoreResult<Option<String>> {
    debug!("Getting KV pair");
    let mut conn = pool.get().map_err(|e| {
        error!("Failed to get database connection: {:?}", e);
        StoreError::DatabaseError(UserKVError::DatabaseError(diesel::result::Error::NotFound))
    })?;

    let encrypted_key = encrypt_key_deterministic(user_secret_key, key.as_bytes());

    let user_kv = UserKV::get_by_user_and_key(&mut conn, user_id, &encrypted_key).map_err(|e| {
        error!("Failed to get KV pair: {:?}", e);
        e
    })?;

    if let Some(user_kv) = user_kv {
        let decrypted_value =
            decrypt_with_key(user_secret_key, &user_kv.value_enc).map_err(|e| {
                error!("Failed to decrypt value: {:?}", e);
                StoreError::DecryptionError
            })?;
        let value_str = String::from_utf8(decrypted_value).map_err(|e| {
            error!("Failed to convert decrypted value to string: {:?}", e);
            StoreError::DecryptionError
        })?;
        Ok(Some(value_str))
    } else {
        Ok(None)
    }
}

pub async fn put(
    pool: &diesel::r2d2::Pool<diesel::r2d2::ConnectionManager<PgConnection>>,
    user_id: Uuid,
    key: String,
    value: String,
    encryption_key: &SecretKey,
    _aws_credential_manager: Arc<tokio::sync::RwLock<Option<AwsCredentialManager>>>,
) -> StoreResult<()> {
    let mut conn = pool.get().map_err(|_| {
        StoreError::DatabaseError(UserKVError::DatabaseError(diesel::result::Error::NotFound))
    })?;

    let encrypted_key = encrypt_key_deterministic(encryption_key, key.as_bytes());
    let encrypted_value = encrypt_with_key(encryption_key, value.as_bytes()).await;

    let new_user_kv = NewUserKV {
        user_id,
        key_enc: encrypted_key,
        value_enc: encrypted_value,
    };

    new_user_kv.insert(&mut conn)?;

    Ok(())
}

pub fn delete(
    pool: &diesel::r2d2::Pool<diesel::r2d2::ConnectionManager<PgConnection>>,
    user_id: Uuid,
    key: &str,
    user_secret_key: &SecretKey,
) -> StoreResult<()> {
    let mut conn = pool.get().map_err(|_| {
        StoreError::DatabaseError(UserKVError::DatabaseError(diesel::result::Error::NotFound))
    })?;

    let encrypted_key = encrypt_key_deterministic(user_secret_key, key.as_bytes());

    let user_kv = UserKV::get_by_user_and_key(&mut conn, user_id, &encrypted_key)?;

    if let Some(user_kv) = user_kv {
        user_kv.delete(&mut conn)?;
        Ok(())
    } else {
        Err(StoreError::KeyNotFound(key.to_string()))
    }
}

pub fn delete_all(
    pool: &diesel::r2d2::Pool<diesel::r2d2::ConnectionManager<PgConnection>>,
    user_id: Uuid,
) -> StoreResult<()> {
    let mut conn = pool.get().map_err(|_| {
        StoreError::DatabaseError(UserKVError::DatabaseError(diesel::result::Error::NotFound))
    })?;

    UserKV::delete_all_for_user(&mut conn, user_id).map_err(StoreError::DatabaseError)
}

pub fn list(
    pool: &diesel::r2d2::Pool<diesel::r2d2::ConnectionManager<PgConnection>>,
    user_id: Uuid,
    user_secret_key: &SecretKey,
) -> StoreResult<Vec<KVPair>> {
    let mut conn = pool.get().map_err(|_| {
        StoreError::DatabaseError(UserKVError::DatabaseError(diesel::result::Error::NotFound))
    })?;
    let user_kvs = UserKV::get_all_for_user(&mut conn, user_id)?;
    let mut pairs = Vec::new();
    for user_kv in user_kvs {
        let decrypted_key = decrypt_key_deterministic(user_secret_key, &user_kv.key_enc)
            .map_err(|_| StoreError::DecryptionError)?;
        let key = String::from_utf8(decrypted_key).map_err(|_| StoreError::DecryptionError)?;

        let decrypted_value = decrypt_with_key(user_secret_key, &user_kv.value_enc)
            .map_err(|_| StoreError::DecryptionError)?;
        let value = String::from_utf8(decrypted_value).map_err(|_| StoreError::DecryptionError)?;

        let created_at = user_kv.created_at.timestamp_millis();
        let updated_at = user_kv.updated_at.timestamp_millis();

        pairs.push(KVPair {
            key,
            value,
            created_at,
            updated_at,
        });
    }
    Ok(pairs)
}
