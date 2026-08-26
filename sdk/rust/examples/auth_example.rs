use opensecret::{OpenSecretClient, Result};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the client
    let client = OpenSecretClient::new("http://localhost:3000")?;

    // Your client ID - either set VITE_TEST_CLIENT_ID env var or replace the string below
    let client_id = std::env::var("VITE_TEST_CLIENT_ID")
        .ok()
        .and_then(|id| Uuid::parse_str(&id).ok())
        .or_else(|| Uuid::parse_str("your-client-id-here").ok())
        .expect("Please set VITE_TEST_CLIENT_ID environment variable or replace 'your-client-id-here' with a valid UUID");

    // Step 1: Perform attestation handshake (required before any encrypted calls)
    println!("Performing attestation handshake...");
    client.perform_attestation_handshake().await?;
    println!("✓ Secure session established");

    // Example 1: Register a new user with email
    println!("\n--- User Registration ---");
    let email = "user@example.com".to_string();
    let password = "secure_password_123".to_string();
    let name = Some("John Doe".to_string());

    match client
        .register(email.clone(), password.clone(), client_id, name)
        .await
    {
        Ok(response) => {
            println!("✓ User registered successfully!");
            println!("  User ID: {}", response.id);
            println!("  Email: {:?}", response.email);
            println!(
                "  Access Token: {}...",
                response.access_token.chars().take(20).collect::<String>()
            );
        }
        Err(e) => {
            println!("Registration failed (user might already exist): {}", e);

            // Try logging in instead
            println!("\n--- User Login ---");
            match client.login(email, password, client_id).await {
                Ok(response) => {
                    println!("✓ Login successful!");
                    println!("  User ID: {}", response.id);
                    println!(
                        "  Access Token: {}...",
                        response.access_token.chars().take(20).collect::<String>()
                    );
                }
                Err(e) => println!("Login failed: {}", e),
            }
        }
    }

    // Example 2: Guest registration (no email required)
    println!("\n--- Guest Registration ---");

    // Logout first if logged in
    if client.get_access_token()?.is_some() {
        client.logout().await?;
        // Need new session after logout
        client.perform_attestation_handshake().await?;
    }

    let guest_password = "guest_password_456".to_string();
    match client
        .register_guest(guest_password.clone(), client_id)
        .await
    {
        Ok(response) => {
            println!("✓ Guest registered successfully!");
            println!("  Guest ID: {}", response.id);
            println!("  No email: {:?}", response.email);

            // Save the guest ID for future logins
            let guest_id = response.id;

            // Logout and login again as guest
            client.logout().await?;
            client.perform_attestation_handshake().await?;

            println!("\n--- Guest Login ---");
            match client
                .login_with_id(guest_id, guest_password, client_id)
                .await
            {
                Ok(_) => println!("✓ Guest login successful!"),
                Err(e) => println!("Guest login failed: {}", e),
            }
        }
        Err(e) => println!("Guest registration failed: {}", e),
    }

    // Example 3: Token refresh
    if client.get_access_token()?.is_some() {
        println!("\n--- Token Refresh ---");
        let old_token = client.get_access_token()?.unwrap();
        println!(
            "Old access token: {}...",
            old_token.chars().take(20).collect::<String>()
        );

        client.refresh_token().await?;

        let new_token = client.get_access_token()?.unwrap();
        println!(
            "New access token: {}...",
            new_token.chars().take(20).collect::<String>()
        );
        println!("✓ Token refreshed successfully!");
    }

    // Example 4: Logout
    if client.get_access_token()?.is_some() {
        println!("\n--- Logout ---");
        client.logout().await?;
        println!("✓ Logged out successfully");
        println!(
            "  Access token cleared: {}",
            client.get_access_token()?.is_none()
        );
        println!("  Session cleared: {}", client.get_session_id()?.is_none());
    }

    Ok(())
}
