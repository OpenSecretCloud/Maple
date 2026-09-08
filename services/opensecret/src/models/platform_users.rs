use crate::{models::schema::platform_users, ApiError};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Deserialize;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum PlatformUserError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
    #[error("Invalid email format: {0}")]
    InvalidEmail(String),
    #[error("Email already exists: {0}")]
    DuplicateEmail(String),
    #[error("Invalid password: {0}")]
    InvalidPassword(String),
}

impl From<PlatformUserError> for ApiError {
    fn from(err: PlatformUserError) -> Self {
        match err {
            PlatformUserError::DatabaseError(e) => {
                tracing::error!("Database error: {:?}", e);
                ApiError::InternalServerError
            }
            PlatformUserError::InvalidEmail(msg) => {
                tracing::error!("Invalid email error: {}", msg);
                ApiError::BadRequest
            }
            PlatformUserError::DuplicateEmail(email) => {
                tracing::error!("Duplicate email error: {}", email);
                ApiError::EmailAlreadyExists
            }
            PlatformUserError::InvalidPassword(msg) => {
                tracing::error!("Invalid password error: {}", msg);
                ApiError::BadRequest
            }
        }
    }
}

#[derive(Queryable, Identifiable, AsChangeset, Deserialize, Clone)]
#[diesel(table_name = platform_users)]
pub struct PlatformUser {
    pub id: i32,
    pub uuid: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub password_enc: Option<Vec<u8>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// Here we've implemented `Debug` manually to avoid accidentally logging the
// password hash.
impl std::fmt::Debug for PlatformUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformUser")
            .field("id", &self.id)
            .field("uuid", &self.uuid)
            .field("email", &self.email)
            .field("name", &self.name)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("password_enc", &"[redacted]")
            .finish()
    }
}

impl PlatformUser {
    pub fn get_by_id(
        conn: &mut PgConnection,
        lookup_id: i32,
    ) -> Result<Option<PlatformUser>, PlatformUserError> {
        platform_users::table
            .filter(platform_users::id.eq(lookup_id))
            .first::<PlatformUser>(conn)
            .optional()
            .map_err(PlatformUserError::DatabaseError)
    }

    pub fn get_by_uuid(
        conn: &mut PgConnection,
        lookup_uuid: Uuid,
    ) -> Result<Option<PlatformUser>, PlatformUserError> {
        platform_users::table
            .filter(platform_users::uuid.eq(lookup_uuid))
            .first::<PlatformUser>(conn)
            .optional()
            .map_err(PlatformUserError::DatabaseError)
    }

    pub fn get_by_email(
        conn: &mut PgConnection,
        lookup_email: &str,
    ) -> Result<Option<PlatformUser>, PlatformUserError> {
        platform_users::table
            .filter(platform_users::email.eq(lookup_email))
            .first::<PlatformUser>(conn)
            .optional()
            .map_err(PlatformUserError::DatabaseError)
    }

    pub fn update_password(
        &self,
        conn: &mut PgConnection,
        new_password_enc: Vec<u8>,
    ) -> Result<(), PlatformUserError> {
        diesel::update(platform_users::table)
            .filter(platform_users::id.eq(self.id))
            .set(platform_users::password_enc.eq(new_password_enc))
            .execute(conn)
            .map(|_| ())
            .map_err(PlatformUserError::DatabaseError)
    }

    pub fn update(&self, conn: &mut PgConnection) -> Result<(), PlatformUserError> {
        diesel::update(platform_users::table)
            .filter(platform_users::id.eq(self.id))
            .set((
                platform_users::email.eq(&self.email),
                platform_users::name.eq(&self.name),
                platform_users::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(PlatformUserError::DatabaseError)
    }
}

#[derive(Insertable)]
#[diesel(table_name = platform_users)]
pub struct NewPlatformUser {
    pub email: String,
    pub name: Option<String>,
    pub password_enc: Option<Vec<u8>>,
}

impl NewPlatformUser {
    pub fn new(email: String, password_enc: Option<Vec<u8>>) -> Self {
        NewPlatformUser {
            email,
            name: None,
            password_enc,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn insert(&self, conn: &mut PgConnection) -> Result<PlatformUser, PlatformUserError> {
        diesel::insert_into(platform_users::table)
            .values(self)
            .get_result::<PlatformUser>(conn)
            .map_err(PlatformUserError::DatabaseError)
    }
}

impl std::fmt::Debug for NewPlatformUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewPlatformUser")
            .field("name", &self.name)
            .field("email", &self.email)
            .field("password_enc", &"[redacted]")
            .finish()
    }
}
