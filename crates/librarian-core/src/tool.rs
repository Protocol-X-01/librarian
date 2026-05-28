use crate::{Category, Source};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A single tool discovered on the system. The `(name, source, path)` triple is the natural key —
/// the same binary surfaced by multiple sources (e.g. on `$PATH` and via `pacman`) yields multiple
/// rows, which the CLI's `describe` command merges at display time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub display_name: Option<String>,
    /// Filesystem path for binaries / scripts. `None` for non-file tools (e.g. Docker images,
    /// where `metadata.invocation` carries the run command).
    pub path: Option<PathBuf>,
    pub source: Source,
    pub category: Option<Category>,
    pub version: Option<String>,
    pub description: Option<String>,
    /// Upstream package name (e.g. pacman pkgname, cargo crate, npm package).
    pub package: Option<String>,
    pub last_seen: DateTime<Utc>,
    pub file_mtime: Option<DateTime<Utc>>,
    pub sha256: Option<String>,
    /// Source-specific extras (Docker image tags, rustup toolchain components, etc.).
    #[serde(default)]
    pub metadata: serde_json::Value,
}

impl Tool {
    pub fn new(name: impl Into<String>, source: Source) -> Self {
        Self {
            name: name.into(),
            display_name: None,
            path: None,
            source,
            category: None,
            version: None,
            description: None,
            package: None,
            last_seen: Utc::now(),
            file_mtime: None,
            sha256: None,
            metadata: serde_json::Value::Null,
        }
    }

    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_version(mut self, v: impl Into<String>) -> Self {
        self.version = Some(v.into());
        self
    }

    pub fn with_description(mut self, d: impl Into<String>) -> Self {
        self.description = Some(d.into());
        self
    }

    pub fn with_package(mut self, p: impl Into<String>) -> Self {
        self.package = Some(p.into());
        self
    }

    pub fn with_category(mut self, c: Category) -> Self {
        self.category = Some(c);
        self
    }
}
