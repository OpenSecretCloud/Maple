use opensecret::{KeyOptions, OpenSecretClient, Result, SigningAlgorithm};
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

    println!("🔐 OpenSecret SDK - API Usage Examples\n");

    // Step 1: Establish secure session
    println!("1️⃣  Establishing secure session...");
    client.perform_attestation_handshake().await?;
    println!("   ✓ Secure session established\n");

    // Step 2: Authentication
    println!("2️⃣  Authentication");
    let email = "demo@example.com";
    let password = "secure_password_123";

    // Try to login, register if user doesn't exist
    match client
        .login(email.to_string(), password.to_string(), client_id)
        .await
    {
        Ok(response) => {
            println!("   ✓ Logged in as existing user");
            println!("   User ID: {}", response.id);
        }
        Err(_) => {
            println!("   User not found, registering...");
            let response = client
                .register(
                    email.to_string(),
                    password.to_string(),
                    client_id,
                    Some("Demo User".to_string()),
                )
                .await?;
            println!("   ✓ Registered new user");
            println!("   User ID: {}", response.id);
        }
    }
    println!();

    // Step 3: Get User Profile
    println!("3️⃣  User Profile");
    let user = client.get_user().await?;
    println!("   Email: {:?}", user.user.email);
    println!("   Verified: {}", user.user.email_verified);
    println!("   Method: {:?}", user.user.login_method);
    println!("   Created: {}", user.user.created_at);
    println!();

    // Step 4: Key-Value Storage
    println!("4️⃣  Key-Value Storage");

    // Store a value
    let key = "user_preference";
    let value = r#"{"theme": "dark", "language": "en"}"#;
    client.kv_put(key, value.to_string()).await?;
    println!("   ✓ Stored: {} = {}", key, value);

    // Retrieve the value
    let retrieved = client.kv_get(key).await?;
    println!("   ✓ Retrieved: {}", retrieved);

    // List all keys
    let keys = client.kv_list().await?;
    println!("   ✓ Total keys: {}", keys.len());
    for item in keys.iter().take(3) {
        println!("     - {}: {}", item.key, item.value);
    }

    // Clean up
    client.kv_delete(key).await?;
    println!("   ✓ Deleted key: {}", key);
    println!();

    // Step 5: Private Key Generation
    println!("5️⃣  Private Key Generation");

    // Generate default mnemonic (12 words)
    let private_key = client.get_private_key(None).await?;
    let word_count = private_key.mnemonic.split_whitespace().count();
    println!("   ✓ Generated {} word mnemonic", word_count);
    println!(
        "   First 3 words: {}...",
        private_key
            .mnemonic
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ")
    );

    // Generate with 24 words using BIP-85 derivation path
    // The 4th segment in the path specifies word count (12, 18, or 24)
    let options = KeyOptions {
        private_key_derivation_path: None,
        seed_phrase_derivation_path: Some("m/83696968'/39'/0'/24'/0'".to_string()),
    };
    let _key_24 = client.get_private_key(Some(options)).await?;
    println!("   ✓ Generated 24 word mnemonic via BIP-85");

    // Get raw private key bytes
    let key_bytes = client.get_private_key_bytes(None).await?;
    println!(
        "   ✓ Private key bytes (hex): {}...",
        &key_bytes.private_key[..20]
    );
    println!();

    // Step 6: Message Signing
    println!("6️⃣  Digital Signatures");
    let message = "Sign this important message";

    // Sign with Schnorr (Bitcoin Taproot compatible)
    let schnorr_sig = client
        .sign_message(message.as_bytes(), SigningAlgorithm::Schnorr, None)
        .await?;
    println!(
        "   ✓ Schnorr signature: {}...",
        &schnorr_sig.signature[..20]
    );

    // Sign with ECDSA (Classic Bitcoin/Ethereum)
    let ecdsa_sig = client
        .sign_message(message.as_bytes(), SigningAlgorithm::Ecdsa, None)
        .await?;
    println!("   ✓ ECDSA signature: {}...", &ecdsa_sig.signature[..20]);

    // Sign with custom derivation path
    let custom_path = "m/44'/0'/0'/0/5";
    let key_opts = KeyOptions {
        private_key_derivation_path: Some(custom_path.to_string()),
        seed_phrase_derivation_path: None,
    };
    let custom_sig = client
        .sign_message(
            message.as_bytes(),
            SigningAlgorithm::Schnorr,
            Some(key_opts),
        )
        .await?;
    println!(
        "   ✓ Signature at path {}: {}...",
        custom_path,
        &custom_sig.signature[..20]
    );
    println!();

    // Step 7: Public Keys
    println!("7️⃣  Public Keys");

    // Get public keys for different algorithms
    let schnorr_pub = client
        .get_public_key(SigningAlgorithm::Schnorr, None)
        .await?;
    println!(
        "   ✓ Schnorr public key: {}...",
        &schnorr_pub.public_key[..20]
    );

    let ecdsa_pub = client.get_public_key(SigningAlgorithm::Ecdsa, None).await?;
    println!("   ✓ ECDSA public key: {}...", &ecdsa_pub.public_key[..20]);

    // Public key at derivation path
    let eth_key_opts = KeyOptions {
        private_key_derivation_path: Some("m/44'/60'/0'/0/0".to_string()), // Ethereum path
        seed_phrase_derivation_path: None,
    };
    let derived_pub = client
        .get_public_key(SigningAlgorithm::Ecdsa, Some(eth_key_opts))
        .await?;
    println!(
        "   ✓ Ethereum public key: {}...",
        &derived_pub.public_key[..20]
    );
    println!();

    // Step 8: Data Encryption
    println!("8️⃣  End-to-End Encryption");
    let secret_data = "This is highly confidential information";

    // Encrypt data
    let encrypted = client.encrypt_data(secret_data.to_string(), None).await?;
    println!("   ✓ Encrypted: {}...", &encrypted.encrypted_data[..30]);

    // Decrypt data
    let decrypted = client
        .decrypt_data(encrypted.encrypted_data.clone(), None)
        .await?;
    println!("   ✓ Decrypted: {}", decrypted);
    assert_eq!(decrypted, secret_data);

    // Encrypt with custom BIP-32 derivation
    let custom_key_opts = KeyOptions {
        private_key_derivation_path: Some("m/0'/1'/2'".to_string()),
        seed_phrase_derivation_path: None,
    };
    let custom_encrypted = client
        .encrypt_data("Secret with custom key".to_string(), Some(custom_key_opts))
        .await?;
    println!(
        "   ✓ Custom encryption: {}...",
        &custom_encrypted.encrypted_data[..30]
    );
    println!();

    // Step 9: Third-Party Tokens
    println!("9️⃣  Third-Party Token Generation");

    // Generate token without audience
    let token = client.generate_third_party_token(None).await?;
    println!("   ✓ Generated token: {}...", &token.token[..30]);

    // Generate token for specific service
    let audience = "https://api.example.com";
    let scoped_token = client
        .generate_third_party_token(Some(audience.to_string()))
        .await?;
    println!(
        "   ✓ Token for {}: {}...",
        audience,
        &scoped_token.token[..30]
    );
    println!();

    // Step 10: Session Management
    println!("🔟 Session Management");

    // Refresh access token
    println!("   Refreshing tokens...");
    client.refresh_token().await?;
    println!("   ✓ Tokens refreshed");

    // Get current session info
    if let Some(session_id) = client.get_session_id()? {
        println!("   Session ID: {}", session_id);
    }

    if let Some(access_token) = client.get_access_token()? {
        println!("   Access Token: {}...", &access_token[..20]);
    }

    // Logout
    println!("   Logging out...");
    client.logout().await?;
    println!("   ✓ Logged out successfully");
    println!();

    println!("✅ All API examples completed successfully!");
    println!("\n📚 For more information, visit: https://docs.opensecret.cloud");

    Ok(())
}
