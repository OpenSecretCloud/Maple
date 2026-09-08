use crate::{
    models::{
        org_memberships::OrgRole,
        project_settings::{AppleOAuthSettings, OAuthProviderSettings},
    },
    web::platform::validation::{
        validate_alphanumeric_only, validate_alphanumeric_with_symbols, validate_secret_size,
    },
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

pub const PROJECT_RESEND_API_KEY: &str = "RESEND_API_KEY";
pub const PROJECT_GOOGLE_OAUTH_SECRET: &str = "GOOGLE_OAUTH_SECRET";
pub const PROJECT_GITHUB_OAUTH_SECRET: &str = "GITHUB_OAUTH_SECRET";
pub const PROJECT_APPLE_OAUTH_SECRET: &str = "APPLE_OAUTH_SECRET";
pub const THIRD_PARTY_JWT_SECRET: &str = "THIRD_PARTY_JWT_SECRET";

// Request Types
#[derive(Deserialize, Clone, Validate)]
pub struct CreateOrgRequest {
    #[validate(length(min = 1, max = 50))]
    #[validate(custom(function = "validate_alphanumeric_with_symbols"))]
    pub name: String,
}

#[derive(Deserialize, Clone, Validate)]
pub struct CreateProjectRequest {
    #[validate(length(min = 1, max = 50))]
    #[validate(custom(function = "validate_alphanumeric_with_symbols"))]
    pub name: String,
    #[validate(length(max = 255))]
    pub description: Option<String>,
}

#[derive(Deserialize, Clone, Validate)]
pub struct UpdateProjectRequest {
    #[validate(length(min = 1, max = 50))]
    #[validate(custom(function = "validate_alphanumeric_with_symbols"))]
    pub name: Option<String>,
    #[validate(length(max = 255))]
    pub description: Option<String>,
    #[validate(custom(function = "validate_project_status"))]
    pub status: Option<String>,
}

#[derive(Deserialize, Clone, Validate)]
pub struct CreateInviteRequest {
    #[validate(email(message = "Invalid email format"))]
    #[validate(length(max = 255, message = "Email must not exceed 255 characters"))]
    pub email: String,
    #[serde(default = "default_invite_role")]
    pub role: OrgRole,
}

#[derive(Deserialize, Clone, Validate)]
pub struct UpdateMembershipRequest {
    pub role: OrgRole,
}

#[derive(Deserialize, Clone, Validate)]
pub struct CreateSecretRequest {
    #[validate(length(min = 1, max = 50))]
    #[validate(custom(function = "validate_alphanumeric_only"))]
    pub key_name: String,
    #[validate(custom(function = "validate_secret_size"))]
    pub secret: String, // Base64 encoded secret value
}

#[derive(Deserialize, Clone, Validate)]
pub struct UpdateEmailSettingsRequest {
    #[validate(length(min = 1, max = 255))]
    #[validate(custom(function = "validate_email_provider"))]
    pub provider: String,
    #[validate(email)]
    pub send_from: String,
    #[validate(length(
        min = 1,
        max = 255,
        message = "URL must not be empty and must not exceed 255 characters"
    ))]
    #[validate(url(message = "Invalid URL format"))]
    pub email_verification_url: String,
}

#[derive(Deserialize, Clone, Validate)]
pub struct UpdateOAuthSettingsRequest {
    pub google_oauth_enabled: bool,
    pub github_oauth_enabled: bool,
    pub apple_oauth_enabled: bool,
    #[validate(custom(function = "validate_oauth_provider_settings"))]
    pub google_oauth_settings: Option<OAuthProviderSettings>,
    #[validate(custom(function = "validate_oauth_provider_settings"))]
    pub github_oauth_settings: Option<OAuthProviderSettings>,
    #[validate(custom(function = "validate_apple_oauth_settings"))]
    pub apple_oauth_settings: Option<AppleOAuthSettings>,
}

// Response Types
#[derive(Serialize)]
pub struct OrgResponse {
    pub id: Uuid,
    pub name: String,
}

#[derive(Serialize)]
pub struct ProjectResponse {
    pub id: Uuid,
    pub client_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct MembershipResponse {
    pub user_id: Uuid,
    pub role: String,
    pub name: Option<String>,
}

#[derive(Serialize)]
pub struct InviteResponse {
    pub code: Uuid,
    pub email: String,
    pub role: String,
    pub used: bool,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct DetailedInviteResponse {
    pub code: Uuid,
    pub email: String,
    pub role: String,
    pub used: bool,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub organization_name: String,
}

#[derive(Serialize)]
pub struct SecretResponse {
    pub key_name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PlatformUserResponse {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub email_verified: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct MeResponse {
    pub user: PlatformUserResponse,
    pub organizations: Vec<OrgResponse>,
}

// Validation Functions
pub fn validate_project_status(status: &str) -> Result<(), validator::ValidationError> {
    match status {
        "active" | "inactive" | "suspended" => Ok(()),
        _ => Err(validator::ValidationError::new("project_status")),
    }
}

pub fn default_invite_role() -> OrgRole {
    OrgRole::Admin
}

pub fn validate_email_provider(provider: &str) -> Result<(), validator::ValidationError> {
    if provider != "resend" {
        let mut error = validator::ValidationError::new("invalid_email_provider");
        error.message = Some("Only 'resend' is supported as an email provider".into());
        return Err(error);
    }
    Ok(())
}

pub fn validate_oauth_provider_settings(
    settings: &OAuthProviderSettings,
) -> Result<(), validator::ValidationError> {
    // Validate client_id
    if settings.client_id.is_empty() || settings.client_id.len() > 255 {
        let mut error = validator::ValidationError::new("oauth_client_id");
        error.message = Some(format!("Client ID must not be empty and must not exceed 255 characters (current length: {})", settings.client_id.len()).into());
        return Err(error);
    }
    // Validate redirect_url
    if settings.redirect_url.is_empty() || settings.redirect_url.len() > 255 {
        let mut error = validator::ValidationError::new("oauth_redirect_url");
        error.message = Some(format!("Redirect URL must not be empty and must not exceed 255 characters (current length: {})", settings.redirect_url.len()).into());
        return Err(error);
    }
    // Basic URL validation
    if let Err(parse_err) = url::Url::parse(&settings.redirect_url) {
        let mut error = validator::ValidationError::new("oauth_redirect_url_invalid");
        error.message = Some(format!("Invalid redirect URL: {}", parse_err).into());
        return Err(error);
    }

    Ok(())
}

pub fn validate_apple_oauth_settings(
    settings: &AppleOAuthSettings,
) -> Result<(), validator::ValidationError> {
    // Validate client_id
    if settings.client_id.is_empty() || settings.client_id.len() > 255 {
        let mut error = validator::ValidationError::new("oauth_client_id");
        error.message = Some(format!("Client ID must not be empty and must not exceed 255 characters (current length: {})", settings.client_id.len()).into());
        return Err(error);
    }
    // Validate redirect_url
    if settings.redirect_url.is_empty() || settings.redirect_url.len() > 255 {
        let mut error = validator::ValidationError::new("oauth_redirect_url");
        error.message = Some(format!("Redirect URL must not be empty and must not exceed 255 characters (current length: {})", settings.redirect_url.len()).into());
        return Err(error);
    }
    // Basic URL validation
    if let Err(parse_err) = url::Url::parse(&settings.redirect_url) {
        let mut error = validator::ValidationError::new("oauth_redirect_url_invalid");
        error.message = Some(format!("Invalid redirect URL: {}", parse_err).into());
        return Err(error);
    }

    // Validate team_id if provided
    if let Some(ref team_id) = settings.team_id {
        if team_id.len() != 10 {
            let mut error = validator::ValidationError::new("oauth_team_id_length");
            error.message = Some(
                format!(
                    "Apple Team ID should be exactly 10 characters long (current length: {})",
                    team_id.len()
                )
                .into(),
            );
            return Err(error);
        }
    }

    // Validate key_id if provided
    if let Some(ref key_id) = settings.key_id {
        if key_id.len() != 10 {
            let mut error = validator::ValidationError::new("oauth_key_id_length");
            error.message = Some(
                format!(
                    "Apple Key ID should be exactly 10 characters long (current length: {})",
                    key_id.len()
                )
                .into(),
            );
            return Err(error);
        }
    }

    Ok(())
}
