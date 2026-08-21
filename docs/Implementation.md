---
## **tierpulse Specification 5.1: The "High-Scale" Blueprint**

### **1. Core Architectural Shift: The Case for Rust**
To meet the "Five Nines" availability and sub-millisecond overhead required by institutional-grade trading bots, tierpulse moves from a Python-based prototype to a **Rust** implementation.

- **Stack:** Rust 2024 edition (MSRV 1.96), [Axum](https://github.com/tokio-rs/axum) (Web Framework), and [ort](https://github.com/pykeio/ort) (ONNX Runtime for Rust).
- **Leanness:** By eliminating the Python interpreter and heavy library overhead, the Docker footprint is reduced from ~650MB to **<100MB**.
- **Performance:** Rust provides a truly concurrent, thread-safe environment without the constraints of a Global Interpreter Lock (GIL), allowing the system to handle thousands of concurrent ticker analyzes with minimal CPU jitter.
- **Safety:** Compile-time memory safety eliminates common production bugs like buffer overflows or data races, critical for high-throughput financial data processing.
---

### **2. Tier 1: Local Inference Engine (Optimized ONNX Pipeline)**

#### **A. Model Optimization & Pruning**

To achieve maximum performance on commodity hardware, the model undergoes a two-step optimization:

1. **Pruning:** Before quantization, the `ProsusAI/finbert` model is **Pruned** (Structured Sparsity).
   - _What exactly is this?_ Pruning involves removing the weights in the neural network that have the least impact on the output (zeroing them out). This reduces the model complexity and improves inference speed by focusing the CPU on "useful" neurons without a significant drop in sentiment accuracy.
2. **Quantization:** The pruned model is converted to **INT8-Quantized ONNX**, reducing RAM requirements to ~120MB and accelerating CPU-bound inference by ~300%.

#### **B. Cost-Efficient Headline Fetching**

Unlike naive implementations that "race" requests (wasting API quota), tierpulse utilizes a **Batch-Optimized Sequential Failover** strategy to preserve tier credits and reduce RTT overhead:

- **Primary:** Tiingo News API (Strict 5s timeout, batched ticker support).
- **Secondary:** MarketAux (batched multi-symbol fallback for unresolved symbols).
- **Tertiary:** Finnhub (final individual-symbol fallback for remaining unresolved symbols).

---

### **3. Performance & Reliability**

#### **A. Intelligent Caching Layer**

tierpulse implements a hybrid caching strategy to minimize redundant inference:

- **Internal Cache:** Uses a high-performance, concurrent [Moka](https://github.com/moka-rs/moka) LRU cache.
- **Distributed Cache:** If the `TP_REDIS_URL` is provided, the system defaults to Redis, allowing for shared state across multiple tierpulse instances.
- **TTL Enforcement:** Results are strictly invalidated based on the `TP_CACHE_TTL` setting.

#### **B. Security & Traffic Control**

- **Log Masking Middleware:** All request/response logs pass through a masking layer that scrubs sensitive API keys (`TP_*_KEY`) before they hit `stdout` or any telemetry sinks.
- **Token Bucket Rate Limiting:** A sophisticated "Token Bucket" rate limiter is implemented at the `/analyze` endpoint. This protects upstream quotas and prevents "noisy neighbor" scenarios where a single internal client consumes all available system credits.

---

### **4. API Endpoints & Contract Specification**

#### **Endpoint 1: /api/v1/analyze (POST)**

- **Orchestration:** Transparently manages Tier 1 (Local), Tier 2 (Provider), and Tier 3 (LLM) fallbacks.

**Request Payload:**

```json
{
  "symbols": [
    { "ticker": "AAPL", "name": "Apple Inc." },
    { "ticker": "TSLA", "name": "Tesla" }
  ],
  "lookback_hours": 24,
  "max_articles_per_symbol": 5
}
```

_Note: Providing the `name` is mandatory for Tier 3 (LLM) fallbacks to ensure the model has context beyond just the ticker._

**Success Response (200 OK):**

```json
{
  "request_id": "tp_9928374",
  "results": [
    {
      "symbol": "AAPL",
      "sentiment_score": 0.82,
      "label": "bullish",
      "confidence": 0.94,
      "source_tier": "tier_1_local_onnx",
      "news_provider": "tiingo",
      "article_count": 5
    }
  ],
  "execution_time_ms": 450
}
```

#### **Endpoint 2: Health & Discovery**

- **Method:** `GET`
- **Path:** `/health`
- **Description:** Returns the real-time availability of all upstream API keys and the "Circuit Breaker" status.

---

### **5. HTTP Status Codes & Intelligence Exhaustion**

| Status Code                 | Meaning              | System Action                                           |
| :-------------------------- | :------------------- | :------------------------------------------------------ |
| **200 OK**                  | Success              | Sentiment delivered via Tier 1, 2, or 3.                |
| **400 Bad Request**         | Validation Error     | Missing keys in payload or unsupported symbols.         |
| **429 Too Many Requests**   | Client Throttling    | tierpulse is throttling the user via Token Bucket.      |
| **503 Service Unavailable** | **Total Exhaustion** | All 3 tiers (News APIs, Local, and LLMs) are exhausted. |

**Exhaustion Payload (503):**

```json
{
  "error": "Intelligence Exhaustion",
  "message": "All upstream providers are currently rate-limited.",
  "retry_after_seconds": 300,
  "status": {
    "tier_1": "exhausted",
    "tier_2": "exhausted",
    "tier_3_llm": "cooldown_active"
  }
}
```

---

### **6. Configuration & Infrastructure**

| Variable | Default | Description |
| :-- | :-- | :-- |
| `PORT` | `8080` | Server listening port. |
| `TP_TIINGO_KEY` | _Required_ | Primary news fetcher API key. |
| `TP_FINNHUB_KEY` | `null` | Tier 1/2 fallback provider key. |
| `TP_MARKETAUX_KEY` | `null` | Tier 2 fallback provider key. |
| `TP_GROK_KEY` | `null` | xAI API key for Tier 3 fallback. |
| `TP_DEEPSEEK_KEY` | `null` | DeepSeek API key for Tier 3 fallback. |
| `TP_OPENAI_KEY` | `null` | OpenAI API key for Tier 3 fallback. |
| `TP_PRIMARY_LLM` | `grok` | Backward-compatible first-provider hint when `TP_LLM_PROVIDER_ORDER` is unset. |
| `TP_LLM_PROVIDER_ORDER` | derived | Comma-separated Tier 3 execution order (`grok`, `deepseek`, `openai`). |
| `TP_GROK_MODEL` | `grok-4.3` | xAI chat-completions model used by the `grok` provider. |
| `TP_DEEPSEEK_MODEL` | `deepseek-v4-flash` | DeepSeek chat-completions model used by the `deepseek` provider. |
| `TP_OPENAI_MODEL` | `gpt-5.4-nano` | OpenAI chat-completions model used by the `openai` provider. |
| `TP_REDIS_URL` | `null` | Redis endpoint (enables distributed caching). |
| `TP_CACHE_TTL` | `300` | In-memory/Redis cache expiration (seconds). |
| `TP_RATE_LIMIT` | `100` | Request tokens per minute (Token Bucket capacity). |
| `TP_ONNX_THREADS` | `2` | CPU thread allocation for the `ort` session. |
| `TP_MODEL_PATH` | `model.onnx` | Path to the ONNX model file. |
| `TP_LOG_LEVEL` | `INFO` | Global logging level (`DEBUG`, `INFO`, `WARNING`, `ERROR`). |

When `TP_LLM_PROVIDER_ORDER` is unset, tierpulse starts with `TP_PRIMARY_LLM` and appends the remaining supported providers in the default sequence `grok,deepseek,openai`.

---

### **7. Tier 3: LLM Fallback (Intelligence Layer)**

When all local inference and news provider tiers are exhausted, tierpulse triggers the configured LLM layer. Providers are attempted sequentially according to `TP_LLM_PROVIDER_ORDER`, skipping providers without configured keys and falling back after request errors, non-2xx responses, or invalid payloads.

**Prompt Template:**
`"Analyze market sentiment for the following symbols: {name} ({ticker}), ... Respond ONLY with a JSON object containing a \"results\" array."`

**LLM Strict Interface (JSON Only):**
The response must adhere to the following strict contract:

```json
{
  "results": [
    {
      "symbol": "string",
      "sentiment_score": "float (-1.0 to 1.0)",
      "confidence": "float (0.0 to 1.0)",
      "reasoning": "string (max 20 words)",
      "label": "string"
    }
  ]
}
```

The LLM transport requests provider JSON mode via `response_format: { "type": "json_object" }`, disables streaming, and validates every returned item before accepting a provider response. A legacy top-level array shape is still accepted for backward compatibility, but the provider-facing contract is the object form above.

---

### **8. CI/CD & Testing Infrastructure**

#### **A. Multi-Stage "Zero-Bloat" Build**

1. **Stage 1 (Model Prep):** A Python container exports and quantizes the configured Hugging Face financial sentiment model to ONNX. The default is `ProsusAI/finbert`; builds can override it with Docker build arg `MODEL_ID=<owner/model>`.
2. **Stage 2 (Binary Build):** A Rust container builds the service using `cargo build --release`.
3. **Stage 3 (Production):** The final image uses a **Distroless** or **Scratch** base, containing _only_ the compiled Rust binary and the `.onnx` model file. Target size: **~85MB**.

#### **B. Automated Deployment**

The CI/CD pipeline (GitHub Actions) automatically builds and pushes the Docker image to Docker Hub at `boxedcode/tierpulse`.

- **Versioning:** Utilizes **Semantic Versioning** for automatic tagging. A Git tag such as `v1.2.1` produces Docker tags `1.2.1`, `1.2`, and `1`; pushes to `master` publish `latest`.
- **Release consistency:** Versioned Docker publishing requires the Git tag, `Cargo.toml` package version, and `CHANGELOG.md` release section to match.
- **Triggers:** Tags matching `v*` and pushes to the `main` branch trigger a deployment.

#### **C. Test Suite Requirements**

- **`test_sequential_failover`:** Verify Finnhub is _never_ called if Tiingo succeeds (quota preservation).
- **`test_token_bucket`:** Assert 429 is returned if the request burst exceeds `TP_RATE_LIMIT`.
- **`test_log_masking`:** Ensure no occurrence of the `TP_TIINGO_KEY` exists in captured trace logs.
- **`test_onnx_accuracy`:** Compares INT8 output against known headlines to ensure no "drift".
