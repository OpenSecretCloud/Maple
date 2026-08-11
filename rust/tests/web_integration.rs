mod common;

use opensecret::{OpenSecretClient, Result, WebExtractRequest, WebSearchRequest};
use uuid::Uuid;

async fn authenticated_client() -> Result<OpenSecretClient> {
    let base_url = std::env::var("VITE_OPEN_SECRET_API_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let email = std::env::var("VITE_TEST_EMAIL").expect("VITE_TEST_EMAIL must be set");
    let password = std::env::var("VITE_TEST_PASSWORD").expect("VITE_TEST_PASSWORD must be set");
    let client_id = std::env::var("VITE_TEST_CLIENT_ID")
        .expect("VITE_TEST_CLIENT_ID must be set")
        .parse::<Uuid>()
        .expect("VITE_TEST_CLIENT_ID must be a UUID");

    let client = common::new_test_client(base_url)?;
    client.perform_attestation_handshake().await?;

    if client
        .login(email.clone(), password.clone(), client_id)
        .await
        .is_err()
    {
        client
            .register(email, password, client_id, Some("Web API Test".to_string()))
            .await?;
    }

    Ok(client)
}

#[tokio::test]
#[ignore = "Requires a live OpenSecret backend and Kagi API access"]
async fn live_web_search_and_extract() -> Result<()> {
    let client = authenticated_client().await?;

    let mut search_request = WebSearchRequest::new(
        std::env::var("VITE_TEST_WEB_QUERY")
            .unwrap_or_else(|_| "Maple private AI assistant".to_string()),
    );
    search_request.limit = Some(5);

    let search = client.web_search(search_request).await?;
    assert!(
        !search.results.is_empty(),
        "Kagi search returned no results"
    );
    assert!(search
        .results
        .iter()
        .all(|result| result.url.starts_with("https://")));

    let extract_url = std::env::var("VITE_TEST_WEB_EXTRACT_URL")
        .unwrap_or_else(|_| "https://kagi.com/api/pricing".to_string());
    let extract = client
        .web_extract(WebExtractRequest::new([extract_url.clone()]))
        .await?;

    assert_eq!(extract.pages.len(), 1);
    assert_eq!(extract.pages[0].url, extract_url);
    assert!(
        extract.pages[0].error.is_none(),
        "Kagi extraction failed: {:?}",
        extract.pages[0].error
    );
    let markdown = extract.pages[0]
        .markdown
        .as_deref()
        .expect("successful extraction should contain markdown");
    assert!(!markdown.trim().is_empty());
    assert!(!markdown.contains("!["));
    assert!(!markdown.to_ascii_lowercase().contains("<img"));

    Ok(())
}
