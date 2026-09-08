use crate::models::schema::project_settings;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProjectSettingError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
    #[error("Invalid settings format: {0}")]
    InvalidSettings(String),
    #[error("Settings serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingCategory {
    Email,
    OAuth,
}

impl SettingCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            SettingCategory::Email => "email",
            SettingCategory::OAuth => "oauth",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailSettings {
    pub provider: String,
    pub send_from: String,
    pub email_verification_url: String,
}

impl Default for EmailSettings {
    fn default() -> Self {
        Self {
            provider: "resend".to_string(),
            send_from: String::new(),
            email_verification_url: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OAuthProviderSettings {
    pub client_id: String,
    pub redirect_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppleOAuthSettings {
    pub client_id: String,
    pub redirect_url: String,
    pub team_id: Option<String>, // Apple Developer Team ID (10 chars)
    pub key_id: Option<String>,  // Apple Private Key ID (10 chars)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OAuthSettings {
    pub google_oauth_enabled: bool,
    pub github_oauth_enabled: bool,
    #[serde(default)]
    pub apple_oauth_enabled: bool,
    pub google_oauth_settings: Option<OAuthProviderSettings>,
    pub github_oauth_settings: Option<OAuthProviderSettings>,
    #[serde(default)]
    pub apple_oauth_settings: Option<AppleOAuthSettings>,
}

#[derive(Queryable, Identifiable)]
#[diesel(table_name = project_settings)]
pub struct ProjectSetting {
    pub id: i32,
    pub project_id: i32,
    pub category: String,
    #[diesel(sql_type = Jsonb)]
    pub settings: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProjectSetting {
    pub fn get_email_settings(&self) -> Result<EmailSettings, ProjectSettingError> {
        serde_json::from_value(self.settings.clone())
            .map_err(ProjectSettingError::SerializationError)
    }

    pub fn get_oauth_settings(&self) -> Result<OAuthSettings, ProjectSettingError> {
        serde_json::from_value(self.settings.clone())
            .map_err(ProjectSettingError::SerializationError)
    }

    pub fn get_by_project_and_category(
        conn: &mut PgConnection,
        lookup_project_id: i32,
        lookup_category: SettingCategory,
    ) -> Result<Option<ProjectSetting>, ProjectSettingError> {
        project_settings::table
            .filter(project_settings::project_id.eq(lookup_project_id))
            .filter(project_settings::category.eq(lookup_category.as_str()))
            .first(conn)
            .optional()
            .map_err(ProjectSettingError::DatabaseError)
    }

    pub fn update(&self, conn: &mut PgConnection) -> Result<(), ProjectSettingError> {
        use crate::models::schema::project_settings::dsl::*;

        diesel::update(project_settings.find(self.id))
            .set((
                category.eq(&self.category),
                settings.eq(&self.settings),
                updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(ProjectSettingError::DatabaseError)
    }
}

#[derive(Insertable)]
#[diesel(table_name = project_settings)]
pub struct NewProjectSetting {
    pub project_id: i32,
    pub category: String,
    #[diesel(sql_type = Jsonb)]
    pub settings: Value,
}

impl NewProjectSetting {
    pub fn new_email_settings(
        project_id: i32,
        email_settings: EmailSettings,
    ) -> Result<Self, ProjectSettingError> {
        Ok(Self {
            project_id,
            category: SettingCategory::Email.as_str().to_string(),
            settings: serde_json::to_value(email_settings)
                .map_err(ProjectSettingError::SerializationError)?,
        })
    }

    pub fn new_oauth_settings(
        project_id: i32,
        oauth_settings: OAuthSettings,
    ) -> Result<Self, ProjectSettingError> {
        Ok(Self {
            project_id,
            category: SettingCategory::OAuth.as_str().to_string(),
            settings: serde_json::to_value(oauth_settings)
                .map_err(ProjectSettingError::SerializationError)?,
        })
    }

    pub fn insert(&self, conn: &mut PgConnection) -> Result<ProjectSetting, ProjectSettingError> {
        diesel::insert_into(project_settings::table)
            .values(self)
            .get_result(conn)
            .map_err(ProjectSettingError::DatabaseError)
    }
}
