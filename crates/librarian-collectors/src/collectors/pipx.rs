//! pipx collector — `pipx list --json` gives a clean app → binary mapping.

use crate::util::{run_capture, which};
use anyhow::Result;
use librarian_core::{Collector, Source, Tool};
use std::path::PathBuf;

pub struct PipxCollector;
impl PipxCollector {
    pub fn new() -> Self { Self }
}
impl Default for PipxCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for PipxCollector {
    fn source(&self) -> Source { Source::Pipx }
    fn is_available(&self) -> bool { which("pipx") }
    fn collect(&self) -> Result<Vec<Tool>> {
        let json = run_capture("pipx", &["list", "--json"], true).unwrap_or_default();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);

        let venvs = parsed.get("venvs").and_then(|v| v.as_object());
        let Some(venvs) = venvs else { return Ok(Vec::new()); };

        let mut tools = Vec::new();
        for (pkg, info) in venvs {
            let main = info
                .pointer("/metadata/main_package")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let version = main.get("package_version").and_then(|v| v.as_str()).unwrap_or("").to_string();

            let apps = main
                .get("app_paths")
                .and_then(|a| a.as_array())
                .cloned()
                .unwrap_or_default();

            for app in apps {
                // pipx serializes Path as {"__type__": "Path", "string": "..."}
                let raw = app.get("string").and_then(|s| s.as_str())
                    .or_else(|| app.as_str())
                    .unwrap_or("");
                if raw.is_empty() { continue; }
                let path = PathBuf::from(raw);
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or(pkg).to_string();
                let mut tool = Tool::new(name, Source::Pipx)
                    .with_path(path)
                    .with_package(pkg.clone());
                if !version.is_empty() {
                    tool = tool.with_version(version.clone());
                }
                tools.push(tool);
            }
        }
        Ok(tools)
    }
}
