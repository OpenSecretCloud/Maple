use crate::{
    email::send_platform_invite_email,
    models::{
        invite_codes::{InviteCodeError, NewInviteCode},
        org_memberships::{NewOrgMembership, OrgRole},
        platform_users::PlatformUser,
    },
    web::encryption_middleware::{decrypt_request, encrypt_response, Decrypted, TransportSession},
    ApiError, AppState, DBError,
};
use axum::{
    extract::{Path, State},
    middleware::from_fn_with_state,
    response::Response,
    routing::{delete, get, post},
    Extension, Router,
};
use std::sync::Arc;
use tokio::spawn;
use tracing::{debug, error};
use uuid::Uuid;

use super::common::{CreateInviteRequest, DetailedInviteResponse, InviteResponse};

pub fn router(app_state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/platform/orgs/:org_id/invites",
            post(create_invite).layer(from_fn_with_state(
                app_state.clone(),
                decrypt_request::<CreateInviteRequest>,
            )),
        )
        .route(
            "/platform/orgs/:org_id/invites",
            get(list_invites).layer(from_fn_with_state(app_state.clone(), decrypt_request::<()>)),
        )
        .route(
            "/platform/orgs/:org_id/invites/:invite_code",
            get(get_invite).layer(from_fn_with_state(app_state.clone(), decrypt_request::<()>)),
        )
        .route(
            "/platform/orgs/:org_id/invites/:invite_code",
            delete(delete_invite)
                .layer(from_fn_with_state(app_state.clone(), decrypt_request::<()>)),
        )
        .route(
            "/platform/accept_invite/:code",
            post(accept_invite).layer(from_fn_with_state(app_state.clone(), decrypt_request::<()>)),
        )
        .with_state(app_state)
}

async fn create_invite(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Path(org_id): Path<Uuid>,
    Decrypted(create_request): Decrypted<CreateInviteRequest>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    debug!("Creating invite");

    // Get the org by UUID
    let org = data
        .db
        .get_org_by_uuid(org_id)
        .map_err(|_| ApiError::BadRequest)?;

    // Verify user has admin or owner role
    let membership = data
        .db
        .get_org_membership_by_platform_user_and_org(platform_user.uuid, org.id)
        .map_err(|_| ApiError::Unauthorized)?;

    let role: OrgRole = membership.role.into();
    if !matches!(role, OrgRole::Owner | OrgRole::Admin) {
        return Err(ApiError::Unauthorized);
    }

    // Create the invite code with the specified role (or default admin)
    // Validate that the role is a valid OrgRole
    let role: OrgRole = create_request
        .role
        .as_str()
        .try_into()
        .map_err(|_| ApiError::BadRequest)?;
    let new_invite = NewInviteCode::new(
        org.id,
        create_request.email.clone(),
        role.as_str().to_string(),
        24, // 24 hour expiry
    );

    let invite = data.db.create_invite_code(new_invite).map_err(|e| {
        error!("Failed to create invite code: {:?}", e);
        ApiError::InternalServerError
    })?;

    // Send invite email in background
    let email = create_request.email;
    let invite_code = invite.code;
    let app_mode = data.app_mode.clone();
    let resend_api_key = data.resend_api_key.clone();
    let org_uuid = org.uuid;
    spawn(async move {
        if let Err(e) = send_platform_invite_email(
            app_mode,
            resend_api_key,
            email,
            org.name,
            invite_code,
            org_uuid,
        )
        .await
        {
            error!("Failed to send invite email: {:?}", e);
        }
    });

    let response = InviteResponse {
        code: invite.code,
        email: invite.email,
        role: invite.role,
        used: invite.used,
        expires_at: invite.expires_at,
        created_at: invite.created_at,
        updated_at: invite.updated_at,
    };

    encrypt_response(&data, &session_id, &response).await
}

async fn list_invites(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Path(org_id): Path<Uuid>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    debug!("Listing organization invites");

    // Get the org by UUID
    let org = data
        .db
        .get_org_by_uuid(org_id)
        .map_err(|_| ApiError::NotFound)?;

    // Verify user has admin or owner role
    let membership = data
        .db
        .get_org_membership_by_platform_user_and_org(platform_user.uuid, org.id)
        .map_err(|_| ApiError::Unauthorized)?;

    let role: OrgRole = membership.role.into();
    if !matches!(role, OrgRole::Owner | OrgRole::Admin) {
        return Err(ApiError::Unauthorized);
    }

    // Get all invite codes for the org
    let all_invites = data.db.get_all_invite_codes_for_org(org.id).map_err(|e| {
        error!("Failed to get invite codes: {:?}", e);
        ApiError::InternalServerError
    })?;

    // Filter out expired or used invites
    let now = chrono::Utc::now();
    let active_invites = all_invites
        .into_iter()
        .filter(|invite| !invite.used && invite.expires_at > now)
        .collect::<Vec<_>>();

    let response: Vec<InviteResponse> = active_invites
        .into_iter()
        .map(|invite| InviteResponse {
            code: invite.code,
            email: invite.email,
            role: invite.role,
            used: invite.used,
            expires_at: invite.expires_at,
            created_at: invite.created_at,
            updated_at: invite.updated_at,
        })
        .collect();

    encrypt_response(&data, &session_id, &response).await
}

async fn get_invite(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Path((org_id, invite_code)): Path<(Uuid, Uuid)>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    debug!("Getting invite by code");

    // Get the org by UUID
    let org = data
        .db
        .get_org_by_uuid(org_id)
        .map_err(|_| ApiError::NotFound)?;

    // Get invite by code
    let invite = data.db.get_invite_code_by_code(invite_code).map_err(|e| {
        error!("Failed to get invite code: {:?}", e);
        match e {
            DBError::InviteCodeNotFound => ApiError::NotFound,
            _ => ApiError::InternalServerError,
        }
    })?;

    // Verify the invite belongs to the specified org
    if invite.org_id != org.id {
        return Err(ApiError::NotFound);
    }

    // Check if user is the invite recipient or an org admin/owner
    let is_invited_user = platform_user.email == invite.email;

    // Fetch membership once and use it for role check
    let is_org_admin = data
        .db
        .get_org_membership_by_platform_user_and_org(platform_user.uuid, org.id)
        .map(|membership| {
            let role: OrgRole = membership.role.into();
            matches!(role, OrgRole::Owner | OrgRole::Admin)
        })
        .unwrap_or(false);

    // Only allow access if user is the invite recipient or an org admin/owner
    if !is_invited_user && !is_org_admin {
        return Err(ApiError::Unauthorized);
    }

    let response = DetailedInviteResponse {
        code: invite.code,
        email: invite.email,
        role: invite.role,
        used: invite.used,
        expires_at: invite.expires_at,
        created_at: invite.created_at,
        updated_at: invite.updated_at,
        organization_name: org.name,
    };

    encrypt_response(&data, &session_id, &response).await
}

async fn delete_invite(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Path((org_id, invite_code)): Path<(Uuid, Uuid)>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    debug!("Deleting invite");

    // Get the org by UUID
    let org = data
        .db
        .get_org_by_uuid(org_id)
        .map_err(|_| ApiError::NotFound)?;

    // Verify user has admin or owner role
    let membership = data
        .db
        .get_org_membership_by_platform_user_and_org(platform_user.uuid, org.id)
        .map_err(|_| ApiError::Unauthorized)?;

    let role: OrgRole = membership.role.into();
    if !matches!(role, OrgRole::Owner | OrgRole::Admin) {
        return Err(ApiError::Unauthorized);
    }

    // Get invite by code
    let invite = data.db.get_invite_code_by_code(invite_code).map_err(|e| {
        error!("Failed to get invite code: {:?}", e);
        match e {
            DBError::InviteCodeNotFound => ApiError::NotFound,
            _ => ApiError::InternalServerError,
        }
    })?;

    // Verify the invite belongs to the specified org
    if invite.org_id != org.id {
        return Err(ApiError::NotFound);
    }

    // Delete the invite
    data.db.delete_invite_code(&invite).map_err(|e| {
        error!("Failed to delete invite code: {:?}", e);
        ApiError::InternalServerError
    })?;

    let response = serde_json::json!({
        "message": "Invite deleted successfully"
    });

    encrypt_response(&data, &session_id, &response).await
}

async fn accept_invite(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Path(code): Path<Uuid>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    debug!("Accepting invite");

    // Get and validate the invite code
    let invite = data
        .db
        .get_invite_code_by_code(code)
        .map_err(|_| ApiError::NotFound)?;

    if invite.used {
        return Err(ApiError::BadRequest);
    }

    if invite.expires_at < chrono::Utc::now() {
        return Err(ApiError::BadRequest);
    }

    if invite.email != platform_user.email {
        return Err(ApiError::Unauthorized);
    }

    // Create the membership and mark invite as used in a single transaction
    let new_membership = NewOrgMembership::new(
        platform_user.uuid,
        invite.org_id,
        invite.role.clone().into(),
    );
    data.db
        .accept_invite_transaction(&invite, new_membership)
        .map_err(|e| {
            error!("Failed to accept invite: {:?}", e);
            match e {
                DBError::InviteCodeError(InviteCodeError::AlreadyUsed) => ApiError::BadRequest,
                DBError::InviteCodeError(InviteCodeError::Expired) => ApiError::BadRequest,
                _ => ApiError::InternalServerError,
            }
        })?;

    let response = serde_json::json!({
        "message": "Invite accepted successfully"
    });

    encrypt_response(&data, &session_id, &response).await
}
