# Requirements Traceability Matrix

Baseline: `Implementation.md` (Specification 5.1)

## Summary

- **Met**: 9
- **Partially Met**: 12
- **Not Met**: 4

## Matrix

| Spec Area             | Requirement                                                | Status        | Evidence                                                                           | Gap / Action                                             |
| --------------------- | ---------------------------------------------------------- | ------------- | ---------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Core stack            | Rust + Axum + ONNX Runtime                                 | Met           | `src/main.rs`, `src/inference.rs`, `Cargo.toml`                                    | None                                                     |
| Leanness              | Distroless final image ~85MB target                        | Partially Met | `Dockerfile` uses distroless                                                       | No size gate in CI; no artifact size assertion           |
| Tier 1 model pipeline | Pruning + INT8 quantization                                | Partially Met | `scripts/export_model.py` has conceptual pruning comment; quantization implemented | Pruning is not concretely implemented/verified           |
| Sequential failover   | Tiingo -> MarketAux -> Finnhub (batch-optimized)           | Met           | `fetch_batch_news` implements Tiingo -> MarketAux -> Finnhub                       | None                                                     |
| Caching               | Moka + optional Redis, TTL control                         | Met           | `src/main.rs`, `src/lib.rs`, `src/config.rs`                                       | Add cache key normalization and stale-read strategy      |
| Log masking           | API keys masked in logs                                    | Partially Met | `Config` debug masking; sensitive headers layer for auth header                    | No global structured redaction middleware for all fields |
| Rate limiting         | Token bucket on `/analyze`                                 | Met           | `src/lib.rs` rate limiter check                                                    | Add per-tenant dimensions and burst tuning               |
| API endpoint          | `POST /api/v1/analyze` contract                            | Partially Met | Implemented in `src/lib.rs`                                                        | Missing strict request validation and error semantics    |
| Health endpoint       | Upstream availability + circuit-breaker status             | **Not Met**   | `/health` reports partial upstream booleans                                        | No circuit-breaker state machine/status exposed          |
| Status behavior       | 400 on validation issues                                   | **Not Met**   | No explicit payload validation logic                                               | Add schema and semantic validation with typed errors     |
| Exhaustion behavior   | 503 with structured payload                                | Met           | `exhaustion_response()` in `src/lib.rs`                                            | Add tier-specific reason codes and cooldown metadata     |
| LLM fallback          | Strict JSON contract                                       | Partially Met | `fetch_llm_batch_sentiment` parses JSON content                                    | Response format and parser contract are brittle          |
| LLM model routing     | Grok/DeepSeek selection                                    | Met           | `src/providers/mod.rs`, `primary_llm`                                              | Add model fallback within tier and result validation     |
| CI/CD                 | Build/push on main/tags                                    | Met           | `.github/workflows/deploy.yml`                                                     | Add tests/security scan/size gates                       |
| Testing               | Sequential failover, token bucket, log masking, ONNX drift | Partially Met | `tests/unit_tests.rs`, `src/inference.rs` tests exist                              | Tests are mocks/placeholders, low behavior confidence    |
| Security controls     | Quota preservation + abuse prevention                      | Partially Met | Token bucket present                                                               | Missing authn/authz, payload bounds, anti-abuse controls |
| Reliability           | 5s timeouts and fallback                                   | Partially Met | Timeouts present in providers                                                      | No jittered retries/circuit break/open-half-open states  |
| Observability         | Operational telemetry readiness                            | Partially Met | JSON logging enabled                                                               | No metrics exporter/histograms/trace correlation         |
| Config                | TP\_ environment conventions                               | Met           | `src/config.rs`, `README.md`, `Dockerfile`                                         | None                                                     |
| API docs              | Updated request/response examples                          | Met           | `README.md`                                                                        | Add explicit error taxonomy docs                         |
| Readiness posture     | “Five nines” claim support                                 | **Not Met**   | No SLOs, load-test evidence, resilience playbooks                                  | Add SLOs, load tests, and canary strategy                |

## Key Contradictions to Resolve First

1. Health endpoint advertises circuit-breaker status but no breaker exists.
2. Validation error behavior documented, but no validation implementation.
3. Test requirements listed, but current tests do not validate production behavior.
