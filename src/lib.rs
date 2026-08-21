#![allow(clippy::collapsible_if)]

use axum::{
    Extension, Json, Router,
    http::{
        HeaderMap, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE, HeaderName},
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use governor::{
    RateLimiter,
    state::{InMemoryState, NotKeyed, keyed::DefaultKeyedStateStore},
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use moka::future::Cache;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info};
use uuid::Uuid;

pub mod config;
pub mod egress;
pub mod inference;
pub mod metrics;
pub mod models;
pub mod providers;
pub mod security;

use config::Config;
use inference::InferenceEngine;
use metrics::MetricsRegistry;
use models::{AnalyzeRequest, AnalyzeResponse, SentimentResult, Symbol};

const MAX_SYMBOLS: usize = 50;
const MAX_TICKER_LENGTH: usize = 16;
const MAX_NAME_LENGTH: usize = 120;
const MIN_LOOKBACK_HOURS: i32 = 1;
const MAX_LOOKBACK_HOURS: i32 = 168;
const MIN_MAX_ARTICLES_PER_SYMBOL: i32 = 1;
const MAX_MAX_ARTICLES_PER_SYMBOL: i32 = 20;
const LLM_FALLBACK_BATCH_SIZE: usize = 15;

pub struct AppState {
    pub config: Config,
    pub engine: InferenceEngine,
    pub global_rate_limiter:
        Arc<RateLimiter<NotKeyed, InMemoryState, governor::clock::DefaultClock>>,
    pub tenant_rate_limiter:
        Arc<RateLimiter<String, DefaultKeyedStateStore<String>, governor::clock::DefaultClock>>,
    pub cache: Cache<String, SentimentResult>,
    pub redis_client: Option<redis::Client>,
    pub http_client: reqwest::Client,
    pub metrics: Arc<MetricsRegistry>,
}

pub async fn app(shared_state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(readiness_handler))
        .route("/health/live", get(liveness_handler))
        .route("/health/ready", get(readiness_handler))
        .route("/metrics", get(metrics_handler))
        .route("/api/v1/analyze", post(analyze_handler))
        .layer(
            tower_http::sensitive_headers::SetSensitiveRequestHeadersLayer::new([
                AUTHORIZATION,
                HeaderName::from_static("x-api-key"),
            ]),
        )
        .layer(Extension(shared_state))
}

pub async fn liveness_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "alive"
        })),
    )
}

pub async fn readiness_handler(Extension(state): Extension<Arc<AppState>>) -> impl IntoResponse {
    let mut tiingo_available = false;

    let tiingo_url = "https://api.tiingo.com/tiingo/news";
    if crate::egress::enforce_allowed_url(tiingo_url, &state.config).is_ok() {
        if let Ok(res) = state
            .http_client
            .get(tiingo_url)
            .header(
                "Authorization",
                format!("Token {}", state.config.tiingo_key),
            )
            .timeout(std::time::Duration::from_secs(1))
            .send()
            .await
        {
            tiingo_available = res.status().is_success() || res.status().as_u16() == 429;
        }
    }

    let marketaux_configured = state.config.marketaux_key.is_some();
    let alphavantage_configured = state.config.alphavantage_key.is_some();
    let finnhub_configured = state.config.finnhub_key.is_some();
    let llm_configured = state.config.grok_key.is_some()
        || state.config.deepseek_key.is_some()
        || state.config.openai_key.is_some();

    let tier_1_status = "operational";

    let tier_2_status = if tiingo_available
        || marketaux_configured
        || alphavantage_configured
        || finnhub_configured
    {
        "operational"
    } else {
        "degraded"
    };

    let tier_2_reason = if tiingo_available {
        serde_json::Value::Null
    } else if marketaux_configured || alphavantage_configured || finnhub_configured {
        serde_json::json!("primary_news_provider_unavailable_using_fallback_capacity")
    } else {
        serde_json::json!("no_news_provider_available")
    };

    let tier_3_status = if llm_configured {
        "operational"
    } else {
        "degraded"
    };
    let tier_3_reason = if llm_configured {
        serde_json::Value::Null
    } else {
        serde_json::json!("no_llm_provider_configured")
    };

    let overall_status = if tier_2_status == "operational" {
        if tier_3_status == "operational" {
            "operational"
        } else {
            "degraded"
        }
    } else {
        "degraded"
    };

    let response = serde_json::json!({
        "status": overall_status,
        "tiers": {
            "tier_1_local_onnx": {
                "status": tier_1_status,
                "degradation_reason": serde_json::Value::Null,
                "breaker_state": "not_configured"
            },
            "tier_2_news": {
                "status": tier_2_status,
                "degradation_reason": tier_2_reason,
                "breaker_state": "not_configured",
                "providers": {
                    "tiingo": {
                        "status": if tiingo_available { "operational" } else { "degraded" },
                        "breaker_state": "not_configured"
                    },
                    "marketaux": {
                        "status": if marketaux_configured { "configured" } else { "not_configured" },
                        "breaker_state": "not_configured"
                    },
                    "alphavantage": {
                        "status": if alphavantage_configured { "configured" } else { "not_configured" },
                        "breaker_state": "not_configured"
                    },
                    "finnhub": {
                        "status": if finnhub_configured { "configured" } else { "not_configured" },
                        "breaker_state": "not_configured"
                    }
                }
            },
            "tier_3_llm": {
                "status": tier_3_status,
                "degradation_reason": tier_3_reason,
                "breaker_state": "not_configured",
                "providers": {
                    "grok": {
                        "status": if state.config.grok_key.is_some() { "configured" } else { "not_configured" },
                        "breaker_state": "not_configured"
                    },
                    "deepseek": {
                        "status": if state.config.deepseek_key.is_some() { "configured" } else { "not_configured" },
                        "breaker_state": "not_configured"
                    },
                    "openai": {
                        "status": if state.config.openai_key.is_some() { "configured" } else { "not_configured" },
                        "breaker_state": "not_configured"
                    }
                },
                "execution_order": state.config.llm_provider_order.clone()
            }
        }
    });

    (StatusCode::OK, Json(response))
}

pub async fn metrics_handler(Extension(state): Extension<Arc<AppState>>) -> impl IntoResponse {
    let body = state.metrics.render_prometheus();
    (
        StatusCode::OK,
        [(CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
}

pub async fn analyze_handler(
    Extension(state): Extension<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AnalyzeRequest>,
) -> impl IntoResponse {
    let start = Instant::now();
    let request_id = format!("tp_{}", Uuid::new_v4().simple());
    let mut tier_exhausted = false;

    let response: Response = (async {
        if let Err(details) = validate_analyze_request(&payload) {
            return validation_error_response(details, &request_id).into_response();
        }

        let tenant_id = match authenticate_and_resolve_tenant(&headers, &state.config, &request_id) {
            Ok(tenant) => tenant,
            Err(err_response) => return err_response.into_response(),
        };

        if state.global_rate_limiter.check().is_err() {
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "GLOBAL_RATE_LIMITED",
                "Global protection guard is active.",
                Some(1),
                &request_id,
                Vec::new(),
            )
            .into_response();
        }

        if state.tenant_rate_limiter.check_key(&tenant_id).is_err() {
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "TENANT_RATE_LIMITED",
                format!("Tenant '{}' exceeded request budget.", tenant_id),
                Some(1),
                &request_id,
                vec![serde_json::json!({
                    "field": "tenant_id",
                    "message": tenant_id
                })],
            )
            .into_response();
        }

        let mut final_results: Vec<Option<SentimentResult>> = vec![None; payload.symbols.len()];
        let mut symbols_to_fetch_news: Vec<(usize, Symbol)> = Vec::new();

        for (idx, symbol) in payload.symbols.into_iter().enumerate() {
            let cache_key = normalized_cache_key(&symbol.ticker);

            if let Some(cached) = state.cache.get(&cache_key).await {
                state.metrics.record_cache_hit_memory();
                debug!(
                    "request_id={} cache_hit source=memory symbol={} key={}",
                    request_id,
                    symbol.ticker,
                    cache_key
                );
                final_results[idx] = Some(cached);
                continue;
            }

            let mut redis_cached = false;
            if let Some(redis_client) = &state.redis_client {
                if let Ok(mut con) = redis_client.get_multiplexed_async_connection().await {
                    use redis::AsyncCommands;
                    if let Ok(json_res) = con.get::<_, String>(&cache_key).await {
                        if let Ok(cached) = serde_json::from_str::<SentimentResult>(&json_res) {
                            state.cache.insert(cache_key.clone(), cached.clone()).await;
                            state.metrics.record_cache_hit_redis();
                            debug!(
                                "request_id={} cache_hit source=redis symbol={} key={}",
                                request_id,
                                symbol.ticker,
                                cache_key
                            );
                            final_results[idx] = Some(cached);
                            redis_cached = true;
                        }
                    }
                }
            }

            if redis_cached {
                continue;
            }

            state.metrics.record_cache_miss();
            debug!(
                "request_id={} cache_miss symbol={} key={}",
                request_id,
                symbol.ticker,
                cache_key
            );
            symbols_to_fetch_news.push((idx, symbol));
        }

        let mut pending_llm_symbols: Vec<(usize, Symbol)> = Vec::new();

        if !symbols_to_fetch_news.is_empty() {
            let only_symbols: Vec<Symbol> = symbols_to_fetch_news
                .iter()
                .map(|(_, s)| s.clone())
                .collect();

            match providers::fetch_batch_news(
                &request_id,
                &only_symbols,
                payload.lookback_hours,
                payload.max_articles_per_symbol,
                &state.config,
                &state.http_client,
                &state.metrics,
            )
            .await
            {
                Ok(batch_outcome) => {
                    let can_escalate_to_llm =
                        batch_outcome.all_news_sources_attempted || batch_outcome.budget_exhausted;
                    for (orig_idx, symbol) in symbols_to_fetch_news {
                        if let Some(list) = batch_outcome.news_by_ticker.get(&symbol.ticker) {
                            if !list.is_empty() {
                                if let Ok((score, label, confidence)) =
                                    state.engine.analyze_sentiment(&combined_text_from_list(list)).await
                                {
                                    let res = SentimentResult {
                                        symbol: symbol.ticker.clone(),
                                        sentiment_score: score,
                                        label,
                                        confidence,
                                        source_tier: "tier_1_local_onnx".to_string(),
                                        news_provider: Some("batch_news".to_string()),
                                        article_count: list.len() as i32,
                                        reasoning: None,
                                    };

                                    let cache_key = normalized_cache_key(&symbol.ticker);

                                    state.cache.insert(cache_key.clone(), res.clone()).await;
                                    if let Some(redis_client) = &state.redis_client {
                                        if let Ok(mut con) =
                                            redis_client.get_multiplexed_async_connection().await
                                        {
                                            use redis::AsyncCommands;
                                            if let Ok(json_res) = serde_json::to_string(&res) {
                                                let _: () = con
                                                    .set_ex::<_, _, ()>(
                                                        &cache_key,
                                                        json_res,
                                                        state.config.cache_ttl_sec,
                                                    )
                                                    .await
                                                    .unwrap_or(());
                                            }
                                        }
                                    }
                                    final_results[orig_idx] = Some(res);
                                    continue;
                                }
                            }
                        }

                        if can_escalate_to_llm
                            && (state.config.grok_key.is_some()
                                || state.config.deepseek_key.is_some()
                                || state.config.openai_key.is_some())
                        {
                            state
                                .metrics
                                .record_fallback_transition("tier_2_news", "tier_3_llm");
                            pending_llm_symbols.push((orig_idx, symbol));
                        } else {
                            info!(
                                "News-tier incomplete and LLM escalation blocked (all_news_sources_attempted={}, budget_exhausted={})",
                                batch_outcome.all_news_sources_attempted,
                                batch_outcome.budget_exhausted
                            );
                            state.metrics.record_fallback_transition(
                                "tier_2_news",
                                "intelligence_exhaustion",
                            );
                            tier_exhausted = true;
                            return exhaustion_response(&request_id).into_response();
                        }
                    }
                }
                Err(e) => {
                    let safe_error = security::redact_sensitive_text(&format!("{}", e), &state.config);
                    info!("Batch news fetch failed: {}. Triggering intelligence exhaustion.", safe_error);
                    state
                        .metrics
                        .record_fallback_transition("tier_2_news", "intelligence_exhaustion");
                    tier_exhausted = true;
                    return exhaustion_response(&request_id).into_response();
                }
            }
        }

        for chunk in pending_llm_symbols.chunks(LLM_FALLBACK_BATCH_SIZE) {
            let symbols_to_fetch: Vec<Symbol> = chunk.iter().map(|(_, s)| s.clone()).collect();
            match providers::fetch_llm_batch_sentiment(
                &request_id,
                &symbols_to_fetch,
                &state.config,
                &state.http_client,
                &state.metrics,
            )
            .await
            {
                Ok(batch_results) => {
                    for res in batch_results {
                        if let Some((orig_idx, _)) = chunk.iter().find(|(_, s)| s.ticker == res.symbol) {
                            let cache_key = normalized_cache_key(&res.symbol);

                            state.cache.insert(cache_key.clone(), res.clone()).await;
                            if let Some(redis_client) = &state.redis_client {
                                if let Ok(mut con) =
                                    redis_client.get_multiplexed_async_connection().await
                                {
                                    use redis::AsyncCommands;
                                    if let Ok(json_res) = serde_json::to_string(&res) {
                                        let _: () = con
                                            .set_ex::<_, _, ()>(
                                                &cache_key,
                                                json_res,
                                                state.config.cache_ttl_sec,
                                            )
                                            .await
                                            .unwrap_or(());
                                    }
                                }
                            }

                            final_results[*orig_idx] = Some(res);
                        }
                    }
                }
                Err(e) => {
                    let safe_error = security::redact_sensitive_text(&format!("{}", e), &state.config);
                    info!("LLM Fallback failed: {}. Triggering Intelligence Exhaustion.", safe_error);
                    state
                        .metrics
                        .record_fallback_transition("tier_3_llm", "intelligence_exhaustion");
                    tier_exhausted = true;
                    return exhaustion_response(&request_id).into_response();
                }
            }
        }

        let results: Vec<SentimentResult> = final_results.into_iter().flatten().collect();

        let response = AnalyzeResponse {
            request_id,
            results,
            execution_time_ms: start.elapsed().as_millis(),
        };

        (StatusCode::OK, Json(response)).into_response()
    })
    .await;

    state
        .metrics
        .observe_request(start.elapsed(), tier_exhausted);
    response
}

#[derive(serde::Deserialize, Clone)]
struct JwtClaims {
    sub: Option<String>,
    tid: Option<String>,
    iss: Option<String>,
    exp: usize,
}

fn authenticate_and_resolve_tenant(
    headers: &HeaderMap,
    config: &Config,
    request_id: &str,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    match config.auth_mode.as_str() {
        "none" => Ok("anonymous".to_string()),
        "api_key" => {
            let provided = headers
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| unauthorized_response(request_id))?;

            config
                .auth_api_keys
                .iter()
                .find(|(_, key)| key == provided)
                .map(|(tenant, _)| tenant.clone())
                .ok_or_else(|| unauthorized_response(request_id))
        }
        "jwt" => {
            let auth_value = headers
                .get(AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| unauthorized_response(request_id))?;

            let token = auth_value
                .strip_prefix("Bearer ")
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| unauthorized_response(request_id))?;

            let secret = config
                .jwt_secret
                .as_ref()
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| unauthorized_response(request_id))?;

            let mut validation = Validation::new(Algorithm::HS256);
            if let Some(issuer) = config.jwt_issuer.as_ref().filter(|v| !v.trim().is_empty()) {
                validation.set_issuer(&[issuer]);
            }

            let data = decode::<JwtClaims>(
                token,
                &DecodingKey::from_secret(secret.as_bytes()),
                &validation,
            )
            .map_err(|_| unauthorized_response(request_id))?;

            let _issuer = data.claims.iss;
            let _exp = data.claims.exp;
            data.claims
                .tid
                .or(data.claims.sub)
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| unauthorized_response(request_id))
        }
        _ => Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "CONFIGURATION_ERROR",
            "Unsupported auth mode configured.",
            None,
            request_id,
            Vec::new(),
        )),
    }
}

fn unauthorized_response(request_id: &str) -> (StatusCode, Json<serde_json::Value>) {
    error_response(
        StatusCode::UNAUTHORIZED,
        "UNAUTHORIZED",
        "Valid authentication credentials are required for this endpoint.",
        None,
        request_id,
        Vec::new(),
    )
}

fn validate_analyze_request(payload: &AnalyzeRequest) -> Result<(), Vec<serde_json::Value>> {
    let mut errors: Vec<serde_json::Value> = Vec::new();

    if payload.symbols.is_empty() || payload.symbols.len() > MAX_SYMBOLS {
        errors.push(serde_json::json!({
            "code": "INVALID_SYMBOL_COUNT",
            "field": "symbols",
            "message": format!("symbols must contain between 1 and {} items", MAX_SYMBOLS)
        }));
    }

    let mut seen = std::collections::HashSet::new();
    for (index, symbol) in payload.symbols.iter().enumerate() {
        let ticker_trimmed = symbol.ticker.trim();
        let name_trimmed = symbol.name.trim();

        if ticker_trimmed.is_empty() || ticker_trimmed.len() > MAX_TICKER_LENGTH {
            errors.push(serde_json::json!({
                "code": "INVALID_TICKER",
                "field": format!("symbols[{}].ticker", index),
                "message": format!("ticker must be between 1 and {} characters", MAX_TICKER_LENGTH)
            }));
        }

        if name_trimmed.is_empty() || name_trimmed.len() > MAX_NAME_LENGTH {
            errors.push(serde_json::json!({
                "code": "INVALID_SYMBOL_NAME",
                "field": format!("symbols[{}].name", index),
                "message": format!("name must be between 1 and {} characters", MAX_NAME_LENGTH)
            }));
        }

        let ticker_key = ticker_trimmed.to_uppercase();
        if !ticker_key.is_empty() && !seen.insert(ticker_key) {
            errors.push(serde_json::json!({
                "code": "DUPLICATE_SYMBOL",
                "field": format!("symbols[{}].ticker", index),
                "message": "duplicate ticker detected in request"
            }));
        }
    }

    if payload.lookback_hours < MIN_LOOKBACK_HOURS || payload.lookback_hours > MAX_LOOKBACK_HOURS {
        errors.push(serde_json::json!({
            "code": "INVALID_LOOKBACK_HOURS",
            "field": "lookback_hours",
            "message": format!(
                "lookback_hours must be between {} and {}",
                MIN_LOOKBACK_HOURS,
                MAX_LOOKBACK_HOURS
            )
        }));
    }

    if payload.max_articles_per_symbol < MIN_MAX_ARTICLES_PER_SYMBOL
        || payload.max_articles_per_symbol > MAX_MAX_ARTICLES_PER_SYMBOL
    {
        errors.push(serde_json::json!({
            "code": "INVALID_MAX_ARTICLES",
            "field": "max_articles_per_symbol",
            "message": format!(
                "max_articles_per_symbol must be between {} and {}",
                MIN_MAX_ARTICLES_PER_SYMBOL,
                MAX_MAX_ARTICLES_PER_SYMBOL
            )
        }));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validation_error_response(
    details: Vec<serde_json::Value>,
    request_id: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    error_response(
        StatusCode::BAD_REQUEST,
        "INVALID_REQUEST",
        "Request validation failed.",
        None,
        request_id,
        details,
    )
}

fn error_response(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
    retry_after_seconds: Option<u64>,
    request_id: &str,
    details: Vec<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "code": code,
            "message": message.into(),
            "retry_after_seconds": retry_after_seconds,
            "request_id": request_id,
            "details": details,
        })),
    )
}

fn exhaustion_response(request_id: &str) -> (StatusCode, Json<serde_json::Value>) {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "INTELLIGENCE_EXHAUSTION",
        "All upstream providers are currently rate-limited or unreachable.",
        Some(300),
        request_id,
        vec![serde_json::json!({
            "tier_1": "exhausted",
            "tier_2": "exhausted",
            "tier_3_llm": "cooldown_active"
        })],
    )
}

fn combined_text_from_list(list: &[String]) -> String {
    list.join(" ")
}

fn normalized_cache_key(ticker: &str) -> String {
    ticker.trim().to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use governor::Quota;
    use governor::state::keyed::DefaultKeyedStateStore;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde::Serialize;
    use std::num::NonZeroU32;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn base_config() -> Config {
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
            deepseek_model: "deepseek-v4-flash".to_string(),
            openai_model: "gpt-5.4-nano".to_string(),
            redis_url: None,
            cache_ttl_sec: 300,
            rate_limit_per_min: 1,
            global_rate_limit_per_min: 10,
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

    #[test]
    fn auth_api_key_valid_maps_tenant() {
        let mut config = base_config();
        config.auth_mode = "api_key".to_string();
        config.auth_api_keys = vec![
            ("tenant-a".to_string(), "key-a".to_string()),
            ("tenant-b".to_string(), "key-b".to_string()),
        ];

        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("key-b"));

        let tenant = authenticate_and_resolve_tenant(&headers, &config, "tp_test")
            .expect("expected valid tenant");
        assert_eq!(tenant, "tenant-b");
    }

    #[test]
    fn auth_api_key_invalid_is_unauthorized() {
        let mut config = base_config();
        config.auth_mode = "api_key".to_string();
        config.auth_api_keys = vec![("tenant-a".to_string(), "key-a".to_string())];

        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("wrong-key"));

        let err = authenticate_and_resolve_tenant(&headers, &config, "tp_test")
            .expect_err("expected unauthorized");
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[derive(Serialize)]
    struct TestClaims {
        sub: Option<String>,
        tid: Option<String>,
        iss: Option<String>,
        exp: usize,
    }

    #[test]
    fn auth_jwt_valid_maps_tenant() {
        let mut config = base_config();
        config.auth_mode = "jwt".to_string();
        config.jwt_secret = Some("super-secret".to_string());
        config.jwt_issuer = Some("tierpulse-tests".to_string());

        let exp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_secs() as usize
            + 3600;

        let claims = TestClaims {
            sub: Some("subject-a".to_string()),
            tid: Some("tenant-jwt".to_string()),
            iss: Some("tierpulse-tests".to_string()),
            exp,
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret("super-secret".as_bytes()),
        )
        .expect("token encoding should succeed");

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token)).expect("header must be valid"),
        );

        let tenant = authenticate_and_resolve_tenant(&headers, &config, "tp_test")
            .expect("expected valid jwt tenant");
        assert_eq!(tenant, "tenant-jwt");
    }

    #[test]
    fn auth_jwt_invalid_is_unauthorized() {
        let mut config = base_config();
        config.auth_mode = "jwt".to_string();
        config.jwt_secret = Some("super-secret".to_string());

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer malformed.token.value"),
        );

        let err = authenticate_and_resolve_tenant(&headers, &config, "tp_test")
            .expect_err("expected unauthorized");
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn tenant_rate_limiter_is_scoped_per_tenant() {
        let limiter: RateLimiter<
            String,
            DefaultKeyedStateStore<String>,
            governor::clock::DefaultClock,
        > = RateLimiter::keyed(Quota::per_minute(
            NonZeroU32::new(1).expect("non zero quota"),
        ));

        assert!(limiter.check_key(&"tenant-a".to_string()).is_ok());
        assert!(limiter.check_key(&"tenant-a".to_string()).is_err());

        assert!(limiter.check_key(&"tenant-b".to_string()).is_ok());
    }

    fn valid_request() -> AnalyzeRequest {
        AnalyzeRequest {
            symbols: vec![Symbol {
                ticker: "AAPL".to_string(),
                name: "Apple Inc.".to_string(),
            }],
            lookback_hours: 24,
            max_articles_per_symbol: 5,
        }
    }

    #[test]
    fn validation_accepts_valid_request() {
        let request = valid_request();
        assert!(validate_analyze_request(&request).is_ok());
    }

    #[test]
    fn validation_rejects_duplicate_symbols_case_insensitive() {
        let mut request = valid_request();
        request.symbols.push(Symbol {
            ticker: "aapl".to_string(),
            name: "Apple Duplicate".to_string(),
        });

        let details =
            validate_analyze_request(&request).expect_err("expected duplicate symbol error");

        assert!(details.iter().any(|entry| entry.get("code")
            == Some(&serde_json::Value::String("DUPLICATE_SYMBOL".to_string()))));
    }

    #[test]
    fn validation_rejects_invalid_bounds() {
        let mut request = valid_request();
        request.lookback_hours = 0;
        request.max_articles_per_symbol = 999;

        let details = validate_analyze_request(&request).expect_err("expected validation failure");

        assert!(details.iter().any(|entry| entry.get("code")
            == Some(&serde_json::Value::String(
                "INVALID_LOOKBACK_HOURS".to_string()
            ))));
        assert!(details.iter().any(|entry| entry.get("code")
            == Some(&serde_json::Value::String(
                "INVALID_MAX_ARTICLES".to_string()
            ))));
    }

    #[test]
    fn llm_fallback_chunking_respects_boundary() {
        let pending: Vec<usize> = (0..31).collect();
        let chunk_sizes: Vec<usize> = pending
            .chunks(LLM_FALLBACK_BATCH_SIZE)
            .map(|chunk| chunk.len())
            .collect();

        assert_eq!(chunk_sizes, vec![15, 15, 1]);
        assert_eq!(chunk_sizes.len(), 3);
    }
}
