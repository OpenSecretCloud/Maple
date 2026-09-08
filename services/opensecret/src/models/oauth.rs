use crate::models::schema::{oauth_providers, user_oauth_connections, users};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum OAuthError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
    #[error("Multiple OAuth connections found for provider subject in project")]
    AmbiguousProviderSubjectForProject,
}

// OAuthProvider model
#[derive(Queryable, Identifiable, AsChangeset, Serialize, Deserialize, Clone, Debug)]
#[diesel(table_name = oauth_providers)]
pub struct OAuthProvider {
    pub id: i32,
    pub name: String,
    pub auth_url: String,
    pub token_url: String,
    pub user_info_url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OAuthProvider {
    pub fn get_by_id(
        conn: &mut PgConnection,
        lookup_id: i32,
    ) -> Result<Option<OAuthProvider>, OAuthError> {
        oauth_providers::table
            .filter(oauth_providers::id.eq(lookup_id))
            .first::<OAuthProvider>(conn)
            .optional()
            .map_err(OAuthError::DatabaseError)
    }

    pub fn get_by_name(
        conn: &mut PgConnection,
        lookup_name: &str,
    ) -> Result<Option<OAuthProvider>, OAuthError> {
        oauth_providers::table
            .filter(oauth_providers::name.eq(lookup_name))
            .first::<OAuthProvider>(conn)
            .optional()
            .map_err(OAuthError::DatabaseError)
    }

    pub fn get_all(conn: &mut PgConnection) -> Result<Vec<OAuthProvider>, OAuthError> {
        oauth_providers::table
            .load::<OAuthProvider>(conn)
            .map_err(OAuthError::DatabaseError)
    }

    pub fn update(&self, conn: &mut PgConnection) -> Result<(), OAuthError> {
        diesel::update(oauth_providers::table)
            .filter(oauth_providers::id.eq(self.id))
            .set(self)
            .execute(conn)
            .map(|_| ())
            .map_err(OAuthError::DatabaseError)
    }

    pub fn delete(&self, conn: &mut PgConnection) -> Result<(), OAuthError> {
        diesel::delete(oauth_providers::table)
            .filter(oauth_providers::id.eq(self.id))
            .execute(conn)
            .map(|_| ())
            .map_err(OAuthError::DatabaseError)
    }
}

#[derive(Insertable)]
#[diesel(table_name = oauth_providers)]
pub struct NewOAuthProvider {
    pub name: String,
    pub auth_url: String,
    pub token_url: String,
    pub user_info_url: String,
}

impl NewOAuthProvider {
    pub fn new(name: String, auth_url: String, token_url: String, user_info_url: String) -> Self {
        NewOAuthProvider {
            name,
            auth_url,
            token_url,
            user_info_url,
        }
    }

    pub fn insert(&self, conn: &mut PgConnection) -> Result<OAuthProvider, OAuthError> {
        diesel::insert_into(oauth_providers::table)
            .values(self)
            .get_result::<OAuthProvider>(conn)
            .map_err(OAuthError::DatabaseError)
    }
}

// UserOAuthConnection model
#[derive(
    Queryable, Identifiable, AsChangeset, Selectable, Serialize, Deserialize, Clone, Debug,
)]
#[diesel(check_for_backend(diesel::pg::Pg))]
#[diesel(table_name = user_oauth_connections)]
pub struct UserOAuthConnection {
    pub id: i32,
    pub user_id: Uuid,
    pub provider_id: i32,
    pub provider_user_id: String,
    pub access_token_enc: Vec<u8>,
    pub refresh_token_enc: Option<Vec<u8>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl UserOAuthConnection {
    pub fn get_by_id(
        conn: &mut PgConnection,
        lookup_id: i32,
    ) -> Result<Option<UserOAuthConnection>, OAuthError> {
        user_oauth_connections::table
            .filter(user_oauth_connections::id.eq(lookup_id))
            .first::<UserOAuthConnection>(conn)
            .optional()
            .map_err(OAuthError::DatabaseError)
    }

    pub fn get_by_user_and_provider(
        conn: &mut PgConnection,
        lookup_user_id: Uuid,
        lookup_provider_id: i32,
    ) -> Result<Option<UserOAuthConnection>, OAuthError> {
        user_oauth_connections::table
            .filter(user_oauth_connections::user_id.eq(lookup_user_id))
            .filter(user_oauth_connections::provider_id.eq(lookup_provider_id))
            .first::<UserOAuthConnection>(conn)
            .optional()
            .map_err(OAuthError::DatabaseError)
    }

    pub fn get_by_provider_subject_and_project(
        conn: &mut PgConnection,
        lookup_provider_id: i32,
        lookup_provider_user_id: &str,
        lookup_project_id: i32,
    ) -> Result<Option<UserOAuthConnection>, OAuthError> {
        let mut matches = user_oauth_connections::table
            .inner_join(users::table.on(users::uuid.eq(user_oauth_connections::user_id)))
            .filter(user_oauth_connections::provider_id.eq(lookup_provider_id))
            .filter(user_oauth_connections::provider_user_id.eq(lookup_provider_user_id))
            .filter(users::project_id.eq(lookup_project_id))
            .select(UserOAuthConnection::as_select())
            .limit(2)
            .load::<UserOAuthConnection>(conn)
            .map_err(OAuthError::DatabaseError)?;

        match matches.len() {
            0 => Ok(None),
            1 => Ok(matches.pop()),
            _ => Err(OAuthError::AmbiguousProviderSubjectForProject),
        }
    }

    pub fn get_all_for_user(
        conn: &mut PgConnection,
        lookup_user_id: Uuid,
    ) -> Result<Vec<UserOAuthConnection>, OAuthError> {
        user_oauth_connections::table
            .filter(user_oauth_connections::user_id.eq(lookup_user_id))
            .load::<UserOAuthConnection>(conn)
            .map_err(OAuthError::DatabaseError)
    }

    pub fn update(&self, conn: &mut PgConnection) -> Result<(), OAuthError> {
        diesel::update(user_oauth_connections::table)
            .filter(user_oauth_connections::id.eq(self.id))
            .set((
                user_oauth_connections::access_token_enc.eq(&self.access_token_enc),
                user_oauth_connections::refresh_token_enc.eq(&self.refresh_token_enc),
                user_oauth_connections::expires_at.eq(self.expires_at),
                user_oauth_connections::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(OAuthError::DatabaseError)
    }

    pub fn delete(&self, conn: &mut PgConnection) -> Result<(), OAuthError> {
        diesel::delete(user_oauth_connections::table)
            .filter(user_oauth_connections::id.eq(self.id))
            .execute(conn)
            .map(|_| ())
            .map_err(OAuthError::DatabaseError)
    }
}

#[derive(Insertable)]
#[diesel(table_name = user_oauth_connections)]
pub struct NewUserOAuthConnection {
    pub user_id: Uuid,
    pub provider_id: i32,
    pub provider_user_id: String,
    pub access_token_enc: Vec<u8>,
    pub refresh_token_enc: Option<Vec<u8>>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl NewUserOAuthConnection {
    pub fn new(
        user_id: Uuid,
        provider_id: i32,
        provider_user_id: String,
        access_token_enc: Vec<u8>,
        refresh_token_enc: Option<Vec<u8>>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        NewUserOAuthConnection {
            user_id,
            provider_id,
            provider_user_id,
            access_token_enc,
            refresh_token_enc,
            expires_at,
        }
    }

    pub fn insert(&self, conn: &mut PgConnection) -> Result<UserOAuthConnection, OAuthError> {
        diesel::insert_into(user_oauth_connections::table)
            .values(self)
            .get_result::<UserOAuthConnection>(conn)
            .map_err(OAuthError::DatabaseError)
    }
}
