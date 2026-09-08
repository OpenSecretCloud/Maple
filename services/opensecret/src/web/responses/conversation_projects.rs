//! Conversation Projects API implementation

use crate::{
    encrypt::{decrypt_string, encrypt_with_key},
    jwt::AuthContext,
    models::responses::{NewConversationProject, ProjectInstructionUpdate},
    models::users::User,
    tokens::count_tokens,
    web::{
        encryption_middleware::{decrypt_request, encrypt_response, Decrypted, TransportSession},
        responses::{
            constants::{
                DEFAULT_PAGINATION_LIMIT, DEFAULT_PAGINATION_ORDER, MAX_PAGINATION_LIMIT,
                OBJECT_TYPE_CONVERSATION_PROJECT, OBJECT_TYPE_LIST,
            },
            error_mapping, DeletedObjectResponse, NullableField, Paginator,
        },
    },
    ApiError, AppState,
};
use axum::{
    extract::{Path, Query, State},
    middleware::from_fn_with_state,
    response::Response,
    routing::{delete, get, post},
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::debug;
use uuid::Uuid;

struct ConversationProjectContext {
    project: crate::models::responses::ConversationProject,
    user_key: secp256k1::SecretKey,
}

impl ConversationProjectContext {
    async fn load(
        state: &AppState,
        project_id: Uuid,
        user: &User,
        auth_context: &AuthContext,
    ) -> Result<Self, ApiError> {
        let project = state
            .db
            .get_conversation_project_by_uuid_and_user(project_id, user.uuid)
            .map_err(error_mapping::map_conversation_project_error)?;

        let user_key = state
            .get_user_key(user, auth_context, None, None)
            .await
            .map_err(|_| error_mapping::map_key_retrieval_error())?;

        Ok(Self { project, user_key })
    }

    fn decrypt_name(&self) -> Result<String, ApiError> {
        decrypt_string(&self.user_key, Some(&self.project.name_enc))
            .map_err(|_| error_mapping::map_decryption_error("conversation project name"))?
            .ok_or(ApiError::InternalServerError)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateConversationProjectRequest {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateConversationProjectRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub instructions: NullableField<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationProjectResponse {
    pub id: Uuid,
    pub object: &'static str,
    pub name: String,
    pub instructions: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationProjectListItem {
    pub id: Uuid,
    pub object: &'static str,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationProjectListResponse {
    pub object: &'static str,
    pub data: Vec<ConversationProjectListItem>,
    pub has_more: bool,
    pub first_id: Option<Uuid>,
    pub last_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct ListConversationProjectsParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub after: Option<Uuid>,
    #[serde(default = "default_order")]
    pub order: String,
}

fn default_limit() -> i64 {
    DEFAULT_PAGINATION_LIMIT
}

fn default_order() -> String {
    DEFAULT_PAGINATION_ORDER.to_string()
}

fn validate_project_name(name: &str) -> Result<String, ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        Err(ApiError::BadRequest)
    } else {
        Ok(trimmed.to_string())
    }
}

fn decrypt_project_instruction(
    user_key: &secp256k1::SecretKey,
    instruction: Option<crate::models::responses::UserInstruction>,
) -> Result<Option<String>, ApiError> {
    let Some(instruction) = instruction else {
        return Ok(None);
    };

    decrypt_string(user_key, Some(&instruction.prompt_enc))
        .map_err(|_| error_mapping::map_decryption_error("conversation project instruction"))
}

fn build_project_response(
    project: &crate::models::responses::ConversationProject,
    name: String,
    instructions: Option<String>,
) -> ConversationProjectResponse {
    ConversationProjectResponse {
        id: project.uuid,
        object: OBJECT_TYPE_CONVERSATION_PROJECT,
        name,
        instructions,
        created_at: project.created_at.timestamp(),
        updated_at: project.updated_at.timestamp(),
    }
}

async fn create_conversation_project(
    State(state): State<Arc<AppState>>,
    Extension(session_id): Extension<TransportSession>,
    Extension(user): Extension<User>,
    Extension(auth_context): Extension<AuthContext>,
    Decrypted(body): Decrypted<CreateConversationProjectRequest>,
) -> Result<Response, ApiError> {
    let name = validate_project_name(&body.name)?;
    let user_key = state
        .get_user_key(&user, &auth_context, None, None)
        .await
        .map_err(|_| error_mapping::map_key_retrieval_error())?;

    let project = state
        .db
        .create_conversation_project(NewConversationProject {
            uuid: Uuid::new_v4(),
            user_id: user.uuid,
            name_enc: encrypt_with_key(&user_key, name.as_bytes()).await,
        })
        .map_err(error_mapping::map_conversation_project_error)?;

    let response = build_project_response(&project, name, None);
    encrypt_response(&state, &session_id, &response).await
}

async fn list_conversation_projects(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListConversationProjectsParams>,
    Extension(session_id): Extension<TransportSession>,
    Extension(user): Extension<User>,
    Extension(auth_context): Extension<AuthContext>,
) -> Result<Response, ApiError> {
    let limit = if params.limit <= 0 {
        DEFAULT_PAGINATION_LIMIT
    } else {
        params.limit.min(MAX_PAGINATION_LIMIT)
    };

    let projects = state
        .db
        .list_conversation_projects(user.uuid, limit + 1, params.after, &params.order)
        .map_err(error_mapping::map_generic_db_error)?;

    let has_more = projects.len() > limit as usize;
    let projects_to_return = if has_more {
        &projects[..limit as usize]
    } else {
        &projects[..]
    };

    let user_key = state
        .get_user_key(&user, &auth_context, None, None)
        .await
        .map_err(|_| error_mapping::map_key_retrieval_error())?;

    let mut data = Vec::with_capacity(projects_to_return.len());
    for project in projects_to_return {
        let name = decrypt_string(&user_key, Some(&project.name_enc))
            .map_err(|_| error_mapping::map_decryption_error("conversation project name"))?
            .ok_or(ApiError::InternalServerError)?;

        data.push(ConversationProjectListItem {
            id: project.uuid,
            object: OBJECT_TYPE_CONVERSATION_PROJECT,
            name,
            created_at: project.created_at.timestamp(),
            updated_at: project.updated_at.timestamp(),
        });
    }

    let (first_id, last_id) = Paginator::get_cursor_ids(projects_to_return, |project| project.uuid);
    let response = ConversationProjectListResponse {
        object: OBJECT_TYPE_LIST,
        data,
        has_more,
        first_id,
        last_id,
    };

    encrypt_response(&state, &session_id, &response).await
}

async fn get_conversation_project(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Extension(session_id): Extension<TransportSession>,
    Extension(user): Extension<User>,
    Extension(auth_context): Extension<AuthContext>,
) -> Result<Response, ApiError> {
    let ctx = ConversationProjectContext::load(&state, project_id, &user, &auth_context).await?;
    let instructions = state
        .db
        .get_project_instruction(ctx.project.id, user.uuid)
        .map_err(error_mapping::map_generic_db_error)?;

    let response = build_project_response(
        &ctx.project,
        ctx.decrypt_name()?,
        decrypt_project_instruction(&ctx.user_key, instructions)?,
    );

    encrypt_response(&state, &session_id, &response).await
}

async fn update_conversation_project(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Extension(session_id): Extension<TransportSession>,
    Extension(user): Extension<User>,
    Extension(auth_context): Extension<AuthContext>,
    Decrypted(body): Decrypted<UpdateConversationProjectRequest>,
) -> Result<Response, ApiError> {
    if body.name.is_none() && body.instructions.is_missing() {
        return Err(ApiError::BadRequest);
    }

    let ctx = ConversationProjectContext::load(&state, project_id, &user, &auth_context).await?;

    let name_enc = if let Some(name) = body.name.as_ref() {
        let name = validate_project_name(name)?;
        Some(encrypt_with_key(&ctx.user_key, name.as_bytes()).await)
    } else {
        None
    };

    let instruction_update = match &body.instructions {
        NullableField::Missing => ProjectInstructionUpdate::Unchanged,
        NullableField::Value(prompt) => {
            if prompt.trim().is_empty() {
                return Err(ApiError::BadRequest);
            }
            let prompt_tokens = count_tokens(prompt);
            let prompt_tokens = prompt_tokens.min(i32::MAX as usize) as i32;
            ProjectInstructionUpdate::Set {
                prompt_enc: encrypt_with_key(&ctx.user_key, prompt.as_bytes()).await,
                prompt_tokens,
            }
        }
        NullableField::Null => ProjectInstructionUpdate::Clear,
    };

    let updated_project = state
        .db
        .update_conversation_project(ctx.project.id, user.uuid, name_enc, instruction_update)
        .map_err(error_mapping::map_conversation_project_error)?;

    let instructions = state
        .db
        .get_project_instruction(updated_project.id, user.uuid)
        .map_err(error_mapping::map_generic_db_error)?;

    let response = build_project_response(
        &updated_project,
        decrypt_string(&ctx.user_key, Some(&updated_project.name_enc))
            .map_err(|_| error_mapping::map_decryption_error("conversation project name"))?
            .ok_or(ApiError::InternalServerError)?,
        decrypt_project_instruction(&ctx.user_key, instructions)?,
    );

    encrypt_response(&state, &session_id, &response).await
}

async fn delete_conversation_project(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Extension(session_id): Extension<TransportSession>,
    Extension(user): Extension<User>,
) -> Result<Response, ApiError> {
    debug!(
        "Deleting conversation project {} for user {}",
        project_id, user.uuid
    );

    let project = state
        .db
        .get_conversation_project_by_uuid_and_user(project_id, user.uuid)
        .map_err(error_mapping::map_conversation_project_error)?;

    state
        .db
        .delete_conversation_project(project.id, user.uuid)
        .map_err(error_mapping::map_conversation_project_error)?;

    let response = DeletedObjectResponse::conversation_project(project.uuid);
    encrypt_response(&state, &session_id, &response).await
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/v1/conversation-projects",
            post(create_conversation_project).layer(from_fn_with_state(
                state.clone(),
                decrypt_request::<CreateConversationProjectRequest>,
            )),
        )
        .route(
            "/v1/conversation-projects",
            get(list_conversation_projects)
                .layer(from_fn_with_state(state.clone(), decrypt_request::<()>)),
        )
        .route(
            "/v1/conversation-projects/:id",
            get(get_conversation_project)
                .layer(from_fn_with_state(state.clone(), decrypt_request::<()>)),
        )
        .route(
            "/v1/conversation-projects/:id",
            post(update_conversation_project).layer(from_fn_with_state(
                state.clone(),
                decrypt_request::<UpdateConversationProjectRequest>,
            )),
        )
        .route(
            "/v1/conversation-projects/:id",
            delete(delete_conversation_project)
                .layer(from_fn_with_state(state.clone(), decrypt_request::<()>)),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::{validate_project_name, UpdateConversationProjectRequest};
    use crate::web::responses::NullableField;
    use crate::ApiError;

    #[test]
    fn update_request_distinguishes_null_from_omitted_instructions() {
        let request: UpdateConversationProjectRequest =
            serde_json::from_str(r#"{"instructions":null}"#).unwrap();
        assert!(matches!(request.instructions, NullableField::Null));

        let omitted: UpdateConversationProjectRequest = serde_json::from_str(r#"{}"#).unwrap();
        assert!(matches!(omitted.instructions, NullableField::Missing));
    }

    #[test]
    fn project_name_validation_allows_duplicate_names() {
        assert_eq!(
            validate_project_name("  Same project  ").unwrap(),
            "Same project"
        );
        assert_eq!(
            validate_project_name("Same project").unwrap(),
            "Same project"
        );
    }

    #[test]
    fn project_name_validation_rejects_blank_names() {
        assert!(matches!(
            validate_project_name("   "),
            Err(ApiError::BadRequest)
        ));
    }
}
