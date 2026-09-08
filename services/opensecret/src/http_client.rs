/// Install the process-wide Rustls provider before constructing any Reqwest
/// client. Tinfoil pins Ring across platforms, so OpenSecret selects the same
/// provider for its other Rustls clients instead of depending on construction
/// order.
pub(crate) fn install_ring_crypto_provider() -> Result<(), ()> {
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        return Ok(());
    }

    rustls::crypto::ring::default_provider()
        .install_default()
        .map(|_| ())
        // Another thread may have installed a provider between the check and
        // install. A configured provider satisfies Reqwest's requirement.
        .or_else(|_| {
            rustls::crypto::CryptoProvider::get_default()
                .is_some()
                .then_some(())
                .ok_or(())
        })
}

pub(crate) fn client_builder() -> reqwest::ClientBuilder {
    install_ring_crypto_provider().expect("failed to configure a Rustls crypto provider");
    reqwest::Client::builder()
}

pub(crate) fn client() -> reqwest::Client {
    client_builder()
        .build()
        .expect("failed to build the default HTTP client")
}
