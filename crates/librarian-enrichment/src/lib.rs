//! Bright Data-backed enrichment for the Librarian index.
//!
//! Triggered by `librarian sync`. Strategy:
//!   1. Find tools missing a usable description, or whose enrichment is older than TTL
//!   2. For each, fire a single SERP query (`<name> github`)
//!   3. Pick the best github.com result; fall back to the top organic result
//!   4. Persist URL + title + snippet to the `enrichment` table
//!
//! Deliberately does NOT fetch each page via Web Unlocker — that doubles the cost for
//! marginal value. The SERP snippet is what an AI consumer of the manifest actually needs.

mod client;
mod service;

pub use client::{BrightDataClient, SerpEngine, SerpResult, SerpResults};
pub use service::{Enrichment, EnrichmentConfig, EnrichmentReport, EnrichmentService};
