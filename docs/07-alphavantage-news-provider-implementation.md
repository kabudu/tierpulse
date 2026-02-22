# Alpha Vantage News Provider Integration Plan (tierpulse)

## 1) Purpose

This document defines the full implementation plan for adding **Alpha Vantage** as a news provider in tierpulse’s tier-2 news pipeline.

Scope:

- Add Alpha Vantage `NEWS_SENTIMENT` as a supported provider.
- Keep existing tiered failover semantics and request-budget enforcement.
- Preserve current security/egress controls and observability model.
- Add tests and operational guidance so behavior is verifiable in CI and production.

Out of scope:

- Replacing Tiingo as primary source (unless explicitly configured later).
- Changing tier-3 LLM fallback logic.
- Changing ONNX or sentiment model behavior.

## Implementation status (as of 2026-02-22)

Legend: `[x]` completed, `[ ]` not yet completed (including partial work still open).

---

## 2) Source-of-Truth API References (reviewed)

Validated against official Alpha Vantage pages on **2026-02-21**:

- Documentation root: https://www.alphavantage.co/documentation/
- Support / limits FAQ: https://www.alphavantage.co/support/#api-key

Relevant official endpoint section:

- **Alpha Intelligence → Market News & Sentiment** (`function=NEWS_SENTIMENT`)

Documented parameters used for integration:

- `function=NEWS_SENTIMENT` (required)
- `tickers` (optional; comma-separated)
- `time_from` / `time_to` in `YYYYMMDDTHHMM` (optional)
- `sort` in `LATEST | EARLIEST | RELEVANCE` (optional)
- `limit` default `50`, max `1000` (optional)
- `apikey` (required)

Published usage note (support FAQ):

- Free-tier usage: **up to 25 requests/day**.

---

## 3) Current tierpulse baseline (as of this doc)

Current tier-2 failover in `fetch_batch_news`:

1. Tiingo (batched)
2. MarketAux (batched)
3. Finnhub (per-symbol final fallback)

Key implementation constraints to preserve:

- Request budget via `TP_PROVIDER_CALL_BUDGET_PER_REQUEST`
- Egress allowlist validation via `enforce_allowed_url`
- Provider metrics via `record_provider_call` / `record_provider_error`
- Structured provider attempt/response logs (now request_id-correlated)

---

## 4) Target behavior after integration

### 4.1 Provider order

Proposed new tier-2 order:

1. Tiingo (batched)
2. MarketAux (batched)
3. **Alpha Vantage NEWS_SENTIMENT (batched for remaining symbols)**
4. Finnhub (per-symbol final fallback)

Rationale:

- Alpha Vantage supports multi-symbol filtering through `tickers`, so it belongs before per-symbol Finnhub.
- This reduces per-request outbound calls when multiple symbols remain unresolved.

### 4.2 Budget semantics

Each provider invocation consumes one budget unit (same as existing providers):

- 1 unit for Tiingo call
- 1 unit for MarketAux call (if configured and still missing)
- 1 unit for Alpha Vantage call (if configured and still missing)
- Finnhub remains per-symbol and consumes per ticker call

No changes to global budget contract.

### 4.3 Result mapping contract

Alpha Vantage response items should be mapped into tierpulse `news_by_ticker: HashMap<String, Vec<String>>` as follows:

- For each returned article, use title-like text for item content (preferred field `title`, fallback handling documented below).
- For ticker attribution, parse ticker metadata from article payload and append article title to each matching missing ticker.

De-duplication:

- Keep existing behavior unless duplicates become excessive.
- Optional low-risk enhancement: per-ticker `HashSet` before final `Vec` materialization.

---

## 5) Data-contract and parsing strategy

Because Alpha Vantage response schemas can evolve and may include optional keys, implement **tolerant parsing**:

### 5.1 Request URL

`https://www.alphavantage.co/query?function=NEWS_SENTIMENT&tickers=<CSV>&time_from=<YYYYMMDDTHHMM>&limit=<N>&sort=LATEST&apikey=<KEY>`

Recommended defaults for tierpulse:

- `sort=LATEST`
- `limit = min(1000, max_articles_per_symbol * missing_tickers_count * amplification_factor)`
  - Suggested `amplification_factor`: `3`
  - Reason: one article may map to multiple symbols; buffer for sparse ticker coverage.
- `time_from` derived from `lookback_hours`
  - Format in UTC: `%Y%m%dT%H%M`

### 5.2 Response handling model

Parse as a minimal strongly-typed structure with fallbacks:

- [x] top-level feed array (commonly `feed`)
- [x] per-item title text (`title`)
- [x] per-item ticker sentiment list (commonly `ticker_sentiment` with `ticker`)

If strict struct decode fails:

- [x] fallback to `serde_json::Value` extraction for required fields.
- [x] skip malformed items (do not fail whole provider call unless response is fundamentally invalid).

### 5.3 Error-shape handling

Alpha Vantage may return HTTP 200 with payload-level error/notice messages.

Implementation rule:

- [x] Treat payload containing known non-data indicators (e.g., `Note`, `Information`, `Error Message`) as provider-level non-success for extraction purposes.
- [x] Record provider error metric with status class surrogate (`2xx_payload_error`) or `4xx/5xx` when actual HTTP status reflects failure.
- [x] Continue failover.

---

## 6) Required code changes

### 6.1 Configuration (`src/config.rs`)

Add:

- [x] `alphavantage_key: Option<String>`

Environment variable:

- [x] `TP_ALPHAVANTAGE_KEY`

Debug masking:

- [x] Ensure key is masked in `fmt::Debug` implementation.

### 6.2 Egress (`src/egress.rs`)

Add host to default allowlist:

- [x] `www.alphavantage.co`

Keep HTTPS-only and port-443 enforcement unchanged.

### 6.3 Provider module (`src/providers/mod.rs`)

Add response models:

- [x] `AlphaVantageNewsResponse`
- [x] `AlphaVantageNewsItem`
- [x] `AlphaVantageTickerSentiment`

Add integration branch in `fetch_batch_news` between MarketAux and Finnhub:

- [x] execute only if `config.alphavantage_key.is_some()` and unresolved tickers remain.
- [x] consume budget before call.
- [x] emit request_id-correlated logs for attempt/response/summary.
- [x] call via existing `send_with_retry("alphavantage", ...)`.

URL construction specifics:

- [x] endpoint: `https://www.alphavantage.co/query`
- [x] params: function, tickers, time_from, sort, limit, apikey

Ticker normalization:

- [x] Compare case-insensitive against requested tickers.
- [x] Preserve existing output key casing (from request ticker).

### 6.4 Documentation (`README.md`)

Update:

- [x] env var table with `TP_ALPHAVANTAGE_KEY`
- [x] provider failover summary to include Alpha Vantage order
- [x] troubleshooting section with Alpha Vantage-specific notes (rate limits and payload notices)

### 6.5 Tests

Unit/integration tests to add or update:

1. **Failover order test updates**

- [x] Existing tests in `tests/provider_failover_llm_schema_tests.rs` and provider module source-order checks.
- [x] Assert order becomes: Tiingo < MarketAux < AlphaVantage < Finnhub.

2. **Alpha Vantage parsing tests**

- [x] Valid sample payload maps titles to expected tickers.
- [x] Payload with `Note`/`Information` triggers non-success path.
- [x] Partial malformed item is skipped, valid items still consumed.

3. **Egress tests**

- [x] `www.alphavantage.co` allowed by default.

4. **Budget behavior test**

- [x] Ensure Alpha Vantage invocation respects and consumes provider budget.

---

## 7) Reliability and quota strategy

Given documented free-tier limit (25/day), treat Alpha Vantage as **optional and best-effort**:

- [x] Do not hard-fail request if Alpha Vantage unavailable/rate-limited.
- [x] Continue to Finnhub and then LLM as designed.
- [x] Record provider errors with clear status class labels.

Optional production enhancement (not required for initial integration):

- Add soft circuit/cooldown in-process when repeated payload-level throttle notices are detected.
- Example: suppress Alpha Vantage calls for a short TTL window to avoid waste.

---

## 8) Logging and observability requirements

All Alpha Vantage logs must include `request_id`:

- [x] provider attempt
- [x] provider response status
- [x] parse/notice outcomes
- [x] batch completion summaries

Metrics impact:

- [x] `provider_calls{provider="alphavantage"}`
- [x] `provider_error_rate{provider="alphavantage",status_class="..."}`
- [x] fallback transitions continue to represent tier transitions, not intra-tier provider hops.

---

## 9) Security requirements

- API key in query string is required by endpoint contract; this is acceptable if:
  - [x] dynamic logs are redacted (already in place for sensitive query params)
  - [x] no raw URL with key is emitted unredacted
- [x] Keep header sensitivity and redaction controls unchanged.
- [x] Keep egress host restrictions enforced.

---

## 10) Backward compatibility

- [x] If `TP_ALPHAVANTAGE_KEY` is unset, behavior remains unchanged except for code path awareness.
- [x] Existing Tiingo/MarketAux/Finnhub and LLM flows remain intact.
- [x] No API contract change for `/api/v1/analyze` responses.

---

## 11) Implementation sequence (recommended)

1. [x] Add config field + env parsing + masked debug.
2. [x] Add egress allowlist host and tests.
3. [x] Implement Alpha Vantage fetch branch and parsing.
4. [x] Update provider failover order tests and parsing tests.
5. [x] Update README/docs.
6. [x] Run full suite:
   - `cargo fmt --all -- --check`

- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets -- --nocapture`

---

## 12) Acceptance criteria

Functional:

- [x] With valid `TP_ALPHAVANTAGE_KEY`, unresolved tickers after MarketAux are queried via Alpha Vantage before Finnhub.
- [x] Results are merged into `news_by_ticker` and consumed by tier-1 local sentiment where available.

Reliability:

- [x] Throttle/notice/error payloads do not crash requests; failover continues.
- [x] Provider budget behavior remains deterministic.

Security/ops:

- [x] No sensitive key leakage in logs.
- [x] `request_id` appears in Alpha Vantage provider logs.

Testing:

- [x] Updated failover order tests pass.
- [x] New Alpha Vantage parsing/error tests pass.

---

## 13) Example direct API checks (ops runbook)

Minimal test:

```bash
curl -i --request GET \
  "https://www.alphavantage.co/query?function=NEWS_SENTIMENT&tickers=AAPL&limit=50&sort=LATEST&apikey=<ALPHAVANTAGE_API_KEY>"
```

Lookback test (`time_from` in `YYYYMMDDTHHMM` UTC):

```bash
curl -i --request GET \
  "https://www.alphavantage.co/query?function=NEWS_SENTIMENT&tickers=AAPL,MSFT&time_from=20260220T0000&limit=100&sort=LATEST&apikey=<ALPHAVANTAGE_API_KEY>"
```

If response body contains informational throttle/limit message, treat as provider unavailable for that request and continue failover.

---

## 14) Open decisions (explicit)

1. Should Alpha Vantage be enabled by default when key exists, or guarded behind a feature flag/env toggle?
2. Should provider order become configurable (`TP_NEWS_PROVIDER_ORDER`) in a later iteration?
3. Should we add a provider-level cooldown cache for payload-level throttle notices to conserve budget?

Current recommendation: enable by key presence only, keep static order, defer cooldown to follow-up unless rate pressure is observed.
