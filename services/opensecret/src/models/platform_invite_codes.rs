use crate::{models::schema::platform_invite_codes, ApiError};
use diesel::prelude::*;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum PlatformInviteCodeError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
    #[error("Invite code not found: {0}")]
    InviteCodeNotFound(String),
}

impl From<PlatformInviteCodeError> for ApiError {
    fn from(err: PlatformInviteCodeError) -> Self {
        match err {
            PlatformInviteCodeError::DatabaseError(e) => {
                tracing::error!("Database error: {:?}", e);
                ApiError::InternalServerError
            }
            PlatformInviteCodeError::InviteCodeNotFound(_) => {
                tracing::error!("Invalid invite code");
                ApiError::BadRequest
            }
        }
    }
}

#[derive(Queryable, Identifiable, AsChangeset, Clone, Debug)]
#[diesel(table_name = platform_invite_codes)]
pub struct PlatformInviteCode {
    pub id: i32,
    pub code: Uuid,
}

impl PlatformInviteCode {
    fn get_by_code(
        conn: &mut PgConnection,
        invite_code: Uuid,
    ) -> Result<Option<PlatformInviteCode>, PlatformInviteCodeError> {
        platform_invite_codes::table
            .filter(platform_invite_codes::code.eq(invite_code))
            .first::<PlatformInviteCode>(conn)
            .optional()
            .map_err(PlatformInviteCodeError::DatabaseError)
    }

    pub fn validate_code(
        conn: &mut PgConnection,
        invite_code: Uuid,
    ) -> Result<PlatformInviteCode, PlatformInviteCodeError> {
        let code = Self::get_by_code(conn, invite_code)?
            .ok_or_else(|| PlatformInviteCodeError::InviteCodeNotFound(invite_code.to_string()))?;

        Ok(code)
    }
}

#[derive(Insertable)]
#[diesel(table_name = platform_invite_codes)]
pub struct NewPlatformInviteCode {
    pub code: Uuid,
}
