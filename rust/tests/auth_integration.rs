mod common;

use opensecret::Result;
use uuid::Uuid;

#[tokio::test]
async fn test_login_signup_flow() -> Result<()> {
    // Load environment variables from .env.local in SDK root
    let env_path = std::path::Path::new("../.env.local");
    if env_path.exists() {
        dotenvy::from_path(env_path).ok();
    } else {
        // Fallback to standard .env
        dotenvy::dotenv().ok();
    }

    // Use VITE_ prefixed variables to match the TypeScript SDK
    let base_url = std::env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    let client_id = std::env::var("VITE_TEST_CLIENT_ID")
        .ok()
        .and_then(|id| Uuid::parse_str(&id).ok())
        .unwrap_or_else(|| {
            panic!("VITE_TEST_CLIENT_ID must be set in .env.local or .env");
        });

    // Create client
    let client = common::new_test_client(base_url)?;

    // Perform attestation handshake first
    println!("Performing attestation handshake...");
    client.perform_attestation_handshake().await?;
    println!("✓ Attestation handshake completed");

    // Test guest registration
    println!("\nTesting guest registration...");
    let guest_password = "test_guest_password_123";
    let guest_response = client
        .register_guest(guest_password.to_string(), client_id)
        .await?;
    println!("✓ Guest registered with ID: {}", guest_response.id);
    assert!(guest_response.email.is_none());
    assert!(!guest_response.access_token.is_empty());
    assert!(!guest_response.refresh_token.is_empty());

    // Test logout
    println!("\nTesting logout...");
    client.logout().await?;
    println!("✓ Logged out successfully");

    // Test guest login
    println!("\nTesting guest login...");
    client.perform_attestation_handshake().await?; // Need new session after logout
    let login_response = client
        .login_with_id(guest_response.id, guest_password.to_string(), client_id)
        .await?;
    println!("✓ Guest logged in successfully");
    assert_eq!(login_response.id, guest_response.id);

    // Test token refresh
    println!("\nTesting token refresh...");
    client.refresh_token().await?;

    // Verify we still have valid tokens after refresh
    assert!(
        client.get_access_token()?.is_some(),
        "Should have access token after refresh"
    );
    assert!(
        client.get_refresh_token()?.is_some(),
        "Should have refresh token after refresh"
    );
    println!("✓ Token refreshed successfully");

    // Test regular user registration with email (matching TypeScript tests)
    let test_email =
        std::env::var("VITE_TEST_EMAIL").expect("VITE_TEST_EMAIL must be set in .env.local");
    let test_password =
        std::env::var("VITE_TEST_PASSWORD").expect("VITE_TEST_PASSWORD must be set in .env.local");
    let test_name = std::env::var("VITE_TEST_NAME").ok();

    println!("\nTesting email/password login...");
    client.logout().await?;
    client.perform_attestation_handshake().await?;

    // Try login first, if it fails then register
    let email_response = match client
        .login(test_email.clone(), test_password.clone(), client_id)
        .await
    {
        Ok(response) => {
            println!("✓ Existing user logged in with email: {}", test_email);
            response
        }
        Err(_) => {
            println!("Login failed, attempting registration...");
            // Register the user
            let register_response = client
                .register(
                    test_email.clone(),
                    test_password.clone(),
                    client_id,
                    test_name,
                )
                .await?;
            println!("✓ User registered with email: {}", test_email);
            register_response
        }
    };

    assert_eq!(email_response.email, Some(test_email.clone()));
    assert!(!email_response.access_token.is_empty());
    assert!(!email_response.refresh_token.is_empty());

    // Test refresh token for email user
    println!("\nTesting email user token refresh...");
    client.refresh_token().await?;
    assert!(client.get_access_token()?.is_some());
    assert!(client.get_refresh_token()?.is_some());
    println!("✓ Email user token refreshed successfully");

    // Test logout for email user
    println!("\nTesting email user logout...");
    client.logout().await?;
    println!("✓ Email user logged out successfully");

    // Verify we can log back in
    println!("\nTesting email user re-login...");
    client.perform_attestation_handshake().await?;
    let relogin_response = client
        .login(test_email.clone(), test_password, client_id)
        .await?;
    assert_eq!(relogin_response.email, Some(test_email));
    println!("✓ Email user re-logged in successfully");

    println!("\n✅ All auth tests passed!");
    Ok(())
}

#[tokio::test]
async fn test_session_management() -> Result<()> {
    let base_url = std::env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let client = common::new_test_client(base_url)?;

    // Should have no session initially
    assert!(client.get_session_id()?.is_none());
    assert!(client.get_access_token()?.is_none());
    assert!(client.get_refresh_token()?.is_none());

    // After attestation, should have session but no tokens
    client.perform_attestation_handshake().await?;
    assert!(client.get_session_id()?.is_some());
    assert!(client.get_access_token()?.is_none());
    assert!(client.get_refresh_token()?.is_none());

    Ok(())
}
