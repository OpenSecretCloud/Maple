//! Offline tests for the authenticated Maple rollout and cache boundaries.
use super::*;
use crate::os_flags::{OsFlagsClient, MAPLE_INFERENCE_FEATURE_FLAG_KEY};
use crate::provider_registry::MapleInstanceId;
use axum::{extract::OriginalUri, Json};
use std::sync::atomic::{AtomicUsize, Ordering};

struct LoopbackFlags {
    base_url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for LoopbackFlags {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn flags_server(app: Router) -> LoopbackFlags {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback flags mock");
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve loopback flags mock");
    });
    LoopbackFlags {
        base_url: format!("http://{address}"),
        task,
    }
}

#[tokio::test]
async fn only_deepseek_and_eligible_auto_candidates_consult_the_cached_provider_flag() {
    let user = Uuid::from_u128(1);
    let calls = Arc::new(AtomicUsize::new(0));
    let request_calls = calls.clone();
    let server = flags_server(Router::new().route(
        "/v1/users/:user_uuid/flags",
        get(move |OriginalUri(uri): OriginalUri| {
            let calls = request_calls.clone();
            async move {
                assert_eq!(uri.query(), Some("keys=maple_inference"));
                calls.fetch_add(1, Ordering::SeqCst);
                Json(json!({"user_uuid": user, "flags": {"maple_inference": true}}))
            }
        }),
    ))
    .await;
    let client = OsFlagsClient::new(server.base_url.clone(), None).unwrap();
    for model in [
        "glm-5-3",
        "glm-5-3-flash",
        "llama3-3-70b",
        "unknown-model",
        "deepseek-v4-1-flash-1017",
        "deepseek-v4-1-flash-1017-extra",
    ] {
        for plan in [ModelPlan::Free, ModelPlan::Paid] {
            assert!(!maple_inference_eligibility_for_model(Some(&client), user, model, plan).await);
        }
    }
    for alias in ["auto:quick", "auto:powerful"] {
        assert!(
            !maple_inference_eligibility_for_model(Some(&client), user, alias, ModelPlan::Free)
                .await
        );
    }
    assert!(
        !maple_inference_eligibility_for_model(
            Some(&client),
            user,
            "auto:powerful",
            ModelPlan::Paid
        )
        .await
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // The generic model and Auto Quick's existing DeepSeek alternate capture
    // the same account-owned provider decision, without changing Auto defaults.
    assert!(
        maple_inference_eligibility_for_model(
            Some(&client),
            user,
            "deepseek-v4-1-flash",
            ModelPlan::Paid
        )
        .await
    );
    assert!(
        maple_inference_eligibility_for_model(Some(&client), user, "auto:quick", ModelPlan::Paid)
            .await
    );
    assert!(maple_inference_eligibility(Some(&client), user).await);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn maple_rollout_requires_matching_account_and_explicit_true_flag() {
    let authenticated_user = Uuid::from_u128(1);
    assert!(!maple_inference_eligibility(None, authenticated_user).await);

    let cases = [
        (
            "enabled",
            StatusCode::OK,
            json!({"user_uuid": authenticated_user, "flags": {"maple_inference": true}})
                .to_string(),
            true,
        ),
        (
            "disabled",
            StatusCode::OK,
            json!({"user_uuid": authenticated_user, "flags": {"maple_inference": false}})
                .to_string(),
            false,
        ),
        (
            "missing",
            StatusCode::OK,
            json!({"user_uuid": authenticated_user, "flags": {"agent": true}}).to_string(),
            false,
        ),
        (
            "wrong account",
            StatusCode::OK,
            json!({"user_uuid": Uuid::from_u128(2), "flags": {"maple_inference": true}})
                .to_string(),
            false,
        ),
        (
            "service unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error": "mock unavailable"}).to_string(),
            false,
        ),
        (
            "malformed JSON",
            StatusCode::OK,
            "{not-valid-json".to_string(),
            false,
        ),
        (
            "flag is not a boolean",
            StatusCode::OK,
            json!({"user_uuid": authenticated_user, "flags": {"maple_inference": "true"}})
                .to_string(),
            false,
        ),
    ];

    for (name, status, response, expected) in cases {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_request = captured.clone();
        let server = flags_server(Router::new().route(
            "/v1/users/:user_uuid/flags",
            get(move |OriginalUri(uri): OriginalUri, headers: HeaderMap| {
                let captured = captured_request.clone();
                let response = response.clone();
                async move {
                    captured.lock().unwrap().push((
                        uri.to_string(),
                        headers
                            .get(header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .map(ToOwned::to_owned),
                    ));
                    (
                        status,
                        [(header::CONTENT_TYPE, "application/json")],
                        response,
                    )
                }
            }),
        ))
        .await;
        let client = OsFlagsClient::new(
            server.base_url.clone(),
            Some("maple-flags-test-only".to_string()),
        )
        .unwrap();

        assert_eq!(
            maple_inference_eligibility(Some(&client), authenticated_user).await,
            expected,
            "{name}"
        );
        let calls = captured.lock().unwrap();
        assert_eq!(calls.len(), 1, "{name}");
        assert_eq!(
            calls[0].0,
            format!("/v1/users/{authenticated_user}/flags?keys=maple_inference"),
            "{name}"
        );
        assert_eq!(
            calls[0].1.as_deref(),
            Some("Bearer maple-flags-test-only"),
            "{name}"
        );
    }
}

#[tokio::test]
async fn maple_rollout_reuses_existing_flags_cache_without_crossing_users() {
    let first_user = Uuid::from_u128(1);
    let second_user = Uuid::from_u128(2);
    let calls = Arc::new(AtomicUsize::new(0));
    let request_calls = calls.clone();
    let server = flags_server(Router::new().route(
        "/v1/users/:user_uuid/flags",
        get(
            move |axum::extract::Path(user): axum::extract::Path<Uuid>,
                  OriginalUri(uri): OriginalUri| {
                let calls = request_calls.clone();
                async move {
                    assert_eq!(uri.query(), Some("keys=maple_inference"));
                    let first_request = calls.fetch_add(1, Ordering::SeqCst) == 0;
                    Json(json!({
                        "user_uuid": user,
                        "flags": { (MAPLE_INFERENCE_FEATURE_FLAG_KEY): first_request }
                    }))
                }
            },
        ),
    ))
    .await;
    let client = OsFlagsClient::new(server.base_url.clone(), None).unwrap();
    assert!(maple_inference_eligibility(Some(&client), first_user).await);
    assert!(maple_inference_eligibility(Some(&client), first_user).await);
    // Request-scoped callers may clone the configured client. They must share
    // its existing TTL cache rather than create a separate eligibility cache.
    assert!(maple_inference_eligibility(Some(&client.clone()), first_user).await);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    assert!(!maple_inference_eligibility(Some(&client), second_user).await);
    assert!(!maple_inference_eligibility(Some(&client), second_user).await);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn unavailable_flags_response_times_out_with_dedicated_inference_disabled() {
    let calls = Arc::new(AtomicUsize::new(0));
    let request_calls = calls.clone();
    let server = flags_server(Router::new().route(
        "/v1/users/:user_uuid/flags",
        get(move || {
            let calls = request_calls.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                std::future::pending::<Response>().await
            }
        }),
    ))
    .await;
    let client = OsFlagsClient::new(server.base_url.clone(), None).unwrap();
    // Exercise the real OsFlagsClient 10-second deadline. The outer guard only
    // prevents a regression from hanging the suite indefinitely.
    let enabled = tokio::time::timeout(
        Duration::from_secs(13),
        maple_inference_eligibility(Some(&client), Uuid::from_u128(1)),
    )
    .await
    .expect("flags client must enforce its request timeout");
    assert!(!enabled);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn nested_and_pinned_completion_contexts_preserve_authenticated_rollout_decision() {
    assert!(!InferenceRoutingContext::new(ModelPlan::Paid).maple_inference_enabled());
    let cache = CompletionCachePolicy::LegacyV1;
    for enabled in [false, true] {
        let caller = InferenceRoutingContext::new(ModelPlan::Paid).with_maple_inference(enabled);
        let child = caller.with_model_plan(ModelPlan::Free);
        assert_eq!(child.model_plan(), ModelPlan::Free);
        assert_eq!(child.maple_inference_enabled(), enabled);
        assert_eq!(caller.model_plan(), ModelPlan::Paid);

        for (provider, public_model) in [
            (
                ProviderId::Maple(MapleInstanceId::I01),
                "deepseek-v4-1-flash",
            ),
            (
                ProviderId::Maple(MapleInstanceId::I02),
                "deepseek-v4-1-flash",
            ),
            (ProviderId::Tinfoil, "deepseek-v4-1-flash"),
        ] {
            let pinned = PinnedCompletionRequest::new(
                InferenceIntent::new(
                    Uuid::from_u128(1),
                    public_model,
                    public_model,
                    ModelPlan::Paid,
                    InferenceSurface::Responses,
                    WorkloadClass::Interactive,
                )
                .with_maple_inference(enabled),
                SelectedProviderRoute {
                    provider,
                    proxy: ProxyConfig {
                        base_url: "https://dedicated.invalid".to_string(),
                        api_key: None,
                        provider_name: provider.as_str().to_string(),
                    },
                    public_model_id: public_model.to_string(),
                    provider_model_id: "deepseek-v4-1-flash".to_string(),
                    response_model_id: public_model.to_string(),
                    bucket: None,
                    selection_source: RouteSelectionSource::StaticSplit,
                },
            );
            let context = CompletionExecutionContext::for_pinned(
                BillingContext::new(AuthMethod::ApiKey, public_model.to_string()),
                &pinned,
                &cache,
            );
            assert_eq!(context.routing.model_plan(), ModelPlan::Paid);
            assert_eq!(context.routing.maple_inference_enabled(), enabled);
        }
    }
}

#[test]
fn maple_instance_routes_keep_shared_cache_namespace_and_billing_identity() {
    let user = Uuid::from_u128(17);
    let model = crate::model_config::DEEPSEEK_V4_1_FLASH_MODEL_ID;
    let cache = CompletionCachePolicy::BoundV2(derive_provider_cache_namespaces(
        &CacheNamespaceRoot::from_bytes([0x42; 32]),
        user,
    ));
    let intent = InferenceIntent::new(
        user,
        model,
        model,
        ModelPlan::Paid,
        InferenceSurface::ChatCompletions,
        WorkloadClass::Interactive,
    )
    .with_maple_inference(true);
    let mut prior_salt = None;
    let mut route_keys = HashSet::new();

    for instance in MapleInstanceId::ALL {
        let proxies = ProxyRouter::new(
            "http://continuum.invalid".to_owned(),
            None,
            "https://tinfoil.invalid".to_owned(),
        )
        .with_maple_api_key(Some("synthetic-maple-key".to_owned()))
        .with_maple_readiness(crate::provider_client::MapleReadiness::for_test_instances(
            &[instance],
        ));
        let route = ProviderRouter::default()
            .select_active_completion_route(&proxies, &intent)
            .unwrap();
        assert_eq!(route.provider, ProviderId::Maple(instance));
        assert_eq!(route.proxy.provider_name, route.provider.as_str());
        assert!(route_keys.insert(route.identity().route_key()));

        let mut body = serde_json::Map::new();
        body.insert("cache_salt".to_owned(), json!("caller-controlled"));
        apply_provider_managed_request_fields(
            &mut body,
            route.provider.family_name(),
            user,
            &cache,
        );
        let salt = body.get("cache_salt").cloned().unwrap();
        assert_ne!(salt, json!("caller-controlled"));
        if let Some(prior) = prior_salt.replace(salt.clone()) {
            assert_eq!(
                salt, prior,
                "moving replicas preserves the user's namespace"
            );
        }

        let event = build_usage_event(
            user,
            CompletionUsage {
                prompt_tokens: 100,
                completion_tokens: 20,
                cached_prompt_tokens: Some(42),
            },
            BigDecimal::from(0),
            true,
            route.provider.family_name().to_owned(),
            route.public_model_id,
        );
        assert_eq!(event.provider_name, "maple");
        assert_eq!(event.model_name, model);
        assert_eq!(event.cached_input_tokens, Some(42));
        assert!(event.is_api_request);
    }
    assert_eq!(route_keys.len(), 2);
}

#[test]
fn direct_maple_cache_salt_is_private_and_provider_fields_are_rebuilt() {
    let user = Uuid::from_u128(1);
    let other_user = Uuid::from_u128(2);
    let root = CacheNamespaceRoot::from_bytes([0x42; 32]);
    let namespaces = derive_provider_cache_namespaces(&root, user);
    let policies = [
        CompletionCachePolicy::LegacyV1,
        CompletionCachePolicy::BoundV2(namespaces),
    ];
    for policy in policies {
        let expected = match &policy {
            CompletionCachePolicy::LegacyV1 => legacy_maple_user_cache_secret(user),
            CompletionCachePolicy::BoundV2(namespace) => namespace.maple_user_cache_secret(),
        };
        for untrusted in [
            json!("caller-controlled"),
            json!(null),
            json!({"other_user": "injected"}),
            json!(["injected"]),
            json!(123),
            json!(false),
        ] {
            let mut body = json!({
                "model": "deepseek-v4-1-flash",
                "messages": [{"role": "user", "content": "synthetic cache boundary test"}],
                "user_cache_secret": untrusted,
                "cache_salt": untrusted,
                "temperature": 0.5
            })
            .as_object()
            .unwrap()
            .clone();
            apply_provider_managed_request_fields(
                &mut body,
                ProviderId::Maple(MapleInstanceId::I01).family_name(),
                user,
                &policy,
            );
            // Dedicated endpoints expose native vLLM, which uses cache_salt.
            // user_cache_secret belongs to Tinfoil's shared-router contract.
            assert_eq!(body.get("cache_salt"), Some(&json!(expected)));
            assert!(!body.contains_key("user_cache_secret"));
            assert_eq!(body.get("model"), Some(&json!("deepseek-v4-1-flash")));
            assert_eq!(body.get("temperature"), Some(&json!(0.5)));

            let mut other_body = body.clone();
            let other_policy = match &policy {
                CompletionCachePolicy::LegacyV1 => CompletionCachePolicy::LegacyV1,
                CompletionCachePolicy::BoundV2(_) => CompletionCachePolicy::BoundV2(
                    derive_provider_cache_namespaces(&root, other_user),
                ),
            };
            apply_provider_managed_request_fields(
                &mut other_body,
                ProviderId::Maple(MapleInstanceId::I01).family_name(),
                other_user,
                &other_policy,
            );
            assert_ne!(other_body.get("cache_salt"), body.get("cache_salt"));

            // Provider field normalization is independent of model identity.
            // Rebuilding fields must never retain another provider's salt;
            // this does not authorize fallback between different checkpoints.
            apply_provider_managed_request_fields(
                &mut body,
                ProviderId::Tinfoil.as_str(),
                user,
                &policy,
            );
            let shared_expected = match &policy {
                CompletionCachePolicy::LegacyV1 => tinfoil_user_cache_secret(user),
                CompletionCachePolicy::BoundV2(namespace) => namespace.tinfoil_user_cache_secret(),
            };
            assert_eq!(body.get("user_cache_secret"), Some(&json!(shared_expected)));
            assert_ne!(body.get("user_cache_secret"), Some(&json!(expected)));
            assert!(!body.contains_key("cache_salt"));

            apply_provider_managed_request_fields(
                &mut body,
                ProviderId::Continuum.as_str(),
                user,
                &policy,
            );
            let continuum_expected = match &policy {
                CompletionCachePolicy::LegacyV1 => legacy_continuum_cache_salt(user),
                CompletionCachePolicy::BoundV2(namespace) => namespace.continuum_cache_salt(),
            };
            assert!(!body.contains_key("user_cache_secret"));
            assert_eq!(body.get("cache_salt"), Some(&json!(continuum_expected)));
            assert_ne!(body.get("cache_salt"), Some(&json!(expected)));
        }
    }
}
