# Requirements Traceability Matrix

Baseline: `Implementation.md` (Specification 5.1)

## Summary

- **Met**: 11
- **Partially Met**: 8
- **Not Met**: 2

## Matrix

| Spec Area             | Requirement                                                | Status        | Evidence                                                                           | Gap / Action                                             |
| --------------------- | ---------------------------------------------------------- | ------------- | ---------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Core stack            | Rust + Axum + ONNX Runtime                                 | Met           | `src/main.rs`, `src/inference.rs`, `Cargo.toml`                                    | None                                                     |
| Leanness              | Distroless final image ~85MB target                        | Partially Met | `Dockerfile` uses distroless                                                       | No size gate in CI; no artifact size assertion           |
| Tier 1 model pipeline | Pruning + INT8 quantization                                | Partially Met | `scripts/export_model.py` has conceptual pruning comment; quantization implemented | Pruning is not concretely implemented/verified           |
| Sequential failover   | Tiingo -> MarketAux -> Alpha Vantage -> Finnhub (batch-optimized) | Met           | `fetch_batch_news` and `fetch_news` implement the same provider order              | None                                                     |
| Caching               | Moka + optional Redis, TTL control                         | Met           | `src/main.rs`, `src/lib.rs`, `src/config.rs`                                       | Add cache key normalization and stale-read strategy      |
| Log masking           | API keys masked in logs                                    | Partially Met | `Config` debug masking; sensitive headers layer for auth header                    | No global structured redaction middleware for all fields |
| Rate limiting         | Token bucket on `/analyze`                                 | Met           | `src/lib.rs` rate limiter check                                                    | Add per-tenant dimensions and burst tuning               |
| API endpoint          | `POST /api/v1/analyze` contract                            | Partially Met | Implemented in `src/lib.rs` with validation and typed error payloads               | Add generated OpenAPI contract tests                     |
| Health endpoint       | Upstream availability + circuit-breaker status             | **Not Met**   | `/health` reports partial upstream booleans                                        | No circuit-breaker state machine/status exposed          |
| Status behavior       | 400 on validation issues                                   | Met           | `validate_analyze_request` enforces symbol, duplicate, lookback, and article bounds | Add OpenAPI/schema generation                            |
| Exhaustion behavior   | 503 with structured payload                                | Met           | `exhaustion_response()` in `src/lib.rs`                                            | Add tier-specific reason codes and cooldown metadata     |
| LLM fallback          | Strict JSON contract                                       | Met           | Provider requests use JSON-object mode and parser validates the `results` array    | Add mocked upstream tests for provider-specific 400s     |
| LLM model routing     | Configurable Grok/DeepSeek/OpenAI execution and fallback order | Met           | `src/providers/mod.rs`, `TP_LLM_PROVIDER_ORDER`                                    | Add integration tests with mocked upstream providers     |
| CI/CD                 | Build/push on main/tags                                    | Met           | `.github/workflows/deploy.yml`                                                     | Add tests/security scan/size gates                       |
| Testing               | Sequential failover, token bucket, log masking, ONNX drift | Partially Met | `tests/unit_tests.rs`, `src/inference.rs` tests exist                              | Tests are mocks/placeholders, low behavior confidence    |
| Security controls     | Quota preservation + abuse prevention                      | Partially Met | Optional API key/JWT auth, tenant rate limits, payload bounds, and egress allowlist | Add stronger abuse analytics and deployment runbooks     |
| Reliability           | 5s timeouts and fallback                                   | Partially Met | Timeouts, bounded jittered retries, and fallback paths exist                       | No circuit break/open-half-open states                   |
| Observability         | Operational telemetry readiness                            | Partially Met | JSON logging and `/metrics` exporter exist                                         | Add trace correlation dashboards/runbooks                |
| Config                | TP\_ environment conventions                               | Met           | `src/config.rs`, `README.md`, `Dockerfile`                                         | None                                                     |
| API docs              | Updated request/response examples                          | Met           | `README.md`                                                                        | Add explicit error taxonomy docs                         |
| Readiness posture     | “Five nines” claim support                                 | **Not Met**   | No SLOs, load-test evidence, resilience playbooks                                  | Add SLOs, load tests, and canary strategy                |

## Key Contradictions to Resolve First

1. Health endpoint advertises circuit-breaker status but no breaker exists.
2. Circuit-breaker status is currently reported as `not_configured`; no breaker state machine exists yet.
3. Some production confidence still depends on adding mocked upstream integration tests.
