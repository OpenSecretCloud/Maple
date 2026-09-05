use maple_proxy::{create_app, Config};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create config programmatically
    let config = Config::new(
        "127.0.0.1".to_string(),
        8081, // Custom port
        "https://enclave.trymaple.ai".to_string(),
    )
    .with_api_key("your-api-key-here".to_string())
    .with_debug(true)
    .with_cors(true);

    // Create the app
    let app = create_app(config.clone());

    // Start the server
    let addr = config.socket_addr()?;
    let listener = TcpListener::bind(addr).await?;

    println!("Maple proxy server running on http://{}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
