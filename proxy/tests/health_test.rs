use axum::http::StatusCode;
use axum_test::TestServer;
use maple_proxy::{create_app, Config};
use serde_json::{json, Value};

#[tokio::test]
async fn test_health_check_endpoint() {
    // Create test config
    let config = Config::new(
        "127.0.0.1".to_string(),
        0, // Use random port for testing
        "http://localhost:3000".to_string(),
    );

    // Create test server
    let app = create_app(config);
    let server = TestServer::new(app).unwrap();

    // Test /health endpoint
    let response = server.get("/health").await;
    response.assert_status(StatusCode::OK);

    let json: Value = response.json();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "maple-proxy");
    assert!(json["version"].is_string());
}

#[tokio::test]
async fn test_root_health_check() {
    let config = Config::new(
        "127.0.0.1".to_string(),
        0,
        "http://localhost:3000".to_string(),
    );

    let app = create_app(config);
    let server = TestServer::new(app).unwrap();

    // Test root endpoint also returns health check
    let response = server.get("/").await;
    response.assert_status(StatusCode::OK);

    let json: Value = response.json();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn chat_completion_accepts_large_payloads_above_axum_default() {
    let config = Config::new(
        "127.0.0.1".to_string(),
        0,
        "http://localhost:3000".to_string(),
    );
    let app = create_app(config);
    let server = TestServer::new(app).unwrap();

    let large_content = "x".repeat((2 * 1024 * 1024) + 1);
    let response = server
        .post("/v1/chat/completions")
        .json(&json!({
            "model": "quick",
            "messages": [
                {
                    "role": "user",
                    "content": large_content
                }
            ],
            "stream": false
        }))
        .await;

    response.assert_status(StatusCode::UNAUTHORIZED);
}
