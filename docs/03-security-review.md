# Security Review

## Executive Security Posture

The service now has foundational production controls (optional API key/JWT auth, tenant-aware rate limiting, request validation, secret masking, and egress allowlisting), but still needs deployment runbooks, stronger abuse analytics, and broader security telemetry before internet exposure.

## Threat Findings

### 1) Authentication/Authorization Boundary Needs Operationalization (Medium)

- API key and JWT modes are implemented, but production deployment must require one of them.
- Key lifecycle, revocation, and tenant ownership procedures are not yet documented.

**Recommendation**

- Require `TP_AUTH_MODE=api_key` or `TP_AUTH_MODE=jwt` outside local development.
- Document key/JWT issuance, revocation, rotation, and tenant ownership.

### 2) Request Validation and Bounds Need Schema Publication (Medium)

- Runtime validation enforces empty/oversized symbol arrays, duplicate tickers, ticker/name lengths, lookback bounds, and article-count bounds.
- An OpenAPI or JSON Schema contract is not yet published for clients.

**Recommendation**

- Publish a generated schema/OpenAPI contract and add contract tests against the handler.

### 3) Incomplete Secrets Redaction Strategy (Medium-High)

- API keys are masked in `Config` debug output and runtime log sanitization redacts known secret values and sensitive query params.
- Some providers require query-string tokens; those values are centrally redacted when logged.

**Recommendation**

- Centralize log redaction for known key patterns and query params.
- Prefer headers over query tokens where providers allow.

**Provider Auth Conformance Note (Verified)**

- **Tiingo:** REST auth supports both query `token` and header `Authorization: Token <api_token>`.
- **Finnhub:** GET auth supports both query `token=apiKey` and header `X-Finnhub-Token: apiKey`.
- **MarketAux:** Public REST docs use query `api_token`; no equivalent header auth was documented in review.
- **Alpha Vantage:** Public REST docs use query `apikey`.
- **xAI/Grok, DeepSeek, OpenAI:** Chat-completions endpoints use bearer-token authentication.

Implementation alignment: Tiingo and Finnhub are configured to use headers (to minimize token exposure in URLs), while MarketAux and Alpha Vantage remain query-token based with centralized redaction controls. LLM providers use bearer auth.

### 4) LLM Output Trust Surface (Medium-High)

- LLM outputs are parsed through a strict schema into internal result objects.
- Malformed LLM responses are treated as provider failures and fall through to the next configured provider.

**Recommendation**

- Add mocked upstream tests for provider-specific malformed payloads and HTTP 400 bodies.

### 5) Egress Policy Needs Deployment-Level Enforcement (Medium)

- Application-level egress allowlisting is implemented for approved HTTPS provider hosts.
- Network-level egress policy still needs deployment configuration.

**Recommendation**

- Mirror the application allowlist in container/orchestrator network policy.

### 6) No Audit/Event Security Telemetry (Medium)

- Missing structured security events for auth failures, abuse spikes, and anomaly patterns.

**Recommendation**

- Emit security event logs/metrics with correlation IDs and tenant metadata.

## Security Hardening Backlog

### P0

- Require auth in production deployment configuration.
- Publish OpenAPI/schema contract and keep validation tests current.
- Maintain LLM response schema/fallback tests for all configured providers.

### P1

- Comprehensive redaction middleware and secret-safe logging policy.
- Tenant-aware quotas and abuse analytics.

### P2

- Threat modeling, dependency CVE gates, and periodic chaos/security drills.

## Suggested Acceptance Criteria

- 100% of invalid payload classes return deterministic 400 with error code.
- 0 plaintext secret values in logs under automated redaction tests.
- Auth bypass attempts rejected and observable via metrics.
