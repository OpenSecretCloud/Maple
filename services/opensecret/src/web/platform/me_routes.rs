use crate::{
    email::send_platform_verification_email,
    models::platform_email_verification::NewPlatformEmailVerification,
    models::platform_users::PlatformUser,
    web::encryption_middleware::{decrypt_request, encrypt_response, Decrypted, TransportSession},
    ApiError, AppState,
};
use axum::{
    extract::State,
    middleware::from_fn_with_state,
    response::Response,
    routing::{get, post},
    Extension, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio::spawn;
use tracing::error;
use validator::Validate;

use super::common::{MeResponse, OrgResponse, PlatformUserResponse};

#[derive(Deserialize, Clone, Validate)]
pub struct PlatformChangePasswordRequest {
    #[validate(length(
        min = 8,
        max = 64,
        message = "Current password must be between 8 and 64 characters"
    ))]
    pub current_password: String,

    #[validate(length(
        min = 8,
        max = 64,
        message = "New password must be between 8 and 64 characters"
    ))]
    pub new_password: String,
}

pub fn router(app_state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/platform/me",
            get(get_platform_user)
                .layer(from_fn_with_state(app_state.clone(), decrypt_request::<()>)),
        )
        .route(
            "/platform/request_verification",
            post(request_platform_verification)
                .layer(from_fn_with_state(app_state.clone(), decrypt_request::<()>)),
        )
        .route(
            "/platform/change-password",
            post(platform_change_password).layer(from_fn_with_state(
                app_state.clone(),
                decrypt_request::<PlatformChangePasswordRequest>,
            )),
        )
        .with_state(app_state)
}

async fn get_platform_user(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    // Check if email is verified
    let email_verified = match data
        .db
        .get_platform_email_verification_by_platform_user_id(platform_user.uuid)
    {
        Ok(verification) => verification.is_verified,
        Err(crate::db::DBError::PlatformEmailVerificationNotFound) => false,
        Err(e) => {
            error!("Error fetching platform email verification: {:?}", e);
            return Err(ApiError::InternalServerError);
        }
    };

    // Get user's organization memberships
    let memberships = match data
        .db
        .get_all_org_memberships_for_platform_user(platform_user.uuid)
    {
        Ok(memberships) => memberships,
        Err(e) => {
            error!("Error fetching organization memberships: {:?}", e);
            return Err(ApiError::InternalServerError);
        }
    };

    // Create the list of organizations with roles
    let organizations = memberships
        .into_iter()
        .filter_map(|membership| {
            let org_id = membership.org_id;

            // Fetch the organization details
            match data.db.get_org_by_id(org_id) {
                Ok(org) => Some(OrgResponse {
                    id: org.uuid,
                    name: org.name,
                }),
                Err(e) => {
                    error!("Error fetching organization {}: {:?}", org_id, e);
                    None // Skip this organization but continue processing others
                }
            }
        })
        .collect::<Vec<_>>();

    // Create the platform user response object
    let user = PlatformUserResponse {
        id: platform_user.uuid,
        email: platform_user.email,
        name: platform_user.name,
        email_verified,
        created_at: platform_user.created_at,
        updated_at: platform_user.updated_at,
    };

    let response = MeResponse {
        user,
        organizations,
    };
    encrypt_response(&data, &session_id, &response).await
}

async fn request_platform_verification(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    // Check if the user is already verified
    match data
        .db
        .get_platform_email_verification_by_platform_user_id(platform_user.uuid)
    {
        Ok(verification) => {
            if verification.is_verified {
                let response = json!({ "error": "User is already verified" });
                return encrypt_response(&data, &session_id, &response).await;
            }
            // Delete the old verification
            if let Err(e) = data.db.delete_platform_email_verification(&verification) {
                error!("Error deleting old platform verification: {:?}", e);
                return Err(ApiError::InternalServerError);
            }
        }
        Err(crate::db::DBError::PlatformEmailVerificationNotFound) => {
            // This is fine, we'll create a new verification
        }
        Err(e) => {
            error!("Error checking platform email verification: {:?}", e);
            return Err(ApiError::InternalServerError);
        }
    }

    // Create a new verification entry
    let new_verification = match NewPlatformEmailVerification::new(platform_user.uuid, 24, false) {
        // 24 hours expiration
        Ok(v) => v,
        Err(e) => {
            error!("Error creating platform email verification: {:?}", e);
            return Err(ApiError::InternalServerError);
        }
    };
    let verification = match data.db.create_platform_email_verification(new_verification) {
        Ok(v) => v,
        Err(e) => {
            error!("Error creating platform email verification: {:?}", e);
            return Err(ApiError::InternalServerError);
        }
    };

    // Send the new verification email
    let email = platform_user.email.clone();
    let verification_code = verification.verification_code;
    let app_state = data.clone();

    spawn(async move {
        if let Err(e) = send_platform_verification_email(
            &app_state,
            app_state.resend_api_key.clone(),
            email,
            verification_code,
        )
        .await
        {
            error!("Could not send platform verification email: {}", e);
        }
    });

    let response = json!({ "message": "New verification code sent successfully" });
    encrypt_response(&data, &session_id, &response).await
}

pub async fn platform_change_password(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Decrypted(change_request): Decrypted<PlatformChangePasswordRequest>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    // Validate request
    if change_request.validate().is_err() {
        error!("Platform password change request validation failed");
        return Err(ApiError::BadRequest);
    }

    // Check if user is an OAuth-only user
    if platform_user.password_enc.is_none() {
        error!("OAuth-only platform user attempted to change password");
        return Err(ApiError::InvalidUsernameOrPassword);
    }

    // Verify current password
    match data
        .authenticate_platform_user(&platform_user.email, change_request.current_password)
        .await
    {
        Ok(Some(authenticated_user)) if authenticated_user.uuid == platform_user.uuid => {
            // Current password is correct, proceed with password change
            match data
                .update_platform_user_password(&platform_user, change_request.new_password)
                .await
            {
                Ok(()) => {
                    let response = json!({ "message": "Password changed successfully" });
                    encrypt_response(&data, &session_id, &response).await
                }
                Err(e) => {
                    error!("Error changing platform user password: {:?}", e);
                    Err(ApiError::InternalServerError)
                }
            }
        }
        _ => {
            // Current password is incorrect
            error!("Invalid current password in platform change password request");
            Err(ApiError::InvalidUsernameOrPassword)
        }
    }
}
