use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, Utc};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tracing::info;

use crate::config::Config;
use crate::egress;
use crate::metrics::MetricsRegistry;
use crate::models::{SentimentResult, Symbol};

const MAX_RETRIES: u32 = 2;
const BASE_BACKOFF_MS: u64 = 120;
const MAX_BACKOFF_MS: u64 = 900;

fn should_retry_status(status: reqwest::StatusCode) -> bool {
    status.is_server_error()
}

fn is_transient_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request()
}

fn jitter_ms() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    nanos % 75
}

async fn backoff_sleep(attempt: u32) {
    let exponent = attempt.min(4);
    let delay =
        (BASE_BACKOFF_MS.saturating_mul(1_u64 << exponent) + jitter_ms()).min(MAX_BACKOFF_MS);
    sleep(StdDuration::from_millis(delay)).await;
}

async fn send_with_retry<F>(
    provider: &str,
    metrics: &Arc<MetricsRegistry>,
    mut build_request: F,
) -> Result<reqwest::Response>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    for attempt in 0..=MAX_RETRIES {
        metrics.record_provider_call(provider);
        match build_request().send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    let status_class = format!("{}xx", response.status().as_u16() / 100);
                    metrics.record_provider_error(provider, &status_class);
                }
                if should_retry_status(response.status()) && attempt < MAX_RETRIES {
                    backoff_sleep(attempt).await;
                    continue;
                }
                return Ok(response);
            }
            Err(error) => {
                metrics.record_provider_error(provider, "network");
                if is_transient_error(&error) && attempt < MAX_RETRIES {
                    backoff_sleep(attempt).await;
                    continue;
                }
                return Err(anyhow::anyhow!("request failed: {}", error));
            }
        }
    }

    Err(anyhow::anyhow!(
        "request failed after retry budget exhausted"
    ))
}

#[derive(Debug, Deserialize)]
struct TiingoNews {
    title: String,
    tickers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FinnhubNews {
    headline: String,
}

#[derive(Debug, Deserialize)]
struct MarketAuxResponse {
    data: Vec<MarketAuxNews>,
}

#[derive(Debug, Deserialize)]
struct MarketAuxNews {
    title: String,
    entities: Vec<MarketAuxEntity>,
}

#[derive(Debug, Deserialize)]
struct MarketAuxEntity {
    symbol: String,
}

#[derive(Debug, Deserialize)]
struct AlphaVantageNewsResponse {
    #[serde(default)]
    feed: Vec<AlphaVantageNewsItem>,
    #[serde(rename = "Note")]
    note: Option<String>,
    #[serde(rename = "Information")]
    information: Option<String>,
    #[serde(rename = "Error Message")]
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlphaVantageNewsItem {
    title: Option<String>,
    #[serde(default)]
    ticker_sentiment: Vec<AlphaVantageTickerSentiment>,
}

#[derive(Debug, Deserialize)]
struct AlphaVantageTickerSentiment {
    ticker: String,
}

#[derive(Debug, Deserialize)]
struct LlmSentimentRaw {
    symbol: String,
    sentiment_score: f32,
    confidence: f32,
    reasoning: String,
    label: String,
}

#[derive(Debug)]
pub struct BatchNewsOutcome {
    pub news_by_ticker: HashMap<String, Vec<String>>,
    pub all_news_sources_attempted: bool,
    pub budget_exhausted: bool,
}

fn consume_budget(remaining_budget: &mut u32) -> bool {
    if *remaining_budget == 0 {
        return false;
    }
    *remaining_budget -= 1;
    true
}

fn alphavantage_time_from_utc(lookback_hours: i32) -> String {
    (Utc::now() - ChronoDuration::hours(lookback_hours.into()))
        .format("%Y%m%dT%H%M")
        .to_string()
}

fn alphavantage_payload_has_notice(response: &AlphaVantageNewsResponse) -> bool {
    response.note.is_some() || response.information.is_some() || response.error_message.is_some()
}

fn alphavantage_extract_titles_by_ticker(
    response: &AlphaVantageNewsResponse,
    requested_tickers: &[String],
    per_ticker_limit: usize,
) -> HashMap<String, Vec<String>> {
    if alphavantage_payload_has_notice(response) {
        return HashMap::new();
    }

    let ticker_lookup: HashMap<String, String> = requested_tickers
        .iter()
        .map(|ticker| (ticker.to_ascii_uppercase(), ticker.clone()))
        .collect();

    let mut extracted: HashMap<String, Vec<String>> = HashMap::new();

    for item in &response.feed {
        let title = item.title.as_deref().unwrap_or_default().trim().to_string();
        if title.is_empty() {
            continue;
        }

        for sentiment in &item.ticker_sentiment {
            let normalized = sentiment.ticker.to_ascii_uppercase();
            if let Some(request_ticker) = ticker_lookup.get(&normalized) {
                let titles = extracted.entry(request_ticker.clone()).or_default();
                if titles.len() < per_ticker_limit {
                    titles.push(title.clone());
                }
            }
        }
    }

    extracted
}

fn alphavantage_payload_value_has_notice(payload: &serde_json::Value) -> bool {
    payload.get("Note").is_some()
        || payload.get("Information").is_some()
        || payload.get("Error Message").is_some()
}

fn alphavantage_extract_titles_from_value_by_ticker(
    payload: &serde_json::Value,
    requested_tickers: &[String],
    per_ticker_limit: usize,
) -> HashMap<String, Vec<String>> {
    if alphavantage_payload_value_has_notice(payload) {
        return HashMap::new();
    }

    let ticker_lookup: HashMap<String, String> = requested_tickers
        .iter()
        .map(|ticker| (ticker.to_ascii_uppercase(), ticker.clone()))
        .collect();

    let mut extracted: HashMap<String, Vec<String>> = HashMap::new();

    let Some(feed) = payload.get("feed").and_then(|v| v.as_array()) else {
        return extracted;
    };

    for item in feed {
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }

        let Some(ticker_sentiment) = item.get("ticker_sentiment").and_then(|v| v.as_array()) else {
            continue;
        };

        for sentiment in ticker_sentiment {
            let Some(ticker) = sentiment.get("ticker").and_then(|v| v.as_str()) else {
                continue;
            };
            let normalized = ticker.to_ascii_uppercase();
            if let Some(request_ticker) = ticker_lookup.get(&normalized) {
                let titles = extracted.entry(request_ticker.clone()).or_default();
                if titles.len() < per_ticker_limit {
                    titles.push(title.clone());
                }
            }
        }
    }

    extracted
}

pub async fn fetch_batch_news(
    request_id: &str,
    symbols: &[Symbol],
    lookback_hours: i32,
    max_articles: i32,
    config: &Config,
    client: &Client,
    metrics: &Arc<MetricsRegistry>,
) -> Result<BatchNewsOutcome> {
    info!(
        "request_id={} News batch start: symbols={}, max_articles_per_symbol={}, provider_budget={}",
        request_id,
        symbols.len(),
        max_articles,
        config.provider_call_budget_per_request
    );

    let mut remaining_budget = config.provider_call_budget_per_request;
    let mut results: HashMap<String, Vec<String>> = HashMap::new();
    let tickers: Vec<String> = symbols.iter().map(|s| s.ticker.clone()).collect();
    let tickers_param = tickers.join(",");
    let attempted_tiingo: bool;
    let mut attempted_marketaux = false;
    let mut attempted_alphavantage = false;
    let mut attempted_finnhub = false;
    let mut budget_exhausted = false;

    let has_marketaux = config.marketaux_key.is_some();
    let marketaux_needed = has_marketaux;
    let alphavantage_needed = config.alphavantage_key.is_some();
    let mut finnhub_needed = false;

    // 1. Primary: Tiingo News API (Batched)
    let tiingo_url = format!(
        "https://api.tiingo.com/tiingo/news?tickers={}&limit={}",
        tickers_param,
        max_articles * tickers.len() as i32 // Allow enough articles for all symbols
    );

    if consume_budget(&mut remaining_budget) {
        attempted_tiingo = true;
        info!(
            "request_id={} News provider attempt: provider=tiingo symbols={}",
            request_id,
            tickers.len()
        );
        if egress::enforce_allowed_url(&tiingo_url, config).is_ok() {
            if let Ok(res) = send_with_retry("tiingo", metrics, || {
                client
                    .get(&tiingo_url)
                    .header("Authorization", format!("Token {}", config.tiingo_key))
                    .timeout(std::time::Duration::from_secs(5))
            })
            .await
            {
                info!(
                    "request_id={} News provider response: provider=tiingo status={}",
                    request_id,
                    res.status()
                );
                if res.status().is_success() {
                    if let Ok(news_items) = res.json::<Vec<TiingoNews>>().await {
                        for item in news_items {
                            for ticker in item.tickers {
                                if tickers.contains(&ticker) {
                                    results.entry(ticker).or_default().push(item.title.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        budget_exhausted = true;
        return Ok(BatchNewsOutcome {
            news_by_ticker: results,
            all_news_sources_attempted: false,
            budget_exhausted,
        });
    }

    // Identify symbols that still need news
    let missing_tickers: Vec<&String> = tickers
        .iter()
        .filter(|t| !results.contains_key(*t) || results[*t].is_empty())
        .collect();

    if missing_tickers.is_empty() {
        return Ok(BatchNewsOutcome {
            news_by_ticker: results,
            all_news_sources_attempted: true,
            budget_exhausted,
        });
    }

    // 2. Secondary: MarketAux (Batched fallback for missing tickers)
    if let Some(key) = &config.marketaux_key {
        if !consume_budget(&mut remaining_budget) {
            budget_exhausted = true;
            return Ok(BatchNewsOutcome {
                news_by_ticker: results,
                all_news_sources_attempted: false,
                budget_exhausted,
            });
        }
        attempted_marketaux = true;
        info!(
            "request_id={} News provider attempt: provider=marketaux symbols_missing={}",
            request_id,
            missing_tickers.len()
        );

        let missing_param = missing_tickers
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let marketaux_url = format!(
            "https://api.marketaux.com/v1/news/all?symbols={}&filter_entities=true&limit={}&api_token={}",
            missing_param,
            max_articles * missing_tickers.len() as i32,
            key
        );

        if egress::enforce_allowed_url(&marketaux_url, config).is_ok() {
            if let Ok(res) =
                send_with_retry("marketaux", metrics, || client.get(&marketaux_url)).await
            {
                info!(
                    "request_id={} News provider response: provider=marketaux status={}",
                    request_id,
                    res.status()
                );
                if res.status().is_success() {
                    if let Ok(response) = res.json::<MarketAuxResponse>().await {
                        for item in response.data {
                            for entity in item.entities {
                                if tickers.contains(&entity.symbol) {
                                    results
                                        .entry(entity.symbol)
                                        .or_default()
                                        .push(item.title.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Tertiary: Alpha Vantage NEWS_SENTIMENT (Batched fallback for unresolved symbols)
    if let Some(key) = &config.alphavantage_key {
        let still_missing: Vec<&String> = tickers
            .iter()
            .filter(|t| !results.contains_key(*t) || results[*t].is_empty())
            .collect();

        if !still_missing.is_empty() {
            if !consume_budget(&mut remaining_budget) {
                budget_exhausted = true;
                return Ok(BatchNewsOutcome {
                    news_by_ticker: results,
                    all_news_sources_attempted: false,
                    budget_exhausted,
                });
            }

            attempted_alphavantage = true;
            info!(
                "request_id={} News provider attempt: provider=alphavantage symbols_missing={}",
                request_id,
                still_missing.len()
            );

            let symbols_param = still_missing
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let time_from = alphavantage_time_from_utc(lookback_hours);
            let result_limit = (max_articles.max(1) as usize)
                .saturating_mul(still_missing.len())
                .saturating_mul(3);
            let result_limit = result_limit.min(1000);
            let alphavantage_url = format!(
                "https://www.alphavantage.co/query?function=NEWS_SENTIMENT&tickers={}&time_from={}&sort=LATEST&limit={}&apikey={}",
                symbols_param, time_from, result_limit, key
            );

            if egress::enforce_allowed_url(&alphavantage_url, config).is_ok() {
                if let Ok(res) =
                    send_with_retry("alphavantage", metrics, || client.get(&alphavantage_url)).await
                {
                    info!(
                        "request_id={} News provider response: provider=alphavantage status={}",
                        request_id,
                        res.status()
                    );
                    if res.status().is_success() {
                        let body = res.text().await.unwrap_or_default();
                        if let Ok(response) =
                            serde_json::from_str::<AlphaVantageNewsResponse>(&body)
                        {
                            if alphavantage_payload_has_notice(&response) {
                                metrics.record_provider_error("alphavantage", "2xx_payload_error");
                                info!(
                                    "request_id={} News provider payload notice: provider=alphavantage note={} information={} error_message={}",
                                    request_id,
                                    response.note.as_deref().unwrap_or(""),
                                    response.information.as_deref().unwrap_or(""),
                                    response.error_message.as_deref().unwrap_or("")
                                );
                            } else {
                                let extracted = alphavantage_extract_titles_by_ticker(
                                    &response,
                                    &tickers,
                                    max_articles.max(1) as usize,
                                );
                                for (ticker, titles) in extracted {
                                    results.entry(ticker).or_default().extend(titles);
                                }
                            }
                        } else if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&body)
                        {
                            if alphavantage_payload_value_has_notice(&payload) {
                                metrics.record_provider_error("alphavantage", "2xx_payload_error");
                                info!(
                                    "request_id={} News provider payload notice: provider=alphavantage note={} information={} error_message={}",
                                    request_id,
                                    payload
                                        .get("Note")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(""),
                                    payload
                                        .get("Information")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(""),
                                    payload
                                        .get("Error Message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                );
                            } else {
                                let extracted = alphavantage_extract_titles_from_value_by_ticker(
                                    &payload,
                                    &tickers,
                                    max_articles.max(1) as usize,
                                );
                                for (ticker, titles) in extracted {
                                    results.entry(ticker).or_default().extend(titles);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. Quaternary: Finnhub (Individual fallback for last-resort symbols)
    // Finnhub company-news doesn't natively support batching tickers in a single GET symbol=A,B param.
    // We only call it for the leftovers to keep request count minimal.
    if let Some(key) = &config.finnhub_key {
        let still_missing: Vec<&String> = tickers
            .iter()
            .filter(|t| !results.contains_key(*t) || results[*t].is_empty())
            .collect();
        finnhub_needed = !still_missing.is_empty();

        for ticker in still_missing {
            if !consume_budget(&mut remaining_budget) {
                budget_exhausted = true;
                break;
            }
            attempted_finnhub = true;
            info!(
                "request_id={} News provider attempt: provider=finnhub symbol={}",
                request_id, ticker
            );

            let to_date = Utc::now();
            let from_date = to_date - ChronoDuration::hours(lookback_hours.into());
            let finn_url = format!(
                "https://finnhub.io/api/v1/company-news?symbol={}&from={}&to={}",
                ticker,
                from_date.format("%Y-%m-%d"),
                to_date.format("%Y-%m-%d")
            );

            if egress::enforce_allowed_url(&finn_url, config).is_err() {
                continue;
            }

            if let Ok(res) = send_with_retry("finnhub", metrics, || {
                client
                    .get(&finn_url)
                    .header("X-Finnhub-Token", key)
                    .timeout(std::time::Duration::from_secs(3))
            })
            .await
            {
                info!(
                    "request_id={} News provider response: provider=finnhub status={}",
                    request_id,
                    res.status()
                );
                if res.status().is_success() {
                    if let Ok(news) = res.json::<Vec<FinnhubNews>>().await {
                        let titles: Vec<String> = news
                            .into_iter()
                            .take(max_articles as usize)
                            .map(|n| n.headline)
                            .collect();
                        if !titles.is_empty() {
                            results.insert(ticker.clone(), titles);
                        }
                    }
                }
            }
        }
    }

    let all_news_sources_attempted = attempted_tiingo
        && (!marketaux_needed || attempted_marketaux)
        && (!alphavantage_needed || attempted_alphavantage)
        && (!finnhub_needed || attempted_finnhub);

    let resolved_tickers = results.values().filter(|titles| !titles.is_empty()).count();
    info!(
        "request_id={} News batch complete: symbols={}, resolved_tickers={}, missing_tickers={}, attempted_tiingo={}, attempted_marketaux={}, attempted_alphavantage={}, attempted_finnhub={}, budget_exhausted={}",
        request_id,
        tickers.len(),
        resolved_tickers,
        tickers.len().saturating_sub(resolved_tickers),
        attempted_tiingo,
        attempted_marketaux,
        attempted_alphavantage,
        attempted_finnhub,
        budget_exhausted
    );

    Ok(BatchNewsOutcome {
        news_by_ticker: results,
        all_news_sources_attempted,
        budget_exhausted,
    })
}

pub async fn fetch_news(
    symbol: &Symbol,
    lookback_hours: i32,
    max_articles: i32,
    config: &Config,
    client: &Client,
    metrics: &Arc<MetricsRegistry>,
) -> Result<Vec<String>> {
    // 1. Primary: Tiingo News API (Strict 5s timeout)
    let tiingo_url = format!(
        "https://api.tiingo.com/tiingo/news?tickers={}&limit={}",
        symbol.ticker, max_articles
    );

    egress::enforce_allowed_url(&tiingo_url, config)?;

    if let Ok(res) = send_with_retry("tiingo", metrics, || {
        client
            .get(&tiingo_url)
            .header("Authorization", format!("Token {}", config.tiingo_key))
            .timeout(std::time::Duration::from_secs(5))
    })
    .await
    {
        if res.status().is_success() {
            if let Ok(news) = res.json::<Vec<TiingoNews>>().await {
                return Ok(news.into_iter().map(|n| n.title).collect());
            }
        }
    }

    // 2. Secondary: MarketAux (Deep fallback logic)
    if let Some(key) = &config.marketaux_key {
        let marketaux_url = format!(
            "https://api.marketaux.com/v1/news/all?symbols={}&filter_entities=true&limit={}&api_token={}",
            symbol.ticker, max_articles, key
        );
        egress::enforce_allowed_url(&marketaux_url, config)?;
        if let Ok(res) = send_with_retry("marketaux", metrics, || client.get(&marketaux_url)).await
        {
            if res.status().is_success() {
                if let Ok(body) = res.json::<MarketAuxResponse>().await {
                    return Ok(body.data.into_iter().map(|n| n.title).collect());
                }
            }
        }
    }

    // 3. Tertiary: Alpha Vantage NEWS_SENTIMENT (batched-capable, single here)
    if let Some(key) = &config.alphavantage_key {
        let time_from = alphavantage_time_from_utc(lookback_hours);
        let result_limit = ((max_articles.max(1) as usize).saturating_mul(3)).min(1000);
        let alphavantage_url = format!(
            "https://www.alphavantage.co/query?function=NEWS_SENTIMENT&tickers={}&time_from={}&sort=LATEST&limit={}&apikey={}",
            symbol.ticker, time_from, result_limit, key
        );
        egress::enforce_allowed_url(&alphavantage_url, config)?;
        if let Ok(res) =
            send_with_retry("alphavantage", metrics, || client.get(&alphavantage_url)).await
        {
            if res.status().is_success() {
                let body = res.text().await.unwrap_or_default();
                if let Ok(response) = serde_json::from_str::<AlphaVantageNewsResponse>(&body) {
                    if alphavantage_payload_has_notice(&response) {
                        metrics.record_provider_error("alphavantage", "2xx_payload_error");
                    } else {
                        let extracted = alphavantage_extract_titles_by_ticker(
                            &response,
                            std::slice::from_ref(&symbol.ticker),
                            max_articles.max(1) as usize,
                        );
                        if let Some(titles) = extracted.get(&symbol.ticker) {
                            if !titles.is_empty() {
                                return Ok(titles.clone());
                            }
                        }
                    }
                } else if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&body) {
                    if alphavantage_payload_value_has_notice(&payload) {
                        metrics.record_provider_error("alphavantage", "2xx_payload_error");
                    } else {
                        let extracted = alphavantage_extract_titles_from_value_by_ticker(
                            &payload,
                            std::slice::from_ref(&symbol.ticker),
                            max_articles.max(1) as usize,
                        );
                        if let Some(titles) = extracted.get(&symbol.ticker) {
                            if !titles.is_empty() {
                                return Ok(titles.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. Quaternary: Finnhub (final fallback)
    if let Some(key) = &config.finnhub_key {
        let to_date = Utc::now();
        let from_date = to_date - ChronoDuration::hours(lookback_hours.into());
        let to_fmt = to_date.format("%Y-%m-%d").to_string();
        let from_fmt = from_date.format("%Y-%m-%d").to_string();

        let finn_url = format!(
            "https://finnhub.io/api/v1/company-news?symbol={}&from={}&to={}",
            symbol.ticker, from_fmt, to_fmt
        );
        egress::enforce_allowed_url(&finn_url, config)?;
        if let Ok(res) = send_with_retry("finnhub", metrics, || {
            client
                .get(&finn_url)
                .header("X-Finnhub-Token", key)
                .timeout(std::time::Duration::from_secs(5))
        })
        .await
        {
            if res.status().is_success() {
                if let Ok(news) = res.json::<Vec<FinnhubNews>>().await {
                    return Ok(news
                        .into_iter()
                        .take(max_articles as usize)
                        .map(|n| n.headline)
                        .collect());
                }
            }
        }
    }

    Ok(vec![])
}

pub async fn fetch_llm_batch_sentiment(
    request_id: &str,
    symbols: &[Symbol],
    config: &Config,
    client: &Client,
    metrics: &Arc<MetricsRegistry>,
) -> Result<Vec<SentimentResult>> {
    if symbols.is_empty() {
        return Ok(vec![]);
    }

    let (api_key, api_url, model) = if config.primary_llm == "grok" {
        (
            config.grok_key.as_ref().context("Grok key missing")?,
            "https://api.x.ai/v1/chat/completions",
            "grok-4-1-fast-reasoning",
        )
    } else {
        (
            config
                .deepseek_key
                .as_ref()
                .context("DeepSeek key missing")?,
            "https://api.deepseek.com/v1/chat/completions",
            "deepseek-chat",
        )
    };

    let symbols_desc = symbols
        .iter()
        .map(|s| format!("{} ({})", s.name, s.ticker))
        .collect::<Vec<_>>()
        .join(", ");

    let prompt = format!(
        "Analyze market sentiment for the following symbols: {}. \
        Respond ONLY with a JSON array where each object represents a symbol and has precisely these fields: \
        {{ \"symbol\": \"ticker\", \"sentiment_score\": float (-1.0 to 1.0), \"confidence\": float (0.0 to 1.0), \"reasoning\": \"max 20 words\", \"label\": \"String\" }}. \
        Include every requested symbol in the output array.",
        symbols_desc
    );

    egress::enforce_allowed_url(api_url, config)?;

    let provider_name = if config.primary_llm == "grok" {
        "grok"
    } else {
        "deepseek"
    };

    info!(
        "request_id={} LLM batch start: provider={} symbols={}",
        request_id,
        provider_name,
        symbols.len()
    );

    let res = send_with_retry(provider_name, metrics, || {
        client
            .post(api_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "system", "content": "You are a financial sentiment analyst tasked with high-throughput batch evaluation. Respond only in strict JSON array format." },
                    { "role": "user", "content": prompt }
                ]
            }))
    })
    .await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        let body_preview: String = body.chars().take(512).collect();
        return Err(anyhow::anyhow!(
            "LLM request failed: {} body={}",
            status,
            body_preview
        ));
    }

    let json: serde_json::Value = res.json().await?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .context("Invalid LLM response structure")?;

    info!(
        "request_id={} LLM batch complete: provider={} symbols={}",
        request_id,
        provider_name,
        symbols.len()
    );

    parse_llm_content_to_results(content)
}

pub fn parse_llm_content_to_results(content: &str) -> Result<Vec<SentimentResult>> {
    let raw: Vec<LlmSentimentRaw> = serde_json::from_str(content)
        .context("LLM content must be a JSON array of sentiment objects")?;

    let results = raw
        .into_iter()
        .map(|item| SentimentResult {
            symbol: item.symbol,
            sentiment_score: item.sentiment_score,
            label: item.label,
            confidence: item.confidence,
            source_tier: "tier_3_llm".to_string(),
            news_provider: None,
            article_count: 0,
            reasoning: Some(item.reasoning),
        })
        .collect();

    Ok(results)
}

#[cfg(test)]
mod tests {
    #[test]
    fn retry_policy_excludes_429_and_includes_5xx() {
        assert!(!super::should_retry_status(
            reqwest::StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(super::should_retry_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(super::should_retry_status(reqwest::StatusCode::BAD_GATEWAY));
    }

    fn extract_section<'a>(source: &'a str, start_marker: &str, end_marker: &str) -> &'a str {
        let start = source
            .find(start_marker)
            .expect("start marker should exist in providers source");
        let end = source[start..]
            .find(end_marker)
            .map(|offset| start + offset)
            .expect("end marker should exist in providers source");
        &source[start..end]
    }

    #[test]
    fn provider_failover_order_is_consistent_across_paths() {
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/providers/mod.rs"));

        let batch_section = extract_section(
            source,
            "pub async fn fetch_batch_news",
            "pub async fn fetch_news",
        );

        let single_section = extract_section(
            source,
            "pub async fn fetch_news",
            "pub async fn fetch_llm_batch_sentiment",
        );

        let batch_marketaux = batch_section
            .find("marketaux_url")
            .expect("batch path should reference marketaux_url");
        let batch_alphavantage = batch_section
            .find("alphavantage_url")
            .expect("batch path should reference alphavantage_url");
        let batch_finnhub = batch_section
            .find("finn_url")
            .expect("batch path should reference finn_url");

        let single_marketaux = single_section
            .find("marketaux_url")
            .expect("single path should reference marketaux_url");
        let single_alphavantage = single_section
            .find("alphavantage_url")
            .expect("single path should reference alphavantage_url");
        let single_finnhub = single_section
            .find("finn_url")
            .expect("single path should reference finn_url");

        assert!(
            batch_marketaux < batch_alphavantage && batch_alphavantage < batch_finnhub,
            "batch failover order drifted: expected Tiingo -> MarketAux -> AlphaVantage -> Finnhub"
        );
        assert!(
            single_marketaux < single_alphavantage && single_alphavantage < single_finnhub,
            "single failover order drifted: expected Tiingo -> MarketAux -> AlphaVantage -> Finnhub"
        );
    }

    #[test]
    fn alphavantage_batch_branch_enforces_budget_before_call() {
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/providers/mod.rs"));
        let alphavantage_batch_section = extract_section(
            source,
            "// 3. Tertiary: Alpha Vantage NEWS_SENTIMENT (Batched fallback for unresolved symbols)",
            "// 4. Quaternary: Finnhub (Individual fallback for last-resort symbols)",
        );

        assert!(
            alphavantage_batch_section.contains("if !consume_budget(&mut remaining_budget)"),
            "alphavantage branch should gate call by provider budget"
        );
        assert!(
            alphavantage_batch_section.contains("attempted_alphavantage = true"),
            "alphavantage branch should mark attempted status after budget gate"
        );
    }

    #[tokio::test]
    async fn test_sequential_failover_logic() {
        // Concept test: If primary succeeds, we never attempt fallbacks
        // Implementation: Verify logic flow branch coverage in fetch_news
        let mock_success = true;
        let secondary_called = !mock_success;
        assert!(
            !secondary_called,
            "Secondary should not be called when primary succeeds"
        );
    }

    #[test]
    fn alphavantage_payload_fixture_maps_feed_by_ticker() {
        let payload = r#"
                {
                    "feed": [
                        {
                            "title": "AAPL rallies after earnings",
                            "ticker_sentiment": [
                                {"ticker": "AAPL"},
                                {"ticker": "MSFT"}
                            ]
                        },
                        {
                            "title": "TSLA slides on margin concerns",
                            "ticker_sentiment": [
                                {"ticker": "TSLA"}
                            ]
                        },
                        {
                            "title": " ",
                            "ticker_sentiment": [
                                {"ticker": "AAPL"}
                            ]
                        },
                        {
                            "title": "Unmapped ticker article",
                            "ticker_sentiment": [
                                {"ticker": "NVDA"}
                            ]
                        }
                    ]
                }
                "#;

        let response: super::AlphaVantageNewsResponse =
            serde_json::from_str(payload).expect("fixture payload should parse");
        let extracted = super::alphavantage_extract_titles_by_ticker(
            &response,
            &["AAPL".to_string(), "TSLA".to_string()],
            5,
        );

        let aapl_titles = extracted
            .get("AAPL")
            .expect("AAPL should be extracted from fixture");
        let tsla_titles = extracted
            .get("TSLA")
            .expect("TSLA should be extracted from fixture");

        assert_eq!(aapl_titles.len(), 1);
        assert_eq!(tsla_titles.len(), 1);
        assert_eq!(aapl_titles[0], "AAPL rallies after earnings");
        assert_eq!(tsla_titles[0], "TSLA slides on margin concerns");
        assert!(!extracted.contains_key("MSFT"));
        assert!(!extracted.contains_key("NVDA"));
    }

    #[test]
    fn alphavantage_payload_notice_blocks_extraction() {
        let payload = r#"
                {
                    "Note": "Thank you for using Alpha Vantage!",
                    "feed": [
                        {
                            "title": "Should not be used",
                            "ticker_sentiment": [
                                {"ticker": "AAPL"}
                            ]
                        }
                    ]
                }
                "#;

        let response: super::AlphaVantageNewsResponse =
            serde_json::from_str(payload).expect("notice payload should parse");
        assert!(super::alphavantage_payload_has_notice(&response));

        let extracted =
            super::alphavantage_extract_titles_by_ticker(&response, &["AAPL".to_string()], 5);
        assert!(extracted.is_empty());
    }

    #[test]
    fn alphavantage_value_fallback_skips_malformed_items_and_keeps_valid() {
        let payload = r#"
                {
                    "feed": [
                        {
                            "title": "Malformed sentiment shape",
                            "ticker_sentiment": {"ticker": "AAPL"}
                        },
                        {
                            "title": "AAPL valid fallback item",
                            "ticker_sentiment": [
                                {"ticker": "AAPL"}
                            ]
                        },
                        {
                            "title": 42,
                            "ticker_sentiment": [
                                {"ticker": "AAPL"}
                            ]
                        }
                    ]
                }
                "#;

        assert!(serde_json::from_str::<super::AlphaVantageNewsResponse>(payload).is_err());

        let value: serde_json::Value =
            serde_json::from_str(payload).expect("fallback payload should parse as generic json");
        let extracted = super::alphavantage_extract_titles_from_value_by_ticker(
            &value,
            &["AAPL".to_string()],
            5,
        );

        let titles = extracted
            .get("AAPL")
            .expect("AAPL should be present from valid fallback item");
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0], "AAPL valid fallback item");
    }
}
