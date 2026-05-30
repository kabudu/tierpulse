use std::num::NonZeroU32;
use std::sync::Arc;

use axum_test::TestServer;
use governor::{
    Quota, RateLimiter,
    state::{InMemoryState, NotKeyed, keyed::DefaultKeyedStateStore},
};
use moka::future::Cache;
use serde_json::json;

use tierpulse::{
    AppState, app, config::Config, inference::InferenceEngine, metrics::MetricsRegistry,
    models::SentimentResult,
};

fn test_config() -> Config {
    Config {
        port: 8080,
        log_level: "INFO".to_string(),
        tiingo_key: "tiingo-test-key".to_string(),
        finnhub_key: None,
        marketaux_key: None,
        alphavantage_key: None,
        grok_key: None,
        deepseek_key: None,
        openai_key: None,
        primary_llm: "grok".to_string(),
        llm_provider_order: vec![
            "grok".to_string(),
            "deepseek".to_string(),
            "openai".to_string(),
        ],
        grok_model: "grok-4.3".to_string(),
        deepseek_model: "deepseek-v4-pro".to_string(),
        openai_model: "gpt-5.4-nano".to_string(),
        redis_url: None,
        cache_ttl_sec: 300,
        rate_limit_per_min: 100,
        global_rate_limit_per_min: 1000,
        auth_mode: "none".to_string(),
        auth_api_keys: vec![],
        jwt_secret: None,
        jwt_issuer: None,
        onnx_threads: 2,
        model_path: "model.onnx".to_string(),
        egress_allowlist: vec![],
        provider_call_budget_per_request: 6,
    }
}

fn build_state(config: Config) -> Arc<AppState> {
    let engine = InferenceEngine::new_stub();

    let global_quota = Quota::per_minute(
        NonZeroU32::new(config.global_rate_limit_per_min).expect("non-zero global quota"),
    );
    let tenant_quota = Quota::per_minute(
        NonZeroU32::new(config.rate_limit_per_min).expect("non-zero tenant quota"),
    );

    Arc::new(AppState {
        config,
        engine,
        global_rate_limiter: Arc::new(RateLimiter::<
            NotKeyed,
            InMemoryState,
            governor::clock::DefaultClock,
        >::direct(global_quota)),
        tenant_rate_limiter: Arc::new(RateLimiter::<
            String,
            DefaultKeyedStateStore<String>,
            governor::clock::DefaultClock,
        >::keyed(tenant_quota)),
        cache: Cache::<String, SentimentResult>::new(1024),
        redis_client: None,
        http_client: reqwest::Client::new(),
        metrics: Arc::new(MetricsRegistry::new()),
    })
}

async fn build_server(config: Config) -> TestServer {
    TestServer::new(app(build_state(config)).await)
}

fn valid_payload() -> serde_json::Value {
    json!({
        "symbols": [
            { "ticker": "AAPL", "name": "Apple Inc." }
        ],
        "lookback_hours": 24,
        "max_articles_per_symbol": 5
    })
}

fn assert_standard_error_envelope(body: &serde_json::Value) {
    assert!(body.get("code").and_then(|v| v.as_str()).is_some());
    assert!(body.get("message").and_then(|v| v.as_str()).is_some());
    assert!(body.get("retry_after_seconds").is_some());
    assert!(
        body.get("request_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .starts_with("tp_")
    );
    assert!(body.get("details").and_then(|v| v.as_array()).is_some());
}

#[tokio::test]
async fn analyze_returns_typed_400_shape_for_invalid_symbol_count() {
    let server = build_server(test_config()).await;

    let payload = json!({
        "symbols": [],
        "lookback_hours": 24,
        "max_articles_per_symbol": 5
    });

    let response = server.post("/api/v1/analyze").json(&payload).await;

    response.assert_status_bad_request();
    let body = response.json::<serde_json::Value>();
    assert_standard_error_envelope(&body);
    assert_eq!(body.get("code"), Some(&json!("INVALID_REQUEST")));
    assert_eq!(
        body.get("message"),
        Some(&json!("Request validation failed."))
    );
    assert_eq!(
        body.get("retry_after_seconds"),
        Some(&serde_json::Value::Null)
    );
    assert!(
        body.get("request_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .starts_with("tp_")
    );

    let details = body
        .get("details")
        .and_then(|v| v.as_array())
        .expect("details must be array");
    assert!(details.iter().any(|entry| {
        entry.get("code") == Some(&json!("INVALID_SYMBOL_COUNT"))
            && entry.get("field") == Some(&json!("symbols"))
    }));
}

#[tokio::test]
async fn health_and_metrics_endpoints_are_exposed() {
    let server = build_server(test_config()).await;

    let live = server.get("/health/live").await;
    live.assert_status_ok();
    live.assert_json(&json!({ "status": "alive" }));

    let ready = server.get("/health/ready").await;
    ready.assert_status_ok();
    let ready_body = ready.json::<serde_json::Value>();
    assert!(ready_body.get("status").and_then(|v| v.as_str()).is_some());
    assert!(ready_body.get("tiers").is_some());
    assert!(
        ready_body
            .get("tiers")
            .and_then(|t| t.get("tier_1_local_onnx"))
            .and_then(|v| v.get("breaker_state"))
            .is_some()
    );
    assert!(
        ready_body
            .get("tiers")
            .and_then(|t| t.get("tier_2_news"))
            .and_then(|v| v.get("degradation_reason"))
            .is_some()
    );
    assert!(
        ready_body
            .get("tiers")
            .and_then(|t| t.get("tier_3_llm"))
            .and_then(|v| v.get("breaker_state"))
            .is_some()
    );

    let metrics = server.get("/metrics").await;
    metrics.assert_status_ok();
    let body = metrics.text();
    assert!(body.contains("request_duration_ms"));
    assert!(body.contains("cache_hit_ratio"));
    assert!(body.contains("cache_hit_memory_total"));
    assert!(body.contains("cache_hit_redis_total"));
    assert!(body.contains("provider_error_rate"));
    assert!(body.contains("fallback_transition_count"));
    assert!(body.contains("tier_exhaustion_rate"));
}

#[tokio::test]
async fn analyze_returns_401_with_standard_error_envelope() {
    let mut config = test_config();
    config.auth_mode = "api_key".to_string();
    config.auth_api_keys = vec![("tenant-a".to_string(), "key-a".to_string())];

    let server = build_server(config).await;
    let payload = valid_payload();

    let response = server.post("/api/v1/analyze").json(&payload).await;
    response.assert_status_unauthorized();

    let body = response.json::<serde_json::Value>();
    assert_standard_error_envelope(&body);
    assert_eq!(body.get("code"), Some(&json!("UNAUTHORIZED")));
    assert_eq!(
        body.get("retry_after_seconds"),
        Some(&serde_json::Value::Null)
    );
}

#[tokio::test]
async fn analyze_returns_429_global_with_standard_error_envelope() {
    let mut config = test_config();
    config.global_rate_limit_per_min = 1;

    let server = build_server(config).await;
    let payload = valid_payload();

    let first = server.post("/api/v1/analyze").json(&payload).await;
    assert_eq!(first.status_code(), 503);

    let second = server.post("/api/v1/analyze").json(&payload).await;
    second.assert_status_too_many_requests();

    let body = second.json::<serde_json::Value>();
    assert_standard_error_envelope(&body);
    assert_eq!(body.get("code"), Some(&json!("GLOBAL_RATE_LIMITED")));
    assert_eq!(body.get("retry_after_seconds"), Some(&json!(1)));
}

#[tokio::test]
async fn analyze_returns_429_tenant_with_standard_error_envelope() {
    let mut config = test_config();
    config.rate_limit_per_min = 1;
    config.auth_mode = "api_key".to_string();
    config.auth_api_keys = vec![("tenant-a".to_string(), "key-a".to_string())];

    let server = build_server(config).await;
    let payload = valid_payload();

    let first = server
        .post("/api/v1/analyze")
        .add_header("x-api-key", "key-a")
        .json(&payload)
        .await;
    assert_eq!(first.status_code(), 503);

    let second = server
        .post("/api/v1/analyze")
        .add_header("x-api-key", "key-a")
        .json(&payload)
        .await;
    second.assert_status_too_many_requests();

    let body = second.json::<serde_json::Value>();
    assert_standard_error_envelope(&body);
    assert_eq!(body.get("code"), Some(&json!("TENANT_RATE_LIMITED")));
    assert_eq!(body.get("retry_after_seconds"), Some(&json!(1)));
    let details = body
        .get("details")
        .and_then(|v| v.as_array())
        .expect("details must be array");
    assert!(
        details
            .iter()
            .any(|entry| entry.get("field") == Some(&json!("tenant_id")))
    );
}

#[tokio::test]
async fn analyze_returns_503_with_standard_error_envelope() {
    let server = build_server(test_config()).await;
    let payload = valid_payload();

    let response = server.post("/api/v1/analyze").json(&payload).await;
    assert_eq!(response.status_code(), 503);

    let body = response.json::<serde_json::Value>();
    assert_standard_error_envelope(&body);
    assert_eq!(body.get("code"), Some(&json!("INTELLIGENCE_EXHAUSTION")));
    assert_eq!(body.get("retry_after_seconds"), Some(&json!(300)));
}
