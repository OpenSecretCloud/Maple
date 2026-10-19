//! Stock Iroh transport, with no public address publication or custom routing.
use iroh::{Endpoint, RelayMode, SecretKey, endpoint::presets};

use super::wire::ALPN;

pub async fn bind(key: SecretKey, relays: RelayMode, relay_only: bool) -> Result<Endpoint, String> {
    let has_relay = !matches!(relays, RelayMode::Disabled);
    let endpoint = builder(key, relays, relay_only)
        .bind()
        .await
        .map_err(|_| "Could not start Maple remote networking".to_string())?;
    // Binding precedes relay readiness. Publish usable addressing on the first
    // registration after restart, rather than an empty hint until the refresh.
    // Offline relays must not prevent a direct path or local controls forever.
    if has_relay {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), endpoint.online()).await;
    }
    Ok(endpoint)
}

fn builder(key: SecretKey, relays: RelayMode, relay_only: bool) -> iroh::endpoint::Builder {
    let mut builder = Endpoint::builder(presets::Minimal)
        .secret_key(key)
        .alpns(vec![ALPN.to_vec()])
        .relay_mode(relays)
        .clear_address_lookup();
    // Used for forced-relay validation without changing this machine's firewall
    // or any other process. Production uses Iroh's ordinary path selection.
    if relay_only {
        builder = builder.clear_ip_transports();
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::{pairing, wire};
    use iroh::EndpointAddr;
    use std::time::Duration;

    async fn exchange(relays: RelayMode, relay_only: bool) {
        let endpoint = |relays| async move {
            if relay_only {
                bind(SecretKey::generate(), relays, true).await.unwrap()
            } else {
                // A deterministic same-machine direct path without relying on
                // the VM's bridged LAN interface or altering its firewall.
                builder(SecretKey::generate(), relays, false)
                    .clear_ip_transports()
                    .bind_addr((std::net::Ipv4Addr::LOCALHOST, 0))
                    .unwrap()
                    .bind()
                    .await
                    .unwrap()
            }
        };
        let a = endpoint(relays.clone()).await;
        let b = endpoint(relays).await;
        let exercise = async {
            if relay_only {
                assert!(a.addr().relay_urls().next().is_some());
                assert!(b.addr().relay_urls().next().is_some());
            }
            let a_key = *a.id().as_bytes();
            let b_key = *b.id().as_bytes();
            let context = pairing::Context {
                controller: &a_key,
                execution: &b_key,
                account: "isolated-test",
                project: "test",
                deployment: "local",
                channel: "unpackaged",
                window: &[9; 16],
            };
            let a_proof =
                pairing::Controller::new(pairing::Code::parse("ABCDEFG2").unwrap(), &context)
                    .unwrap();
            let destination: EndpointAddr = b.addr();
            let connect = async {
                a.connect(destination, ALPN)
                    .await
                    .map_err(|e| e.to_string())
            };
            let accept = async { b.accept().await.unwrap().await.map_err(|e| e.to_string()) };
            let (client, server) = tokio::try_join!(connect, accept).unwrap();
            assert_eq!(client.remote_id(), b.id());
            assert_eq!(server.remote_id(), a.id());
            if relay_only {
                assert!(client.paths().iter().all(|path| !path.is_ip()));
                assert!(client.paths().iter().any(|path| path.is_relay()));
            } else {
                assert!(client.paths().iter().any(|path| path.is_ip()));
            }
            let controller = async {
                let (mut tx, mut rx) = client.open_bi().await.unwrap();
                wire::write(&mut tx, &a_proof.message(), wire::MAX_PREFACE)
                    .await
                    .unwrap();
                let (message, host_tag): (Vec<u8>, Vec<u8>) =
                    wire::read(&mut rx, wire::MAX_PREFACE).await.unwrap();
                let tag = a_proof.confirm(message, &host_tag).unwrap();
                wire::write(&mut tx, &tag.as_slice(), wire::MAX_PREFACE)
                    .await
                    .unwrap();
                let ack: String = wire::read(&mut rx, wire::MAX_PREFACE).await.unwrap();
                assert_eq!(ack, "proof verified");
            };
            let execution = async {
                let (mut tx, mut rx) = server.accept_bi().await.unwrap();
                let message = wire::read(&mut rx, wire::MAX_PREFACE).await.unwrap();
                let proof = pairing::Execution::new(
                    pairing::Code::parse("ABCDEFG2").unwrap(),
                    &context,
                    message,
                )
                .unwrap();
                wire::write(
                    &mut tx,
                    &(proof.message(), proof.confirmation().as_slice()),
                    wire::MAX_PREFACE,
                )
                .await
                .unwrap();
                let tag: Vec<u8> = wire::read(&mut rx, wire::MAX_PREFACE).await.unwrap();
                proof.verify(&tag).unwrap();
                wire::write(&mut tx, &"proof verified", wire::MAX_PREFACE)
                    .await
                    .unwrap();
            };
            tokio::join!(controller, execution);
        };
        let result = tokio::time::timeout(Duration::from_secs(60), exercise).await;
        tokio::join!(a.close(), b.close());
        result.expect("isolated Iroh exchange timed out");
    }

    #[tokio::test]
    async fn separate_endpoints_pair_over_direct_iroh() {
        exchange(RelayMode::Disabled, false).await;
    }

    #[tokio::test]
    #[ignore = "explicit public relay network validation, not an offline CI test"]
    async fn separate_endpoints_pair_over_public_relay_without_ip_transports() {
        exchange(RelayMode::Default, true).await;
    }
}
