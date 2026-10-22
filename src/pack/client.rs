//! A minimal Claude Messages API client over raw HTTP (Rust has no official
//! SDK): one structured-output request at a time, with retries for the
//! retryable statuses and explicit handling of every stop reason.

use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use serde_json::{Value, json};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
/// `fallbacks: "default"`: on a safety decline, the API re-runs the request on
/// the recommended fallback model instead of returning the refusal.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_ATTEMPTS: u32 = 5;

pub struct Client {
    api_key: String,
    pub model: String,
    pub effort: String,
    agent: ureq::Agent,
}

impl Client {
    pub fn from_env(model: String, effort: String) -> anyhow::Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
            .context("ANTHROPIC_API_KEY is not set (run `ant auth login` and export a key, or set it directly)")?;
        // Error statuses come back as responses: retries need their headers.
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_send_body(Some(Duration::from_secs(60)))
            .timeout_recv_body(Some(Duration::from_secs(600)))
            .build();
        Ok(Client { api_key, model, effort, agent: ureq::Agent::new_with_config(config) })
    }

    /// Ask for JSON matching `schema`; returns the parsed object.
    pub fn structured(&self, system: &str, prompt: &str, schema: &Value, max_tokens: u32) -> anyhow::Result<Value> {
        let body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "system": system,
            "fallbacks": "default",
            "output_config": {
                "effort": self.effort,
                "format": {"type": "json_schema", "schema": schema},
            },
            "messages": [{"role": "user", "content": prompt}],
        });
        let response = self.send(&body)?;
        match response["stop_reason"].as_str() {
            Some("end_turn") => {}
            Some("max_tokens") => bail!("the response hit max_tokens ({max_tokens}); raise it or ask for less"),
            Some("refusal") => {
                let category = response["stop_details"]["category"].as_str().unwrap_or("unspecified");
                bail!("the request was declined (category: {category}) even after the fallback model");
            }
            other => bail!("unexpected stop_reason {other:?}"),
        }
        let text: String = response["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect();
        serde_json::from_str(&text).with_context(|| format!("response was not the requested JSON: {}", &text[..text.len().min(300)]))
    }

    fn send(&self, body: &Value) -> anyhow::Result<Value> {
        let payload = body.to_string();
        let mut delay = Duration::from_secs(2);
        for attempt in 1..=MAX_ATTEMPTS {
            let result = self
                .agent
                .post(ENDPOINT)
                .header("content-type", "application/json")
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", API_VERSION)
                .header("anthropic-beta", FALLBACK_BETA)
                .send(&payload);
            match result {
                Ok(mut resp) => {
                    let code = resp.status().as_u16();
                    let wait = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .map(Duration::from_secs);
                    let text = resp.body_mut().read_to_string().context("reading the API response")?;
                    if (200..300).contains(&code) {
                        return serde_json::from_str(&text).context("parsing the API response");
                    }
                    // 408/409/429/5xx (529 = overloaded) are worth retrying.
                    let retryable = matches!(code, 408 | 409 | 429) || code >= 500;
                    if !retryable || attempt == MAX_ATTEMPTS {
                        return Err(anyhow!("API error {code}: {text}"));
                    }
                    eprintln!("  API {code}, retrying ({attempt}/{MAX_ATTEMPTS})");
                    std::thread::sleep(wait.unwrap_or(delay));
                }
                Err(e) => {
                    if attempt == MAX_ATTEMPTS {
                        return Err(anyhow!("network error: {e}"));
                    }
                    eprintln!("  network error, retrying ({attempt}/{MAX_ATTEMPTS}): {e}");
                    std::thread::sleep(delay);
                }
            }
            delay = (delay * 2).min(Duration::from_secs(60));
        }
        unreachable!("the loop returns on its last attempt")
    }
}
