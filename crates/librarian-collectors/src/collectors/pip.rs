//! pip / pip2 collectors — Python packages from system and user site-packages.
//!
//! Pip doesn't map packages → console scripts in a single command, so for the first pass
//! we record each installed package as a Tool with `path = None`. The Path collector still
//! catches the actual scripts; sources can be joined at describe-time.

use crate::util::{run_capture, which};
use anyhow::Result;
use librarian_core::{Category, Collector, Source, Tool};
use serde::Deserialize;

#[derive(Deserialize)]
struct PipPkg {
    name: String,
    version: String,
}

fn collect_pip(bin: &str, src: Source) -> Result<Vec<Tool>> {
    let mut tools = Vec::new();
    let json = run_capture(bin, &["list", "--format=json", "--disable-pip-version-check"], true)
        .unwrap_or_default();
    let pkgs: Vec<PipPkg> = serde_json::from_str(&json).unwrap_or_default();
    for p in pkgs {
        let cat = infer_pip_category(&p.name);
        let mut tool = Tool::new(p.name.clone(), src)
            .with_version(p.version.clone())
            .with_package(p.name.clone());
        if let Some(c) = cat {
            tool = tool.with_category(c);
        }
        tools.push(tool);
    }
    Ok(tools)
}

fn infer_pip_category(name: &str) -> Option<Category> {
    let n = name.to_ascii_lowercase();
    let n = n.as_str();
    if matches!(n, "slither-analyzer" | "mythril" | "manticore" | "crytic-compile" | "solc-select" | "vyper") {
        return Some(Category::SmartContractAudit);
    }
    if matches!(n, "impacket" | "ldap3" | "bloodhound" | "certipy-ad" | "pypykatz") {
        return Some(Category::Exploitation);
    }
    if matches!(n, "scapy" | "pwntools" | "scapy3k") {
        return Some(Category::Networking);
    }
    if matches!(n, "torch" | "tensorflow" | "scikit-learn" | "numpy" | "pandas" | "jupyter" | "ipython") {
        return Some(Category::DataScience);
    }
    None
}

pub struct PipCollector;
impl PipCollector {
    pub fn new() -> Self { Self }
}
impl Default for PipCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for PipCollector {
    fn source(&self) -> Source { Source::Pip }
    fn is_available(&self) -> bool { which("pip") || which("pip3") }
    fn collect(&self) -> Result<Vec<Tool>> {
        let bin = if which("pip3") { "pip3" } else { "pip" };
        collect_pip(bin, Source::Pip)
    }
}

pub struct Pip2Collector;
impl Pip2Collector {
    pub fn new() -> Self { Self }
}
impl Default for Pip2Collector {
    fn default() -> Self { Self::new() }
}
impl Collector for Pip2Collector {
    fn source(&self) -> Source { Source::Pip2 }
    fn is_available(&self) -> bool { which("pip2") }
    fn collect(&self) -> Result<Vec<Tool>> { collect_pip("pip2", Source::Pip2) }
}
