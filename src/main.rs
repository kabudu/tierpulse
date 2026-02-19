use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use moka::future::Cache;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{info, Level};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use tierpulse::config::Config;
use tierpulse::inference::InferenceEngine;
use tierpulse::metrics::MetricsRegistry;
use tierpulse::{app, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Load Configuration
    let config = Config::from_env()?;

    // 2. Initialize Logging
    let filter = EnvFilter::new(&config.log_level).add_directive(Level::INFO.into());

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(filter)
        .json()
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Initializing tierpulse v0.1.0 on port {}", config.port);

    // 3. Initialize High-Performance Core
    let engine = InferenceEngine::new(&config)?;

    let global_rate_limiter = Arc::new(RateLimiter::direct(Quota::per_minute(
        std::num::NonZeroU32::new(config.global_rate_limit_per_min)
            .expect("Global rate limit must be > 0"),
    )));

    let tenant_rate_limiter: Arc<
        RateLimiter<String, DefaultKeyedStateStore<String>, governor::clock::DefaultClock>,
    > = Arc::new(RateLimiter::keyed(Quota::per_minute(
        std::num::NonZeroU32::new(config.rate_limit_per_min).expect("Rate limit must be > 0"),
    )));

    let cache = Cache::builder()
        .max_capacity(10_000)
        .time_to_live(std::time::Duration::from_secs(config.cache_ttl_sec))
        .build();

    let redis_client = config
        .redis_url
        .as_ref()
        .and_then(|url| redis::Client::open(url.as_str()).ok());

    let shared_state = Arc::new(AppState {
        config: config.clone(),
        engine,
        global_rate_limiter,
        tenant_rate_limiter,
        cache,
        redis_client,
        http_client: reqwest::Client::new(),
        metrics: Arc::new(MetricsRegistry::new()),
    });

    // 4. Start Server
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = TcpListener::bind(&addr).await?;
    info!("Server listening on {}", addr);

    axum::serve(listener, app(shared_state).await).await?;

    Ok(())
}
