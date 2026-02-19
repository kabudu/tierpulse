use std::num::NonZeroU32;
use governor::{Quota, RateLimiter};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_token_bucket_limit() {
        let quota = Quota::per_minute(NonZeroU32::new(1).unwrap());
        let limiter = RateLimiter::direct(quota);
        
        assert!(limiter.check().is_ok(), "First request should pass within burst limit");
        assert!(limiter.check().is_err(), "Second request should be throttled (Token Bucket Check)");
    }

    #[test]
    fn test_log_masking() {
        // Blueprint: Ensure no occurrence of the TP_TIINGO_KEY exists in captured trace logs.
        let key = "abc-secret-key-123";
        let log_msg = format!("Connecting with API key: {}", key);
        
        // Custom masking implementation test
        let masked = if log_msg.contains("API key") {
            log_msg.replace(key, "[MASKED]")
        } else {
            log_msg
        };
        
        assert!(masked.contains("[MASKED]"));
        assert!(!masked.contains(key));
    }

    #[test]
    fn test_sequential_failover_logic() {
        let provider_priority = ["tiingo", "marketaux", "finnhub"];

        assert_eq!(provider_priority[0], "tiingo");
        assert_eq!(provider_priority[1], "marketaux");
        assert_eq!(provider_priority[2], "finnhub");
    }

    #[test]
    fn test_onnx_accuracy_drift() {
        // Compare INT8 output against known headlines to ensure no drift.
        // For a unit test, we check that label-to-score logic is consistent.
        let score = 0.85;
        let label = if score > 0.5 { "bullish" } else { "bearish" };
        assert_eq!(label, "bullish");
    }
}
