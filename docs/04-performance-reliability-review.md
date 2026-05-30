# Performance & Reliability Review

## Observed Strengths

- Async architecture with Tokio and Axum supports high concurrency.
- Batch orchestration and LLM chunking reduce external RTT amplification.
- In-memory + Redis cache layering is directionally correct.

## Material Gaps

### 1) Failover Strategy Inconsistency (High)

Resolved in implementation: batch and single-symbol paths now use the same provider ordering (`Tiingo -> MarketAux -> Alpha Vantage -> Finnhub`), and the parity test guards against future drift.

### 2) Circuit Breaker Missing (High)

Timeouts exist, but no open/half-open/closed breaker state for unstable dependencies.

### 3) Retry Policy Incomplete (High)

Resolved in implementation: outbound provider and LLM calls use bounded retries with jittered exponential backoff for transient network failures and HTTP 5xx responses. HTTP 429 is intentionally excluded.

### 4) Limited Concurrency Controls (Medium-High)

No explicit concurrency bulkheads/semaphores for outbound providers and LLM tiers.

### 5) Observability Gaps (Medium)

The `/metrics` endpoint exposes request duration quantiles, cache hit ratio, provider error rate, fallback transition count, and tier exhaustion rate. Remaining gaps are dashboards, trace correlation, and alert/runbook definitions.

### 6) Graceful Shutdown & Draining (Medium)

No explicit shutdown hooks for in-flight request draining and dependency cleanup.

### 7) Test Realism and Capacity Evidence (Medium)

No load test artifacts, percentile latency baselines, or regression performance gates.

## Performance Recommendations

### Immediate

- Keep failover-order parity covered by tests.
- Keep bounded retries (HTTP 5xx/transient network only; exclude 429) covered by policy tests.
- Introduce per-provider concurrency semaphores and request budgets.

### Near-term

- Add dashboards and alert thresholds for `request_duration_ms`, `cache_hit_ratio`, `provider_error_rate`, `fallback_transition_count`, and `tier_exhaustion_rate`.
- Keep readiness and liveness split endpoints wired into deployment probes.

### Longer-term

- Run load tests with realistic symbol cardinality distributions.
- Set and enforce SLOs (e.g., 99% under target latency under defined load).

## Reliability Acceptance Criteria

- Verified failover order parity between batch/single paths.
- Circuit breaker transitions observed in tests and exposed via health/metrics once circuit breakers are implemented.
- Load test demonstrates stable p95/p99 with no runaway fallback loops.
