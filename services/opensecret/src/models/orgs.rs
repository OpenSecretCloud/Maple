use crate::models::schema::orgs;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum OrgError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
}

#[derive(Queryable, Identifiable, AsChangeset, Serialize, Deserialize, Clone, Debug)]
#[diesel(table_name = orgs)]
pub struct Org {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Org {
    pub fn get_by_id(conn: &mut PgConnection, lookup_id: i32) -> Result<Option<Org>, OrgError> {
        orgs::table
            .filter(orgs::id.eq(lookup_id))
            .first::<Org>(conn)
            .optional()
            .map_err(OrgError::DatabaseError)
    }

    pub fn get_by_uuid(
        conn: &mut PgConnection,
        lookup_uuid: Uuid,
    ) -> Result<Option<Org>, OrgError> {
        orgs::table
            .filter(orgs::uuid.eq(lookup_uuid))
            .first::<Org>(conn)
            .optional()
            .map_err(OrgError::DatabaseError)
    }

    pub fn get_by_name(
        conn: &mut PgConnection,
        lookup_name: &str,
    ) -> Result<Option<Org>, OrgError> {
        orgs::table
            .filter(orgs::name.eq(lookup_name))
            .first::<Org>(conn)
            .optional()
            .map_err(OrgError::DatabaseError)
    }

    pub fn get_all(conn: &mut PgConnection) -> Result<Vec<Org>, OrgError> {
        orgs::table
            .load::<Org>(conn)
            .map_err(OrgError::DatabaseError)
    }

    pub fn update(&self, conn: &mut PgConnection) -> Result<(), OrgError> {
        diesel::update(orgs::table)
            .filter(orgs::id.eq(self.id))
            .set((
                orgs::name.eq(&self.name),
                orgs::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(OrgError::DatabaseError)
    }

    pub fn delete(&self, conn: &mut PgConnection) -> Result<(), OrgError> {
        diesel::delete(orgs::table)
            .filter(orgs::id.eq(self.id))
            .execute(conn)
            .map(|_| ())
            .map_err(OrgError::DatabaseError)
    }
}

#[derive(Insertable)]
#[diesel(table_name = orgs)]
pub struct NewOrg {
    pub name: String,
}

impl NewOrg {
    pub fn new(name: String) -> Self {
        NewOrg { name }
    }

    pub fn insert(&self, conn: &mut PgConnection) -> Result<Org, OrgError> {
        diesel::insert_into(orgs::table)
            .values(self)
            .get_result::<Org>(conn)
            .map_err(OrgError::DatabaseError)
    }
}
