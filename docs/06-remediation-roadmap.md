# Remediation Roadmap

## Goal

Close conformance gaps against `Implementation.md` while raising production readiness for security, reliability, and maintainability.

## Phase Plan

## Phase 0 (Week 1) — Correctness & Safety Baseline

### Deliverables

- Maintain request validation coverage (symbols, bounds, payload limits).
- Publish the normalized error contract (`retry_after_seconds`, stable `code`) through OpenAPI/schema docs.
- Maintain LLM response parsing with schema validation and fallback handling.

### Exit Criteria

- All validation failure classes return deterministic 400 responses and are covered by contract tests.
- No panic paths in primary request flow under malformed upstream payloads.

## Phase 1 (Weeks 2–3) — Resilience & Security Hardening

### Deliverables

- Add circuit breaker per upstream provider and expose breaker state in health.
- Keep bounded retries with jittered backoff for transient failures covered by tests.
- Introduce tenant-aware auth and per-tenant rate limits.
- Add structured redaction middleware and secret leak tests.

### Exit Criteria

- Breaker transitions validated in integration tests.
- Auth and quota enforcement verified with negative tests.
- Secret redaction tests pass against structured logs.

## Phase 2 (Weeks 4–6) — Operability & Performance Governance

### Deliverables

- Add metrics and dashboards for tier transitions, latencies, errors, cache hit ratio.
- Add load test suite and baseline performance budgets.
- Add CI gates: format, lint, tests, security scan, image size threshold.

### Exit Criteria

- p95/p99 latency baselines established and tracked in CI/perf reports.
- SLOs published and linked to dashboards/runbooks.
- Release pipeline blocks on failing quality gates.

## Ownership Model

- **Platform/Infra**: CI gates, container posture, release controls.
- **Backend**: failover correctness, validation, breaker/retries.
- **SRE/SecEng**: observability, security controls, incident readiness.

## Risk Register (Top)

1. Increased latency if retries are unbounded or poorly tuned.
2. High-cardinality metrics explosion without dimensional hygiene.

## Mitigation

- Roll out behind feature flags.
- Canary in low-traffic environment with fallback telemetry.
- Cap metric labels and review before production promotion.
