# UX / DX / Operability Review

## API Consumer UX

### Findings

- Error payloads are inconsistent (`retry_after_ms` in 429 vs `retry_after_seconds` in 503).
- Validation error taxonomy is undocumented and largely unimplemented.
- Health endpoint does not provide actionable dependency state beyond booleans.

### Recommendations

- Standardize error schema: `{ code, message, retry_after_seconds, request_id, details[] }`.
- Document all 4xx/5xx codes with examples and recovery guidance.
- Expand health/readiness output with per-tier degradation reason and breaker state.

## Developer Experience (DX)

### Findings

- Tests are primarily conceptual; confidence in refactors is low.
- No quality gates in CI for unit tests, integration tests, linting, or security scans.
- No explicit API schema (OpenAPI) to prevent drift between docs and implementation.

### Recommendations

- Add CI stages: `fmt`, `clippy`, `test`, and dependency vulnerability scan.
- Introduce integration tests for provider failover and LLM schema handling.
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

1. Unify retry field units and error envelope format.
2. Add request validation and deterministic error codes.
3. Add integration tests that verify real fallback transitions.
4. Add `clippy + tests` as mandatory CI checks.
