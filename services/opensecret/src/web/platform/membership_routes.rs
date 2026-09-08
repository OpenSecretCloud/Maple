use crate::{
    models::{
        org_memberships::{OrgMembershipError, OrgRole},
        platform_users::PlatformUser,
    },
    web::encryption_middleware::{decrypt_request, encrypt_response, Decrypted, TransportSession},
    ApiError, AppState, DBError,
};
use axum::{
    extract::{Path, State},
    middleware::from_fn_with_state,
    response::Response,
    routing::{delete, get, patch},
    Extension, Router,
};
use std::sync::Arc;
use tracing::{debug, error};
use uuid::Uuid;

use super::common::{MembershipResponse, UpdateMembershipRequest};

pub fn router(app_state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/platform/orgs/:org_id/memberships",
            get(list_memberships)
                .layer(from_fn_with_state(app_state.clone(), decrypt_request::<()>)),
        )
        .route(
            "/platform/orgs/:org_id/memberships/:user_id",
            patch(update_membership).layer(from_fn_with_state(
                app_state.clone(),
                decrypt_request::<UpdateMembershipRequest>,
            )),
        )
        .route(
            "/platform/orgs/:org_id/memberships/:user_id",
            delete(delete_membership)
                .layer(from_fn_with_state(app_state.clone(), decrypt_request::<()>)),
        )
        .with_state(app_state)
}

async fn list_memberships(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Path(org_id): Path<Uuid>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    debug!("Listing memberships");

    // Get the org by UUID
    let org = data
        .db
        .get_org_by_uuid(org_id)
        .map_err(|_| ApiError::NotFound)?;

    // Verify user has any role in the org
    let _membership = data
        .db
        .get_org_membership_by_platform_user_and_org(platform_user.uuid, org.id)
        .map_err(|_| ApiError::Unauthorized)?;

    // Get all memberships with user information in a single query
    let memberships_with_users = data
        .db
        .get_all_org_memberships_with_users_for_org(org.id)
        .map_err(|e| {
            error!("Failed to get memberships with users: {:?}", e);
            ApiError::InternalServerError
        })?;

    // Create response directly from the joined results
    let response: Vec<MembershipResponse> = memberships_with_users
        .into_iter()
        .map(|m| MembershipResponse {
            user_id: m.platform_user_id,
            role: m.role,
            name: m.user_name,
        })
        .collect();

    encrypt_response(&data, &session_id, &response).await
}

async fn update_membership(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
    Decrypted(update_request): Decrypted<UpdateMembershipRequest>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    debug!(
        "Updating membership for user {} in org {} to role {:?}",
        user_id, org_id, update_request.role
    );

    // Get the org by UUID
    let org = match data.db.get_org_by_uuid(org_id) {
        Ok(org) => org,
        Err(e) => {
            error!("Organization not found: {} - Error: {:?}", org_id, e);
            return Err(ApiError::NotFound);
        }
    };

    // Verify user has owner role
    let membership = match data
        .db
        .get_org_membership_by_platform_user_and_org(platform_user.uuid, org.id)
    {
        Ok(m) => m,
        Err(e) => {
            error!(
                "Current user {} not in org {} - Error: {:?}",
                platform_user.uuid, org_id, e
            );
            return Err(ApiError::Unauthorized);
        }
    };

    let role: OrgRole = membership.role.clone().into();
    if !matches!(role, OrgRole::Owner) {
        error!(
            "User {} attempted to update membership but has role '{}', not Owner",
            platform_user.uuid, membership.role
        );
        return Err(ApiError::Unauthorized);
    }

    // Get and update the target membership
    let mut target_membership = match data
        .db
        .get_org_membership_by_platform_user_and_org(user_id, org.id)
    {
        Ok(m) => m,
        Err(e) => {
            error!(
                "Target user {} not found in org {} - Error: {:?}",
                user_id, org_id, e
            );
            return Err(ApiError::NotFound);
        }
    };

    debug!(
        "Changing user {} role from '{}' to '{:?}'",
        user_id, target_membership.role, update_request.role
    );

    // Update role with transactional owner check
    data.db
        .update_membership_role(&mut target_membership, update_request.role.clone())
        .map_err(|e| match e {
            DBError::OrgMembershipError(OrgMembershipError::DatabaseError(
                diesel::result::Error::RollbackTransaction,
            )) => {
                error!("Cannot demote the last owner of the organization");
                ApiError::BadRequest
            }
            _ => {
                error!(
                    "Failed to update membership for user {} from '{}' to '{:?}': {:?}",
                    user_id, target_membership.role, update_request.role, e
                );
                ApiError::InternalServerError
            }
        })?;

    // Get the membership with user info in a single query
    let membership_with_user = data
        .db
        .get_org_membership_by_platform_user_and_org_with_user(user_id, org.id)
        .map_err(|e| {
            error!("Failed to get membership with user after update: {:?}", e);
            ApiError::InternalServerError
        })?;

    debug!(
        "Successfully updated user {} role to '{}'",
        user_id, membership_with_user.role
    );

    let response = MembershipResponse {
        user_id: membership_with_user.platform_user_id,
        role: membership_with_user.role,
        name: membership_with_user.user_name,
    };

    encrypt_response(&data, &session_id, &response).await
}

async fn delete_membership(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    debug!("Deleting membership");

    // Get the org by UUID
    let org = data
        .db
        .get_org_by_uuid(org_id)
        .map_err(|_| ApiError::NotFound)?;

    // Verify user has owner role
    let membership = data
        .db
        .get_org_membership_by_platform_user_and_org(platform_user.uuid, org.id)
        .map_err(|_| ApiError::Unauthorized)?;

    let role: OrgRole = membership.role.clone().into();
    if !matches!(role, OrgRole::Owner) {
        return Err(ApiError::Unauthorized);
    }

    // Get the target membership
    let target_membership = data
        .db
        .get_org_membership_by_platform_user_and_org(user_id, org.id)
        .map_err(|_| ApiError::NotFound)?;

    // Delete with transactional owner check
    data.db
        .delete_membership_with_owner_check(&target_membership)
        .map_err(|e| match e {
            DBError::OrgMembershipError(OrgMembershipError::DatabaseError(
                diesel::result::Error::RollbackTransaction,
            )) => {
                error!("Cannot delete the last owner of the organization");
                ApiError::BadRequest
            }
            _ => {
                error!("Failed to delete membership: {:?}", e);
                ApiError::InternalServerError
            }
        })?;

    let response = serde_json::json!({
        "message": "Membership deleted successfully"
    });

    encrypt_response(&data, &session_id, &response).await
}
