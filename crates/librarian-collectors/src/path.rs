//! `$PATH` collector — fallback for anything no other source claimed.
//!
//! Walks every directory in `$PATH`, records executables whose paths are not already
//! claimed by a higher-priority source (pacman, aur, cargo, npm, …). The orchestrator
//! passes the set of claimed paths via [`PathCollector::with_skip_set`].

use anyhow::Result;
use librarian_core::{Collector, Source, Tool};
use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct PathCollector {
    skip: HashSet<PathBuf>,
}

impl PathCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_skip_set(mut self, skip: HashSet<PathBuf>) -> Self {
        self.skip = skip;
        self
    }
}

impl Collector for PathCollector {
    fn source(&self) -> Source {
        Source::Path
    }

    fn is_available(&self) -> bool {
        std::env::var_os("PATH").is_some()
    }

    fn collect(&self) -> Result<Vec<Tool>> {
        let path_var = match std::env::var_os("PATH") {
            Some(v) => v,
            None => return Ok(Vec::new()),
        };

        let mut tools = Vec::new();
        let mut seen_names: HashSet<String> = HashSet::new();

        for dir in std::env::split_paths(&path_var) {
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in read.flatten() {
                let path = entry.path();
                if self.skip.contains(&path) {
                    continue;
                }
                if !is_executable(&path) {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else {
                    continue;
                };
                // Within the Path collector itself, dedup by name + canonical path so a
                // single binary symlinked into multiple PATH dirs doesn't produce N rows.
                let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                let key = format!("{}@{}", name, canonical.display());
                if !seen_names.insert(key) {
                    continue;
                }

                let mut tool = Tool::new(name, Source::Path).with_path(path);
                if let Ok(meta) = entry.metadata() {
                    if let Ok(mtime) = meta.modified() {
                        let dt: chrono::DateTime<chrono::Utc> = mtime.into();
                        tool.file_mtime = Some(dt);
                    }
                }
                tools.push(tool);
            }
        }

        Ok(tools)
    }
}

fn is_executable(p: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    meta.permissions().mode() & 0o111 != 0
}
