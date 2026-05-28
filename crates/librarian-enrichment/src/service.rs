//! Enrichment orchestrator: pick tools needing enrichment, query SERP, persist results.

use crate::client::{BrightDataClient, SerpEngine, SerpResult};
use anyhow::{Context, Result};
use chrono::Utc;
use librarian_core::Store;
use rusqlite::params;

const DEFAULT_TTL_SECONDS: i64 = 30 * 24 * 60 * 60; // 30 days

/// Per-tool enrichment payload written to the `enrichment` table.
#[derive(Debug, Clone, Default)]
pub struct Enrichment {
    pub github_url: Option<String>,
    pub github_title: Option<String>,
    pub snippet: Option<String>,
    pub all_results: Vec<SerpResult>,
}

#[derive(Debug, Clone)]
pub struct EnrichmentConfig {
    /// Max tools to enrich per run.
    pub max_per_run: usize,
    /// Sleep between SERP requests (politeness rate-limit).
    pub delay_ms: u64,
    /// Cache TTL — enrichments older than this are re-fetched on next sync.
    pub ttl_seconds: i64,
    /// Force re-enrich tools that already have non-stale enrichment.
    pub force: bool,
}

impl Default for EnrichmentConfig {
    fn default() -> Self {
        Self {
            max_per_run: std::env::var("LIBRARIAN_ENRICH_MAX")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100),
            delay_ms: std::env::var("LIBRARIAN_ENRICH_DELAY_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(200),
            ttl_seconds: std::env::var("LIBRARIAN_ENRICH_TTL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_TTL_SECONDS),
            force: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct EnrichmentReport {
    pub candidates: usize,
    pub enriched: usize,
    pub skipped: usize,
    pub errors: usize,
    pub no_results: usize,
}

pub struct EnrichmentService<'a> {
    store: &'a Store,
    client: BrightDataClient,
    cfg: EnrichmentConfig,
}

impl<'a> EnrichmentService<'a> {
    pub fn new(store: &'a Store, client: BrightDataClient, cfg: EnrichmentConfig) -> Self {
        Self { store, client, cfg }
    }

    /// Enrich tools that:
    ///   - Have no row in the `enrichment` table, or
    ///   - Have an enrichment row older than `ttl_seconds`, or
    ///   - Match `restrict_to_name` if provided (forces re-enrichment of that tool).
    ///
    /// Returns the report. Caller decides whether to update sources_state.
    pub async fn run(&self, restrict_to_name: Option<&str>) -> Result<EnrichmentReport> {
        let now = Utc::now();
        let ttl = self.cfg.ttl_seconds;

        // Find candidate tool IDs: distinct tools (by name + best path) that need enrichment.
        // We pick the most-informative source row per name — pacman/aur first, else any.
        let candidates = self
            .pick_candidates(now.timestamp(), ttl, restrict_to_name, self.cfg.max_per_run)
            .context("selecting enrichment candidates")?;

        let mut report = EnrichmentReport {
            candidates: candidates.len(),
            ..Default::default()
        };

        for name in candidates {
            // Throttle.
            if self.cfg.delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.cfg.delay_ms)).await;
            }

            let query = format!("{name} github cli");
            let enriched = match self.client.serp_search(&query, SerpEngine::Google).await {
                Ok(results) => pick_best(&results.results),
                Err(e) => {
                    tracing::warn!(tool = %name, error = %e, "SERP query failed");
                    report.errors += 1;
                    continue;
                }
            };

            if enriched.github_url.is_none() && enriched.snippet.is_none() {
                report.no_results += 1;
                continue;
            }

            if let Err(e) = self.persist(&name, &enriched, now.to_rfc3339()) {
                tracing::warn!(tool = %name, error = %e, "persist failed");
                report.errors += 1;
                continue;
            }
            report.enriched += 1;
        }

        Ok(report)
    }

    /// Pick unique tool *names* that need enrichment. Returns names rather than tool_ids
    /// so the candidate set survives refresh cycles (the underlying tool rows get
    /// recycled, but enrichment is keyed by name and persistent).
    fn pick_candidates(
        &self,
        now_ts: i64,
        ttl: i64,
        restrict_to_name: Option<&str>,
        limit: usize,
    ) -> Result<Vec<String>> {
        let stale_cutoff = now_ts - ttl;

        // DISTINCT names from tools, LEFT JOINed against enrichment by name. Candidate
        // when either (a) no enrichment row exists yet, or (b) the existing row is stale.
        let mut sql = String::from(
            r#"
            SELECT DISTINCT t.name
            FROM tools t
            LEFT JOIN enrichment e ON e.name = t.name
            WHERE 1=1
            "#,
        );

        if !self.cfg.force {
            sql.push_str(
                " AND (e.name IS NULL OR \
                      CAST(strftime('%s', e.last_fetched) AS INTEGER) < ?1)",
            );
        } else {
            // Force mode: skip the freshness filter so re-enrichment of the restrict-to
            // tool happens even if its cache is fresh.
            sql.push_str(" AND ?1 = ?1");
        }

        if restrict_to_name.is_some() {
            sql.push_str(" AND t.name = ?2");
        }

        sql.push_str(" ORDER BY t.name LIMIT ?3");

        let mut stmt = self.store.conn().prepare(&sql)?;
        let rows = stmt
            .query_map(
                params![stale_cutoff, restrict_to_name.unwrap_or(""), limit as i64],
                |r| r.get::<_, String>(0),
            )?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();
        Ok(rows)
    }

    fn persist(&self, name: &str, e: &Enrichment, now: String) -> Result<()> {
        let advisories = serde_json::to_string(&Vec::<String>::new()).unwrap_or_default();
        let raw_serp = serde_json::to_string(&e.all_results).unwrap_or_default();

        self.store.conn().execute(
            r#"
            INSERT INTO enrichment
                (name, github_url, github_stars, last_commit, readme_excerpt,
                 advisories, raw_serp, last_fetched, ttl_seconds)
            VALUES (?1, ?2, NULL, NULL, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(name) DO UPDATE SET
                github_url     = excluded.github_url,
                readme_excerpt = excluded.readme_excerpt,
                advisories     = excluded.advisories,
                raw_serp       = excluded.raw_serp,
                last_fetched   = excluded.last_fetched,
                ttl_seconds    = excluded.ttl_seconds
            "#,
            params![
                name,
                e.github_url,
                e.snippet,
                advisories,
                raw_serp,
                now,
                self.cfg.ttl_seconds,
            ],
        )?;

        // Backfill onto every tool row sharing this name whose own description is empty.
        // Survives across refreshes because the same backfill runs at end-of-refresh.
        if let Some(snip) = &e.snippet {
            self.store.conn().execute(
                "UPDATE tools SET description = ?1 \
                 WHERE name = ?2 AND (description IS NULL OR description = '')",
                params![snip, name],
            )?;
        }
        Ok(())
    }
}

/// Pick the best SERP result for a tool name: prefer github.com, then known security
/// tool registries, then the first non-junk result. Returns an empty Enrichment if
/// nothing usable came back.
fn pick_best(results: &[SerpResult]) -> Enrichment {
    let mut en = Enrichment {
        all_results: results.iter().take(5).cloned().collect(),
        ..Default::default()
    };

    let prefer = |r: &SerpResult| -> u8 {
        let u = r.url.to_ascii_lowercase();
        if u.contains("github.com") { 0 }
        else if u.contains("gitlab.com") || u.contains("codeberg.org") { 1 }
        else if u.contains("kali.org") || u.contains("blackarch.org") { 2 }
        else if u.contains("pypi.org") || u.contains("crates.io") || u.contains("npmjs.com") { 3 }
        else if u.contains("man7.org") || u.contains("manpages.") { 4 }
        else { 5 }
    };

    let best = results.iter().min_by_key(|r| prefer(r));
    if let Some(r) = best {
        en.github_url = Some(r.url.clone());
        en.github_title = Some(r.title.clone());
        if !r.snippet.is_empty() {
            en.snippet = Some(r.snippet.clone());
        }
    }
    en
}
