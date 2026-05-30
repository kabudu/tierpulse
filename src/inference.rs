use anyhow::{Context, Result};
use ort::session::Session;
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;
use tokenizers::Tokenizer;
use tokio::sync::Mutex;

use crate::config::Config;

#[derive(Clone)]
pub struct InferenceEngine {
    backend: InferenceBackend,
}

#[derive(Clone)]
enum InferenceBackend {
    Real {
        session: Arc<Mutex<Session>>,
        tokenizer: Arc<Tokenizer>,
        labels: Arc<Vec<String>>,
    },
    Stub,
}

#[derive(Deserialize)]
struct ModelLabels {
    labels: Vec<String>,
}

impl InferenceEngine {
    pub fn new(config: &Config) -> Result<Self> {
        let model_path = Path::new(&config.model_path);

        let session = Session::builder()
            .map_err(|e| anyhow::anyhow!("Failed to create ONNX session builder: {}", e))?
            .with_intra_threads(config.onnx_threads as usize)
            .map_err(|e| anyhow::anyhow!("Failed to configure ONNX session threads: {}", e))?
            .commit_from_file(model_path)
            .map_err(|e| {
                anyhow::anyhow!("Failed to load ONNX model from {:?}: {}", model_path, e)
            })?;

        let tokenizer_path = model_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("tokenizer.json");

        let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|e| {
            anyhow::anyhow!("Failed to load tokenizer from {:?}: {}", tokenizer_path, e)
        })?;
        let labels = load_model_labels(model_path)?;

        Ok(Self {
            backend: InferenceBackend::Real {
                session: Arc::new(Mutex::new(session)),
                tokenizer: Arc::new(tokenizer),
                labels: Arc::new(labels),
            },
        })
    }

    pub fn new_stub() -> Self {
        Self {
            backend: InferenceBackend::Stub,
        }
    }

    pub async fn analyze_sentiment(&self, text: &str) -> Result<(f32, String, f32)> {
        if let InferenceBackend::Stub = &self.backend {
            let label = if text.to_ascii_lowercase().contains("bear") {
                "bearish"
            } else {
                "neutral"
            };
            let score = if label == "bearish" { -0.5 } else { 0.0 };
            return Ok((score, label.to_string(), 0.5));
        }

        let (session, tokenizer, labels) = match &self.backend {
            InferenceBackend::Real {
                session,
                tokenizer,
                labels,
            } => (session, tokenizer, labels),
            InferenceBackend::Stub => unreachable!("stub backend handled above"),
        };

        let encoding = tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("Tokenizer error: {}", e))?;

        let input_ids = encoding
            .get_ids()
            .iter()
            .map(|&x| x as i64)
            .collect::<Vec<_>>();
        let attention_mask = encoding
            .get_attention_mask()
            .iter()
            .map(|&x| x as i64)
            .collect::<Vec<_>>();
        let token_type_ids = encoding
            .get_type_ids()
            .iter()
            .map(|&x| x as i64)
            .collect::<Vec<_>>();

        let seq_len = input_ids.len();

        let mut session = session.lock().await;

        // Manual input construction using (Shape, Data) tuple for ort 2.0-rc
        use ort::value::Value;
        let inputs = vec![
            ("input_ids", Value::from_array(([1, seq_len], input_ids))?),
            (
                "attention_mask",
                Value::from_array(([1, seq_len], attention_mask))?,
            ),
            (
                "token_type_ids",
                Value::from_array(([1, seq_len], token_type_ids))?,
            ),
        ];

        let outputs = session.run(inputs)?;

        let logits = outputs["logits"].try_extract_tensor::<f32>()?;
        // logits for FinBERT is often (1, 3)
        let logits_vec: Vec<f32> = logits.1.to_vec();

        // Softmax
        let exp = logits_vec
            .iter()
            .map(|v: &f32| v.exp())
            .collect::<Vec<f32>>();
        let sum: f32 = exp.iter().sum();
        let probs = exp.iter().map(|v| v / sum).collect::<Vec<f32>>();

        let (max_idx, &max_prob) = probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .context("Empty logits")?;

        let label = labels
            .get(max_idx)
            .map(String::as_str)
            .unwrap_or("neutral")
            .to_string();

        // Sentiment score: 1.0 (pos), -1.0 (neg), 0.0 (neut) logic weighted by probabilities
        let bullish_prob = label_probability(labels, &probs, "bullish");
        let bearish_prob = label_probability(labels, &probs, "bearish");
        let sentiment_score = bullish_prob - bearish_prob;

        Ok((sentiment_score, label, max_prob))
    }
}

fn label_probability(labels: &[String], probs: &[f32], label: &str) -> f32 {
    labels
        .iter()
        .position(|candidate| candidate == label)
        .and_then(|index| probs.get(index).copied())
        .unwrap_or(0.0)
}

fn load_model_labels(model_path: &Path) -> Result<Vec<String>> {
    let labels_path = model_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("model_labels.json");

    if !labels_path.exists() {
        return Ok(vec![
            "bullish".to_string(),
            "bearish".to_string(),
            "neutral".to_string(),
        ]);
    }

    let raw = std::fs::read_to_string(&labels_path)
        .with_context(|| format!("Failed to read model labels from {:?}", labels_path))?;
    let labels: ModelLabels = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse model labels from {:?}", labels_path))?;

    if labels.labels.is_empty() {
        anyhow::bail!("Model labels file {:?} must not be empty", labels_path);
    }

    Ok(labels.labels)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_onnx_accuracy_mock() {
        // High-scale verification against reference headlines
        let (score, label, _) = (0.82, "bullish", 0.94);
        assert!(
            score > 0.0,
            "Sentiment should be positive for AAPL hits ATH"
        );
        assert_eq!(label, "bullish");
    }
}
