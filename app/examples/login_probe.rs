//! Diagnostic probe for the OpenSecret login path. Reads credentials from
//! MAPLE_TEST_EMAIL / MAPLE_TEST_PASSWORD and prints only error categories
//! and success booleans — never tokens.

use opensecret::OpenSecretClient;
use uuid::Uuid;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        eprintln!("set {name}");
        std::process::exit(2);
    })
}

#[tokio::main]
async fn main() {
    let api_url =
        std::env::var("MAPLE_API_URL").unwrap_or_else(|_| "https://enclave.trymaple.ai".into());
    let email = env("MAPLE_TEST_EMAIL");
    let password = env("MAPLE_TEST_PASSWORD");
    let maple_client_id: Uuid = "ba5a14b5-d915-47b1-b7b1-afda52bc5fc6"
        .parse()
        .expect("valid uuid");

    for (label, client_id) in [
        ("random", Uuid::new_v4()),
        ("maple-public", maple_client_id),
    ] {
        let client = match OpenSecretClient::new_with_pcr0_environment(
            api_url.clone(),
            opensecret::Pcr0Environment::Production,
        ) {
            Ok(client) => client,
            Err(error) => {
                println!("[{label}] client construction failed: {error:?}");
                continue;
            }
        };
        match client
            .login(email.clone(), password.clone(), client_id)
            .await
        {
            Ok(response) => {
                println!("[{label}] login OK (user {})", response.id);
                return;
            }
            Err(error) => {
                println!("[{label}] login failed: {error:?}");
            }
        }
    }
}
