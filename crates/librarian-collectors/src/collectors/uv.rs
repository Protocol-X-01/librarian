//! uv tool collector — `uv tool list` outputs `<name> v<version>` headers followed by
//! their installed binaries, similar to `cargo install --list`.

use crate::util::{home, list_executables, run_capture, which};
use anyhow::Result;
use librarian_core::{Collector, Source, Tool};
use std::path::PathBuf;

pub struct UvCollector;
impl UvCollector {
    pub fn new() -> Self { Self }
    fn bin_dir(&self) -> PathBuf {
        std::env::var_os("UV_TOOL_BIN_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/bin"))
    }
}
impl Default for UvCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for UvCollector {
    fn source(&self) -> Source { Source::Uv }
    fn is_available(&self) -> bool { which("uv") }
    fn collect(&self) -> Result<Vec<Tool>> {
        let listing = run_capture("uv", &["tool", "list"], true).unwrap_or_default();
        let bin_dir = self.bin_dir();

        let (pkgs, mut binary_to_pkg) = parse_uv_tool_list(&listing);
        let pkg_versions: std::collections::HashMap<String, String> =
            pkgs.into_iter().collect();

        let mut tools = Vec::new();
        for path in list_executables(&bin_dir) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            let pkg = binary_to_pkg.remove(&name);
            // Only emit if this binary was attributed to a uv tool — otherwise it's just
            // a script that happens to live in ~/.local/bin (caught by Path collector).
            let Some(pkg) = pkg else { continue; };
            let version = pkg_versions.get(&pkg).cloned().unwrap_or_default();
            let mut tool = Tool::new(name, Source::Uv).with_path(path).with_package(pkg);
            if !version.is_empty() {
                tool = tool.with_version(version);
            }
            tools.push(tool);
        }
        Ok(tools)
    }
}

/// Returns `(pkg → version, binary_name → pkg)`.
fn parse_uv_tool_list(text: &str) -> (Vec<(String, String)>, std::collections::HashMap<String, String>) {
    let mut pkgs: Vec<(String, String)> = Vec::new();
    let mut bin_to_pkg: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut current_pkg: Option<String> = None;

    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with('-') || line.starts_with(' ') {
            if let Some(pkg) = current_pkg.as_ref() {
                let bin = line.trim_start_matches('-').trim().to_string();
                if !bin.is_empty() {
                    bin_to_pkg.insert(bin, pkg.clone());
                }
            }
            continue;
        }
        // header: "<pkg> v<version>"
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else { continue; };
        let version = parts.next().unwrap_or("").trim_start_matches('v').to_string();
        pkgs.push((name.to_string(), version));
        current_pkg = Some(name.to_string());
    }
    (pkgs, bin_to_pkg)
}
