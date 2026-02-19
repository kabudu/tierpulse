use crate::config::Config;

const REDACTED: &str = "[REDACTED]";

pub fn redact_sensitive_text(input: &str, config: &Config) -> String {
    let mut redacted = input.to_string();

    for secret in sensitive_values(config) {
        if !secret.is_empty() {
            redacted = redacted.replace(&secret, REDACTED);
        }
    }

    for key in [
        "token",
        "api_token",
        "api_key",
        "apikey",
        "key",
        "authorization",
        "x-api-key",
    ] {
        redacted = redact_query_param(&redacted, key);
    }

    redact_bearer_tokens(&redacted)
}

fn sensitive_values(config: &Config) -> Vec<String> {
    let mut values = Vec::new();

    values.push(config.tiingo_key.clone());
    if let Some(v) = &config.finnhub_key {
        values.push(v.clone());
    }
    if let Some(v) = &config.marketaux_key {
        values.push(v.clone());
    }
    if let Some(v) = &config.grok_key {
        values.push(v.clone());
    }
    if let Some(v) = &config.deepseek_key {
        values.push(v.clone());
    }
    if let Some(v) = &config.jwt_secret {
        values.push(v.clone());
    }
    for (_, key) in &config.auth_api_keys {
        values.push(key.clone());
    }

    values
}

fn redact_query_param(text: &str, key: &str) -> String {
    let mut out = text.to_string();
    let needle = format!("{}=", key);
    let mut start_idx = 0;

    while let Some(found) = out[start_idx..].find(&needle) {
        let absolute = start_idx + found;
        let value_start = absolute + needle.len();
        let value_end = out[value_start..]
            .find(['&', ' ', '\'', '"', '\n'])
            .map(|offset| value_start + offset)
            .unwrap_or(out.len());

        out.replace_range(value_start..value_end, REDACTED);
        start_idx = value_start + REDACTED.len();
    }

    out
}

fn redact_bearer_tokens(text: &str) -> String {
    let mut out = text.to_string();
    let needle = "Bearer ";
    let mut start_idx = 0;

    while let Some(found) = out[start_idx..].find(needle) {
        let absolute = start_idx + found;
        let value_start = absolute + needle.len();
        let value_end = out[value_start..]
            .find([' ', '\'', '"', '\n'])
            .map(|offset| value_start + offset)
            .unwrap_or(out.len());

        out.replace_range(value_start..value_end, REDACTED);
        start_idx = value_start + REDACTED.len();
    }

    out
}
