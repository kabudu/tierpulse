use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Symbol {
    pub ticker: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct AnalyzeRequest {
    pub symbols: Vec<Symbol>,
    pub lookback_hours: i32,
    pub max_articles_per_symbol: i32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SentimentResult {
    pub symbol: String,
    pub sentiment_score: f32, // -1.0 to 1.0
    pub label: String,        // "bullish" | "bearish" | "neutral"
    pub confidence: f32,
    pub source_tier: String, // tier_1_local_onnx, tier_2_provider, tier_3_llm
    pub news_provider: Option<String>,
    pub article_count: i32,
    pub reasoning: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeResponse {
    pub request_id: String,
    pub results: Vec<SentimentResult>,
    pub execution_time_ms: u128,
}

#[derive(Debug, Serialize)]
pub struct HealthStatus {
    pub status: String,
    pub upstreams: Option<UpstreamStatus>,
}

#[derive(Debug, Serialize)]
pub struct UpstreamStatus {
    pub tiingo: bool,
    pub finnhub: bool,
    pub grok: bool,
}
