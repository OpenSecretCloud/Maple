use crate::{
    models::{org_memberships::OrgRole, orgs::NewOrg, platform_users::PlatformUser},
    web::encryption_middleware::{decrypt_request, encrypt_response, Decrypted, TransportSession},
    ApiError, AppState,
};
use axum::{
    extract::{Path, State},
    middleware::from_fn_with_state,
    response::Response,
    routing::{delete, get, post},
    Extension, Router,
};
use std::sync::Arc;
use tracing::{debug, error};
use uuid::Uuid;
use validator::Validate;

use super::common::{CreateOrgRequest, OrgResponse};

pub fn router(app_state: Arc<AppState>) -> Router {
    Router::new()
        // Organization routes
        .route(
            "/platform/orgs",
            post(create_org).layer(from_fn_with_state(
                app_state.clone(),
                decrypt_request::<CreateOrgRequest>,
            )),
        )
        .route(
            "/platform/orgs",
            get(list_orgs).layer(from_fn_with_state(app_state.clone(), decrypt_request::<()>)),
        )
        .route(
            "/platform/orgs/:org_id",
            delete(delete_org).layer(from_fn_with_state(app_state.clone(), decrypt_request::<()>)),
        )
        .with_state(app_state)
}

async fn create_org(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Decrypted(create_request): Decrypted<CreateOrgRequest>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    debug!("Creating new organization");

    // Validate request
    if create_request.validate().is_err() {
        error!("Organization creation request validation failed");
        return Err(ApiError::BadRequest);
    }

    // Create the organization and owner membership in a single transaction
    let new_org = NewOrg::new(create_request.name);
    let org = data
        .db
        .create_org_with_owner(new_org, platform_user.uuid)
        .map_err(|e| {
            error!("Failed to create organization with owner: {:?}", e);
            ApiError::InternalServerError
        })?;

    let response = OrgResponse {
        id: org.uuid,
        name: org.name,
    };

    encrypt_response(&data, &session_id, &response).await
}

async fn list_orgs(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    debug!("Listing organizations");

    // Get all memberships for the user
    let memberships = data
        .db
        .get_all_org_memberships_for_platform_user(platform_user.uuid)
        .map_err(|e| {
            error!("Failed to get org memberships: {:?}", e);
            ApiError::InternalServerError
        })?;

    // Get org details for each membership
    let mut orgs = Vec::new();
    for membership in memberships {
        match data.db.get_org_by_id(membership.org_id) {
            Ok(org) => {
                orgs.push(OrgResponse {
                    id: org.uuid,
                    name: org.name,
                });
            }
            Err(e) => {
                error!(
                    "Failed to get org details for org_id {}: {:?}",
                    membership.org_id, e
                );
                // Continue with the next membership
            }
        }
    }

    encrypt_response(&data, &session_id, &orgs).await
}

async fn delete_org(
    State(data): State<Arc<AppState>>,
    Extension(platform_user): Extension<PlatformUser>,
    Path(org_id): Path<Uuid>,
    Extension(session_id): Extension<TransportSession>,
) -> Result<Response, ApiError> {
    debug!("Deleting organization");

    // Get org by UUID instead of ID
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

    // Delete the org
    data.db.delete_org(&org).map_err(|e| {
        error!("Failed to delete organization: {:?}", e);
        ApiError::InternalServerError
    })?;

    let response = serde_json::json!({
        "message": "Organization deleted successfully"
    });

    encrypt_response(&data, &session_id, &response).await
}
