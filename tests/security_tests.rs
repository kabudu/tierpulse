use tierpulse::config::Config;
use tierpulse::security::redact_sensitive_text;

fn test_config() -> Config {
    Config {
        port: 8080,
        log_level: "INFO".to_string(),
        tiingo_key: "tiingo-secret".to_string(),
        finnhub_key: Some("finnhub-secret".to_string()),
        marketaux_key: Some("marketaux-secret".to_string()),
        alphavantage_key: Some("alphavantage-secret".to_string()),
        grok_key: Some("grok-secret".to_string()),
        deepseek_key: Some("deepseek-secret".to_string()),
        openai_key: Some("openai-secret".to_string()),
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
        auth_mode: "api_key".to_string(),
        auth_api_keys: vec![("tenant-a".to_string(), "tenant-key-secret".to_string())],
        jwt_secret: Some("jwt-secret".to_string()),
        jwt_issuer: Some("issuer".to_string()),
        onnx_threads: 2,
        model_path: "model.onnx".to_string(),
        egress_allowlist: vec![],
        provider_call_budget_per_request: 6,
    }
}

#[test]
fn redacts_sensitive_query_parameters() {
    let cfg = test_config();
    let input = "GET /v1/news?symbol=AAPL&api_token=marketaux-secret&token=finnhub-secret&api_key=tenant-key-secret";

    let redacted = redact_sensitive_text(input, &cfg);

    assert!(redacted.contains("api_token=[REDACTED]"));
    assert!(redacted.contains("token=[REDACTED]"));
    assert!(redacted.contains("api_key=[REDACTED]"));
    assert!(!redacted.contains("marketaux-secret"));
    assert!(!redacted.contains("finnhub-secret"));
    assert!(!redacted.contains("tenant-key-secret"));
}

#[test]
fn redacts_bearer_tokens() {
    let cfg = test_config();
    let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature";

    let redacted = redact_sensitive_text(input, &cfg);

    assert!(redacted.contains("Authorization: Bearer [REDACTED]"));
    assert!(!redacted.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature"));
}

#[test]
fn redacts_mixed_query_params_and_bearer_token_in_single_line() {
    let cfg = test_config();
    let input = "request failed url=/v1/news?symbol=AAPL&api_token=marketaux-secret auth='Bearer super-secret-jwt-token' x-api-key=tenant-key-secret";

    let redacted = redact_sensitive_text(input, &cfg);

    assert!(redacted.contains("api_token=[REDACTED]"));
    assert!(redacted.contains("x-api-key=[REDACTED]"));
    assert!(redacted.contains("Bearer [REDACTED]"));
    assert!(!redacted.contains("marketaux-secret"));
    assert!(!redacted.contains("tenant-key-secret"));
    assert!(!redacted.contains("super-secret-jwt-token"));
}
