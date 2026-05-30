use serde::Deserialize;
use std::env;
use std::fmt;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub port: u16,
    pub log_level: String,

    // API Keys
    pub tiingo_key: String,
    pub finnhub_key: Option<String>,
    pub marketaux_key: Option<String>,
    pub alphavantage_key: Option<String>,
    pub grok_key: Option<String>,
    pub deepseek_key: Option<String>,
    pub openai_key: Option<String>,

    pub primary_llm: String, // Backward-compatible hint for first LLM provider.
    pub llm_provider_order: Vec<String>,
    pub grok_model: String,
    pub deepseek_model: String,
    pub openai_model: String,

    // Caching
    pub redis_url: Option<String>,
    pub cache_ttl_sec: u64,

    // Throttling
    pub rate_limit_per_min: u32,
    pub global_rate_limit_per_min: u32,

    // Authentication
    pub auth_mode: String,                    // "none" | "api_key" | "jwt"
    pub auth_api_keys: Vec<(String, String)>, // (tenant_id, api_key)
    pub jwt_secret: Option<String>,
    pub jwt_issuer: Option<String>,

    // Performance
    pub onnx_threads: u32,
    pub model_path: String,

    // Security
    pub egress_allowlist: Vec<String>,

    // Reliability
    pub provider_call_budget_per_request: u32,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("port", &self.port)
            .field("log_level", &self.log_level)
            .field("tiingo_key", &"[MASKED]")
            .field(
                "finnhub_key",
                &self.finnhub_key.as_ref().map(|_| "[MASKED]"),
            )
            .field(
                "alphavantage_key",
                &self.alphavantage_key.as_ref().map(|_| "[MASKED]"),
            )
            .field("grok_key", &self.grok_key.as_ref().map(|_| "[MASKED]"))
            .field(
                "deepseek_key",
                &self.deepseek_key.as_ref().map(|_| "[MASKED]"),
            )
            .field("openai_key", &self.openai_key.as_ref().map(|_| "[MASKED]"))
            .field("primary_llm", &self.primary_llm)
            .field("llm_provider_order", &self.llm_provider_order)
            .field("grok_model", &self.grok_model)
            .field("deepseek_model", &self.deepseek_model)
            .field("openai_model", &self.openai_model)
            .field("auth_mode", &self.auth_mode)
            .field(
                "auth_api_keys",
                &self
                    .auth_api_keys
                    .iter()
                    .map(|(tenant, _)| tenant)
                    .collect::<Vec<_>>(),
            )
            .field("jwt_secret", &self.jwt_secret.as_ref().map(|_| "[MASKED]"))
            .field("jwt_issuer", &self.jwt_issuer)
            .field("redis_url", &self.redis_url.as_ref().map(|_| "[MASKED]"))
            .field("rate_limit_per_min", &self.rate_limit_per_min)
            .field("global_rate_limit_per_min", &self.global_rate_limit_per_min)
            .field("egress_allowlist", &self.egress_allowlist)
            .field(
                "provider_call_budget_per_request",
                &self.provider_call_budget_per_request,
            )
            .finish()
    }
}

fn parse_auth_api_keys(value: &str) -> anyhow::Result<Vec<(String, String)>> {
    value
        .split(',')
        .filter(|pair| !pair.trim().is_empty())
        .map(|pair| {
            let mut parts = pair.splitn(2, ':');
            let tenant = parts.next().unwrap_or_default().trim();
            let key = parts.next().unwrap_or_default().trim();
            if tenant.is_empty() || key.is_empty() {
                anyhow::bail!("Invalid TP_AUTH_API_KEYS entry: {}", pair);
            }
            Ok((tenant.to_string(), key.to_string()))
        })
        .collect()
}

fn parse_egress_allowlist(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|host| host.trim().to_ascii_lowercase())
        .filter(|host| !host.is_empty())
        .collect()
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_llm_provider(provider: &str) -> anyhow::Result<String> {
    let provider = provider.trim().to_ascii_lowercase();
    match provider.as_str() {
        "grok" | "deepseek" | "openai" => Ok(provider),
        _ => anyhow::bail!(
            "Unsupported LLM provider '{}'; expected one of: grok, deepseek, openai",
            provider
        ),
    }
}

pub fn parse_llm_provider_order(
    raw_order: Option<&str>,
    primary_llm: &str,
) -> anyhow::Result<Vec<String>> {
    let configured = raw_order
        .map(|order| {
            order
                .split(',')
                .map(str::trim)
                .filter(|provider| !provider.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|providers| !providers.is_empty());

    let providers = if let Some(providers) = configured {
        providers
            .into_iter()
            .map(normalize_llm_provider)
            .collect::<anyhow::Result<Vec<_>>>()?
    } else {
        let mut providers = vec![normalize_llm_provider(primary_llm)?];
        for provider in ["grok", "deepseek", "openai"] {
            if !providers.iter().any(|existing| existing == provider) {
                providers.push(provider.to_string());
            }
        }
        providers
    };

    let mut deduped = Vec::new();
    for provider in providers {
        if !deduped.contains(&provider) {
            deduped.push(provider);
        }
    }

    Ok(deduped)
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        let auth_mode = env::var("TP_AUTH_MODE")
            .unwrap_or_else(|_| "none".to_string())
            .to_lowercase();
        let auth_api_keys_raw = env::var("TP_AUTH_API_KEYS").unwrap_or_default();
        let auth_api_keys = if auth_api_keys_raw.trim().is_empty() {
            Vec::new()
        } else {
            parse_auth_api_keys(&auth_api_keys_raw)?
        };
        let jwt_secret = optional_env("TP_JWT_SECRET");
        let jwt_issuer = optional_env("TP_JWT_ISSUER");
        let egress_allowlist =
            parse_egress_allowlist(&env::var("TP_EGRESS_ALLOWLIST").unwrap_or_default());
        let primary_llm = normalize_llm_provider(
            &env::var("TP_PRIMARY_LLM").unwrap_or_else(|_| "grok".to_string()),
        )?;
        let llm_provider_order = parse_llm_provider_order(
            env::var("TP_LLM_PROVIDER_ORDER").ok().as_deref(),
            &primary_llm,
        )?;

        match auth_mode.as_str() {
            "none" => {}
            "api_key" => {
                if auth_api_keys.is_empty() {
                    anyhow::bail!("TP_AUTH_API_KEYS is required when TP_AUTH_MODE=api_key");
                }
            }
            "jwt" => {
                if jwt_secret
                    .as_ref()
                    .map(|v| v.trim().is_empty())
                    .unwrap_or(true)
                {
                    anyhow::bail!("TP_JWT_SECRET is required when TP_AUTH_MODE=jwt");
                }
            }
            _ => anyhow::bail!("TP_AUTH_MODE must be one of: none, api_key, jwt"),
        }

        Ok(Config {
            port: env::var("PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()?,
            log_level: env::var("TP_LOG_LEVEL").unwrap_or_else(|_| "INFO".to_string()),

            tiingo_key: env::var("TP_TIINGO_KEY")
                .expect("TP_TIINGO_KEY is required")
                .trim()
                .to_string(),
            finnhub_key: optional_env("TP_FINNHUB_KEY"),
            marketaux_key: optional_env("TP_MARKETAUX_KEY"),
            alphavantage_key: optional_env("TP_ALPHAVANTAGE_KEY"),
            grok_key: optional_env("TP_GROK_KEY"),
            deepseek_key: optional_env("TP_DEEPSEEK_KEY"),
            openai_key: optional_env("TP_OPENAI_KEY"),

            primary_llm,
            llm_provider_order,
            grok_model: optional_env("TP_GROK_MODEL").unwrap_or_else(|| "grok-4.3".to_string()),
            deepseek_model: optional_env("TP_DEEPSEEK_MODEL")
                .unwrap_or_else(|| "deepseek-v4-pro".to_string()),
            openai_model: optional_env("TP_OPENAI_MODEL")
                .unwrap_or_else(|| "gpt-5.4-nano".to_string()),

            redis_url: optional_env("TP_REDIS_URL"),
            cache_ttl_sec: env::var("TP_CACHE_TTL")
                .unwrap_or_else(|_| "300".to_string())
                .parse()?,

            rate_limit_per_min: env::var("TP_RATE_LIMIT")
                .unwrap_or_else(|_| "100".to_string())
                .parse()?,
            global_rate_limit_per_min: env::var("TP_GLOBAL_RATE_LIMIT")
                .unwrap_or_else(|_| "1000".to_string())
                .parse()?,

            auth_mode,
            auth_api_keys,
            jwt_secret,
            jwt_issuer,

            onnx_threads: env::var("TP_ONNX_THREADS")
                .unwrap_or_else(|_| "2".to_string())
                .parse()?,
            model_path: env::var("TP_MODEL_PATH").unwrap_or_else(|_| "model.onnx".to_string()),
            egress_allowlist,
            provider_call_budget_per_request: env::var("TP_PROVIDER_CALL_BUDGET_PER_REQUEST")
                .unwrap_or_else(|_| "6".to_string())
                .parse()?,
        })
    }
}
