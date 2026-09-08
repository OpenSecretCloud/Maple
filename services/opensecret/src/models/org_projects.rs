use crate::models::schema::org_projects;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum OrgProjectError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
}

#[derive(Queryable, Identifiable, AsChangeset, Serialize, Deserialize, Clone, Debug)]
#[diesel(table_name = org_projects)]
pub struct OrgProject {
    pub id: i32,
    pub uuid: Uuid,
    pub client_id: Uuid,
    pub org_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OrgProject {
    pub fn get_by_id(
        conn: &mut PgConnection,
        lookup_id: i32,
    ) -> Result<Option<OrgProject>, OrgProjectError> {
        org_projects::table
            .filter(org_projects::id.eq(lookup_id))
            .first::<OrgProject>(conn)
            .optional()
            .map_err(OrgProjectError::DatabaseError)
    }

    pub fn get_by_uuid(
        conn: &mut PgConnection,
        lookup_uuid: Uuid,
    ) -> Result<Option<OrgProject>, OrgProjectError> {
        org_projects::table
            .filter(org_projects::uuid.eq(lookup_uuid))
            .first::<OrgProject>(conn)
            .optional()
            .map_err(OrgProjectError::DatabaseError)
    }

    pub fn get_by_client_id(
        conn: &mut PgConnection,
        lookup_client_id: Uuid,
    ) -> Result<Option<OrgProject>, OrgProjectError> {
        org_projects::table
            .filter(org_projects::client_id.eq(lookup_client_id))
            .first::<OrgProject>(conn)
            .optional()
            .map_err(OrgProjectError::DatabaseError)
    }

    pub fn get_by_name_and_org(
        conn: &mut PgConnection,
        lookup_name: &str,
        lookup_org_id: i32,
    ) -> Result<Option<OrgProject>, OrgProjectError> {
        org_projects::table
            .filter(org_projects::name.eq(lookup_name))
            .filter(org_projects::org_id.eq(lookup_org_id))
            .first::<OrgProject>(conn)
            .optional()
            .map_err(OrgProjectError::DatabaseError)
    }

    pub fn get_all_for_org(
        conn: &mut PgConnection,
        lookup_org_id: i32,
    ) -> Result<Vec<OrgProject>, OrgProjectError> {
        org_projects::table
            .filter(org_projects::org_id.eq(lookup_org_id))
            .load::<OrgProject>(conn)
            .map_err(OrgProjectError::DatabaseError)
    }

    pub fn get_active_for_org(
        conn: &mut PgConnection,
        lookup_org_id: i32,
    ) -> Result<Vec<OrgProject>, OrgProjectError> {
        org_projects::table
            .filter(org_projects::org_id.eq(lookup_org_id))
            .filter(org_projects::status.eq("active"))
            .load::<OrgProject>(conn)
            .map_err(OrgProjectError::DatabaseError)
    }

    pub fn update(&self, conn: &mut PgConnection) -> Result<(), OrgProjectError> {
        diesel::update(org_projects::table)
            .filter(org_projects::id.eq(self.id))
            .set((
                org_projects::name.eq(&self.name),
                org_projects::description.eq(&self.description),
                org_projects::status.eq(&self.status),
                org_projects::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(OrgProjectError::DatabaseError)
    }

    pub fn delete(&self, conn: &mut PgConnection) -> Result<(), OrgProjectError> {
        diesel::delete(org_projects::table)
            .filter(org_projects::id.eq(self.id))
            .execute(conn)
            .map(|_| ())
            .map_err(OrgProjectError::DatabaseError)
    }
}

#[derive(Insertable)]
#[diesel(table_name = org_projects)]
pub struct NewOrgProject {
    pub org_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
}

impl NewOrgProject {
    pub fn new(org_id: i32, name: String) -> Self {
        NewOrgProject {
            org_id,
            name,
            description: None,
            status: "active".to_string(),
        }
    }

    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_status(mut self, status: String) -> Self {
        self.status = status;
        self
    }

    pub fn insert(&self, conn: &mut PgConnection) -> Result<OrgProject, OrgProjectError> {
        diesel::insert_into(org_projects::table)
            .values(self)
            .get_result::<OrgProject>(conn)
            .map_err(OrgProjectError::DatabaseError)
    }
}
