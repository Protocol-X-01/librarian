//! Bright Data HTTP client. Ported from
//! `MULTI-CHAIN-FRAMEWORK/intelligence/src/bright_data.rs` and pared down: the Librarian
//! only needs SERP + Unlocker. Same env var names as OmniScan for cred portability.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const UNLOCKER_ENDPOINT: &str = "https://api.brightdata.com/request";

#[derive(Debug, Clone)]
pub struct BrightDataClient {
    api_key: String,
    unlocker_zone: String,
    serp_zone: String,
    client: reqwest::Client,
}

impl BrightDataClient {
    /// Build a client from environment variables. Loads `/etc/librarian/.env` first if
    /// present (silent if missing). Returns `None` when no API key is configured —
    /// callers should treat that as "skip enrichment, fall back to local rescan only."
    pub fn from_env() -> Option<Self> {
        let _ = dotenvy::from_filename("/etc/librarian/.env");
        // Also try a user-level override for development.
        if let Some(home) = std::env::var_os("HOME") {
            let path = std::path::PathBuf::from(home).join(".config/librarian/.env");
            let _ = dotenvy::from_filename(&path);
        }

        let api_key = std::env::var("BRIGHT_DATA_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())?;
        Some(Self {
            api_key,
            unlocker_zone: std::env::var("BRIGHT_DATA_UNLOCKER_ZONE")
                .unwrap_or_else(|_| "web_unlocker1".to_string()),
            serp_zone: std::env::var("BRIGHT_DATA_SERP_ZONE")
                .unwrap_or_else(|_| "serp_api1".to_string()),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
        })
    }

    pub fn is_configured(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// Query the SERP API. Uses `brd_json=1` for parsed JSON.
    pub async fn serp_search(&self, query: &str, engine: SerpEngine) -> Result<SerpResults> {
        let search_url = engine.build_url_json(query);

        #[derive(Serialize)]
        struct Req<'a> {
            zone: &'a str,
            url: &'a str,
            format: &'a str,
        }
        let req = Req { zone: &self.serp_zone, url: &search_url, format: "raw" };

        let resp = self
            .client
            .post(UNLOCKER_ENDPOINT)
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await
            .context("SERP request send")?;

        let status = resp.status();
        let body = resp.text().await.context("SERP body read")?;
        if !status.is_success() {
            return Err(anyhow!(
                "Bright Data SERP {status}: {}",
                body.chars().take(400).collect::<String>()
            ));
        }

        let results = parse_brd_serp_json(&body).unwrap_or_default();
        Ok(SerpResults { query: query.to_string(), engine, results, raw_length: body.len() })
    }

    /// Scrape a URL via Web Unlocker. Returns raw HTML. The Librarian doesn't use this in
    /// the default enrichment path (we rely on SERP snippets), but exposed for callers that
    /// want to deepen a specific tool's record.
    pub async fn unlock(&self, url: &str) -> Result<String> {
        #[derive(Serialize)]
        struct Req<'a> {
            zone: &'a str,
            url: &'a str,
            format: &'a str,
        }
        let req = Req { zone: &self.unlocker_zone, url, format: "raw" };

        let resp = self
            .client
            .post(UNLOCKER_ENDPOINT)
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await
            .context("Unlocker request send")?;

        let status = resp.status();
        let body = resp.text().await.context("Unlocker body read")?;
        if !status.is_success() {
            return Err(anyhow!(
                "Bright Data Unlocker {status}: {}",
                body.chars().take(400).collect::<String>()
            ));
        }
        Ok(body)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SerpEngine {
    Google,
    Bing,
    DuckDuckGo,
}

impl SerpEngine {
    fn build_url(&self, query: &str) -> String {
        let encoded = urlencode_min(query);
        match self {
            SerpEngine::Google => format!("https://www.google.com/search?q={encoded}&num=10"),
            SerpEngine::Bing => format!("https://www.bing.com/search?q={encoded}&count=10"),
            SerpEngine::DuckDuckGo => format!("https://duckduckgo.com/html/?q={encoded}"),
        }
    }
    fn build_url_json(&self, query: &str) -> String {
        let base = self.build_url(query);
        if base.contains('?') { format!("{base}&brd_json=1") } else { format!("{base}?brd_json=1") }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerpResults {
    pub query: String,
    pub engine: SerpEngine,
    pub results: Vec<SerpResult>,
    pub raw_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerpResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

fn urlencode_min(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                String::from(b as char)
            }
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn parse_brd_serp_json(body: &str) -> Option<Vec<SerpResult>> {
    let trimmed = body.trim_start();
    if !trimmed.starts_with('{') {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let organic = v.get("organic")?.as_array()?;
    let mut out = Vec::new();
    for item in organic {
        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let url = item.get("link").or_else(|| item.get("url"))
            .and_then(|v| v.as_str()).unwrap_or("").to_string();
        let snippet = item.get("description").or_else(|| item.get("snippet"))
            .and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !title.is_empty() && !url.is_empty() {
            out.push(SerpResult { title, url, snippet });
        }
    }
    Some(out)
}
