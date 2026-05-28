//! npm/yarn/pnpm/bun collectors — JavaScript ecosystem global installs.

use crate::util::{home, list_executables, run_capture, which};
use anyhow::Result;
use librarian_core::{Collector, Source, Tool};
use std::path::PathBuf;

/// Global bin dir resolution. Falls back to common defaults if the manager isn't queryable.
fn npm_prefix() -> Option<PathBuf> {
    run_capture("npm", &["config", "get", "prefix"], true)
        .ok()
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.as_os_str() != "undefined" && !p.as_os_str().is_empty())
}

pub struct NpmCollector;
impl NpmCollector {
    pub fn new() -> Self { Self }
}
impl Default for NpmCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for NpmCollector {
    fn source(&self) -> Source { Source::Npm }
    fn is_available(&self) -> bool { which("npm") }
    fn collect(&self) -> Result<Vec<Tool>> {
        // Parse the top-level dependencies tree.
        let json = run_capture("npm", &["ls", "-g", "--json", "--depth=0"], true).unwrap_or_default();
        let parsed: serde_json::Value =
            serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
        let deps = parsed.get("dependencies").and_then(|d| d.as_object());

        let mut tools = Vec::new();
        let bin_dir = npm_prefix().map(|p| p.join("bin"));

        if let Some(deps) = deps {
            for (pkg, info) in deps {
                let version = info
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                // Use package name as binary name — close enough; refined when we
                // cross-reference the bin dir below.
                let mut tool = Tool::new(pkg.clone(), Source::Npm)
                    .with_package(pkg.clone());
                if !version.is_empty() {
                    tool = tool.with_version(version);
                }
                if let Some(bd) = &bin_dir {
                    let candidate = bd.join(pkg);
                    if candidate.exists() {
                        tool = tool.with_path(candidate);
                    }
                }
                tools.push(tool);
            }
        }
        Ok(tools)
    }
}

pub struct YarnCollector;
impl YarnCollector {
    pub fn new() -> Self { Self }
    fn bin_dir(&self) -> PathBuf { home().join(".yarn/bin") }
}
impl Default for YarnCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for YarnCollector {
    fn source(&self) -> Source { Source::Yarn }
    fn is_available(&self) -> bool { which("yarn") && self.bin_dir().exists() }
    fn collect(&self) -> Result<Vec<Tool>> {
        let mut tools = Vec::new();
        for path in list_executables(&self.bin_dir()) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if name.is_empty() { continue; }
            tools.push(Tool::new(name, Source::Yarn).with_path(path));
        }
        Ok(tools)
    }
}

pub struct PnpmCollector;
impl PnpmCollector {
    pub fn new() -> Self { Self }
    fn bin_dir(&self) -> PathBuf {
        std::env::var_os("PNPM_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share/pnpm"))
    }
}
impl Default for PnpmCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for PnpmCollector {
    fn source(&self) -> Source { Source::Pnpm }
    fn is_available(&self) -> bool { which("pnpm") && self.bin_dir().exists() }
    fn collect(&self) -> Result<Vec<Tool>> {
        let mut tools = Vec::new();
        for path in list_executables(&self.bin_dir()) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if name.is_empty() || name == "pnpm" { continue; }
            tools.push(Tool::new(name, Source::Pnpm).with_path(path));
        }
        Ok(tools)
    }
}

pub struct BunCollector;
impl BunCollector {
    pub fn new() -> Self { Self }
    fn bin_dir(&self) -> PathBuf { home().join(".bun/bin") }
}
impl Default for BunCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for BunCollector {
    fn source(&self) -> Source { Source::Bun }
    fn is_available(&self) -> bool { which("bun") && self.bin_dir().exists() }
    fn collect(&self) -> Result<Vec<Tool>> {
        let mut tools = Vec::new();
        for path in list_executables(&self.bin_dir()) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if name.is_empty() { continue; }
            tools.push(Tool::new(name, Source::Bun).with_path(path));
        }
        Ok(tools)
    }
}
