use anyhow::{Result, anyhow};
use reqwest::Url;
use std::collections::HashSet;

use crate::config::Config;

fn default_allowed_hosts() -> HashSet<&'static str> {
    [
        "api.tiingo.com",
        "finnhub.io",
        "api.marketaux.com",
        "www.alphavantage.co",
        "api.x.ai",
        "api.deepseek.com",
        "api.openai.com",
    ]
    .into_iter()
    .collect()
}

fn host_is_allowed(host: &str, config: &Config) -> bool {
    if default_allowed_hosts().contains(host) {
        return true;
    }

    config
        .egress_allowlist
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(host))
}

pub fn enforce_allowed_url(url: &str, config: &Config) -> Result<()> {
    let parsed = Url::parse(url).map_err(|e| anyhow!("EGRESS_INVALID_URL: {}", e))?;

    if parsed.scheme() != "https" {
        return Err(anyhow!("EGRESS_SCHEME_DENIED: only https is allowed"));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("EGRESS_INVALID_URL: missing host"))?
        .to_ascii_lowercase();

    if !host_is_allowed(host.as_str(), config) {
        return Err(anyhow!("EGRESS_HOST_DENIED: {}", host));
    }

    if let Some(port) = parsed.port() {
        if port != 443 {
            return Err(anyhow!("EGRESS_PORT_DENIED: {}", port));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    use super::enforce_allowed_url;

    fn test_config() -> Config {
        Config {
            port: 8080,
            log_level: "INFO".to_string(),
            tiingo_key: "tiingo-test-key".to_string(),
            finnhub_key: None,
            marketaux_key: None,
            alphavantage_key: None,
            grok_key: None,
            deepseek_key: None,
            openai_key: None,
            primary_llm: "grok".to_string(),
            llm_provider_order: vec![
                "grok".to_string(),
                "deepseek".to_string(),
                "openai".to_string(),
            ],
            grok_model: "grok-4.3".to_string(),
            deepseek_model: "deepseek-v4-pro".to_string(),
            openai_model: "gpt-5.4-nano".to_string(),
            redis_url: None,
            cache_ttl_sec: 300,
            rate_limit_per_min: 100,
            global_rate_limit_per_min: 1000,
            auth_mode: "none".to_string(),
            auth_api_keys: vec![],
            jwt_secret: None,
            jwt_issuer: None,
            onnx_threads: 2,
            model_path: "model.onnx".to_string(),
            egress_allowlist: vec![],
            provider_call_budget_per_request: 6,
        }
    }

    #[test]
    fn allows_known_https_host() {
        let result = enforce_allowed_url(
            "https://api.tiingo.com/tiingo/news?tickers=AAPL",
            &test_config(),
        );
        assert!(result.is_ok());

        let alpha_result = enforce_allowed_url(
            "https://www.alphavantage.co/query?function=NEWS_SENTIMENT&tickers=AAPL",
            &test_config(),
        );
        assert!(alpha_result.is_ok());

        let openai_result =
            enforce_allowed_url("https://api.openai.com/v1/chat/completions", &test_config());
        assert!(openai_result.is_ok());
    }

    #[test]
    fn denies_unknown_host() {
        let result = enforce_allowed_url("https://example.com/path", &test_config());
        assert!(result.is_err());
    }

    #[test]
    fn denies_non_https_scheme() {
        let result = enforce_allowed_url("http://api.tiingo.com/tiingo/news", &test_config());
        assert!(result.is_err());
    }

    #[test]
    fn allows_host_from_config_override() {
        let mut config = test_config();
        config.egress_allowlist = vec!["internal.egress.local".to_string()];

        let result = enforce_allowed_url("https://internal.egress.local/path", &config);
        assert!(result.is_ok());
    }
}
