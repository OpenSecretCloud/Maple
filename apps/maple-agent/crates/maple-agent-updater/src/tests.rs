use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use blake2::{Blake2b512, Digest};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use sha2::Sha256;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{
    Artifact, CheckOutcome, Config, Downloads, Manifest, Revalidation, Target, Version,
    client::test_updater,
    manifest::{self, verify_envelope},
};

const PAYLOAD: &[u8] = b"signed payload fixture bytes";

fn key(seed: u8) -> String {
    let signing = SigningKey::from_bytes(&[seed; 32]);
    let mut bytes = b"Ed12345678".to_vec();
    bytes.extend_from_slice(signing.verifying_key().as_bytes());
    STANDARD.encode(bytes)
}

fn sign_text(text: &str, comment: &str, seed: u8) -> Vec<u8> {
    // Deterministic test-only key, never an application pin or production key.
    let signing = SigningKey::from_bytes(&[seed; 32]);
    let message_signature = signing.sign(&Blake2b512::digest(text.as_bytes()));
    let mut packet = b"ED12345678".to_vec();
    packet.extend_from_slice(&message_signature.to_bytes());
    let mut global_message = message_signature.to_bytes().to_vec();
    global_message.extend_from_slice(comment.as_bytes());
    let global_signature = signing.sign(&global_message);
    serde_json::to_vec(&json!({
        "manifest": text,
        "signature": format!("untrusted comment: TEST ONLY\n{}\ntrusted comment: {comment}\n{}\n", STANDARD.encode(packet), STANDARD.encode(global_signature.to_bytes()))
    })).unwrap()
}

fn envelope(manifest: &Manifest) -> Vec<u8> {
    sign_text(
        &serde_json::to_string(manifest).unwrap(),
        &format!(
            "product=maple-agent channel=stable version={}",
            manifest.version
        ),
        7,
    )
}

fn artifact(version: &str, filename: &str) -> Artifact {
    Artifact {
        path: format!("/agent/stable/releases/{version}/{filename}"),
        size: PAYLOAD.len() as u64,
        sha256: hex::encode(Sha256::digest(PAYLOAD)),
    }
}

fn manifest(version: &str) -> Manifest {
    Manifest {
        format: "maple-agent-update-v1".into(),
        product: "maple-agent".into(),
        channel: "stable".into(),
        version: version.into(),
        build: "2".into(),
        source_sha: "a".repeat(40),
        pub_date: "2026-10-21T15:00:00Z".into(),
        notes: "A test update.\nPlain text.".into(),
        artifacts: BTreeMap::from([(
            "darwin-aarch64".into(),
            artifact(version, "Maple-Agent.app.tar.gz"),
        )]),
        downloads: Downloads {
            macos: artifact(version, "Maple-Agent.dmg"),
            linux_appimage: None,
        },
    }
}

fn origin() -> crate::Url {
    "https://updates.example.test".parse().unwrap()
}

#[test]
fn static_public_publisher_fixture_uses_the_same_authenticated_contract() {
    let release = verify_envelope(
        include_bytes!("../tests/fixtures/signed-envelope.json"),
        &origin(),
        &[include_str!("../tests/fixtures/public-test-key.pub").to_owned()],
    )
    .unwrap();
    assert_eq!(release.manifest, manifest("0.0.2"));
    assert_eq!(
        release.manifest.artifacts["darwin-aarch64"].sha256,
        hex::encode(Sha256::digest(PAYLOAD))
    );
}

#[test]
fn exact_bytes_and_authenticated_comment_are_required() {
    let manifest = manifest("0.0.2");
    let bytes = envelope(&manifest);
    assert_eq!(
        verify_envelope(&bytes, &origin(), &[key(7)])
            .unwrap()
            .manifest,
        manifest
    );
    let mut altered: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    altered["manifest"] = json!(format!("{} ", altered["manifest"].as_str().unwrap()));
    assert!(verify_envelope(&serde_json::to_vec(&altered).unwrap(), &origin(), &[key(7)]).is_err());
    let wrong_comment = sign_text(
        &serde_json::to_string(&manifest).unwrap(),
        "product=maple-agent channel=stable version=0.0.3",
        7,
    );
    assert!(verify_envelope(&wrong_comment, &origin(), &[key(7)]).is_err());
}

#[test]
fn primary_or_backup_key_must_verify_before_fields_are_parsed() {
    assert!(verify_envelope(&envelope(&manifest("0.0.2")), &origin(), &[key(8)]).is_err());
    assert!(verify_envelope(&envelope(&manifest("0.0.2")), &origin(), &[key(8), key(7)]).is_ok());
    let invalid = sign_text("this is not JSON", "irrelevant", 8);
    assert!(
        verify_envelope(&invalid, &origin(), &[key(7)])
            .unwrap_err()
            .to_string()
            .contains("pinned key")
    );
}

#[test]
fn schema_identity_and_supported_inventory_are_strict() {
    let mutations: Vec<fn(&mut Manifest)> = vec![
        |m| m.format = "future".into(),
        |m| m.product = "research".into(),
        |m| m.channel = "dev".into(),
        |m| m.version = "0.0.2-beta.1".into(),
        |m| m.version = "0.0.2+build".into(),
        |m| m.build = "02".into(),
        |m| m.build = "2.0".into(),
        |m| m.source_sha = "A".repeat(40),
        |m| m.pub_date = "2026-10-21T15:00:00+00:00".into(),
        |m| m.notes = "bad\u{0}".into(),
        |m| {
            m.artifacts.remove("darwin-aarch64");
        },
        |m| {
            m.artifacts
                .insert("windows-x86_64".into(), artifact("0.0.2", "x.exe"));
        },
        |m| m.downloads.linux_appimage = Some(artifact("0.0.2", "x.AppImage")),
    ];
    for mutation in mutations {
        let mut value = manifest("0.0.2");
        mutation(&mut value);
        assert!(
            verify_envelope(&envelope(&value), &origin(), &[key(7)]).is_err(),
            "accepted {value:?}"
        );
    }
}

#[test]
fn paths_sizes_and_digests_are_bound_to_exact_version_and_target() {
    for path in [
        "/agent/stable/releases/0.0.1/x.app.tar.gz",
        "//evil.test/x.app.tar.gz",
        "https://evil.test/x.app.tar.gz",
        "/agent/stable/releases/0.0.2/../x.app.tar.gz",
        "/agent/stable/releases/0.0.2/%2e%2e.app.tar.gz",
        "/agent/stable/releases/0.0.2/x.app.tar.gz?x",
        "/agent/stable/releases/0.0.2/x.app.tar.gz#x",
        "/agent/stable/releases/0.0.2/.hidden.app.tar.gz",
        "/agent/stable/releases/0.0.2/x.dmg",
    ] {
        let mut value = manifest("0.0.2");
        value.artifacts.get_mut("darwin-aarch64").unwrap().path = path.into();
        assert!(
            verify_envelope(&envelope(&value), &origin(), &[key(7)]).is_err(),
            "accepted {path}"
        );
    }
    for size in [0, manifest::MAX_ARTIFACT_BYTES + 1] {
        let mut value = manifest("0.0.2");
        value.artifacts.get_mut("darwin-aarch64").unwrap().size = size;
        assert!(verify_envelope(&envelope(&value), &origin(), &[key(7)]).is_err());
    }
    let mut value = manifest("0.0.2");
    value.artifacts.get_mut("darwin-aarch64").unwrap().sha256 = "A".repeat(64);
    assert!(verify_envelope(&envelope(&value), &origin(), &[key(7)]).is_err());
}

#[test]
fn oversized_unknown_and_duplicate_envelope_fields_are_rejected() {
    assert!(
        verify_envelope(
            &vec![b' '; manifest::MAX_ENVELOPE_BYTES + 1],
            &origin(),
            &[key(7)]
        )
        .is_err()
    );
    let mut value: serde_json::Value =
        serde_json::from_slice(&envelope(&manifest("0.0.2"))).unwrap();
    value["unknown"] = json!(true);
    assert!(verify_envelope(&serde_json::to_vec(&value).unwrap(), &origin(), &[key(7)]).is_err());
    assert!(
        verify_envelope(
            br#"{"manifest":"a","manifest":"b","signature":"x"}"#,
            &origin(),
            &[key(7)]
        )
        .is_err()
    );
}

struct Fixture {
    url: crate::Url,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn new(responses: Vec<Vec<u8>>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/agent/stable/latest.json",
            listener.local_addr().unwrap()
        )
        .parse()
        .unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let task = tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if stream.read(&mut byte).await.unwrap() == 0 {
                        break;
                    }
                    request.extend_from_slice(&byte);
                    assert!(request.len() < 8192);
                }
                observed
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap());
                stream.write_all(&response).await.unwrap();
                stream.shutdown().await.unwrap();
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }

    fn config(&self, temp: &tempfile::TempDir) -> Config {
        let installed = temp.path().join("Maple Agent.app");
        std::fs::create_dir_all(&installed).unwrap();
        Config {
            feed_url: self.url.clone(),
            public_keys: vec![key(7)],
            current_version: Version::new(0, 0, 1),
            target: Target::DarwinAarch64,
            installed_path: installed,
            state_dir: temp.path().join("updates"),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn response(status: &str, body: &[u8], headers: &str) -> Vec<u8> {
    let mut bytes = format!("HTTP/1.1 {status}\r\nConnection: close\r\n{headers}\r\n").into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn ok(body: &[u8]) -> Vec<u8> {
    response(
        "200 OK",
        body,
        &format!("Content-Length: {}\r\n", body.len()),
    )
}

async fn available(updater: &crate::Updater) -> crate::VerifiedRelease {
    let CheckOutcome::Available(release) = updater.check().await.unwrap() else {
        panic!("expected update")
    };
    *release
}

#[tokio::test]
async fn downloads_and_reauthenticates_preparation_across_restart() {
    let signed = envelope(&manifest("0.0.2"));
    let server = Fixture::new(vec![ok(&signed), ok(PAYLOAD), ok(&signed)]).await;
    let temp = tempfile::tempdir().unwrap();
    let updater = test_updater(server.config(&temp)).unwrap();
    let release = available(&updater).await;
    let prepared = updater.download(&release).await.unwrap();
    assert_eq!(std::fs::read(&prepared.payload_path).unwrap(), PAYLOAD);
    drop(updater);
    let updater = test_updater(server.config(&temp)).unwrap();
    let restored = updater.restore_prepared().await.unwrap().unwrap();
    assert!(matches!(
        updater.revalidate(&restored).await.unwrap(),
        Revalidation::Ready(_)
    ));
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| {
        request
            .to_ascii_lowercase()
            .contains("user-agent: maple-agent-updater/1")
    }));
    assert!(
        requests
            .iter()
            .all(|request| !request.to_ascii_lowercase().contains("authorization:"))
    );
}

#[tokio::test]
async fn rollback_clears_offer_without_lowering_persisted_high_watermark() {
    let server = Fixture::new(vec![
        ok(&envelope(&manifest("0.0.3"))),
        ok(PAYLOAD),
        ok(&envelope(&manifest("0.0.2"))),
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();
    let updater = test_updater(server.config(&temp)).unwrap();
    let prepared = updater.download(&available(&updater).await).await.unwrap();
    assert!(matches!(
        updater.revalidate(&prepared).await.unwrap(),
        Revalidation::Withdrawn
    ));
    assert!(updater.restore_prepared().await.unwrap().is_none());
    let record = std::fs::read_to_string(temp.path().join("updates/state.json")).unwrap();
    assert!(record.contains("\"highest_verified\":\"0.0.3\""));
}

#[tokio::test]
async fn newer_pointer_is_returned_immediately_to_schedule_download() {
    let server = Fixture::new(vec![
        ok(&envelope(&manifest("0.0.2"))),
        ok(PAYLOAD),
        ok(&envelope(&manifest("0.0.3"))),
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();
    let updater = test_updater(server.config(&temp)).unwrap();
    let prepared = updater.download(&available(&updater).await).await.unwrap();
    let Revalidation::Superseded(newer) = updater.revalidate(&prepared).await.unwrap() else {
        panic!("expected newer")
    };
    assert_eq!(newer.version(), Version::new(0, 0, 3));
    assert!(updater.restore_prepared().await.unwrap().is_none());
}

#[tokio::test]
async fn discovery_failure_keeps_preparation_for_retry() {
    let server = Fixture::new(vec![
        ok(&envelope(&manifest("0.0.2"))),
        ok(PAYLOAD),
        response("503 Unavailable", b"", "Content-Length: 0\r\n"),
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();
    let updater = test_updater(server.config(&temp)).unwrap();
    let prepared = updater.download(&available(&updater).await).await.unwrap();
    assert!(updater.revalidate(&prepared).await.is_err());
    assert!(updater.restore_prepared().await.unwrap().is_some());
}

#[tokio::test]
async fn tampered_payload_and_saved_destination_never_restore_ready() {
    let server = Fixture::new(vec![ok(&envelope(&manifest("0.0.2"))), ok(PAYLOAD)]).await;
    let temp = tempfile::tempdir().unwrap();
    let updater = test_updater(server.config(&temp)).unwrap();
    let prepared = updater.download(&available(&updater).await).await.unwrap();
    std::fs::write(&prepared.payload_path, vec![b'x'; PAYLOAD.len()]).unwrap();
    assert!(updater.restore_prepared().await.is_err());
    assert!(updater.restore_prepared().await.unwrap().is_none());
    let mut config = server.config(&temp);
    config.installed_path = temp.path().join("Other.app");
    std::fs::create_dir_all(&config.installed_path).unwrap();
    let moved = test_updater(config).unwrap();
    assert!(moved.restore_prepared().await.unwrap().is_none());
    let state = std::fs::read_to_string(temp.path().join("updates/state.json")).unwrap();
    assert!(state.contains("\"highest_verified\":\"0.0.2\""));
}

#[tokio::test]
async fn resumes_only_exact_valid_ranges_and_final_digest() {
    let signed = envelope(&manifest("0.0.2"));
    let server = Fixture::new(vec![
        ok(&signed),
        response(
            "206 Partial Content",
            &PAYLOAD[6..],
            &format!(
                "Content-Length: {}\r\nContent-Range: bytes 6-{}/{}\r\n",
                PAYLOAD.len() - 6,
                PAYLOAD.len() - 1,
                PAYLOAD.len()
            ),
        ),
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();
    let updater = test_updater(server.config(&temp)).unwrap();
    let release = available(&updater).await;
    let partial = temp.path().join("updates/payloads").join(format!(
        "{}.part",
        release.artifact(Target::DarwinAarch64).unwrap().sha256
    ));
    std::fs::write(partial, &PAYLOAD[..6]).unwrap();
    updater.download(&release).await.unwrap();
    assert!(server.requests.lock().unwrap()[1].contains("range: bytes=6-"));
}

#[tokio::test]
async fn ignored_range_restarts_instead_of_appending() {
    let signed = envelope(&manifest("0.0.2"));
    let server = Fixture::new(vec![ok(&signed), ok(PAYLOAD)]).await;
    let temp = tempfile::tempdir().unwrap();
    let updater = test_updater(server.config(&temp)).unwrap();
    let release = available(&updater).await;
    let partial = temp.path().join("updates/payloads").join(format!(
        "{}.part",
        release.artifact(Target::DarwinAarch64).unwrap().sha256
    ));
    std::fs::write(partial, &PAYLOAD[..6]).unwrap();
    assert_eq!(
        std::fs::read(updater.download(&release).await.unwrap().payload_path).unwrap(),
        PAYLOAD
    );
}

#[tokio::test]
async fn interrupted_transfer_preserves_a_resumable_verified_prefix() {
    let signed = envelope(&manifest("0.0.2"));
    let server = Fixture::new(vec![
        ok(&signed),
        response(
            "200 OK",
            &PAYLOAD[..6],
            &format!("Content-Length: {}\r\n", PAYLOAD.len()),
        ),
        response(
            "206 Partial Content",
            &PAYLOAD[6..],
            &format!(
                "Content-Length: {}\r\nContent-Range: bytes 6-{}/{}\r\n",
                PAYLOAD.len() - 6,
                PAYLOAD.len() - 1,
                PAYLOAD.len()
            ),
        ),
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();
    let updater = test_updater(server.config(&temp)).unwrap();
    let release = available(&updater).await;
    assert!(updater.download(&release).await.is_err());
    let prepared = updater.download(&release).await.unwrap();
    assert_eq!(std::fs::read(prepared.payload_path).unwrap(), PAYLOAD);
    assert!(server.requests.lock().unwrap()[2].contains("range: bytes=6-"));
}

#[tokio::test]
async fn invalid_range_oversized_and_truncated_payloads_fail_closed() {
    let bad_responses = vec![
        response(
            "206 Partial Content",
            &PAYLOAD[6..],
            &format!(
                "Content-Range: bytes 0-{}/{}\r\n",
                PAYLOAD.len() - 1,
                PAYLOAD.len()
            ),
        ),
        response("200 OK", &[PAYLOAD, b"overrun"].concat(), ""),
        ok(&PAYLOAD[..6]),
        ok(&vec![b'x'; PAYLOAD.len()]),
    ];
    for bad in bad_responses {
        let server = Fixture::new(vec![ok(&envelope(&manifest("0.0.2"))), bad]).await;
        let temp = tempfile::tempdir().unwrap();
        let updater = test_updater(server.config(&temp)).unwrap();
        let release = available(&updater).await;
        assert!(updater.download(&release).await.is_err());
        assert!(updater.restore_prepared().await.unwrap().is_none());
    }
}

#[tokio::test]
async fn production_constructor_and_redirects_reject_unsafe_urls() {
    let temp = tempfile::tempdir().unwrap();
    let server = Fixture::new(vec![response(
        "302 Found",
        b"",
        "Location: https://evil.test/payload\r\nContent-Length: 0\r\n",
    )])
    .await;
    assert!(crate::Updater::new(server.config(&temp)).is_err());
    let updater = test_updater(server.config(&temp)).unwrap();
    assert!(updater.check().await.is_err());
    for url in [
        "https://name:password@example.test/feed",
        "https://example.test/feed?token=x",
        "https://example.test/feed#x",
    ] {
        let mut config = server.config(&temp);
        config.feed_url = url.parse().unwrap();
        assert!(crate::Updater::new(config).is_err());
    }
}

#[tokio::test]
async fn redirect_loops_and_oversized_metadata_are_bounded() {
    let server = Fixture::new(
        (0..5)
            .map(|_| {
                response(
                    "302 Found",
                    b"",
                    "Location: /again\r\nContent-Length: 0\r\n",
                )
            })
            .collect(),
    )
    .await;
    let temp = tempfile::tempdir().unwrap();
    assert!(
        test_updater(server.config(&temp))
            .unwrap()
            .check()
            .await
            .is_err()
    );
    assert_eq!(server.requests.lock().unwrap().len(), 5);
    let server = Fixture::new(vec![response(
        "200 OK",
        b"",
        "Content-Length: 999999999\r\n",
    )])
    .await;
    assert!(
        test_updater(server.config(&temp))
            .unwrap()
            .check()
            .await
            .is_err()
    );
}

#[tokio::test]
async fn absent_linux_does_not_invalidate_macos_and_offers_no_linux_update() {
    let signed = envelope(&manifest("0.0.2"));
    assert!(verify_envelope(&signed, &origin(), &[key(7)]).is_ok());
    let server = Fixture::new(vec![ok(&signed)]).await;
    let temp = tempfile::tempdir().unwrap();
    let mut config = server.config(&temp);
    config.target = Target::LinuxX86_64;
    assert!(matches!(
        test_updater(config).unwrap().check().await.unwrap(),
        CheckOutcome::Current
    ));
}

#[test]
fn only_the_two_compiled_origins_allow_cross_origin_migration() {
    let allowed = |from: &str, to: &str| {
        crate::client::origin_allowed(&from.parse().unwrap(), &to.parse().unwrap())
    };
    assert!(allowed(
        "https://updates.trymaple.ai/agent/stable/latest.json",
        "https://updates.maple.ai/agent/stable/latest.json"
    ));
    assert!(allowed(
        "https://updates.maple.ai/agent/stable/latest.json",
        "https://updates.trymaple.ai/agent/stable/latest.json"
    ));
    for target in [
        "https://other.maple.ai/file",
        "https://evil.test/file",
        "https://updates.maple.ai:444/file",
        "http://updates.maple.ai/file",
    ] {
        assert!(!allowed("https://updates.trymaple.ai/feed", target));
    }
    assert!(allowed(
        "https://example.test:444/feed",
        "https://example.test:444/file"
    ));
    assert!(!allowed(
        "https://example.test:444/feed",
        "https://example.test/file"
    ));
    assert!(!allowed(
        "https://example.test/feed",
        "https://updates.maple.ai/file"
    ));
}

#[tokio::test]
async fn moved_installation_discards_preparation_and_retains_rollback_memory() {
    let server = Fixture::new(vec![
        ok(&envelope(&manifest("0.0.3"))),
        ok(PAYLOAD),
        ok(&envelope(&manifest("0.0.2"))),
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();
    let updater = test_updater(server.config(&temp)).unwrap();
    updater.download(&available(&updater).await).await.unwrap();
    drop(updater);
    let mut config = server.config(&temp);
    config.installed_path = temp.path().join("Moved Agent.app");
    std::fs::create_dir_all(&config.installed_path).unwrap();
    let updater = test_updater(config).unwrap();
    assert!(updater.restore_prepared().await.unwrap().is_none());
    assert!(matches!(
        updater.check().await.unwrap(),
        CheckOutcome::Current
    ));
    let record = std::fs::read_to_string(temp.path().join("updates/state.json")).unwrap();
    assert!(record.contains("\"highest_verified\":\"0.0.3\""));
    assert!(record.contains("Moved Agent.app"));
}

#[tokio::test]
async fn newer_process_verification_between_ready_and_commit_blocks_old_installation() {
    let signed = envelope(&manifest("0.0.2"));
    let server = Fixture::new(vec![
        ok(&signed),
        ok(PAYLOAD),
        ok(&signed),
        ok(&envelope(&manifest("0.0.3"))),
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();
    let first = test_updater(server.config(&temp)).unwrap();
    let second = test_updater(server.config(&temp)).unwrap();
    let prepared = first.download(&available(&first).await).await.unwrap();
    assert!(matches!(
        first.revalidate(&prepared).await.unwrap(),
        Revalidation::Ready(_)
    ));
    assert_eq!(available(&second).await.version(), Version::new(0, 0, 3));
    let installed = std::cell::Cell::new(false);
    assert!(
        first
            .with_commit_guard(&prepared, || {
                installed.set(true);
                Ok(())
            })
            .is_err()
    );
    assert!(!installed.get());
}

#[tokio::test]
async fn commit_guard_excludes_competing_advancement_without_blocking() {
    let signed = envelope(&manifest("0.0.2"));
    let server = Fixture::new(vec![ok(&signed), ok(PAYLOAD)]).await;
    let temp = tempfile::tempdir().unwrap();
    let updater = test_updater(server.config(&temp)).unwrap();
    let prepared = updater.download(&available(&updater).await).await.unwrap();
    let competing =
        crate::state::StateStore::new(&temp.path().join("updates"), &prepared.installed_path)
            .unwrap();
    updater
        .with_commit_guard(&prepared, || {
            assert!(competing.advance(&Version::new(0, 0, 3)).is_err());
            Ok(())
        })
        .unwrap();
    competing.advance(&Version::new(0, 0, 3)).unwrap();
    assert!(updater.with_commit_guard(&prepared, || Ok(())).is_err());
}

#[tokio::test]
async fn successful_commit_result_survives_loss_of_state_directory_after_replacement() {
    let server = Fixture::new(vec![ok(&envelope(&manifest("0.0.2"))), ok(PAYLOAD)]).await;
    let temp = tempfile::tempdir().unwrap();
    let updater = test_updater(server.config(&temp)).unwrap();
    let prepared = updater.download(&available(&updater).await).await.unwrap();
    let result = updater
        .with_commit_guard(&prepared, || {
            // Model an unrelated filesystem failure after replacement. Returning
            // the committed outcome cannot depend on any later state-directory IO.
            std::fs::remove_dir_all(temp.path().join("updates")).map_err(|e| e.to_string())?;
            Ok("committed")
        })
        .unwrap();
    assert_eq!(result, "committed");
}

#[cfg(unix)]
#[tokio::test]
async fn saved_payload_symlinks_are_never_followed() {
    use std::os::unix::fs::symlink;
    let server = Fixture::new(vec![ok(&envelope(&manifest("0.0.2"))), ok(PAYLOAD)]).await;
    let temp = tempfile::tempdir().unwrap();
    let updater = test_updater(server.config(&temp)).unwrap();
    let prepared = updater.download(&available(&updater).await).await.unwrap();
    let outside: PathBuf = temp.path().join("outside");
    std::fs::write(&outside, PAYLOAD).unwrap();
    std::fs::remove_file(&prepared.payload_path).unwrap();
    symlink(&outside, &prepared.payload_path).unwrap();
    assert!(updater.restore_prepared().await.is_err());
    assert_eq!(std::fs::read(outside).unwrap(), PAYLOAD);
}
