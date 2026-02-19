# tierpulse: High-Scale Financial Sentiment Intelligence Engine

[![Pipeline Status](https://github.com/kabudu/tierpulse/actions/workflows/pipeline.yml/badge.svg)](https://github.com/kabudu/tierpulse/actions/workflows/pipeline.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**tierpulse** is an institutional-grade, high-throughput financial sentiment analysis engine built in **Rust**. It utilizes a three-tier "Intelligence Failover" strategy with **High-Scale Batching** to provide ultra-reliable sentiment analyzes for trading bots and financial applications.

## 🚀 Key Features

- **Blazing Fast Inference:** Powered by **Axum** and **ONNX Runtime (`ort`)**
- **High-Scale Batching:** Requests are automatically batched for both news providers and LLMs (Grok/DeepSeek) to minimize latency and RTT.
- **Sequential Failover:** Transparently switches between news providers (Tiingo, MarketAux, Finnhub) and LLMs (Grok, DeepSeek) to preserve API quotas and ensure availability.
- **Zero-Bloat Container:** Distroless base image (~85MB) for minimal attack surface and fast startup.
- **Scalable Caching:** Concurrent **Moka** (In-Memory) and **Redis** (Distributed) caching strategy with multiplexed connections.
- **Traffic Control:** Token Bucket rate-limiting protects upstream provider quotas.
- **Resilient Upstream Calls:** Bounded retries with jittered exponential backoff for transient network errors and HTTP 5xx responses (HTTP 429 is not retried).
- **Intelligence Exhaustion:** Graceful degradation with detailed status reporting when all tiers are unavailable.

## 🛠 Tech Stack

- **Server:** Rust, Axum, Tokio, Tower
- **ML Engine:** ONNX Runtime (Quantized INT8 FinBERT)
- **Caching:** Moka, Redis
- **Infra:** Docker (Multi-stage), GitHub Actions, Distroless

## 📦 Getting Started

### 1. Environment Configuration

Create a `.env` file or set environment variables. All variables prefixed with `TP_` are used to configure the service.

| Variable                              | Default      | Description                                                                                                      |
| :------------------------------------ | :----------- | :--------------------------------------------------------------------------------------------------------------- |
| `PORT`                                | `8080`       | Server listening port.                                                                                           |
| `TP_TIINGO_KEY`                       | **Required** | Primary news provider API key.                                                                                   |
| `TP_FINNHUB_KEY`                      | `null`       | Tertiary news provider key (final fallback tier).                                                                |
| `TP_MARKETAUX_KEY`                    | `null`       | Secondary news provider key (batched fallback tier).                                                             |
| `TP_GROK_KEY`                         | `null`       | xAI API key (utilizes `grok-4-1-fast-reasoning`).                                                                |
| `TP_DEEPSEEK_KEY`                     | `null`       | DeepSeek API key (utilizes `deepseek-chat`).                                                                     |
| `TP_PRIMARY_LLM`                      | `grok`       | Primary LLM engine (`grok` or `deepseek`).                                                                       |
| `TP_REDIS_URL`                        | `null`       | Redis URL for distributed caching (e.g., `redis://localhost:6379`).                                              |
| `TP_CACHE_TTL`                        | `300`        | In-memory/Redis cache expiration in seconds.                                                                     |
| `TP_AUTH_MODE`                        | `none`       | Authentication mode: `none`, `api_key`, or `jwt`.                                                                |
| `TP_AUTH_API_KEYS`                    | `null`       | Required for `api_key` mode. Format: `tenantA:keyA,tenantB:keyB`.                                                |
| `TP_JWT_SECRET`                       | `null`       | Required for `jwt` mode (HS256 signing secret).                                                                  |
| `TP_JWT_ISSUER`                       | `null`       | Optional JWT issuer (`iss`) validation.                                                                          |
| `TP_RATE_LIMIT`                       | `100`        | Per-tenant tokens per minute (tenant-scoped limiter).                                                            |
| `TP_GLOBAL_RATE_LIMIT`                | `1000`       | Global protection guard tokens per minute (service-wide limiter).                                                |
| `TP_EGRESS_ALLOWLIST`                 | `""`         | Optional comma-separated extra HTTPS hosts allowed for outbound egress (Layer 1).                                |
| `TP_PROVIDER_CALL_BUDGET_PER_REQUEST` | `6`          | Max outbound news-provider calls allowed per `/analyze` request before news-tier budget is considered exhausted. |
| `TP_ONNX_THREADS`                     | `2`          | CPU thread allocation for the `ort` session.                                                                     |
| `TP_MODEL_PATH`                       | `model.onnx` | Path to the INT8 quantized ONNX model.                                                                           |
| `TP_LOG_LEVEL`                        | `INFO`       | Logging level (`DEBUG`, `INFO`, `WARNING`, `ERROR`).                                                             |

### Authentication Recommendation: API Key vs JWT

- **JWT (recommended for production multi-tenant systems):** Better for identity propagation, expiry, revocation workflows, and integration with existing IdPs.
- **API key (recommended for internal/MVP services):** Simpler to bootstrap and operate, but weaker lifecycle and claims semantics.

This service supports both via env configuration:

- `TP_AUTH_MODE=api_key` + `TP_AUTH_API_KEYS=tenantA:keyA,tenantB:keyB`
- `TP_AUTH_MODE=jwt` + `TP_JWT_SECRET=...` (+ optional `TP_JWT_ISSUER=...`)

When auth is enabled, tenant identity is extracted at request time and enforced through tenant-scoped rate limiting.

### Security Hardening (Logging + Provider Auth)

- **Centralized redaction:** Runtime log sanitization now redacts known secret values, bearer tokens, and sensitive query params such as `token`, `api_token`, `api_key`, `authorization`, and `x-api-key`.
- **Sensitive request headers:** `Authorization` and `x-api-key` are marked as sensitive in the HTTP layer to prevent accidental exposure in request logs.
- **Header-over-query preference:** For providers that support it (for example Tiingo), credentials are sent via headers instead of query-string tokens.
- **Fallback behavior:** Providers that only support query tokens remain supported, with token values redacted from dynamic log output.
- **Retry policy:** Outbound provider/LLM calls use bounded jittered exponential backoff retries for HTTP 5xx and transient network errors; HTTP 429 is intentionally not retried to preserve quota-pooling behavior.
- **Strict LLM escalation guard:** Tier-3 LLM fallback is only allowed after news sources were fully attempted in order (`Tiingo -> MarketAux -> Finnhub`) or when `TP_PROVIDER_CALL_BUDGET_PER_REQUEST` is exhausted.
- **Outbound egress allowlist (Layer 1):** Application-level outbound URL enforcement only allows HTTPS traffic to approved provider domains (`api.tiingo.com`, `finnhub.io`, `api.marketaux.com`, `api.x.ai`, `api.deepseek.com`).
- **Config-driven egress override:** `TP_EGRESS_ALLOWLIST` can extend the allowed host set (for controlled environment-specific endpoints) without code changes.

### Operational Endpoints (Observability + Health)

- `GET /metrics` returns Prometheus-style metrics text for:
  - `request_duration_ms` (quantiles: p50/p95/p99)
  - `cache_hit_ratio`
  - `provider_error_rate{provider,status_class}`
  - `fallback_transition_count{from,to}`
  - `tier_exhaustion_rate`
- `GET /health/live` is a liveness probe (process is up).
- `GET /health/ready` is a readiness probe with per-tier status, degradation reason, and breaker state.
- `GET /health` remains mapped to readiness for backward compatibility.

Readiness response shape (example):

```json
{
  "status": "degraded",
  "tiers": {
    "tier_1_local_onnx": {
      "status": "operational",
      "degradation_reason": null,
      "breaker_state": "not_configured"
    },
    "tier_2_news": {
      "status": "operational",
      "degradation_reason": "primary_news_provider_unavailable_using_fallback_capacity",
      "breaker_state": "not_configured",
      "providers": {
        "tiingo": { "status": "degraded", "breaker_state": "not_configured" },
        "marketaux": {
          "status": "configured",
          "breaker_state": "not_configured"
        },
        "finnhub": {
          "status": "not_configured",
          "breaker_state": "not_configured"
        }
      }
    },
    "tier_3_llm": {
      "status": "degraded",
      "degradation_reason": "no_llm_provider_configured",
      "breaker_state": "not_configured",
      "providers": {
        "grok": {
          "status": "not_configured",
          "breaker_state": "not_configured"
        },
        "deepseek": {
          "status": "not_configured",
          "breaker_state": "not_configured"
        }
      }
    }
  }
}
```

Provider auth transport summary (verified against provider docs):

- **Tiingo:** Supports both query `token` and `Authorization: Token <api_token>` header for REST.
- **Finnhub:** Supports both query `token` and `X-Finnhub-Token: <api_key>` header for GET requests.
- **MarketAux:** Documentation uses `api_token` as a query parameter for REST requests.

### 2. Run with Docker

```bash
docker pull boxedcode/tierpulse:latest
docker run -p 8080:8080 --env-file .env boxedcode/tierpulse:latest
```

## 📊 API Contract

### `POST /api/v1/analyze`

Analyze sentiment for a list of symbols with automatic batching and multi-tier failover.

**Request validation (enforced):**

- `symbols`: required, 1 to 50 items
- `symbols[].ticker`: required, trimmed length 1 to 16
- `symbols[].name`: required, trimmed length 1 to 120
- `symbols[].ticker`: must be unique within the request (case-insensitive)
- `lookback_hours`: 1 to 168
- `max_articles_per_symbol`: 1 to 20

All error responses use a standardized envelope:

- `code`: machine-readable error code
- `message`: human-readable summary
- `retry_after_seconds`: retry guidance when applicable (`null` if not applicable)
- `request_id`: correlation ID for support and tracing
- `details`: array of structured detail objects (may be empty)

**Error Codes**

| Code                      | HTTP Status | Meaning                                                                    | Recovery Guidance                                                                              |
| :------------------------ | :---------- | :------------------------------------------------------------------------- | :--------------------------------------------------------------------------------------------- |
| `INVALID_REQUEST`         | `400`       | Request payload failed schema/semantic validation.                         | Fix request fields based on `details[]` and retry.                                             |
| `UNAUTHORIZED`            | `401`       | Missing/invalid API key or JWT credentials.                                | Provide valid auth headers (`x-api-key` or `Authorization: Bearer <jwt>`) and retry.           |
| `TENANT_RATE_LIMITED`     | `429`       | Tenant-scoped rate limiter was exceeded.                                   | Back off for `retry_after_seconds`, then retry with jitter; reduce tenant request burst.       |
| `GLOBAL_RATE_LIMITED`     | `429`       | Global protection guard was exceeded.                                      | Back off for `retry_after_seconds`, then retry with jitter; reduce overall traffic/load.       |
| `INTELLIGENCE_EXHAUSTION` | `503`       | All provider/LLM tiers are unavailable or exhausted for this request path. | Retry after `retry_after_seconds`; verify provider key health/quota and upstream reachability. |

**Headers (when auth is enabled):**

- API key mode: `x-api-key: <tenant key>`
- JWT mode: `Authorization: Bearer <jwt>` (tenant identity from `tid` or `sub` claim)

**Request:**

```json
{
  "symbols": [
    { "ticker": "AAPL", "name": "Apple Inc." },
    { "ticker": "TSLA", "name": "Tesla, Inc." },
    { "ticker": "BTC", "name": "Bitcoin" }
  ],
  "lookback_hours": 24,
  "max_articles_per_symbol": 5
}
```

**Response:**

```json
{
  "request_id": "tp_550e8400-e29b-41d4-a716-446655440000",
  "results": [
    {
      "symbol": "AAPL",
      "sentiment_score": 0.82,
      "label": "bullish",
      "confidence": 0.94,
      "source_tier": "tier_1_local_onnx",
      "news_provider": "batch_news",
      "article_count": 5,
      "reasoning": null
    },
    {
      "symbol": "BTC",
      "sentiment_score": -0.45,
      "label": "bearish",
      "confidence": 0.88,
      "source_tier": "tier_3_llm",
      "news_provider": null,
      "article_count": 0,
      "reasoning": "Recent regulatory tightening in EU leading to cautious sentiment."
    }
  ],
  "execution_time_ms": 450
}
```

**Validation Error Response (`400`)**

```json
{
  "code": "INVALID_REQUEST",
  "message": "Request validation failed.",
  "retry_after_seconds": null,
  "request_id": "tp_550e8400e29b41d4a716446655440000",
  "details": [
    {
      "code": "INVALID_SYMBOL_COUNT",
      "field": "symbols",
      "message": "symbols must contain between 1 and 50 items"
    },
    {
      "code": "INVALID_LOOKBACK_HOURS",
      "field": "lookback_hours",
      "message": "lookback_hours must be between 1 and 168"
    }
  ]
}
```

**Tenant Rate Limit Response (`429`)**

```json
{
  "code": "TENANT_RATE_LIMITED",
  "message": "Tenant 'tenantA' exceeded request budget.",
  "retry_after_seconds": 1,
  "request_id": "tp_550e8400e29b41d4a716446655440000",
  "details": [
    {
      "field": "tenant_id",
      "message": "tenantA"
    }
  ]
}
```

**Intelligence Exhaustion Response (`503`)**

```json
{
  "code": "INTELLIGENCE_EXHAUSTION",
  "message": "All upstream providers are currently rate-limited or unreachable.",
  "retry_after_seconds": 300,
  "request_id": "tp_550e8400e29b41d4a716446655440000",
  "details": [
    {
      "tier_1": "exhausted",
      "tier_2": "exhausted",
      "tier_3_llm": "cooldown_active"
    }
  ]
}
```

## 🏗 CI/CD Architecture

The system utilizes a multi-stage pipeline:

1. **Model Prep:** Python prunes/quantizes `finbert` to INT8 ONNX.
2. **Build:** Rust compiles the binary using a stripped release profile.
3. **Deploy:** GitHub Actions pushes the final image to Docker Hub with automatic Semantic Versioning.

### Quality Gates & Contract Enforcement

- **CI quality gates:** `.github/workflows/pipeline.yml` runs `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets --all-features`, and `cargo audit`.
- **Integration test coverage:** `tests/provider_failover_llm_schema_tests.rs` verifies provider failover ordering parity and strict LLM schema parsing behavior.
- **HTTP contract/error coverage:** `tests/analyze_validation_http_tests.rs` validates typed error envelope parity (`400/401/429/503`) and operational endpoint exposure.
- **Versioned OpenAPI contract:** `openapi/openapi.v1.yaml` is the source-of-truth versioned API contract for public endpoints.
- **Contract enforcement path:** `tests/openapi_contract_tests.rs` validates core OpenAPI invariants (version, required paths, response envelope refs, and `ErrorEnvelope` required fields) and is executed by the dedicated CI `contract-test` job.

## 🤝 Contributing

We welcome contributions of all kinds! Please see our [Contributing Guide](CONTRIBUTORS.md) for details on how to get started, our code of conduct, and our pull request process.

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
