use crate::models::schema::org_project_secrets;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Deserialize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum OrgProjectSecretError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
}

#[derive(Queryable, Identifiable, AsChangeset, Deserialize, Clone)]
#[diesel(table_name = org_project_secrets)]
pub struct OrgProjectSecret {
    pub id: i32,
    pub project_id: i32,
    pub key_name: String,
    pub secret_enc: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl std::fmt::Debug for OrgProjectSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrgProjectSecret")
            .field("id", &self.id)
            .field("project_id", &self.project_id)
            .field("key_name", &self.key_name)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("secret_enc", &"[redacted]")
            .finish()
    }
}

impl OrgProjectSecret {
    pub fn get_by_id(
        conn: &mut PgConnection,
        lookup_id: i32,
    ) -> Result<Option<OrgProjectSecret>, OrgProjectSecretError> {
        org_project_secrets::table
            .filter(org_project_secrets::id.eq(lookup_id))
            .first::<OrgProjectSecret>(conn)
            .optional()
            .map_err(OrgProjectSecretError::DatabaseError)
    }

    pub fn get_by_key_name_and_project(
        conn: &mut PgConnection,
        lookup_key_name: &str,
        lookup_project_id: i32,
    ) -> Result<Option<OrgProjectSecret>, OrgProjectSecretError> {
        org_project_secrets::table
            .filter(org_project_secrets::key_name.eq(lookup_key_name))
            .filter(org_project_secrets::project_id.eq(lookup_project_id))
            .first::<OrgProjectSecret>(conn)
            .optional()
            .map_err(OrgProjectSecretError::DatabaseError)
    }

    pub fn get_all_for_project(
        conn: &mut PgConnection,
        lookup_project_id: i32,
    ) -> Result<Vec<OrgProjectSecret>, OrgProjectSecretError> {
        org_project_secrets::table
            .filter(org_project_secrets::project_id.eq(lookup_project_id))
            .load::<OrgProjectSecret>(conn)
            .map_err(OrgProjectSecretError::DatabaseError)
    }

    pub fn update(&self, conn: &mut PgConnection) -> Result<(), OrgProjectSecretError> {
        diesel::update(org_project_secrets::table)
            .filter(org_project_secrets::id.eq(self.id))
            .set((
                org_project_secrets::key_name.eq(&self.key_name),
                org_project_secrets::secret_enc.eq(&self.secret_enc),
                org_project_secrets::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(OrgProjectSecretError::DatabaseError)
    }

    pub fn delete(&self, conn: &mut PgConnection) -> Result<(), OrgProjectSecretError> {
        diesel::delete(org_project_secrets::table)
            .filter(org_project_secrets::id.eq(self.id))
            .execute(conn)
            .map(|_| ())
            .map_err(OrgProjectSecretError::DatabaseError)
    }
}

#[derive(Insertable)]
#[diesel(table_name = org_project_secrets)]
pub struct NewOrgProjectSecret {
    pub project_id: i32,
    pub key_name: String,
    pub secret_enc: Vec<u8>,
}

impl std::fmt::Debug for NewOrgProjectSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewOrgProjectSecret")
            .field("project_id", &self.project_id)
            .field("key_name", &self.key_name)
            .field("secret_enc", &"[redacted]")
            .finish()
    }
}

impl NewOrgProjectSecret {
    pub fn new(project_id: i32, key_name: String, secret_enc: Vec<u8>) -> Self {
        NewOrgProjectSecret {
            project_id,
            key_name,
            secret_enc,
        }
    }

    pub fn insert(
        &self,
        conn: &mut PgConnection,
    ) -> Result<OrgProjectSecret, OrgProjectSecretError> {
        diesel::insert_into(org_project_secrets::table)
            .values(self)
            .on_conflict((
                org_project_secrets::project_id,
                org_project_secrets::key_name,
            ))
            .do_update()
            .set(org_project_secrets::secret_enc.eq(&self.secret_enc))
            .get_result::<OrgProjectSecret>(conn)
            .map_err(OrgProjectSecretError::DatabaseError)
    }
}
