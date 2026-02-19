use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const MAX_DURATION_SAMPLES: usize = 4096;

#[derive(Default)]
pub struct MetricsRegistry {
    request_count: AtomicU64,
    request_duration_sum_ms: AtomicU64,
    request_durations_ms: Mutex<Vec<u64>>,

    cache_hits: AtomicU64,
    cache_misses: AtomicU64,

    provider_calls: Mutex<HashMap<String, u64>>,
    provider_errors: Mutex<HashMap<(String, String), u64>>,

    fallback_transitions: Mutex<HashMap<(String, String), u64>>,
    tier_exhaustions: AtomicU64,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe_request(&self, duration: Duration, tier_exhausted: bool) {
        let duration_ms = duration.as_millis() as u64;
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.request_duration_sum_ms.fetch_add(duration_ms, Ordering::Relaxed);

        if tier_exhausted {
            self.tier_exhaustions.fetch_add(1, Ordering::Relaxed);
        }

        let mut durations = self.request_durations_ms.lock().expect("request_durations_ms poisoned");
        durations.push(duration_ms);
        if durations.len() > MAX_DURATION_SAMPLES {
            let overflow = durations.len() - MAX_DURATION_SAMPLES;
            durations.drain(0..overflow);
        }
    }

    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_provider_call(&self, provider: &str) {
        let mut calls = self.provider_calls.lock().expect("provider_calls poisoned");
        *calls.entry(provider.to_string()).or_insert(0) += 1;
    }

    pub fn record_provider_error(&self, provider: &str, status_class: &str) {
        let mut errors = self.provider_errors.lock().expect("provider_errors poisoned");
        *errors
            .entry((provider.to_string(), status_class.to_string()))
            .or_insert(0) += 1;
    }

    pub fn record_fallback_transition(&self, from: &str, to: &str) {
        let mut transitions = self
            .fallback_transitions
            .lock()
            .expect("fallback_transitions poisoned");
        *transitions
            .entry((from.to_string(), to.to_string()))
            .or_insert(0) += 1;
    }

    fn quantile(sorted: &[u64], q: f64) -> f64 {
        if sorted.is_empty() {
            return 0.0;
        }
        let index = ((sorted.len() - 1) as f64 * q).round() as usize;
        sorted[index] as f64
    }

    pub fn render_prometheus(&self) -> String {
        let request_count = self.request_count.load(Ordering::Relaxed);
        let request_sum = self.request_duration_sum_ms.load(Ordering::Relaxed);
        let cache_hits = self.cache_hits.load(Ordering::Relaxed);
        let cache_misses = self.cache_misses.load(Ordering::Relaxed);
        let tier_exhaustions = self.tier_exhaustions.load(Ordering::Relaxed);

        let mut durations = self
            .request_durations_ms
            .lock()
            .expect("request_durations_ms poisoned")
            .clone();
        durations.sort_unstable();

        let p50 = Self::quantile(&durations, 0.50);
        let p95 = Self::quantile(&durations, 0.95);
        let p99 = Self::quantile(&durations, 0.99);

        let mut out = String::new();

        out.push_str("# HELP request_duration_ms Request duration summary in milliseconds.\\n");
        out.push_str("# TYPE request_duration_ms summary\\n");
        out.push_str(&format!("request_duration_ms{{quantile=\"0.50\"}} {:.3}\\n", p50));
        out.push_str(&format!("request_duration_ms{{quantile=\"0.95\"}} {:.3}\\n", p95));
        out.push_str(&format!("request_duration_ms{{quantile=\"0.99\"}} {:.3}\\n", p99));
        out.push_str(&format!("request_duration_ms_sum {}\\n", request_sum));
        out.push_str(&format!("request_duration_ms_count {}\\n", request_count));

        let total_cache = cache_hits + cache_misses;
        let cache_hit_ratio = if total_cache == 0 {
            0.0
        } else {
            cache_hits as f64 / total_cache as f64
        };
        out.push_str("# HELP cache_hit_ratio Cache hit ratio across in-memory and Redis lookups.\\n");
        out.push_str("# TYPE cache_hit_ratio gauge\\n");
        out.push_str(&format!("cache_hit_ratio {:.6}\\n", cache_hit_ratio));

        out.push_str("# HELP provider_error_rate Provider error ratio by provider and status class.\\n");
        out.push_str("# TYPE provider_error_rate gauge\\n");

        let calls = self.provider_calls.lock().expect("provider_calls poisoned").clone();
        let errors = self.provider_errors.lock().expect("provider_errors poisoned").clone();
        for ((provider, status_class), error_count) in errors {
            let call_count = calls.get(&provider).copied().unwrap_or(0);
            let rate = if call_count == 0 {
                0.0
            } else {
                error_count as f64 / call_count as f64
            };
            out.push_str(&format!(
                "provider_error_rate{{provider=\"{}\",status_class=\"{}\"}} {:.6}\\n",
                provider, status_class, rate
            ));
        }

        out.push_str("# HELP fallback_transition_count Count of fallback transitions between tiers.\\n");
        out.push_str("# TYPE fallback_transition_count counter\\n");
        let transitions = self
            .fallback_transitions
            .lock()
            .expect("fallback_transitions poisoned")
            .clone();
        for ((from, to), count) in transitions {
            out.push_str(&format!(
                "fallback_transition_count{{from=\"{}\",to=\"{}\"}} {}\\n",
                from, to, count
            ));
        }

        let exhaustion_rate = if request_count == 0 {
            0.0
        } else {
            tier_exhaustions as f64 / request_count as f64
        };
        out.push_str("# HELP tier_exhaustion_rate Ratio of analyze requests ending in intelligence exhaustion.\\n");
        out.push_str("# TYPE tier_exhaustion_rate gauge\\n");
        out.push_str(&format!("tier_exhaustion_rate {:.6}\\n", exhaustion_rate));

        out
    }
}
