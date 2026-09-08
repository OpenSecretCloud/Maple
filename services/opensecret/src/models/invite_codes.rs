use crate::models::schema::invite_codes;
use chrono::{DateTime, Duration, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum InviteCodeError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
    #[error("Invite code expired")]
    Expired,
    #[error("Invite code already used")]
    AlreadyUsed,
}

#[derive(
    Queryable, Identifiable, AsChangeset, Serialize, Deserialize, Clone, Debug, Selectable,
)]
#[diesel(table_name = invite_codes)]
pub struct InviteCode {
    pub id: i32,
    pub code: Uuid,
    pub org_id: i32,
    pub email: String,
    pub role: String,
    pub used: bool,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl InviteCode {
    pub fn get_by_id(
        conn: &mut PgConnection,
        lookup_id: i32,
    ) -> Result<Option<InviteCode>, InviteCodeError> {
        invite_codes::table
            .filter(invite_codes::id.eq(lookup_id))
            .select(InviteCode::as_select())
            .first(conn)
            .optional()
            .map_err(InviteCodeError::DatabaseError)
    }

    pub fn get_by_code(
        conn: &mut PgConnection,
        lookup_code: Uuid,
    ) -> Result<Option<InviteCode>, InviteCodeError> {
        invite_codes::table
            .filter(invite_codes::code.eq(lookup_code))
            .select(InviteCode::as_select())
            .first(conn)
            .optional()
            .map_err(InviteCodeError::DatabaseError)
    }

    pub fn get_by_email_and_org(
        conn: &mut PgConnection,
        lookup_email: &str,
        lookup_org_id: i32,
    ) -> Result<Option<InviteCode>, InviteCodeError> {
        invite_codes::table
            .filter(invite_codes::email.eq(lookup_email))
            .filter(invite_codes::org_id.eq(lookup_org_id))
            .filter(invite_codes::used.eq(false))
            .filter(invite_codes::expires_at.gt(diesel::dsl::now))
            .select(InviteCode::as_select())
            .first(conn)
            .optional()
            .map_err(InviteCodeError::DatabaseError)
    }

    pub fn get_all_for_org(
        conn: &mut PgConnection,
        lookup_org_id: i32,
    ) -> Result<Vec<InviteCode>, InviteCodeError> {
        invite_codes::table
            .filter(invite_codes::org_id.eq(lookup_org_id))
            .select(InviteCode::as_select())
            .load::<InviteCode>(conn)
            .map_err(InviteCodeError::DatabaseError)
    }

    pub fn mark_as_used(&self, conn: &mut PgConnection) -> Result<(), InviteCodeError> {
        if self.used {
            return Err(InviteCodeError::AlreadyUsed);
        }

        if self.expires_at < Utc::now() {
            return Err(InviteCodeError::Expired);
        }

        diesel::update(invite_codes::table)
            .filter(invite_codes::id.eq(self.id))
            .filter(invite_codes::used.eq(false))
            .set((
                invite_codes::used.eq(true),
                invite_codes::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)
            .map(|rows| {
                if rows == 0 {
                    Err(InviteCodeError::AlreadyUsed)
                } else {
                    Ok(())
                }
            })
            .map_err(InviteCodeError::DatabaseError)?
    }

    pub fn update(&self, conn: &mut PgConnection) -> Result<(), InviteCodeError> {
        diesel::update(invite_codes::table)
            .filter(invite_codes::id.eq(self.id))
            .set((
                invite_codes::email.eq(&self.email),
                invite_codes::used.eq(self.used),
                invite_codes::expires_at.eq(self.expires_at),
                invite_codes::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(InviteCodeError::DatabaseError)
    }

    pub fn delete(&self, conn: &mut PgConnection) -> Result<(), InviteCodeError> {
        diesel::delete(invite_codes::table)
            .filter(invite_codes::id.eq(self.id))
            .execute(conn)
            .map(|_| ())
            .map_err(InviteCodeError::DatabaseError)
    }
}

#[derive(Insertable)]
#[diesel(table_name = invite_codes)]
pub struct NewInviteCode {
    pub code: Uuid,
    pub org_id: i32,
    pub email: String,
    pub role: String,
    pub expires_at: DateTime<Utc>,
}

impl NewInviteCode {
    pub fn new(org_id: i32, email: String, role: String, expiry_hours: i64) -> Self {
        NewInviteCode {
            code: Uuid::new_v4(),
            org_id,
            email,
            role,
            expires_at: Utc::now() + Duration::hours(expiry_hours),
        }
    }

    pub fn insert(&self, conn: &mut PgConnection) -> Result<InviteCode, InviteCodeError> {
        diesel::insert_into(invite_codes::table)
            .values(self)
            .get_result::<InviteCode>(conn)
            .map_err(InviteCodeError::DatabaseError)
    }
}
