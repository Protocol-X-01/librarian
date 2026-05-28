//! ExtraPaths collector — user-defined directories from /etc/librarian/sources.toml.
//!
//! Use case: custom source builds, github/gist drops, security toolkits in non-standard
//! locations. Walks each path recursively with a depth cap (default 3), records every
//! executable file.

use crate::util::is_executable;
use anyhow::Result;
use librarian_core::{Collector, Source, Tool};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const CONFIG_PATH: &str = "/etc/librarian/sources.toml";
const DEFAULT_MAX_DEPTH: usize = 3;

#[derive(Deserialize, Debug, Default)]
struct Config {
    #[serde(default)]
    extra_paths: ExtraPathsSection,
}

#[derive(Deserialize, Debug, Default)]
struct ExtraPathsSection {
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    max_depth: Option<usize>,
}

pub struct ExtraPathsCollector {
    config_path: PathBuf,
}

impl ExtraPathsCollector {
    pub fn new() -> Self {
        Self { config_path: PathBuf::from(CONFIG_PATH) }
    }

    /// Override config location (useful for tests + non-system installs).
    pub fn with_config_path(mut self, path: PathBuf) -> Self {
        self.config_path = path;
        self
    }
}

impl Default for ExtraPathsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for ExtraPathsCollector {
    fn source(&self) -> Source { Source::ExtraPaths }

    fn is_available(&self) -> bool { self.config_path.is_file() }

    fn collect(&self) -> Result<Vec<Tool>> {
        let raw = match std::fs::read_to_string(&self.config_path) {
            Ok(s) => s,
            Err(_) => return Ok(Vec::new()),
        };
        let cfg: Config = toml::from_str(&raw).unwrap_or_default();
        let max_depth = cfg.extra_paths.max_depth.unwrap_or(DEFAULT_MAX_DEPTH);

        let mut tools = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for raw_path in &cfg.extra_paths.paths {
            let root = expand_path(raw_path);
            if !root.is_dir() {
                tracing::warn!(path = %root.display(), "extra_paths entry is not a directory; skipping");
                continue;
            }
            for entry in WalkDir::new(&root).max_depth(max_depth).follow_links(false) {
                let Ok(entry) = entry else { continue; };
                let path = entry.into_path();
                if !is_executable(&path) { continue; }
                let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if !seen.insert(canonical.clone()) { continue; }
                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let mut tool = Tool::new(name, Source::ExtraPaths).with_path(path);
                tool.metadata = serde_json::json!({
                    "config_root": root.to_string_lossy(),
                });
                tools.push(tool);
            }
        }

        Ok(tools)
    }
}

fn expand_path(s: &str) -> PathBuf {
    // Minimal expansion: ~/... → $HOME/...
    if let Some(stripped) = s.strip_prefix("~/") {
        return crate::util::home().join(stripped);
    }
    if s == "~" {
        return crate::util::home();
    }
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn skips_when_no_config() {
        let c = ExtraPathsCollector::new().with_config_path(PathBuf::from("/nonexistent/sources.toml"));
        assert!(!c.is_available());
    }

    #[test]
    fn parses_config_and_walks() {
        let tmp = tempfile_dir();
        let bin_dir = tmp.join("custom");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let bin = bin_dir.join("mytool");
        let mut f = std::fs::File::create(&bin).unwrap();
        writeln!(f, "#!/bin/sh\necho hi").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&bin, perms).unwrap();

        let cfg_path = tmp.join("sources.toml");
        std::fs::write(
            &cfg_path,
            format!("[extra_paths]\npaths = [\"{}\"]\n", bin_dir.display()),
        ).unwrap();

        let c = ExtraPathsCollector::new().with_config_path(cfg_path);
        assert!(c.is_available());
        let tools = c.collect().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "mytool");
    }

    fn tempfile_dir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("librarian-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
