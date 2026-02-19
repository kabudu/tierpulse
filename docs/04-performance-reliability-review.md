# Performance & Reliability Review

## Observed Strengths

- Async architecture with Tokio and Axum supports high concurrency.
- Batch orchestration and LLM chunking reduce external RTT amplification.
- In-memory + Redis cache layering is directionally correct.

## Material Gaps

### 1) Failover Strategy Inconsistency (High)

Batch and single-symbol paths do not use identical provider ordering, causing inconsistent latency/cost behavior.

### 2) Circuit Breaker Missing (High)

Timeouts exist, but no open/half-open/closed breaker state for unstable dependencies.

### 3) Retry Policy Incomplete (High)

No bounded retries with jittered exponential backoff; transient failures immediately escalate tiers.

### 4) Limited Concurrency Controls (Medium-High)

No explicit concurrency bulkheads/semaphores for outbound providers and LLM tiers.

### 5) Observability Gaps (Medium-High)

No histograms for per-tier latency, cache hit ratio, provider error classes, fallback rates, or saturation signals.

### 6) Graceful Shutdown & Draining (Medium)

No explicit shutdown hooks for in-flight request draining and dependency cleanup.

### 7) Test Realism and Capacity Evidence (Medium)

No load test artifacts, percentile latency baselines, or regression performance gates.

## Performance Recommendations

### Immediate

- Standardize failover ordering across all code paths.
- Add bounded retries (HTTP 5xx/transient network only; exclude 429) with jittered backoff.
- Introduce per-provider concurrency semaphores and request budgets.

### Near-term

- Add metrics:
  - `request_duration_ms` (p50/p95/p99)
  - `cache_hit_ratio`
  - `provider_error_rate{provider,status_class}`
  - `fallback_transition_count{from,to}`
  - `tier_exhaustion_rate`
- Add readiness and liveness split endpoints.

### Longer-term

- Run load tests with realistic symbol cardinality distributions.
- Set and enforce SLOs (e.g., 99% under target latency under defined load).

## Reliability Acceptance Criteria

- Verified failover order parity between batch/single paths.
- Circuit breaker transitions observed in tests and exposed via health/metrics.
- Load test demonstrates stable p95/p99 with no runaway fallback loops.
