pub mod common;
mod invite_routes;
mod login_routes;
mod me_routes;
mod membership_routes;
pub mod org_routes;
mod project_routes;
mod validation;

pub use login_routes::router as login_routes;

// Re-export constants for backward compatibility
pub use common::{
    PROJECT_GITHUB_OAUTH_SECRET, PROJECT_GOOGLE_OAUTH_SECRET, PROJECT_RESEND_API_KEY,
};

// Export the composite router
pub fn router(app_state: std::sync::Arc<crate::AppState>) -> axum::Router {
    // Combine all routers into a single router
    org_routes::router(app_state.clone())
        .merge(project_routes::router(app_state.clone()))
        .merge(membership_routes::router(app_state.clone()))
        .merge(invite_routes::router(app_state.clone()))
        .merge(me_routes::router(app_state.clone()))
}
