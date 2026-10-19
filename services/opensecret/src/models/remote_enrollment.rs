use diesel::prelude::*;
use uuid::Uuid;

use crate::{
    db::DBError,
    models::schema::{remote_enrollment_records, users},
};

#[derive(Clone, Queryable, Insertable, AsChangeset)]
#[diesel(table_name = remote_enrollment_records)]
pub struct Record {
    pub user_id: Uuid,
    pub project_id: i32,
    pub kind: String,
    pub record_id: String,
    pub nonce: Uuid,
    // Index/cleanup hint only. Authorization reads expiry from the ciphertext.
    pub expires_at: i64,
    pub ciphertext: Vec<u8>,
}

impl Record {
    pub fn relay_candidates(conn: &mut PgConnection, endpoint: &str) -> Result<Vec<Self>, DBError> {
        remote_enrollment_records::table
            .filter(remote_enrollment_records::kind.eq("device"))
            .filter(remote_enrollment_records::record_id.eq(endpoint))
            .limit(64)
            .load(conn)
            .map_err(Into::into)
    }
    pub fn get(
        conn: &mut PgConnection,
        user: Uuid,
        kind: &str,
        id: &str,
    ) -> Result<Option<Self>, DBError> {
        remote_enrollment_records::table
            .find((user, kind, id))
            .first(conn)
            .optional()
            .map_err(Into::into)
    }

    pub fn list(conn: &mut PgConnection, user: Uuid, kind: &str) -> Result<Vec<Self>, DBError> {
        remote_enrollment_records::table
            .filter(remote_enrollment_records::user_id.eq(user))
            .filter(remote_enrollment_records::kind.eq(kind))
            .limit(128)
            .load(conn)
            .map_err(Into::into)
    }

    pub fn put(&self, conn: &mut PgConnection, now: i64) -> Result<Self, DBError> {
        conn.transaction(|conn| {
            // Serialize only this account's bounded enrollment writes across
            // enclaves. No network awaits or process-local correctness cache.
            users::table
                .filter(users::uuid.eq(self.user_id))
                .select(users::id)
                .for_update()
                .first::<i32>(conn)?;
            diesel::delete(
                remote_enrollment_records::table
                    .filter(remote_enrollment_records::user_id.eq(self.user_id))
                    .filter(remote_enrollment_records::expires_at.le(now))
                    .filter(remote_enrollment_records::kind.ne("window")),
            )
            .execute(conn)?;
            let previous = Self::get(conn, self.user_id, &self.kind, &self.record_id)?;
            // An identical W registration never extends its original expiry.
            if self.kind == "window" {
                if let Some(ref record) = previous {
                    if record.nonce == self.nonce {
                        return Ok(record.clone());
                    }
                }
            }
            if previous.is_none() {
                let count = remote_enrollment_records::table
                    .filter(remote_enrollment_records::user_id.eq(self.user_id))
                    .filter(remote_enrollment_records::kind.eq(&self.kind))
                    .count()
                    .get_result::<i64>(conn)?;
                let limit = if self.kind == "admission" { 128 } else { 64 };
                if count >= limit {
                    return Err(DBError::RemoteEnrollmentCapacity);
                }
            }
            diesel::insert_into(remote_enrollment_records::table)
                .values(self)
                .on_conflict((
                    remote_enrollment_records::user_id,
                    remote_enrollment_records::kind,
                    remote_enrollment_records::record_id,
                ))
                .do_update()
                .set(self)
                .get_result(conn)
                .map_err(Into::into)
        })
    }
}
