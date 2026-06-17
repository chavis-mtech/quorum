//! AiEngine adapter — communicates with the Python AI sidecar over HTTP (JSON)
//!
//! Python runs `ai-layer/http_server.py` (stdlib only, no framework required)
//! endpoint: POST /analyze {"symbol":"BTC"} → Analysis JSON, GET /health

use async_trait::async_trait;

use crate::domain::models::{Analysis, AnalyzeProgress};
use crate::domain::ports::{AiEngine, DomainError, DomainResult};

pub struct SidecarAiEngine {
    base_url: String,
    http: reqwest::Client,
}

impl SidecarAiEngine {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("build http client"),
        }
    }
}

#[async_trait]
impl AiEngine for SidecarAiEngine {
    async fn analyze_with(
        &self,
        symbol: &str,
        judge_override: Option<serde_json::Value>,
    ) -> DomainResult<Analysis> {
        let url = format!("{}/analyze", self.base_url);
        let mut body = serde_json::json!({ "symbol": symbol });
        if let Some(j) = judge_override {
            body["judge_override"] = j;
        }
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| DomainError::Ai(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(DomainError::Ai(format!("sidecar status {}", resp.status())));
        }
        resp.json::<Analysis>()
            .await
            .map_err(|e| DomainError::Ai(format!("parse analysis: {e}")))
    }

    async fn analyze_stream(
        &self,
        symbol: &str,
        judge_override: Option<serde_json::Value>,
        on_progress: &(dyn Fn(AnalyzeProgress) + Send + Sync),
    ) -> DomainResult<Analysis> {
        let url = format!("{}/analyze/stream", self.base_url);
        let mut body = serde_json::json!({ "symbol": symbol });
        if let Some(j) = judge_override {
            body["judge_override"] = j;
        }
        let mut resp = self
            .http
            .post(&url)
            .json(&body)
            // large model thinking can take several minutes — allow a long timeout
            .timeout(std::time::Duration::from_secs(900))
            .send()
            .await
            .map_err(|e| DomainError::Ai(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(DomainError::Ai(format!("sidecar status {}", resp.status())));
        }

        let mut buf: Vec<u8> = Vec::new();
        let mut analysis: Option<Analysis> = None;
        // read body chunk by chunk → split into NDJSON lines → parse events
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| DomainError::Ai(e.to_string()))?
        {
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line = &line[..line.len() - 1]; // strip '\n'
                if line.is_empty() {
                    continue;
                }
                let ev: serde_json::Value = match serde_json::from_slice(line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match ev.get("type").and_then(|t| t.as_str()) {
                    Some("done") => {
                        if let Some(a) = ev.get("analysis") {
                            analysis = serde_json::from_value(a.clone()).ok();
                        }
                    }
                    Some("think") | Some("stage") => {
                        on_progress(AnalyzeProgress {
                            pct: ev.get("pct").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
                            stage: ev
                                .get("stage")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            label: ev
                                .get("label")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            delta: ev
                                .get("delta")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        });
                    }
                    Some("error") => {
                        let msg = ev
                            .get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or("stream error");
                        return Err(DomainError::Ai(msg.to_string()));
                    }
                    _ => {}
                }
            }
        }

        analysis.ok_or_else(|| DomainError::Ai("stream ended without an analysis result".into()))
    }

    async fn health(&self) -> DomainResult<()> {
        let url = format!("{}/health", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| DomainError::Ai(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(DomainError::Ai(format!("health status {}", resp.status())))
        }
    }
}
