# UX / DX / Operability Review

## API Consumer UX

### Findings

- Error payloads now use `retry_after_seconds`, but the full taxonomy still needs an OpenAPI contract.
- Validation exists for core analyze payload bounds and duplicate tickers.
- Readiness exposes per-tier provider state, degradation reasons, and placeholder breaker state.

### Recommendations

- Generate and version the error schema: `{ code, message, retry_after_seconds, request_id, details[] }`.
- Document all 4xx/5xx codes with examples and recovery guidance.
- Replace placeholder breaker state with real circuit-breaker state once implemented.

## Developer Experience (DX)

### Findings

- Tests cover validation, auth, rate limiting, failover order parity, retry policy, log redaction, and LLM schema parsing; mocked upstream integration coverage is still thin.
- No quality gates in CI for unit tests, integration tests, linting, or security scans.
- No explicit API schema (OpenAPI) to prevent drift between docs and implementation.

### Recommendations

- Add CI stages: `fmt`, `clippy`, `test`, and dependency vulnerability scan.
- Introduce mocked upstream integration tests for provider failover and LLM provider fallback.
- Generate and version OpenAPI contract; enforce contract tests.

## Operability (SRE Lens)

### Findings

- No SLOs/error budgets defined despite high-availability claims.
- No dashboards/runbooks included for incident handling.
- Logging exists, but trace correlation and metric cardinality strategy are missing.

### Recommendations

- Define SLOs for latency, availability, and correctness.
- Add golden dashboards and pager runbooks for tier exhaustion incidents.
- Emit correlation IDs end-to-end and include provider/tier decision metadata.

## Quick Wins

1. Generate OpenAPI/schema docs for the current error envelope.
2. Add mocked upstream tests that verify real fallback transitions.
3. Add provider-specific LLM 400 fixtures for xAI, DeepSeek, and OpenAI.
4. Add `clippy + tests` as mandatory CI checks.
