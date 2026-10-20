use super::*;
use iroh::{Endpoint, RelayMode, endpoint::presets};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn endpoints() -> (Endpoint, Endpoint, Connection, Connection) {
    async fn endpoint() -> Endpoint {
        Endpoint::builder(presets::Minimal)
            .alpns(vec![wire::ALPN.to_vec()])
            .relay_mode(RelayMode::Disabled)
            .clear_address_lookup()
            .clear_ip_transports()
            .bind_addr((std::net::Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .transport_config(transport_config())
            .bind()
            .await
            .unwrap()
    }
    let a = endpoint().await;
    let b = endpoint().await;
    let (client, server) = tokio::join!(
        async { a.connect(b.addr(), wire::ALPN).await.unwrap() },
        async { b.accept().await.unwrap().await.unwrap() },
    );
    (a, b, client, server)
}

fn command(id: u64, method: Method) -> Command {
    Command {
        id,
        method,
        send_id: None,
        params: json!(["account", "task"]),
    }
}

async fn receive(
    connection: &Connection,
    capacity: &Capacity,
) -> (Header, SendStream, RecvStream, OwnedSemaphorePermit) {
    let (tx, mut rx) = connection.accept_bi().await.unwrap();
    let header = Header::read(&mut rx).await.unwrap();
    let slot = capacity.acquire(header.method).unwrap();
    (header, tx, rx, slot)
}

#[tokio::test]
async fn stop_and_answers_pass_stalled_upload_and_download() {
    let (a, b, client, server) = endpoints().await;
    let exercise = async {
        let capacity = Capacity::new();
        let (mut upload, mut upload_reply) = client.open_bi().await.unwrap();
        let body = wire::encode(
            &json!(["account", {"image": "x".repeat(2 * 1024 * 1024)}]),
            wire::MAX_FRAME,
        )
        .unwrap();
        wire::write(
            &mut upload,
            &Header {
                id: 1,
                method: Method::SendMessage,
                send_id: Some("original-send".into()),
            },
            wire::MAX_PREFACE,
        )
        .await
        .unwrap();
        upload.write_u32(body.len() as u32).await.unwrap();
        upload.write_all(&body[..1024]).await.unwrap();
        let (header, mut tx, mut rx, upload_slot) = receive(&server, &capacity).await;
        let upload_received = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let admitted = upload_received.clone();
        let upload_task = tokio::spawn(async move {
            let _slot = upload_slot;
            let request = header.body(&mut rx).await.unwrap();
            assert_eq!(request.send_id.as_deref(), Some("original-send"));
            admitted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            reply(
                &mut tx,
                &Message::Response {
                    id: request.id,
                    result: Ok(json!("admitted")),
                },
            )
            .await
            .unwrap();
        });

        // Request an image, then stop reading its reply after the length. Its
        // payload exceeds the actual per-stream receive window, forcing QUIC
        // backpressure rather than relying on a sleeping mock provider.
        let (mut download, mut image) = client.open_bi().await.unwrap();
        wire::write(
            &mut download,
            &Header {
                id: 2,
                method: Method::ReadImageAttachment,
                send_id: None,
            },
            wire::MAX_PREFACE,
        )
        .await
        .unwrap();
        wire::write(&mut download, &json!([]), wire::MAX_FRAME)
            .await
            .unwrap();
        download.finish().unwrap();
        let (header, mut tx, mut rx, download_slot) = receive(&server, &capacity).await;
        let request = header.body(&mut rx).await.unwrap();
        let download_task = tokio::spawn(async move {
            let _slot = download_slot;
            reply(
                &mut tx,
                &Message::Response {
                    id: request.id,
                    result: Ok(json!("y".repeat(2 * 1024 * 1024))),
                },
            )
            .await
            .unwrap();
        });
        let image_len = image.read_u32().await.unwrap() as usize;
        assert!(image_len > 512 * 1024);

        // Fill the ordinary admission pool as well. Urgent work must not depend
        // on a free ordinary slot, a completed upload or a drained reply.
        let _ordinary: Vec<_> = (0..30)
            .map(|_| capacity.acquire(Method::ListSessions).unwrap())
            .collect();
        assert!(capacity.acquire(Method::SendMessage).is_err());
        for (id, method) in [(3, Method::CancelRun), (4, Method::AnswerQuestion)] {
            let send = call(&client, command(id, method));
            let answer = async {
                let (header, mut tx, mut rx, _slot) = receive(&server, &capacity).await;
                assert!(header.method.urgent());
                let request = header.body(&mut rx).await.unwrap();
                reply(
                    &mut tx,
                    &Message::Response {
                        id: request.id,
                        result: Ok(json!("done")),
                    },
                )
                .await
                .unwrap();
            };
            let (response, ()) =
                tokio::time::timeout(Duration::from_secs(3), async { tokio::join!(send, answer) })
                    .await
                    .expect("urgent command blocked behind a transfer");
            assert!(
                matches!(response.unwrap(), Message::Response { id: received, result: Ok(value) } if received == id && value == json!("done"))
            );
        }
        assert_eq!(upload_received.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(!download_task.is_finished());

        // Completing the upload still admits exactly one original command.
        let finish_upload = async {
            upload.write_all(&body[1024..]).await.unwrap();
            upload.finish().unwrap();
            assert!(matches!(
                wire::read::<Message>(&mut upload_reply, wire::MAX_FRAME)
                    .await
                    .unwrap(),
                Message::Response {
                    id: 1,
                    result: Ok(_)
                }
            ));
        };
        let finish_download = async {
            let mut body = vec![0; image_len];
            image.read_exact(&mut body).await.unwrap();
            assert!(matches!(
                serde_json::from_slice::<Message>(&body).unwrap(),
                Message::Response {
                    id: 2,
                    result: Ok(_)
                }
            ));
        };
        tokio::join!(finish_upload, finish_download);
        upload_task.await.unwrap();
        download_task.await.unwrap();
        assert_eq!(upload_received.load(std::sync::atomic::Ordering::SeqCst), 1);
    };
    let result = tokio::time::timeout(Duration::from_secs(20), exercise).await;
    tokio::join!(a.close(), b.close());
    result.unwrap();
}

#[tokio::test]
async fn closing_connection_interrupts_incomplete_body_without_admission() {
    let (a, b, client, server) = endpoints().await;
    let exercise = async {
        let (mut tx, _rx) = client.open_bi().await.unwrap();
        wire::write(
            &mut tx,
            &Header {
                id: 1,
                method: Method::SendMessage,
                send_id: None,
            },
            wire::MAX_PREFACE,
        )
        .await
        .unwrap();
        tx.write_u32(4096).await.unwrap();
        tx.write_all(b"[").await.unwrap();
        let capacity = Capacity::new();
        let (header, _reply, mut body, _slot) = receive(&server, &capacity).await;
        // Used by revocation/account change and explicit connection teardown.
        server.close(0u32.into(), b"revoked");
        assert!(header.body(&mut body).await.is_err());
    };
    let result = tokio::time::timeout(Duration::from_secs(5), exercise).await;
    tokio::join!(a.close(), b.close());
    result.unwrap();
}

#[tokio::test]
async fn urgent_body_is_bounded_and_reply_identity_is_checked() {
    let (a, b, client, server) = endpoints().await;
    let exercise = async {
        let mut oversized = command(7, Method::AnswerQuestion);
        oversized.params = json!("x".repeat(MAX_CONTROL));
        assert!(matches!(
            call(&client, oversized).await.unwrap(),
            Message::Response {
                id: 7,
                result: Err(_)
            }
        ));
        // No stream was emitted for that rejected body: the first accepted
        // header below must be the next request.
        let send = call(&client, command(8, Method::CancelRun));
        let wrong_reply = async {
            let capacity = Capacity::new();
            let (header, mut tx, mut rx, _slot) = receive(&server, &capacity).await;
            assert_eq!(header.id, 8);
            header.body(&mut rx).await.unwrap();
            reply(
                &mut tx,
                &Message::Response {
                    id: 9,
                    result: Ok(json!(null)),
                },
            )
            .await
            .unwrap();
        };
        let (response, ()) = tokio::join!(send, wrong_reply);
        assert!(matches!(response.unwrap(), Message::Closed { .. }));
    };
    let result = tokio::time::timeout(Duration::from_secs(5), exercise).await;
    tokio::join!(a.close(), b.close());
    result.unwrap();
}

#[tokio::test]
async fn large_image_and_oversized_reply_do_not_break_other_requests() {
    let (a, b, client, server) = endpoints().await;
    let exercise = async {
        let bytes = vec![255_u8; 10 * 1024 * 1024];
        let image = encode_result(
            Method::ReadImageAttachment,
            Ok(serde_json::to_value(&bytes).unwrap()),
        )
        .unwrap();
        assert!(wire::encode(&image, wire::MAX_FRAME).is_ok());
        let cases = [
            (1, Method::ReadImageAttachment, image),
            (2, Method::ListSessions, json!("x".repeat(wire::MAX_FRAME))),
            (3, Method::CancelRun, json!("stopped")),
        ];
        for (id, method, value) in cases {
            let send = call(&client, command(id, method));
            let serve = async {
                let capacity = Capacity::new();
                let (header, mut tx, mut rx, _slot) = receive(&server, &capacity).await;
                header.body(&mut rx).await.unwrap();
                reply(
                    &mut tx,
                    &Message::Response {
                        id,
                        result: Ok(value),
                    },
                )
                .await
                .unwrap();
            };
            let (reply, ()) = tokio::join!(send, serve);
            let Message::Response { result, .. } = decode_result(method, reply.unwrap()) else {
                panic!("request closed the connection")
            };
            match id {
                1 => assert_eq!(
                    serde_json::from_value::<Vec<u8>>(result.unwrap()).unwrap(),
                    bytes
                ),
                2 => assert!(result.unwrap_err().contains("too large")),
                _ => assert_eq!(result.unwrap(), json!("stopped")),
            }
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(30), exercise).await;
    tokio::join!(a.close(), b.close());
    result.unwrap();
}
