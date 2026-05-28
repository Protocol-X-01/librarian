//! Librarian CLI entry point.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use librarian_collectors::{all_collectors, collector_for, path::PathCollector};
use librarian_core::{Collector, Source, Store};
use librarian_enrichment::{BrightDataClient, EnrichmentConfig, EnrichmentService};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Default system-wide DB location. Overridable via --db or LIBRARIAN_DB env var.
const DEFAULT_DB: &str = "/var/lib/librarian/index.db";
const DEFAULT_MANIFEST: &str = "/var/lib/librarian/manifest.txt";

#[derive(Parser, Debug)]
#[command(
    name = "librarian",
    version,
    about = "Definitive index of every tool installed on this system.",
    long_about = None
)]
struct Cli {
    /// SQLite database path.
    #[arg(long, env = "LIBRARIAN_DB", default_value = DEFAULT_DB, global = true)]
    db: PathBuf,

    /// Increase log verbosity (-v info, -vv debug, -vvv trace).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Full rescan + Bright Data enrichment for newly discovered tools.
    ///
    /// Enrichment fires one SERP query per tool that lacks a description or whose
    /// cached enrichment is older than LIBRARIAN_ENRICH_TTL (default 30 days). Requires
    /// /etc/librarian/.env with BRIGHT_DATA_API_KEY set — without it, sync degrades to
    /// a plain refresh.
    Sync {
        /// Restrict refresh to one or more sources (does not affect enrichment scope).
        #[arg(long)]
        source: Vec<Source>,
        /// Skip the Bright Data enrichment pass; behave like `refresh`.
        #[arg(long)]
        no_enrich: bool,
        /// Force re-enrichment of a specific tool, even if its cache is fresh.
        #[arg(long, value_name = "TOOL")]
        reenrich: Option<String>,
    },
    /// Rescan sources without enrichment.
    Refresh {
        /// Restrict to one or more sources. Repeatable: --source pacman --source aur.
        /// When omitted, every available collector runs (plus the Path fallback).
        #[arg(long)]
        source: Vec<Source>,
        /// Suppress per-source progress output (for use in pacman hooks).
        #[arg(long)]
        quiet: bool,
    },
    /// Search by name / description / package.
    Search {
        query: String,
        #[arg(long)]
        source: Option<Source>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// List tools, optionally filtered.
    List {
        #[arg(long)]
        source: Option<Source>,
        #[arg(long)]
        category: Option<String>,
    },
    /// Print the full record for a tool (merges rows from every source it appears in).
    Describe {
        name: String,
        #[arg(long)]
        enrich: bool,
    },
    /// Show the category tree with per-category counts.
    Categories,
    /// Regenerate the plain-text manifest at /var/lib/librarian/manifest.txt.
    Dump {
        #[arg(long, default_value = "text")]
        format: String,
        #[arg(long, default_value = DEFAULT_MANIFEST)]
        out: PathBuf,
    },
    /// Show DB stats: total tools, per-source counts, last scan times.
    Stats,
}

fn main() -> Result<()> {
    reset_sigpipe();
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match cli.command {
        Command::Refresh { source, quiet } => cmd_refresh(&cli.db, &source, quiet)?,
        Command::Stats => cmd_stats(&cli.db)?,
        Command::Search { query, source, category, limit } => {
            cmd_search(&cli.db, &query, source, category.as_deref(), limit)?
        }
        Command::List { source, category } => cmd_list(&cli.db, source, category.as_deref())?,
        Command::Dump { format, out } => cmd_dump(&cli.db, &format, &out)?,
        Command::Sync { source, no_enrich, reenrich } => {
            cmd_sync(&cli.db, &source, no_enrich, reenrich.as_deref())?
        }
        Command::Describe { .. } => println!("describe: not yet implemented"),
        Command::Categories => println!("categories: not yet implemented"),
    }

    Ok(())
}

/// Read+write open. Used by commands that mutate (refresh, sync). Creates the parent
/// directory if it doesn't yet exist (first-run before install.sh has been called).
fn open_store(db: &Path) -> Result<Store> {
    if let Some(parent) = db.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
    }
    Store::open(db)
}

/// Read-only open. Used by every query command so non-root users can search the
/// system index without needing write access to /var/lib/librarian/.
fn open_store_ro(db: &Path) -> Result<Store> {
    if !db.exists() {
        anyhow::bail!(
            "Librarian database not found at {}. Run `sudo librarian refresh` or `sudo bash scripts/install.sh` first.",
            db.display()
        );
    }
    Store::open_readonly(db)
}

fn cmd_refresh(db: &Path, sources: &[Source], quiet: bool) -> Result<()> {
    let store = open_store(db)?;

    // Restrict mode: explicit --source list (path is allowed but treated as phase 2).
    let restricted = !sources.is_empty();
    let want_path = if restricted {
        sources.contains(&Source::Path)
    } else {
        true
    };

    let phase1: Vec<Box<dyn Collector>> = if restricted {
        sources
            .iter()
            .filter(|s| **s != Source::Path)
            .filter_map(|s| collector_for(*s))
            .collect()
    } else {
        all_collectors()
            .into_iter()
            .filter(|c| c.source() != Source::Path)
            .collect()
    };

    let mut claimed_paths: HashSet<PathBuf> = HashSet::new();

    for c in phase1 {
        let src = c.source();
        store.clear_source(src)?;
        let tools = c.collect()?;
        for t in &tools {
            if let Some(p) = &t.path {
                claimed_paths.insert(p.clone());
            }
            store.upsert_tool(t)?;
        }
        if !quiet {
            println!("  {:<12} → {} tools", src.to_string(), tools.len());
        }
    }

    if want_path {
        store.clear_source(Source::Path)?;
        let path_collector = PathCollector::new().with_skip_set(claimed_paths);
        let tools = path_collector.collect()?;
        for t in &tools {
            store.upsert_tool(t)?;
        }
        if !quiet {
            println!("  {:<12} → {} tools (unclaimed)", "path", tools.len());
        }
    }

    // Re-apply cached enrichment to freshly inserted tool rows. Without this, the FTS
    // index would forget every enriched description on every refresh (the row was just
    // reinserted with the package manager's bare description). Enrichment table itself
    // already survives — it's keyed by name, not tool id.
    let restored = store.backfill_descriptions_from_enrichment().unwrap_or(0);
    if !quiet && restored > 0 {
        println!("  (restored {restored} cached descriptions from enrichment)");
    }

    if !quiet {
        let total = store.count()?;
        println!("\nTotal tools in index: {total}");
    }
    Ok(())
}

fn cmd_sync(
    db: &Path,
    sources: &[Source],
    no_enrich: bool,
    reenrich: Option<&str>,
) -> Result<()> {
    // Phase 1: always refresh first so enrichment sees an up-to-date set of tools.
    println!("Phase 1 — rescanning sources");
    cmd_refresh(db, sources, false)?;

    if no_enrich {
        println!("\n(--no-enrich set; skipping Bright Data pass)");
        return Ok(());
    }

    // Phase 2: enrichment.
    let store = open_store(db)?;
    let Some(client) = BrightDataClient::from_env() else {
        println!(
            "\nNo BRIGHT_DATA_API_KEY in /etc/librarian/.env (or ~/.config/librarian/.env) — \
             skipping enrichment. Edit that file and re-run `librarian sync` to populate."
        );
        return Ok(());
    };

    let mut cfg = EnrichmentConfig::default();
    if reenrich.is_some() {
        cfg.force = true;
    }
    println!(
        "\nPhase 2 — Bright Data enrichment (max {} tools, {}ms between requests)",
        cfg.max_per_run, cfg.delay_ms
    );

    let service = EnrichmentService::new(&store, client, cfg);

    let runtime = tokio::runtime::Runtime::new().context("starting tokio runtime")?;
    let report = runtime.block_on(service.run(reenrich))?;

    println!(
        "  candidates: {}    enriched: {}    skipped: {}    no_results: {}    errors: {}",
        report.candidates, report.enriched, report.skipped, report.no_results, report.errors
    );

    // Regenerate the manifest so the freshly enriched descriptions land in
    // /var/lib/librarian/manifest.txt for AI consumers.
    let default_manifest = PathBuf::from("/var/lib/librarian/manifest.txt");
    if default_manifest.parent().map_or(false, |p| p.is_dir()) {
        if let Err(e) = cmd_dump(db, "text", &default_manifest) {
            tracing::warn!(error = %e, "manifest regeneration failed (non-fatal)");
        }
    }

    Ok(())
}

fn cmd_dump(db: &Path, format: &str, out: &Path) -> Result<()> {
    use std::io::Write;
    let store = open_store_ro(db)?;

    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
    }

    let mut file = std::fs::File::create(out)
        .with_context(|| format!("opening {} for write", out.display()))?;

    match format {
        "text" | "txt" => {
            let total = store.count()?;
            let now = chrono::Utc::now().to_rfc3339();
            writeln!(file, "# Librarian tool index — generated {now}")?;
            writeln!(file, "# Total: {total} tools")?;
            writeln!(file, "# Schema: name | source | category | version | path | description")?;
            writeln!(file)?;

            let mut stmt = store.conn().prepare(
                "SELECT name, source, COALESCE(category,'-'), COALESCE(version,'-'), \
                        COALESCE(path,'-'), COALESCE(description,'') \
                 FROM tools ORDER BY source, name",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?;
            for row in rows {
                let (name, src, cat, ver, path, desc) = row?;
                // Pipe-delimited so grep/awk consumers stay simple; descriptions get any
                // pipes stripped because the format would otherwise be ambiguous.
                let clean_desc = desc.replace('|', "/").replace('\n', " ");
                writeln!(file, "{name} | {src} | {cat} | {ver} | {path} | {clean_desc}")?;
            }
        }
        "json" => {
            let mut stmt = store.conn().prepare(
                "SELECT name, source, category, version, path, description, package, metadata \
                 FROM tools ORDER BY source, name",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(serde_json::json!({
                    "name": r.get::<_, String>(0)?,
                    "source": r.get::<_, String>(1)?,
                    "category": r.get::<_, Option<String>>(2)?,
                    "version": r.get::<_, Option<String>>(3)?,
                    "path": r.get::<_, Option<String>>(4)?,
                    "description": r.get::<_, Option<String>>(5)?,
                    "package": r.get::<_, Option<String>>(6)?,
                    "metadata": r.get::<_, Option<String>>(7)?
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                        .unwrap_or(serde_json::Value::Null),
                }))
            })?;
            let arr: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
            serde_json::to_writer_pretty(&mut file, &arr)?;
        }
        other => anyhow::bail!("unknown dump format: {other} (try `text` or `json`)"),
    }

    println!("Wrote manifest to {}", out.display());
    Ok(())
}

fn cmd_stats(db: &Path) -> Result<()> {
    let store = open_store_ro(db)?;
    let total = store.count()?;
    println!("Total tools: {total}\n");

    let mut stmt = store
        .conn()
        .prepare("SELECT source, COUNT(*) FROM tools GROUP BY source ORDER BY COUNT(*) DESC")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;

    println!("By source:");
    for r in rows {
        let (src, count) = r?;
        println!("  {src:<14} {count}");
    }

    let mut stmt = store.conn().prepare(
        "SELECT COALESCE(category, '(none)'), COUNT(*) FROM tools \
         GROUP BY category ORDER BY COUNT(*) DESC LIMIT 20",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;

    println!("\nTop 20 categories:");
    for r in rows {
        let (cat, count) = r?;
        println!("  {cat:<22} {count}");
    }

    Ok(())
}

fn cmd_search(
    db: &Path,
    query: &str,
    source: Option<Source>,
    category: Option<&str>,
    limit: usize,
) -> Result<()> {
    let store = open_store_ro(db)?;
    let mut sql = String::from(
        "SELECT t.name, t.source, COALESCE(t.path,''), COALESCE(t.category,'-'), \
                COALESCE(t.version,''), COALESCE(t.description,'') \
         FROM tools t \
         JOIN tools_fts f ON f.rowid = t.id \
         WHERE tools_fts MATCH ?1",
    );
    if source.is_some() {
        sql.push_str(" AND t.source = ?2");
    }
    if category.is_some() {
        sql.push_str(" AND t.category = ?3");
    }
    sql.push_str(" ORDER BY rank LIMIT ?4");

    let mut stmt = store.conn().prepare(&sql)?;
    let src_str = source.map(|s| s.to_string()).unwrap_or_default();
    let cat_str = category.unwrap_or("").to_string();
    let rows = stmt.query_map(
        rusqlite::params![query, src_str, cat_str, limit as i64],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        },
    )?;

    for row in rows {
        let (name, src, path, cat, ver, desc) = row?;
        println!("{name}  [{src}/{cat}]  {ver}");
        if !path.is_empty() {
            println!("    {path}");
        }
        if !desc.is_empty() {
            let truncated: String = desc.chars().take(100).collect();
            println!("    {truncated}");
        }
    }
    Ok(())
}

fn cmd_list(db: &Path, source: Option<Source>, category: Option<&str>) -> Result<()> {
    let store = open_store_ro(db)?;
    let mut sql = String::from(
        "SELECT name, source, COALESCE(category,'-'), COALESCE(path,'') FROM tools WHERE 1=1",
    );
    let mut params: Vec<String> = Vec::new();
    if let Some(s) = source {
        sql.push_str(" AND source = ?");
        params.push(s.to_string());
    }
    if let Some(c) = category {
        sql.push_str(" AND category = ?");
        params.push(c.to_string());
    }
    sql.push_str(" ORDER BY name LIMIT 500");

    let mut stmt = store.conn().prepare(&sql)?;
    let params_dyn: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let rows = stmt.query_map(params_dyn.as_slice(), |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;

    for row in rows {
        let (name, src, cat, path) = row?;
        println!("{name:<30}  [{src:<8}/{cat:<20}]  {path}");
    }
    Ok(())
}

/// Restore SIGPIPE's default behavior so `librarian list | head` exits cleanly instead
/// of panicking on the broken pipe write.
#[allow(unsafe_code)]
fn reset_sigpipe() {
    use nix::sys::signal::{signal, SigHandler, Signal};
    // SAFETY: a one-shot signal disposition reset at process start; no concurrent handlers exist yet.
    unsafe {
        let _ = signal(Signal::SIGPIPE, SigHandler::SigDfl);
    }
}

fn init_tracing(verbosity: u8) {
    use tracing_subscriber::EnvFilter;
    let default = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
