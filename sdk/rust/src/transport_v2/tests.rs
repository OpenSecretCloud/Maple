use std::{
    sync::{Arc, Barrier},
    thread,
};

use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};

use super::{
    crypto::{
        decode_canonical_base64, decrypt_key_exchange_record, derive_handshake_key_for_test,
        encode_canonical_base64, encrypt_key_exchange_record_for_test,
        encrypt_key_exchange_record_with_nonce, request_record_aad, stream_response_record_aad,
        unary_response_record_aad, DirectionalKeys, SessionMaster, MIN_RECORD_LEN,
    },
    envelope::{
        encode_canonical_opaque_path_segment, CacheNamespaceRoot, Credential, EncodedBytes,
        EnvelopeLimits, HeaderField, LogicalMethod, LogicalRequest, RequestEnvelope, RequestId,
        ResponseMode, StreamRecord, UnaryResponseEnvelope, Version2, MAX_OUTER_REQUEST_BYTES,
        MAX_OUTER_RESPONSE_BYTES, MAX_STREAM_CHUNK_BYTES,
    },
    session::{KeyExchangeCompletion, PreparedKeyExchange, V2Session},
    stream::{max_stream_carrier_frame_bytes_for_test, StreamDecoder, StreamEvent},
    TransportV2Error,
};

#[derive(Deserialize)]
struct GoldenVectors {
    fixture_version: u8,
    protocol_version: u8,
    shared_secret_hex: String,
    session_master_hex: String,
    session_id: String,
    session_id_hex: String,
    expires_at_unix_seconds: u64,
    request_id_hex: String,
    stream_sequence: u64,
    handshake: HandshakeVector,
    request: DirectionalVector,
    unary_response: DirectionalVector,
    stream_response: RecordVector,
    request_without_body_json: String,
    request_with_empty_body_json: String,
}

#[derive(Deserialize)]
struct HandshakeVector {
    info_utf8: String,
    derived_key_hex: String,
    aad_hex: String,
    nonce_hex: String,
    plaintext_hex: String,
    record_hex: String,
    record_base64: String,
}

#[derive(Deserialize)]
struct DirectionalVector {
    info_utf8: String,
    derived_key_hex: String,
    aad_hex: String,
    nonce_hex: String,
    plaintext_utf8: String,
    plaintext_hex: String,
    record_hex: String,
    record_base64: String,
}

#[derive(Deserialize)]
struct RecordVector {
    aad_hex: String,
    plaintext_utf8: String,
    plaintext_hex: String,
    record_hex: String,
    record_base64: String,
}

fn vectors() -> GoldenVectors {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/testdata/transport-v2-golden-vectors.json"
    )))
    .expect("shared transport-v2 fixture")
}

#[test]
fn package_fixture_matches_the_shared_sdk_fixture() {
    let package_fixture = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/testdata/transport-v2-golden-vectors.json"
    ));
    let shared_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/transport-v2-golden-vectors.json");

    // The shared cross-language fixture is intentionally outside the Cargo
    // package. Compare it byte-for-byte in the repository while keeping the
    // published crate independently testable when that parent file is absent.
    if let Ok(shared_fixture) = std::fs::read(shared_path) {
        assert_eq!(package_fixture.as_slice(), shared_fixture.as_slice());
    }
}

fn fixed_hex<const N: usize>(encoded: &str) -> [u8; N] {
    hex::decode(encoded)
        .expect("fixture hex")
        .try_into()
        .expect("fixture fixed length")
}

fn assert_record(record: &[u8], expected_hex: &str, expected_base64: &str) {
    assert_eq!(
        record,
        hex::decode(expected_hex).expect("fixture record hex")
    );
    assert_eq!(encode_canonical_base64(record), expected_base64);
    assert_eq!(
        decode_canonical_base64(expected_base64, record.len()).expect("canonical fixture base64"),
        record
    );
}

#[test]
fn shared_golden_vectors_fix_all_keys_aad_and_records() {
    let fixture = vectors();
    assert_eq!(fixture.fixture_version, 1);
    assert_eq!(fixture.protocol_version, Version2::VALUE);
    assert_eq!(
        fixture.handshake.info_utf8,
        "opensecret/transport-v2/handshake-key"
    );
    assert_eq!(
        fixture.request.info_utf8,
        "opensecret/transport-v2/client-request"
    );
    assert_eq!(
        fixture.unary_response.info_utf8,
        "opensecret/transport-v2/enclave-response"
    );

    let shared_secret = fixed_hex::<32>(&fixture.shared_secret_hex);
    let session_master_bytes = fixed_hex::<32>(&fixture.session_master_hex);
    let session_id = Uuid::parse_str(&fixture.session_id).expect("fixture session ID");
    assert_eq!(
        session_id.as_bytes(),
        &fixed_hex::<16>(&fixture.session_id_hex)
    );
    let request_id = RequestId::from_bytes(fixed_hex(&fixture.request_id_hex));

    assert_eq!(
        derive_handshake_key_for_test(&shared_secret).expect("handshake key"),
        fixed_hex::<32>(&fixture.handshake.derived_key_hex)
    );
    assert_eq!(
        b"opensecret/transport-v2/key-exchange",
        hex::decode(&fixture.handshake.aad_hex)
            .expect("fixture handshake AAD")
            .as_slice()
    );
    let handshake_plaintext = hex::decode(&fixture.handshake.plaintext_hex).expect("handshake");
    let handshake_record = encrypt_key_exchange_record_with_nonce(
        &shared_secret,
        &handshake_plaintext,
        fixed_hex(&fixture.handshake.nonce_hex),
    )
    .expect("encrypt fixture handshake");
    assert_record(
        &handshake_record,
        &fixture.handshake.record_hex,
        &fixture.handshake.record_base64,
    );
    let handshake =
        decrypt_key_exchange_record(&shared_secret, &handshake_record).expect("decrypt handshake");
    assert_eq!(handshake.session_id, session_id);
    assert_eq!(
        handshake.expires_at_unix_seconds,
        fixture.expires_at_unix_seconds
    );

    let master = SessionMaster::from_bytes(session_master_bytes);
    let keys = DirectionalKeys::derive(&master).expect("directional keys");
    assert_eq!(
        keys.request_key_bytes(),
        &fixed_hex::<32>(&fixture.request.derived_key_hex)
    );
    assert_eq!(
        keys.response_key_bytes(),
        &fixed_hex::<32>(&fixture.unary_response.derived_key_hex)
    );

    assert_eq!(
        request_record_aad(&session_id),
        hex::decode(&fixture.request.aad_hex).expect("request AAD")
    );
    assert_eq!(
        fixture.request.plaintext_utf8.as_bytes(),
        hex::decode(&fixture.request.plaintext_hex)
            .expect("request plaintext")
            .as_slice()
    );
    let request_record = keys
        .encrypt_request_record_with_nonce(
            &session_id,
            fixture.request.plaintext_utf8.as_bytes(),
            fixed_hex(&fixture.request.nonce_hex),
        )
        .expect("request record");
    assert_record(
        &request_record,
        &fixture.request.record_hex,
        &fixture.request.record_base64,
    );

    assert_eq!(
        unary_response_record_aad(&session_id, &request_id),
        hex::decode(&fixture.unary_response.aad_hex).expect("unary AAD")
    );
    let unary_record = hex::decode(&fixture.unary_response.record_hex).expect("unary record");
    assert_record(
        &unary_record,
        &fixture.unary_response.record_hex,
        &fixture.unary_response.record_base64,
    );
    let unary_plaintext = keys
        .decrypt_unary_response_record(&session_id, &request_id, &unary_record)
        .expect("unary response");
    assert_eq!(
        unary_plaintext,
        fixture.unary_response.plaintext_utf8.as_bytes()
    );
    assert_eq!(
        unary_plaintext,
        hex::decode(&fixture.unary_response.plaintext_hex).expect("unary plaintext")
    );

    assert_eq!(
        stream_response_record_aad(&session_id, &request_id, fixture.stream_sequence),
        hex::decode(&fixture.stream_response.aad_hex).expect("stream AAD")
    );
    let stream_record = hex::decode(&fixture.stream_response.record_hex).expect("stream record");
    assert_record(
        &stream_record,
        &fixture.stream_response.record_hex,
        &fixture.stream_response.record_base64,
    );
    let stream_plaintext = keys
        .decrypt_stream_response_record(
            &session_id,
            &request_id,
            fixture.stream_sequence,
            &stream_record,
        )
        .expect("stream response");
    assert_eq!(
        stream_plaintext,
        fixture.stream_response.plaintext_utf8.as_bytes()
    );
    assert_eq!(
        stream_plaintext,
        hex::decode(&fixture.stream_response.plaintext_hex).expect("stream plaintext")
    );
}

#[test]
fn records_fail_closed_for_wrong_direction_or_binding() {
    let fixture = vectors();
    let master = SessionMaster::from_bytes(fixed_hex(&fixture.session_master_hex));
    let keys = DirectionalKeys::derive(&master).expect("keys");
    let session_id = Uuid::parse_str(&fixture.session_id).expect("session ID");
    let request_id = RequestId::from_bytes(fixed_hex(&fixture.request_id_hex));
    let other_session = Uuid::from_bytes([0x55; 16]);
    let other_request = RequestId::from_bytes([0x66; 16]);

    let request_record = hex::decode(&fixture.request.record_hex).expect("request record");
    assert_eq!(
        keys.decrypt_unary_response_record(&session_id, &request_id, &request_record),
        Err(TransportV2Error::AuthenticationFailed)
    );

    let unary_record = hex::decode(&fixture.unary_response.record_hex).expect("unary record");
    assert_eq!(
        keys.decrypt_unary_response_record(&other_session, &request_id, &unary_record),
        Err(TransportV2Error::AuthenticationFailed)
    );
    assert_eq!(
        keys.decrypt_unary_response_record(&session_id, &other_request, &unary_record),
        Err(TransportV2Error::AuthenticationFailed)
    );

    let stream_record = hex::decode(&fixture.stream_response.record_hex).expect("stream record");
    assert_eq!(
        keys.decrypt_stream_response_record(
            &session_id,
            &request_id,
            fixture.stream_sequence + 1,
            &stream_record,
        ),
        Err(TransportV2Error::AuthenticationFailed)
    );

    let mut tampered = stream_record;
    *tampered.last_mut().expect("tag") ^= 1;
    assert_eq!(
        keys.decrypt_stream_response_record(
            &session_id,
            &request_id,
            fixture.stream_sequence,
            &tampered,
        ),
        Err(TransportV2Error::AuthenticationFailed)
    );
    assert_eq!(
        keys.decrypt_unary_response_record(&session_id, &request_id, &[0; MIN_RECORD_LEN - 1]),
        Err(TransportV2Error::RecordTooShort)
    );
    assert_eq!(
        decrypt_key_exchange_record(
            &[0; 32],
            &hex::decode(&fixture.handshake.record_hex).expect("handshake record"),
        )
        .expect_err("all-zero shared secret"),
        TransportV2Error::NonContributoryKeyExchange
    );
}

#[test]
fn shared_envelope_vectors_preserve_canonical_json_and_body_presence() {
    let fixture = vectors();
    let without_body = RequestEnvelope::from_json_slice(
        fixture.request_without_body_json.as_bytes(),
        &EnvelopeLimits::DEFAULT,
    )
    .expect("request without body");
    assert_eq!(without_body.response_mode, ResponseMode::Unary);
    assert_eq!(without_body.request.method, LogicalMethod::Get);
    assert!(without_body.request.body_base64.is_none());
    assert_eq!(
        without_body
            .to_json_vec(&EnvelopeLimits::DEFAULT)
            .expect("serialize request"),
        fixture.request_without_body_json.as_bytes()
    );

    let with_empty_body = RequestEnvelope::from_json_slice(
        fixture.request_with_empty_body_json.as_bytes(),
        &EnvelopeLimits::DEFAULT,
    )
    .expect("request with empty body");
    assert_eq!(with_empty_body.request.method, LogicalMethod::Post);
    assert!(with_empty_body
        .request
        .body_base64
        .as_ref()
        .expect("present body")
        .is_empty());
    assert_eq!(
        with_empty_body
            .to_json_vec(&EnvelopeLimits::DEFAULT)
            .expect("serialize request"),
        fixture.request_with_empty_body_json.as_bytes()
    );
}

#[test]
fn credentials_and_cache_root_use_exact_non_null_wire_shapes() {
    let request_id = RequestId::from_bytes([0x31; 16]);
    let cache_root = [0x32; 32];
    let cache_root_base64 = encode_canonical_base64(&cache_root);

    for (credential, kind, value) in [
        (
            Credential::api_key(b"api-key".to_vec()),
            "api_key",
            b"api-key".as_slice(),
        ),
        (
            Credential::resumption(b"resumption-token".to_vec()),
            "resumption",
            b"resumption-token".as_slice(),
        ),
    ] {
        let envelope = RequestEnvelope {
            version: Version2,
            request_id,
            response_mode: ResponseMode::Unary,
            credential: Some(credential),
            cache_namespace_root_base64: Some(CacheNamespaceRoot::from_bytes(cache_root)),
            request: test_request(ResponseMode::Unary),
        };
        let encoded = envelope
            .to_json_vec(&EnvelopeLimits::DEFAULT)
            .expect("envelope");
        let value_json: serde_json::Value = serde_json::from_slice(&encoded).expect("JSON");
        assert_eq!(value_json["credential"]["kind"], kind);
        assert_eq!(
            value_json["credential"]["value_base64"],
            encode_canonical_base64(value)
        );
        assert_eq!(value_json["cache_namespace_root_base64"], cache_root_base64);
        RequestEnvelope::from_json_slice(&encoded, &EnvelopeLimits::DEFAULT)
            .expect("strict round trip");
    }
}

#[test]
fn unary_carrier_limits_admit_exact_logical_boundaries_and_reject_one_byte_more() {
    let request_limit = EnvelopeLimits::DEFAULT.logical_body_bytes;
    let session_id = Uuid::nil();
    let master_bytes = [0x33; 32];
    let request_id = RequestId::from_bytes([0x34; 16]);
    let session =
        V2Session::from_master_for_test(session_id, master_bytes, u64::MAX).expect("session");

    let prepared = session
        .prepare_request_for_test(
            (0, request_id),
            ResponseMode::Unary,
            None,
            None,
            LogicalRequest::new(
                LogicalMethod::Post,
                "/v1/chat/completions",
                None,
                vec![],
                Some(vec![0_u8; request_limit]),
            ),
        )
        .expect("exact 50 MiB logical request body");
    let (outer_request, _) = prepared.into_parts();
    assert!(outer_request.len() <= MAX_OUTER_REQUEST_BYTES);
    assert!(outer_request.len() >= MIN_RECORD_LEN);
    drop(outer_request);

    assert_eq!(
        LogicalRequest::new(
            LogicalMethod::Post,
            "/v1/chat/completions",
            None,
            vec![],
            Some(vec![0_u8; request_limit + 1]),
        )
        .validate(&EnvelopeLimits::DEFAULT)
        .expect_err("request body over 50 MiB"),
        TransportV2Error::LimitExceeded {
            field: "logical body",
            limit: request_limit,
        }
    );

    let response_limit = EnvelopeLimits::RESPONSE.logical_body_bytes;
    let response = UnaryResponseEnvelope {
        version: Version2,
        request_id,
        status: 200,
        headers: vec![],
        body_base64: Some(EncodedBytes::from_bytes(vec![0_u8; response_limit])),
    };
    response
        .validate(&EnvelopeLimits::RESPONSE)
        .expect("exact 28 MiB logical response body");
    let plaintext = serde_json::to_vec(&response).expect("response JSON");
    assert!(plaintext.len() <= EnvelopeLimits::RESPONSE.envelope_bytes);
    let keys = DirectionalKeys::derive(&SessionMaster::from_bytes(master_bytes)).expect("keys");
    let outer_response = keys
        .encrypt_unary_response_record_for_test(&session_id, &request_id, &plaintext)
        .expect("raw response record");
    assert_eq!(outer_response.len(), plaintext.len() + MIN_RECORD_LEN);
    assert!(outer_response.len() <= MAX_OUTER_RESPONSE_BYTES);
    drop(outer_response);
    drop(plaintext);
    drop(response);

    assert_eq!(
        UnaryResponseEnvelope {
            version: Version2,
            request_id,
            status: 200,
            headers: vec![],
            body_base64: Some(EncodedBytes::from_bytes(vec![0_u8; response_limit + 1])),
        }
        .validate(&EnvelopeLimits::RESPONSE)
        .expect_err("response body over 28 MiB"),
        TransportV2Error::LimitExceeded {
            field: "logical body",
            limit: response_limit,
        }
    );
}

#[test]
fn opaque_item_paths_preserve_released_rust_sdk_encoding_and_are_method_aware() {
    for (value, encoded) in [
        ("simple123", "simple123"),
        ("key/part", "key%2Fpart"),
        ("key\\part", "key%5Cpart"),
        (".", "%2E"),
        ("..", "%2E%2E"),
        ("%2F", "%252F"),
        ("café", "caf%C3%A9"),
        ("🔐", "%F0%9F%94%90"),
    ] {
        assert_eq!(encode_canonical_opaque_path_segment(value), encoded);
        for method in [
            LogicalMethod::Get,
            LogicalMethod::Put,
            LogicalMethod::Delete,
        ] {
            LogicalRequest::new(
                method,
                format!("/protected/kv/{encoded}"),
                None,
                vec![],
                None,
            )
            .validate(&EnvelopeLimits::DEFAULT)
            .expect("canonical KV item path");
        }
    }

    LogicalRequest::new(
        LogicalMethod::Delete,
        "/protected/api-keys/Production%20Key",
        None,
        vec![],
        None,
    )
    .validate(&EnvelopeLimits::DEFAULT)
    .expect("canonical API-key name");

    for (method, path) in [
        (LogicalMethod::Post, "/protected/kv/key%2Fpart"),
        (LogicalMethod::Get, "/protected/api-keys/name%2Fpart"),
        (LogicalMethod::Get, "/protected/kv/%2f"),
        (LogicalMethod::Put, "/protected/kv/%41"),
        (LogicalMethod::Delete, "/protected/kv/raw_punctuation"),
        (LogicalMethod::Delete, "/protected/api-keys/literal-hyphen"),
        (LogicalMethod::Delete, "/protected/api-keys/%20leading"),
        (LogicalMethod::Delete, "/protected/api-keys/caf%C3%A9"),
    ] {
        assert!(
            LogicalRequest::new(method, path, None, vec![], None)
                .validate(&EnvelopeLimits::DEFAULT)
                .is_err(),
            "accepted noncanonical opaque path {method:?} {path}"
        );
    }
}

#[test]
fn dynamic_uuid_routes_require_one_lowercase_hyphenated_spelling() {
    let id = "00112233-4455-6677-8899-aabbccddeeff";
    let other = "10112233-4455-6677-8899-aabbccddeeff";
    for (method, path) in [
        (LogicalMethod::Get, format!("/verify-email/{id}")),
        (LogicalMethod::Get, format!("/platform/verify-email/{id}")),
        (LogicalMethod::Post, format!("/platform/accept_invite/{id}")),
        (LogicalMethod::Delete, format!("/platform/orgs/{id}")),
        (
            LogicalMethod::Get,
            format!("/platform/orgs/{id}/projects/{other}"),
        ),
        (
            LogicalMethod::Delete,
            format!("/platform/orgs/{id}/projects/{other}/secrets/key_name"),
        ),
        (
            LogicalMethod::Get,
            format!("/v1/conversation-projects/{id}"),
        ),
        (LogicalMethod::Get, format!("/v1/conversations/{id}")),
        (LogicalMethod::Get, format!("/v1/conversations/{id}/items")),
        (
            LogicalMethod::Get,
            format!("/v1/conversations/{id}/items/{other}"),
        ),
        (
            LogicalMethod::Post,
            format!("/v1/instructions/{id}/set-default"),
        ),
        (LogicalMethod::Get, format!("/v1/responses/{id}")),
        (LogicalMethod::Post, format!("/v1/responses/{id}/cancel")),
    ] {
        LogicalRequest::new(method, path, None, vec![], None)
            .validate(&EnvelopeLimits::DEFAULT)
            .expect("canonical dynamic route");
    }

    let uppercase = "00112233-4455-6677-8899-AABBCCDDEEFF";
    let simple = "00112233445566778899aabbccddeeff";
    for (method, path) in [
        (LogicalMethod::Get, format!("/verify-email/{uppercase}")),
        (
            LogicalMethod::Get,
            format!("/platform/verify-email/{simple}"),
        ),
        (
            LogicalMethod::Post,
            format!("/platform/accept_invite/{uppercase}"),
        ),
        (LogicalMethod::Delete, format!("/platform/orgs/{simple}")),
        (
            LogicalMethod::Get,
            format!("/platform/orgs/{id}/projects/{uppercase}"),
        ),
        (
            LogicalMethod::Delete,
            format!("/platform/orgs/{id}/projects/{other}/secrets/bad-name"),
        ),
        (
            LogicalMethod::Get,
            format!("/v1/conversation-projects/{simple}"),
        ),
        (LogicalMethod::Get, format!("/v1/conversations/{uppercase}")),
        (
            LogicalMethod::Get,
            format!("/v1/conversations/{id}/items/{simple}"),
        ),
        (
            LogicalMethod::Post,
            format!("/v1/instructions/{uppercase}/set-default"),
        ),
        (LogicalMethod::Get, format!("/v1/responses/{simple}")),
        (
            LogicalMethod::Post,
            format!("/v1/responses/{uppercase}/cancel"),
        ),
    ] {
        assert!(
            LogicalRequest::new(method, path, None, vec![], None)
                .validate(&EnvelopeLimits::DEFAULT)
                .is_err(),
            "accepted noncanonical dynamic route {method:?}"
        );
    }
}

#[test]
fn envelopes_reject_noncanonical_or_structurally_ambiguous_input() {
    let fixture = vectors();
    let base = fixture.request_without_body_json;
    for invalid in [
        base.replace("\"version\":2,", "\"version\":2,\"version\":2,"),
        base.replace("\"query\":\"limit=10\",", ""),
        base.replace(
            "\"body_base64\":null",
            "\"body_base64\":null,\"extra\":true",
        ),
        base.replace("YmV0YQ==", "YmV0YQ"),
        base.replace("YmV0YQ==", "YmV0YR=="),
        base.replace("/v1/models", "/v1/%2e%2E/models"),
        base.replace("limit=10", "?limit=10"),
        base.replace("x-provider-beta", "X-Provider-Beta"),
    ] {
        assert!(
            RequestEnvelope::from_json_slice(invalid.as_bytes(), &EnvelopeLimits::DEFAULT).is_err(),
            "accepted malformed request"
        );
    }

    let auto = base.replace("\"response_mode\":\"unary\"", "\"response_mode\":\"auto\"");
    assert_eq!(
        RequestEnvelope::from_json_slice(auto.as_bytes(), &EnvelopeLimits::DEFAULT)
            .expect("codec retains reserved mode")
            .response_mode,
        ResponseMode::Auto
    );
}

#[test]
fn key_exchange_is_one_shot_strict_and_binds_inner_and_outer_session_ids() {
    let nonce = "attestation-nonce".to_string();
    let client_secret_bytes = [0x11; 32];
    let enclave_secret = StaticSecret::from([0x22; 32]);
    let enclave_public = PublicKey::from(&enclave_secret);
    let client_public = PublicKey::from(&StaticSecret::from(client_secret_bytes));
    let shared = enclave_secret.diffie_hellman(&client_public);
    assert!(shared.was_contributory());

    let prepared = PreparedKeyExchange::new(nonce.clone(), *enclave_public.as_bytes())
        .expect("prepared exchange");
    let (request_body, _) = prepared.into_parts();
    let request_json: serde_json::Value =
        serde_json::from_slice(&request_body).expect("request JSON");
    assert_eq!(request_json["nonce"], nonce);
    let client_key = request_json["client_public_key"]
        .as_str()
        .expect("client key");
    assert_eq!(
        decode_canonical_base64(client_key, 32)
            .expect("canonical client public key")
            .len(),
        32
    );

    let session_id = Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").expect("session ID");
    let session_master = [0x33; 32];
    let expiry = 1_800_003_900_u64;
    let mut payload = Vec::with_capacity(57);
    payload.push(Version2::VALUE);
    payload.extend_from_slice(session_id.as_bytes());
    payload.extend_from_slice(&session_master);
    payload.extend_from_slice(&expiry.to_be_bytes());
    let encrypted = encrypt_key_exchange_record_for_test(shared.as_bytes(), &payload)
        .expect("encrypted handshake");
    let response_body = serde_json::to_vec(&json!({
        "session_id": session_id.hyphenated().to_string(),
        "encrypted_session_key": encode_canonical_base64(&encrypted),
    }))
    .expect("response JSON");
    let completion =
        KeyExchangeCompletion::from_parts_for_test(client_secret_bytes, *enclave_public.as_bytes());
    let session = completion
        .complete(&response_body)
        .expect("completed exchange");
    assert_eq!(session.session_id(), session_id);
    assert_eq!(session.expires_at_unix_seconds(), expiry);

    let mismatched = serde_json::to_vec(&json!({
        "session_id": "10112233-4455-6677-8899-aabbccddeeff",
        "encrypted_session_key": encode_canonical_base64(&encrypted),
    }))
    .expect("mismatched response");
    let completion =
        KeyExchangeCompletion::from_parts_for_test(client_secret_bytes, *enclave_public.as_bytes());
    assert_eq!(
        completion
            .complete(&mismatched)
            .expect_err("binding mismatch"),
        TransportV2Error::BindingMismatch
    );

    let noncanonical = response_body.to_vec();
    let noncanonical = String::from_utf8(noncanonical)
        .expect("UTF-8")
        .replace(&session_id.to_string(), &session_id.simple().to_string());
    let completion =
        KeyExchangeCompletion::from_parts_for_test(client_secret_bytes, *enclave_public.as_bytes());
    assert_eq!(
        completion
            .complete(noncanonical.as_bytes())
            .expect_err("noncanonical UUID"),
        TransportV2Error::InvalidKeyExchange
    );

    let completion = KeyExchangeCompletion::from_parts_for_test(client_secret_bytes, [0_u8; 32]);
    assert_eq!(
        completion
            .complete(&response_body)
            .expect_err("non-contributory exchange"),
        TransportV2Error::NonContributoryKeyExchange
    );
}

#[test]
fn key_exchange_rejects_empty_or_oversized_attestation_nonces() {
    let enclave_public = *PublicKey::from(&StaticSecret::from([0x42; 32])).as_bytes();
    assert_eq!(
        PreparedKeyExchange::new(String::new(), enclave_public)
            .expect_err("empty challenge must fail"),
        TransportV2Error::InvalidKeyExchange
    );
    assert_eq!(
        PreparedKeyExchange::new("x".repeat(513), enclave_public)
            .expect_err("oversized challenge must fail"),
        TransportV2Error::InvalidKeyExchange
    );
}

fn test_request(_response_mode: ResponseMode) -> LogicalRequest {
    LogicalRequest::new(
        LogicalMethod::Post,
        "/v1/responses",
        None,
        vec![HeaderField::new(
            "content-type",
            b"application/json".to_vec(),
        )],
        Some(br#"{"model":"test"}"#.to_vec()),
    )
}

fn response_context(
    session_id: Uuid,
    master: [u8; 32],
    request_id: RequestId,
    response_mode: ResponseMode,
) -> super::session::ResponseContext {
    let session =
        V2Session::from_master_for_test(session_id, master, u64::MAX).expect("test session");
    let prepared = session
        .prepare_request_for_test(
            (0, request_id),
            response_mode,
            None,
            None,
            test_request(response_mode),
        )
        .expect("prepared request");
    let (_, context) = prepared.into_parts();
    context
}

#[test]
fn prepared_requests_are_exact_session_bound_and_reject_reserved_auto_mode() {
    let fixture = vectors();
    let session_id = Uuid::parse_str(&fixture.session_id).expect("session ID");
    let master = fixed_hex::<32>(&fixture.session_master_hex);
    let request_id = RequestId::from_bytes(fixed_hex(&fixture.request_id_hex));
    let session = V2Session::from_master_for_test(session_id, master, u64::MAX).expect("session");

    assert_eq!(
        session
            .prepare_request(
                ResponseMode::Auto,
                None,
                None,
                test_request(ResponseMode::Auto)
            )
            .expect_err("auto remains reserved"),
        TransportV2Error::InvalidRequest
    );

    let prepared = session
        .prepare_request_for_test(
            (0, request_id),
            ResponseMode::Unary,
            None,
            Some(CacheNamespaceRoot::from_bytes([0x7a; 32])),
            test_request(ResponseMode::Unary),
        )
        .expect("request");
    assert_eq!(prepared.session_id(), session_id);
    assert_eq!(prepared.request_id(), request_id);
    assert_eq!(prepared.response_mode(), ResponseMode::Unary);
    let debug = format!("{prepared:?}");
    assert!(!debug.contains("test"));
    assert!(!debug.contains(&encode_canonical_base64(&[0x7a; 32])));

    let (outer_body, _) = prepared.into_parts();
    assert!(outer_body.len() <= MAX_OUTER_REQUEST_BYTES);
    assert!(outer_body.len() >= MIN_RECORD_LEN);
}

#[test]
fn sessions_reject_requests_at_or_after_their_expiry() {
    let session = V2Session::from_master_for_test(Uuid::nil(), [0x5a; 32], 100).expect("session");
    let request_id = RequestId::from_bytes([0x5b; 16]);
    session
        .prepare_request_for_test(
            (99, request_id),
            ResponseMode::Unary,
            None,
            None,
            test_request(ResponseMode::Unary),
        )
        .expect("request immediately before expiry");

    assert_eq!(
        session
            .prepare_request_for_test(
                (100, request_id),
                ResponseMode::Unary,
                None,
                None,
                test_request(ResponseMode::Unary),
            )
            .expect_err("request at expiry must fail"),
        TransportV2Error::SessionExpired
    );
}

#[test]
fn sessions_enforce_request_id_uniqueness_and_request_record_budgets() {
    let session_id = Uuid::nil();
    let master_bytes = [0x5e; 32];
    let first_id = RequestId::from_bytes([0x5f; 16]);
    let second_id = RequestId::from_bytes([0x60; 16]);
    let third_id = RequestId::from_bytes([0x61; 16]);
    let session =
        V2Session::from_master_with_budgets_for_test(session_id, master_bytes, u64::MAX, 2, 2)
            .expect("session");

    let first = session
        .prepare_request_for_test(
            (0, first_id),
            ResponseMode::Unary,
            None,
            None,
            test_request(ResponseMode::Unary),
        )
        .expect("first request");
    assert_eq!(
        session
            .prepare_request_for_test(
                (0, first_id),
                ResponseMode::Unary,
                None,
                None,
                test_request(ResponseMode::Unary),
            )
            .expect_err("duplicate request ID"),
        TransportV2Error::RequestIdCollision
    );
    let second = session
        .prepare_request_for_test(
            (0, second_id),
            ResponseMode::Unary,
            None,
            None,
            test_request(ResponseMode::Unary),
        )
        .expect("second request");
    assert_eq!(
        session
            .prepare_request_for_test(
                (0, third_id),
                ResponseMode::Unary,
                None,
                None,
                test_request(ResponseMode::Unary),
            )
            .expect_err("request record budget"),
        TransportV2Error::RequestRecordBudgetExhausted
    );

    let keys = DirectionalKeys::derive(&SessionMaster::from_bytes(master_bytes)).expect("keys");
    let outer_response = |request_id| {
        let response = UnaryResponseEnvelope {
            version: Version2,
            request_id,
            status: 200,
            headers: vec![],
            body_base64: None,
        };
        let plaintext = serde_json::to_vec(&response).expect("response JSON");
        keys.encrypt_unary_response_record_for_test(&session_id, &request_id, &plaintext)
            .expect("response record")
    };
    let (_, first_context) = first.into_parts();
    first_context
        .decrypt_unary_outer(&outer_response(first_id))
        .expect("first response record");
    let (_, second_context) = second.into_parts();
    second_context
        .decrypt_unary_outer(&outer_response(second_id))
        .expect("second response record");
}

#[test]
fn sessions_reserve_response_capacity_before_emitting_requests() {
    let session =
        V2Session::from_master_with_budgets_for_test(Uuid::nil(), [0x68; 32], u64::MAX, 4, 2)
            .expect("session");

    let first = session
        .prepare_request_for_test(
            (0, RequestId::from_bytes([0x69; 16])),
            ResponseMode::Unary,
            None,
            None,
            test_request(ResponseMode::Unary),
        )
        .expect("first unary reservation");
    drop(first);

    assert_eq!(
        session
            .prepare_request_for_test(
                (0, RequestId::from_bytes([0x6b; 16])),
                ResponseMode::Stream,
                None,
                None,
                test_request(ResponseMode::Stream),
            )
            .expect_err("a stream needs both Start and terminal capacity"),
        TransportV2Error::ResponseRecordBudgetExhausted
    );

    let second = session
        .prepare_request_for_test(
            (0, RequestId::from_bytes([0x6d; 16])),
            ResponseMode::Unary,
            None,
            None,
            test_request(ResponseMode::Unary),
        )
        .expect("second unary reservation");
    let (_, second_context) = second.into_parts();
    assert!(second_context.decrypt_unary_outer(b"{}").is_err());

    assert_eq!(
        session
            .prepare_request_for_test(
                (0, RequestId::from_bytes([0x6f; 16])),
                ResponseMode::Unary,
                None,
                None,
                test_request(ResponseMode::Unary),
            )
            .expect_err("dropped and failed contexts retain their reservations"),
        TransportV2Error::ResponseRecordBudgetExhausted
    );
}

#[test]
fn authenticated_stream_pre_start_error_releases_the_unused_terminal_slot() {
    let session_id = Uuid::nil();
    let master_bytes = [0x6f; 32];
    let stream_request_id = RequestId::from_bytes([0x70; 16]);
    let session =
        V2Session::from_master_with_budgets_for_test(session_id, master_bytes, u64::MAX, 3, 2)
            .expect("session");
    let stream = session
        .prepare_request_for_test(
            (0, stream_request_id),
            ResponseMode::Stream,
            None,
            None,
            test_request(ResponseMode::Stream),
        )
        .expect("stream reservation");

    let response = UnaryResponseEnvelope {
        version: Version2,
        request_id: stream_request_id,
        status: 503,
        headers: vec![],
        body_base64: None,
    };
    let plaintext = serde_json::to_vec(&response).expect("response JSON");
    let keys = DirectionalKeys::derive(&SessionMaster::from_bytes(master_bytes)).expect("keys");
    let record = keys
        .encrypt_unary_response_record_for_test(&session_id, &stream_request_id, &plaintext)
        .expect("response record");
    let outer = record;
    let (_, context) = stream.into_parts();
    context
        .decrypt_stream_pre_start_error_outer(&outer)
        .expect("authenticated pre-Start error");

    session
        .prepare_request_for_test(
            (0, RequestId::from_bytes([0x73; 16])),
            ResponseMode::Unary,
            None,
            None,
            test_request(ResponseMode::Unary),
        )
        .expect("released stream slot admits one unary request");
}

#[test]
fn concurrent_requests_cannot_overbook_the_last_response_slot() {
    let session = Arc::new(
        V2Session::from_master_with_budgets_for_test(Uuid::nil(), [0x75; 32], u64::MAX, 4, 1)
            .expect("session"),
    );
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for index in 0..2_u8 {
        let session = Arc::clone(&session);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            session.prepare_request_for_test(
                (0, RequestId::from_bytes([0x76 + index; 16])),
                ResponseMode::Unary,
                None,
                None,
                test_request(ResponseMode::Unary),
            )
        }));
    }
    barrier.wait();

    let mut admitted = 0;
    let mut exhausted = 0;
    for handle in handles {
        match handle.join().expect("request thread") {
            Ok(_) => admitted += 1,
            Err(TransportV2Error::ResponseRecordBudgetExhausted) => exhausted += 1,
            Err(error) => panic!("unexpected request error: {error:?}"),
        }
    }
    assert_eq!(admitted, 1);
    assert_eq!(exhausted, 1);
}

#[test]
fn unary_response_requires_exact_aad_and_inner_request_id() {
    let session_id = Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").expect("session ID");
    let master_bytes = [0x61; 32];
    let request_id = RequestId::from_bytes([0x62; 16]);
    let keys = DirectionalKeys::derive(&SessionMaster::from_bytes(master_bytes)).expect("keys");

    let response = UnaryResponseEnvelope {
        version: Version2,
        request_id,
        status: 200,
        headers: vec![HeaderField::new(
            "content-type",
            b"application/json".to_vec(),
        )],
        body_base64: Some(EncodedBytes::from_bytes(br#"{"ok":true}"#.to_vec())),
    };
    let plaintext = serde_json::to_vec(&response).expect("response JSON");
    let record = keys
        .encrypt_unary_response_record_for_test(&session_id, &request_id, &plaintext)
        .expect("response record");
    let outer = record;
    let response = response_context(session_id, master_bytes, request_id, ResponseMode::Unary)
        .decrypt_unary_outer(&outer)
        .expect("authenticated unary response");
    assert_eq!(response.status, 200);
    assert_eq!(response.body.as_deref(), Some(br#"{"ok":true}"#.as_slice()));

    let other_request_id = RequestId::from_bytes([0x64; 16]);
    let mismatched = UnaryResponseEnvelope {
        version: Version2,
        request_id: other_request_id,
        status: 200,
        headers: vec![],
        body_base64: None,
    };
    let plaintext = serde_json::to_vec(&mismatched).expect("response JSON");
    let record = keys
        .encrypt_unary_response_record_for_test(&session_id, &request_id, &plaintext)
        .expect("response record");
    let outer = record;
    assert_eq!(
        response_context(session_id, master_bytes, request_id, ResponseMode::Unary)
            .decrypt_unary_outer(&outer)
            .expect_err("inner request mismatch"),
        TransportV2Error::BindingMismatch
    );
}

#[test]
fn response_context_enforces_mode_and_only_allows_pre_start_stream_errors() {
    let session_id = Uuid::nil();
    let master_bytes = [0x66; 32];
    let request_id = RequestId::from_bytes([0x67; 16]);
    let keys = DirectionalKeys::derive(&SessionMaster::from_bytes(master_bytes)).expect("keys");

    let encrypted_outer = |status| {
        let response = UnaryResponseEnvelope {
            version: Version2,
            request_id,
            status,
            headers: vec![],
            body_base64: Some(EncodedBytes::from_bytes(b"redacted-error".to_vec())),
        };
        let plaintext = serde_json::to_vec(&response).expect("response JSON");
        keys.encrypt_unary_response_record_for_test(&session_id, &request_id, &plaintext)
            .expect("response record")
    };

    assert_eq!(
        response_context(session_id, master_bytes, request_id, ResponseMode::Stream)
            .decrypt_unary_outer(&encrypted_outer(401))
            .expect_err("stream context must reject ordinary unary decoding"),
        TransportV2Error::ResponseModeMismatch
    );
    assert_eq!(
        response_context(session_id, master_bytes, request_id, ResponseMode::Unary)
            .into_stream_decoder()
            .expect_err("unary context must reject stream decoding"),
        TransportV2Error::ResponseModeMismatch
    );

    let error = response_context(session_id, master_bytes, request_id, ResponseMode::Stream)
        .decrypt_stream_pre_start_error_outer(&encrypted_outer(401))
        .expect("authenticated pre-Start error");
    assert_eq!(error.status, 401);
    assert_eq!(
        response_context(session_id, master_bytes, request_id, ResponseMode::Stream)
            .decrypt_stream_pre_start_error_outer(&encrypted_outer(200))
            .expect_err("successful stream response must use stream records"),
        TransportV2Error::InvalidResponse
    );
    assert_eq!(
        response_context(session_id, master_bytes, request_id, ResponseMode::Unary)
            .decrypt_stream_pre_start_error_outer(&encrypted_outer(401))
            .expect_err("unary context cannot select the stream error path"),
        TransportV2Error::ResponseModeMismatch
    );
}

fn encrypted_stream_frame(
    keys: &DirectionalKeys,
    session_id: &Uuid,
    request_id: &RequestId,
    sequence: u64,
    record: StreamRecord,
) -> Vec<u8> {
    let plaintext = serde_json::to_vec(&record).expect("stream JSON");
    let encrypted = keys
        .encrypt_stream_response_record_for_test(session_id, request_id, sequence, &plaintext)
        .expect("stream record");
    format!("data: {}\n\n", encode_canonical_base64(&encrypted)).into_bytes()
}

fn stream_decoder(session_id: Uuid, master: [u8; 32], request_id: RequestId) -> StreamDecoder {
    response_context(session_id, master, request_id, ResponseMode::Stream)
        .into_stream_decoder()
        .expect("stream decoder")
}

#[test]
fn stream_chunks_charge_capacity_beyond_the_reserved_start_and_terminal() {
    let session_id = Uuid::nil();
    let master_bytes = [0x80; 32];
    let request_id = RequestId::from_bytes([0x81; 16]);
    let session =
        V2Session::from_master_with_budgets_for_test(session_id, master_bytes, u64::MAX, 1, 3)
            .expect("session");
    let prepared = session
        .prepare_request_for_test(
            (0, request_id),
            ResponseMode::Stream,
            None,
            None,
            test_request(ResponseMode::Stream),
        )
        .expect("stream request");
    let (_, context) = prepared.into_parts();
    let mut decoder = context.into_stream_decoder().expect("stream decoder");
    let keys = DirectionalKeys::derive(&SessionMaster::from_bytes(master_bytes)).expect("keys");

    let start = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        0,
        StreamRecord::Start {
            version: Version2,
            request_id,
            sequence: 0,
            status: 200,
            headers: vec![],
        },
    );
    decoder.push(&start).expect("reserved Start record");

    let first_chunk = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        1,
        StreamRecord::Chunk {
            version: Version2,
            request_id,
            sequence: 1,
            body_base64: EncodedBytes::from_bytes(vec![1]),
        },
    );
    decoder
        .push(&first_chunk)
        .expect("one dynamically charged chunk");

    let second_chunk = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        2,
        StreamRecord::Chunk {
            version: Version2,
            request_id,
            sequence: 2,
            body_base64: EncodedBytes::from_bytes(vec![2]),
        },
    );
    assert_eq!(
        decoder
            .push(&second_chunk)
            .expect_err("response budget must reject another chunk"),
        TransportV2Error::ResponseRecordBudgetExhausted
    );
}

#[test]
fn stream_decoder_handles_split_and_coalesced_frames_with_authenticated_terminal() {
    let session_id = Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").expect("session ID");
    let master_bytes = [0x71; 32];
    let request_id = RequestId::from_bytes([0x72; 16]);
    let keys = DirectionalKeys::derive(&SessionMaster::from_bytes(master_bytes)).expect("keys");

    let start = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        0,
        StreamRecord::Start {
            version: Version2,
            request_id,
            sequence: 0,
            status: 200,
            headers: vec![HeaderField::new(
                "content-type",
                b"text/event-stream".to_vec(),
            )],
        },
    );
    let chunk = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        1,
        StreamRecord::Chunk {
            version: Version2,
            request_id,
            sequence: 1,
            body_base64: EncodedBytes::from_bytes(b"data: hello\n\n".to_vec()),
        },
    );
    let end = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        2,
        StreamRecord::End {
            version: Version2,
            request_id,
            sequence: 2,
        },
    );

    let mut decoder = stream_decoder(session_id, master_bytes, request_id);
    let split = start.len() / 2;
    assert!(decoder
        .push(&start[..split])
        .expect("partial frame")
        .is_empty());
    assert!(matches!(
        decoder.push(&start[split..]).expect("start").as_slice(),
        [StreamEvent::Start { status: 200, .. }]
    ));
    let mut coalesced = chunk;
    coalesced.extend_from_slice(&end);
    assert_eq!(
        decoder.push(&coalesced).expect("chunk and end"),
        vec![
            StreamEvent::Chunk(b"data: hello\n\n".to_vec()),
            StreamEvent::End
        ]
    );
    decoder
        .finish()
        .expect("authenticated terminal and clean EOF");
}

#[test]
fn stream_decoder_accepts_authenticated_error_as_the_only_terminal() {
    let session_id = Uuid::nil();
    let master_bytes = [0x81; 32];
    let request_id = RequestId::from_bytes([0x82; 16]);
    let keys = DirectionalKeys::derive(&SessionMaster::from_bytes(master_bytes)).expect("keys");
    let start = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        0,
        StreamRecord::Start {
            version: Version2,
            request_id,
            sequence: 0,
            status: 200,
            headers: vec![],
        },
    );
    let error = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        1,
        StreamRecord::Error {
            version: Version2,
            request_id,
            sequence: 1,
            status: 503,
            body_base64: EncodedBytes::from_bytes(b"unavailable".to_vec()),
        },
    );

    let mut decoder = stream_decoder(session_id, master_bytes, request_id);
    decoder.push(&start).expect("start");
    assert_eq!(
        decoder.push(&error).expect("error terminal"),
        vec![StreamEvent::Error {
            status: 503,
            body: b"unavailable".to_vec(),
        }]
    );
    decoder.finish().expect("terminal error is complete");
}

#[test]
fn stream_decoder_rejects_truncation_framing_tampering_and_extra_records() {
    let session_id = Uuid::nil();
    let master_bytes = [0x91; 32];
    let request_id = RequestId::from_bytes([0x92; 16]);
    let keys = DirectionalKeys::derive(&SessionMaster::from_bytes(master_bytes)).expect("keys");
    let start = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        0,
        StreamRecord::Start {
            version: Version2,
            request_id,
            sequence: 0,
            status: 200,
            headers: vec![],
        },
    );
    let end = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        1,
        StreamRecord::End {
            version: Version2,
            request_id,
            sequence: 1,
        },
    );

    let mut truncated = stream_decoder(session_id, master_bytes, request_id);
    truncated.push(&start).expect("start");
    assert_eq!(
        truncated.finish().expect_err("missing terminal"),
        TransportV2Error::TruncatedStream
    );

    for malformed in [
        b"data:not-spaced\n\n".as_slice(),
        b"event: message\ndata: YQ==\n\n".as_slice(),
        b"data: YQ==\r\n\r\n".as_slice(),
        b": comment\n\n".as_slice(),
    ] {
        let mut decoder = stream_decoder(session_id, master_bytes, request_id);
        assert!(decoder.push(malformed).is_err());
    }

    let mut tampered = start.clone();
    let payload = std::str::from_utf8(&tampered[6..tampered.len() - 2]).expect("base64 frame");
    let mut record = decode_canonical_base64(payload, 128 * 1024).expect("record");
    *record.last_mut().expect("tag") ^= 1;
    tampered = format!("data: {}\n\n", encode_canonical_base64(&record)).into_bytes();
    let mut decoder = stream_decoder(session_id, master_bytes, request_id);
    assert_eq!(
        decoder.push(&tampered).expect_err("tampered tag"),
        TransportV2Error::AuthenticationFailed
    );

    let mut extra = stream_decoder(session_id, master_bytes, request_id);
    extra.push(&start).expect("start");
    let mut terminal_and_extra = end.clone();
    terminal_and_extra.extend_from_slice(&end);
    assert_eq!(
        extra
            .push(&terminal_and_extra)
            .expect_err("record after terminal"),
        TransportV2Error::StreamAlreadyTerminal
    );

    let mut oversized = stream_decoder(session_id, master_bytes, request_id);
    let carrier = vec![b'x'; max_stream_carrier_frame_bytes_for_test() + 1];
    assert!(matches!(
        oversized.push(&carrier),
        Err(TransportV2Error::LimitExceeded {
            field: "stream carrier frame",
            ..
        })
    ));
}

#[test]
fn stream_decoder_bounds_cumulative_logical_chunk_bytes() {
    let session_id = Uuid::nil();
    let master_bytes = [0x9a; 32];
    let request_id = RequestId::from_bytes([0x9b; 16]);
    let keys = DirectionalKeys::derive(&SessionMaster::from_bytes(master_bytes)).expect("keys");
    let start = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        0,
        StreamRecord::Start {
            version: Version2,
            request_id,
            sequence: 0,
            status: 200,
            headers: vec![],
        },
    );
    let first = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        1,
        StreamRecord::Chunk {
            version: Version2,
            request_id,
            sequence: 1,
            body_base64: EncodedBytes::from_bytes(b"one".to_vec()),
        },
    );
    let second = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        2,
        StreamRecord::Chunk {
            version: Version2,
            request_id,
            sequence: 2,
            body_base64: EncodedBytes::from_bytes(b"two".to_vec()),
        },
    );

    let mut decoder =
        StreamDecoder::new_with_logical_limit(session_id, request_id, Arc::new(keys), 5);
    decoder.push(&start).expect("start");
    assert_eq!(
        decoder.push(&first).expect("first chunk"),
        vec![StreamEvent::Chunk(b"one".to_vec())]
    );
    assert_eq!(
        decoder.push(&second).expect_err("cumulative limit"),
        TransportV2Error::LimitExceeded {
            field: "logical stream",
            limit: 5,
        }
    );
}

#[test]
fn stream_decoder_rejects_wrong_inner_binding_and_oversized_chunks() {
    let session_id = Uuid::nil();
    let master_bytes = [0xa1; 32];
    let request_id = RequestId::from_bytes([0xa2; 16]);
    let keys = DirectionalKeys::derive(&SessionMaster::from_bytes(master_bytes)).expect("keys");

    let wrong_inner = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        0,
        StreamRecord::Start {
            version: Version2,
            request_id: RequestId::from_bytes([0xa3; 16]),
            sequence: 0,
            status: 200,
            headers: vec![],
        },
    );
    let mut decoder = stream_decoder(session_id, master_bytes, request_id);
    assert_eq!(
        decoder
            .push(&wrong_inner)
            .expect_err("wrong inner request ID"),
        TransportV2Error::BindingMismatch
    );

    let start = encrypted_stream_frame(
        &keys,
        &session_id,
        &request_id,
        0,
        StreamRecord::Start {
            version: Version2,
            request_id,
            sequence: 0,
            status: 200,
            headers: vec![],
        },
    );
    let oversized_plaintext = format!(
        "{{\"version\":2,\"request_id\":\"{}\",\"sequence\":1,\"kind\":\"chunk\",\"body_base64\":\"{}\"}}",
        request_id,
        encode_canonical_base64(&vec![0_u8; MAX_STREAM_CHUNK_BYTES + 1])
    );
    let oversized_record = keys
        .encrypt_stream_response_record_for_test(
            &session_id,
            &request_id,
            1,
            oversized_plaintext.as_bytes(),
        )
        .expect("oversized encrypted record");
    let oversized_frame =
        format!("data: {}\n\n", encode_canonical_base64(&oversized_record)).into_bytes();
    let mut decoder = stream_decoder(session_id, master_bytes, request_id);
    decoder.push(&start).expect("start");
    assert!(matches!(
        decoder.push(&oversized_frame),
        Err(TransportV2Error::LimitExceeded {
            field: "stream chunk",
            limit: MAX_STREAM_CHUNK_BYTES,
        })
    ));
}

#[test]
fn secret_bearing_debug_output_is_redacted() {
    let master = SessionMaster::from_bytes([0xbb; 32]);
    let keys = Arc::new(DirectionalKeys::derive(&master).expect("keys"));
    let cache_root = CacheNamespaceRoot::from_bytes([0xcc; 32]);
    let decoder = StreamDecoder::new_with_logical_limit(
        Uuid::nil(),
        RequestId::from_bytes([0xdd; 16]),
        keys,
        usize::MAX,
    );
    let plaintext = b"debug-plaintext-sentinel".to_vec();
    let unary = super::session::UnaryResponse {
        status: 500,
        headers: vec![HeaderField::new("x-secret", plaintext.clone())],
        body: Some(plaintext.clone()),
    };
    let chunk = StreamEvent::Chunk(plaintext.clone());
    let error = StreamEvent::Error {
        status: 500,
        body: plaintext,
    };

    assert_eq!(format!("{master:?}"), "SessionMaster([REDACTED])");
    assert_eq!(format!("{cache_root:?}"), "CacheNamespaceRoot([REDACTED])");
    assert!(!format!("{decoder:?}").contains(&hex::encode([0xbb; 32])));
    assert!(!format!("{decoder:?}").contains(&hex::encode([0xcc; 32])));
    assert!(!format!("{unary:?}").contains("debug-plaintext-sentinel"));
    assert!(!format!("{chunk:?}").contains("debug-plaintext-sentinel"));
    assert!(!format!("{error:?}").contains("debug-plaintext-sentinel"));
}
