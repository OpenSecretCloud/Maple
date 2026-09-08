use crate::models::schema::token_usage;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum TokenUsageError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
}

#[derive(Queryable, Identifiable, Serialize, Deserialize, Clone, Debug)]
#[diesel(table_name = token_usage)]
pub struct TokenUsage {
    pub id: i64,
    pub user_id: Uuid,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub estimated_cost: BigDecimal,
    pub created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = token_usage)]
pub struct NewTokenUsage {
    pub user_id: Uuid,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub estimated_cost: BigDecimal,
}

impl NewTokenUsage {
    pub fn new(
        user_id: Uuid,
        input_tokens: i32,
        output_tokens: i32,
        estimated_cost: BigDecimal,
    ) -> Self {
        NewTokenUsage {
            user_id,
            input_tokens,
            output_tokens,
            estimated_cost,
        }
    }

    pub fn insert(&self, conn: &mut PgConnection) -> Result<TokenUsage, TokenUsageError> {
        diesel::insert_into(token_usage::table)
            .values(self)
            .get_result::<TokenUsage>(conn)
            .map_err(TokenUsageError::DatabaseError)
    }
}
