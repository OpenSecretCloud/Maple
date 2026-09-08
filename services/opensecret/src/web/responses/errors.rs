//! Error mapping utilities for Responses and Conversations APIs

use crate::{db::DBError, models::responses::ResponsesError, ApiError};
use tracing::error;

/// Centralized error mapping utilities
///
/// This module consolidates all the repeated error mapping patterns
/// found throughout the Responses and Conversations APIs, providing
/// consistent error handling and logging.
pub mod error_mapping {
    use super::*;

    /// Map conversation-related database errors to API errors
    ///
    /// # Arguments
    /// * `e` - The database error to map
    ///
    /// # Returns
    /// Appropriate ApiError with logging
    pub fn map_conversation_error(e: DBError) -> ApiError {
        match e {
            DBError::ResponsesError(ResponsesError::ConversationNotFound) => {
                // Don't log NotFound as error - it's expected for invalid IDs
                ApiError::NotFound
            }
            DBError::ResponsesError(ResponsesError::ValidationError) => ApiError::BadRequest,
            _ => {
                error!("Conversation database error: {:?}", e);
                ApiError::InternalServerError
            }
        }
    }

    /// Map batch conversation project update errors to API errors
    pub fn map_batch_conversation_project_error(e: DBError) -> ApiError {
        match e {
            DBError::ResponsesError(ResponsesError::ConversationNotFound)
            | DBError::ResponsesError(ResponsesError::ConversationProjectNotFound) => {
                ApiError::NotFound
            }
            DBError::ResponsesError(ResponsesError::ValidationError) => ApiError::BadRequest,
            _ => {
                error!("Batch conversation project database error: {:?}", e);
                ApiError::InternalServerError
            }
        }
    }

    /// Map conversation-project-related database errors to API errors
    pub fn map_conversation_project_error(e: DBError) -> ApiError {
        match e {
            DBError::ResponsesError(ResponsesError::ConversationProjectNotFound) => {
                ApiError::NotFound
            }
            DBError::ResponsesError(ResponsesError::ValidationError) => ApiError::BadRequest,
            _ => {
                error!("Conversation project database error: {:?}", e);
                ApiError::InternalServerError
            }
        }
    }

    /// Map instruction-related database errors to API errors
    ///
    /// # Arguments
    /// * `e` - The database error to map
    ///
    /// # Returns
    /// Appropriate ApiError with logging
    pub fn map_instruction_error(e: DBError) -> ApiError {
        match e {
            DBError::ResponsesError(ResponsesError::SystemPromptNotFound) => {
                // Don't log NotFound as error - it's expected for invalid IDs
                ApiError::NotFound
            }
            _ => {
                error!("Instruction database error: {:?}", e);
                ApiError::InternalServerError
            }
        }
    }

    /// Map response-related database errors to API errors
    ///
    /// # Arguments
    /// * `e` - The database error to map
    ///
    /// # Returns
    /// Appropriate ApiError with logging
    pub fn map_response_error(e: DBError) -> ApiError {
        match e {
            DBError::ResponsesError(ResponsesError::ResponseNotFound) => ApiError::NotFound,
            DBError::ResponsesError(ResponsesError::Unauthorized) => ApiError::Unauthorized,
            DBError::ResponsesError(ResponsesError::ValidationError) => ApiError::BadRequest,
            _ => {
                error!("Response database error: {:?}", e);
                ApiError::InternalServerError
            }
        }
    }

    /// Map message-related database errors to API errors
    ///
    /// # Arguments
    /// * `e` - The database error to map
    ///
    /// # Returns
    /// ApiError with logging
    pub fn map_message_error(e: DBError) -> ApiError {
        error!("Message database error: {:?}", e);
        ApiError::InternalServerError
    }

    /// Map generic database errors to API errors
    ///
    /// Use this when the specific error type doesn't need special handling
    ///
    /// # Arguments
    /// * `e` - The database error to map
    ///
    /// # Returns
    /// ApiError with logging
    pub fn map_generic_db_error(e: DBError) -> ApiError {
        error!("Database error: {:?}", e);
        ApiError::InternalServerError
    }

    /// Map decryption errors to API errors
    ///
    /// # Arguments
    /// * `context` - Description of what was being decrypted (for logging)
    ///
    /// # Returns
    /// ApiError with logging
    pub fn map_decryption_error(context: &str) -> ApiError {
        error!("Failed to decrypt {}", context);
        ApiError::InternalServerError
    }

    /// Map key retrieval errors to API errors
    ///
    /// # Returns
    /// ApiError with logging
    pub fn map_key_retrieval_error() -> ApiError {
        error!("Failed to get user encryption key");
        ApiError::InternalServerError
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation_not_found_returns_not_found() {
        let error = DBError::ResponsesError(ResponsesError::ConversationNotFound);
        let api_error = error_mapping::map_conversation_error(error);
        assert!(matches!(api_error, ApiError::NotFound));
    }

    #[test]
    fn test_conversation_validation_error_returns_bad_request() {
        let error = DBError::ResponsesError(ResponsesError::ValidationError);
        let api_error = error_mapping::map_conversation_error(error);
        assert!(matches!(api_error, ApiError::BadRequest));
    }

    #[test]
    fn test_response_not_found_returns_not_found() {
        let error = DBError::ResponsesError(ResponsesError::ResponseNotFound);
        let api_error = error_mapping::map_response_error(error);
        assert!(matches!(api_error, ApiError::NotFound));
    }

    #[test]
    fn test_conversation_project_not_found_returns_not_found() {
        let error = DBError::ResponsesError(ResponsesError::ConversationProjectNotFound);
        let api_error = error_mapping::map_conversation_project_error(error);
        assert!(matches!(api_error, ApiError::NotFound));
    }

    #[test]
    fn test_unauthorized_returns_unauthorized() {
        let error = DBError::ResponsesError(ResponsesError::Unauthorized);
        let api_error = error_mapping::map_response_error(error);
        assert!(matches!(api_error, ApiError::Unauthorized));
    }

    #[test]
    fn test_validation_error_returns_bad_request() {
        let error = DBError::ResponsesError(ResponsesError::ValidationError);
        let api_error = error_mapping::map_response_error(error);
        assert!(matches!(api_error, ApiError::BadRequest));
    }

    #[test]
    fn test_conversation_project_validation_error_returns_bad_request() {
        let error = DBError::ResponsesError(ResponsesError::ValidationError);
        let api_error = error_mapping::map_conversation_project_error(error);
        assert!(matches!(api_error, ApiError::BadRequest));
    }

    #[test]
    fn test_batch_conversation_project_not_found_returns_not_found() {
        let error = DBError::ResponsesError(ResponsesError::ConversationProjectNotFound);
        let api_error = error_mapping::map_batch_conversation_project_error(error);
        assert!(matches!(api_error, ApiError::NotFound));
    }

    #[test]
    fn test_batch_conversation_validation_error_returns_bad_request() {
        let error = DBError::ResponsesError(ResponsesError::ValidationError);
        let api_error = error_mapping::map_batch_conversation_project_error(error);
        assert!(matches!(api_error, ApiError::BadRequest));
    }
}
