//! Bright Data HTTP client. Ported from
//! `MULTI-CHAIN-FRAMEWORK/intelligence/src/bright_data.rs` and pared down: the Librarian
//! only needs SERP + Unlocker. Same env var names as OmniScan for cred portability.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const UNLOCKER_ENDPOINT: &str = "https://api.brightdata.com/request";

#[derive(Debug, Clone)]
pub struct BrightDataClient {
    api_key: String,
    unlocker_zone: String,
    serp_zone: String,
    client: reqwest::Client,
}

/// Why a `BrightDataClient::from_env_diagnosed` call did or didn't produce a client.
/// Callers (the CLI) format this into a user-facing message — silent failures here
/// are how things like "the .env file exists but you can't read it" become invisible.
#[derive(Debug)]
pub enum LoadDiagnostic {
    /// API key present, client built successfully.
    Loaded,
    /// No env file existed at any of the searched paths.
    NoFileFound { searched: Vec<PathBuf> },
    /// A file existed but couldn't be read or parsed. Almost always perm-denied on
    /// `/etc/librarian/.env` when the user's shell isn't in the `librarian` group.
    Unreadable { path: PathBuf, error: String },
    /// One or more env files were read, but `BRIGHT_DATA_API_KEY` was missing or empty.
    NoApiKey { loaded: Vec<PathBuf> },
}

impl BrightDataClient {
    /// Backwards-compatible shim — drops the diagnostic.
    pub fn from_env() -> Option<Self> {
        Self::from_env_diagnosed().0
    }

    /// Build a client from environment, returning a structured diagnostic alongside
    /// the optional client so the CLI can print actionable error messages.
    ///
    /// Search order (later files override earlier ones, matching shell convention):
    ///   1. `/etc/librarian/.env`
    ///   2. `$HOME/.config/librarian/.env` (dev override)
    pub fn from_env_diagnosed() -> (Option<Self>, LoadDiagnostic) {
        let mut search_paths: Vec<PathBuf> = vec![PathBuf::from("/etc/librarian/.env")];
        if let Some(home) = std::env::var_os("HOME") {
            search_paths.push(PathBuf::from(home).join(".config/librarian/.env"));
        }

        let mut loaded_paths: Vec<PathBuf> = Vec::new();

        for path in &search_paths {
            match dotenvy::from_filename(path) {
                Ok(_) => loaded_paths.push(path.clone()),
                Err(e) => {
                    // dotenvy wraps io errors; treat "not found" as expected, anything
                    // else (permission denied, parse error, isadirectory, …) as a hard
                    // diagnostic so the user knows exactly why enrichment skipped.
                    if let Some(io_err) = io_error_of(&e) {
                        if io_err.kind() == std::io::ErrorKind::NotFound {
                            continue;
                        }
                    }
                    return (
                        None,
                        LoadDiagnostic::Unreadable {
                            path: path.clone(),
                            error: e.to_string(),
                        },
                    );
                }
            }
        }

        let api_key = match std::env::var("BRIGHT_DATA_API_KEY") {
            Ok(k) if !k.is_empty() => k,
            _ => {
                let diag = if loaded_paths.is_empty() {
                    LoadDiagnostic::NoFileFound { searched: search_paths }
                } else {
                    LoadDiagnostic::NoApiKey { loaded: loaded_paths }
                };
                return (None, diag);
            }
        };

        let client = Self {
            api_key,
            unlocker_zone: std::env::var("BRIGHT_DATA_UNLOCKER_ZONE")
                .unwrap_or_else(|_| "web_unlocker1".to_string()),
            serp_zone: std::env::var("BRIGHT_DATA_SERP_ZONE")
                .unwrap_or_else(|_| "serp_api1".to_string()),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
        };
        (Some(client), LoadDiagnostic::Loaded)
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

/// Extract an `io::Error` from a `dotenvy::Error` regardless of which variant wraps it.
/// dotenvy's enum keeps changing across versions, so we match on Debug as a fallback.
fn io_error_of(e: &dotenvy::Error) -> Option<&std::io::Error> {
    match e {
        dotenvy::Error::Io(io) => Some(io),
        _ => None,
    }
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
