# Security Review

## Executive Security Posture

The service has foundational controls (rate limiting, basic masking in config debug output), but overall posture is **insufficient for internet-exposed production**.

## Threat Findings

### 1) No Authentication/Authorization Boundary (High)

- Any caller can invoke costly upstream requests.
- This creates quota exhaustion and cost abuse risk.

**Recommendation**

- Add API key/JWT auth at edge and enforce tenant identity in handler context.
- Move from global limiter to tenant-scoped limiter + global circuit guard.

### 2) Missing Request Validation and Bounds (High)

- No enforcement for empty symbol arrays, duplicate symbols, oversized payloads, malformed ticker/name lengths, or invalid lookback/article bounds.
- Potential for memory/CPU abuse and undefined behavior.

**Recommendation**

- Add schema and semantic validation with hard caps (e.g., max symbols, max name length, max lookback).
- Return typed 400 errors with stable machine-readable codes.

### 3) Incomplete Secrets Redaction Strategy (Medium-High)

- API keys are masked in `Config` debug output, but there is no comprehensive redaction middleware for all structured logs.
- URL query strings currently carry provider tokens.

**Recommendation**

- Centralize log redaction for known key patterns and query params.
- Prefer headers over query tokens where providers allow.

**Provider Auth Conformance Note (Verified)**

- **Tiingo:** REST auth supports both query `token` and header `Authorization: Token <api_token>`.
- **Finnhub:** GET auth supports both query `token=apiKey` and header `X-Finnhub-Token: apiKey`.
- **MarketAux:** Public REST docs use query `api_token`; no equivalent header auth was documented in review.

Implementation alignment: Tiingo and Finnhub are configured to use headers (to minimize token exposure in URLs), while MarketAux remains query-token based with centralized redaction controls.

### 4) LLM Output Trust Surface (Medium-High)

- LLM outputs are parsed directly as model responses with minimal structural validation.
- Mismatch between expected response format and parser assumptions can trigger parse failures and outages.

**Recommendation**

- Enforce strict JSON schema validation post-response.
- Reject and retry on malformed output with bounded attempts.

### 5) No Egress Policy / Domain Allowlist (Medium)

- Outbound requests are hardcoded but not policy-enforced.
- In larger deployments this weakens blast-radius controls.

**Recommendation**

- Add explicit outbound policy and environment-level egress restrictions.

### 6) No Audit/Event Security Telemetry (Medium)

- Missing structured security events for auth failures, abuse spikes, and anomaly patterns.

**Recommendation**

- Emit security event logs/metrics with correlation IDs and tenant metadata.

## Security Hardening Backlog

### P0

- Authn/authz at API edge.
- Strict request validation and caps.
- LLM response schema validation + robust fallback parse strategy.

### P1

- Comprehensive redaction middleware and secret-safe logging policy.
- Tenant-aware quotas and abuse analytics.

### P2

- Threat modeling, dependency CVE gates, and periodic chaos/security drills.

## Suggested Acceptance Criteria

- 100% of invalid payload classes return deterministic 400 with error code.
- 0 plaintext secret values in logs under automated redaction tests.
- Auth bypass attempts rejected and observable via metrics.
