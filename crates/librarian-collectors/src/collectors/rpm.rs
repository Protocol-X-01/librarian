//! RPM collector — unusual on Arch but present when users install `rpm-tools` for
//! repacking or forensic work. Same shape as pacman: query installed → query files.

use crate::util::{run_capture, which};
use anyhow::Result;
use librarian_core::{Collector, Source, Tool};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct RpmCollector;
impl RpmCollector {
    pub fn new() -> Self { Self }
}
impl Default for RpmCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for RpmCollector {
    fn source(&self) -> Source { Source::Rpm }
    fn is_available(&self) -> bool { which("rpm") }
    fn collect(&self) -> Result<Vec<Tool>> {
        let listing = run_capture(
            "rpm",
            &["-qa", "--queryformat", "%{NAME}|%{VERSION}|%{SUMMARY}\n"],
            true,
        )
        .unwrap_or_default();

        let mut pkg_meta: HashMap<String, (String, String)> = HashMap::new();
        for line in listing.lines() {
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() < 2 { continue; }
            pkg_meta.insert(
                parts[0].to_string(),
                (
                    parts[1].to_string(),
                    parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
                ),
            );
        }

        let mut tools = Vec::new();
        for (pkg, (version, summary)) in &pkg_meta {
            let files_out = run_capture("rpm", &["-ql", pkg], true).unwrap_or_default();
            for path_line in files_out.lines() {
                let path_line = path_line.trim();
                if !is_bin_path(path_line) { continue; }
                let path = PathBuf::from(path_line);
                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let mut tool = Tool::new(name, Source::Rpm)
                    .with_path(path)
                    .with_version(version.clone())
                    .with_package(pkg.clone());
                if !summary.is_empty() {
                    tool = tool.with_description(summary.clone());
                }
                tools.push(tool);
            }
        }
        Ok(tools)
    }
}

fn is_bin_path(p: &str) -> bool {
    p.starts_with("/usr/bin/")
        || p.starts_with("/usr/sbin/")
        || p.starts_with("/usr/local/bin/")
        || p.starts_with("/usr/local/sbin/")
}
