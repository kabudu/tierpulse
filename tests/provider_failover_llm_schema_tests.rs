use tierpulse::{config::parse_llm_provider_order, providers::parse_llm_content_to_results};

#[test]
fn provider_failover_order_is_consistent_in_source() {
    let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/providers/mod.rs"));

    let batch_start = source
        .find("pub async fn fetch_batch_news")
        .expect("fetch_batch_news should exist");
    let batch_end = source[batch_start..]
        .find("pub async fn fetch_news")
        .map(|offset| batch_start + offset)
        .expect("fetch_news should delimit batch section");
    let batch_section = &source[batch_start..batch_end];

    let single_start = source
        .find("pub async fn fetch_news")
        .expect("fetch_news should exist");
    let single_end = source[single_start..]
        .find("pub async fn fetch_llm_batch_sentiment")
        .map(|offset| single_start + offset)
        .expect("fetch_llm_batch_sentiment should delimit single section");
    let single_section = &source[single_start..single_end];

    let batch_tiingo = batch_section
        .find("tiingo_url")
        .expect("batch tiingo reference missing");
    let batch_marketaux = batch_section
        .find("marketaux_url")
        .expect("batch marketaux reference missing");
    let batch_alphavantage = batch_section
        .find("alphavantage_url")
        .expect("batch alphavantage reference missing");
    let batch_finnhub = batch_section
        .find("finn_url")
        .expect("batch finnhub reference missing");

    let single_tiingo = single_section
        .find("tiingo_url")
        .expect("single tiingo reference missing");
    let single_marketaux = single_section
        .find("marketaux_url")
        .expect("single marketaux reference missing");
    let single_alphavantage = single_section
        .find("alphavantage_url")
        .expect("single alphavantage reference missing");
    let single_finnhub = single_section
        .find("finn_url")
        .expect("single finnhub reference missing");

    assert!(
        batch_tiingo < batch_marketaux
            && batch_marketaux < batch_alphavantage
            && batch_alphavantage < batch_finnhub
    );
    assert!(
        single_tiingo < single_marketaux
            && single_marketaux < single_alphavantage
            && single_alphavantage < single_finnhub
    );
}

#[test]
fn llm_provider_order_defaults_to_primary_then_remaining_providers() {
    let order = parse_llm_provider_order(None, "deepseek").expect("order should parse");

    assert_eq!(order, vec!["deepseek", "grok", "openai"]);
}

#[test]
fn llm_provider_order_accepts_explicit_deduped_sequence() {
    let order = parse_llm_provider_order(Some("openai,grok,openai,deepseek"), "grok")
        .expect("order should parse");

    assert_eq!(order, vec!["openai", "grok", "deepseek"]);
}

#[test]
fn llm_provider_order_rejects_unknown_providers() {
    let err = parse_llm_provider_order(Some("grok,anthropic"), "grok")
        .expect_err("unknown providers should fail");

    assert!(format!("{}", err).contains("Unsupported LLM provider"));
}

#[test]
fn llm_schema_parser_accepts_valid_json_array() {
    let content = r#"[
      {"symbol":"AAPL","sentiment_score":0.82,"confidence":0.94,"reasoning":"Strong earnings outlook","label":"bullish"},
      {"symbol":"TSLA","sentiment_score":-0.35,"confidence":0.71,"reasoning":"Margin pressure concerns","label":"bearish"}
    ]"#;

    let parsed = parse_llm_content_to_results(content).expect("valid content should parse");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].source_tier, "tier_3_llm");
    assert!(parsed[0].reasoning.is_some());
}

#[test]
fn llm_schema_parser_accepts_provider_json_object() {
    let content = r#"{
      "results": [
        {"symbol":"AAPL","sentiment_score":0.82,"confidence":0.94,"reasoning":"Strong earnings outlook","label":"bullish"},
        {"symbol":"TSLA","sentiment_score":-0.35,"confidence":0.71,"reasoning":"Margin pressure concerns","label":"bearish"}
      ]
    }"#;

    let parsed = parse_llm_content_to_results(content).expect("valid object content should parse");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].symbol, "AAPL");
    assert_eq!(parsed[1].label, "bearish");
}

#[test]
fn llm_schema_parser_rejects_json_object_without_results_array() {
    let content = r#"{"symbol":"AAPL","sentiment_score":0.82}"#;
    let err =
        parse_llm_content_to_results(content).expect_err("object without results should fail");
    assert!(format!("{}", err).contains("results array"));
}

#[test]
fn llm_schema_parser_rejects_missing_required_fields() {
    let content = r#"[{"symbol":"AAPL","sentiment_score":0.82}]"#;
    assert!(parse_llm_content_to_results(content).is_err());
}

#[test]
fn llm_schema_parser_normalizes_provider_label_variants() {
    let content = r#"{
      "results": [
        {"symbol":"AAPL","sentiment_score":0.42,"confidence":0.74,"reasoning":"Positive demand outlook","label":"Positive"},
        {"symbol":"TSLA","sentiment_score":-0.28,"confidence":0.69,"reasoning":"Negative margin pressure","label":"Negative"}
      ]
    }"#;

    let parsed = parse_llm_content_to_results(content).expect("provider labels should parse");
    assert_eq!(parsed[0].label, "bullish");
    assert_eq!(parsed[1].label, "bearish");
}
