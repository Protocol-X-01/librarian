//! Cargo collector — binaries installed via `cargo install`, found in `$CARGO_HOME/bin`
//! (default `~/.cargo/bin`). `cargo install --list` provides crate → binary mapping.

use crate::util::{home, list_executables, run_capture, which};
use anyhow::Result;
use librarian_core::{Category, Collector, Source, Tool};
use std::path::PathBuf;

pub struct CargoCollector;

impl CargoCollector {
    pub fn new() -> Self {
        Self
    }
    fn bin_dir(&self) -> PathBuf {
        std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".cargo"))
            .join("bin")
    }
}

impl Default for CargoCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for CargoCollector {
    fn source(&self) -> Source {
        Source::Cargo
    }

    fn is_available(&self) -> bool {
        which("cargo") || self.bin_dir().is_dir()
    }

    fn collect(&self) -> Result<Vec<Tool>> {
        let mut tools = Vec::new();
        let bin_dir = self.bin_dir();

        // Parse `cargo install --list` to get crate → binary mapping with versions.
        let listing = run_capture("cargo", &["install", "--list"], true).unwrap_or_default();
        let crate_map = parse_cargo_install_list(&listing);

        // Each binary in ~/.cargo/bin becomes a Tool. Crate metadata fills version/package.
        for path in list_executables(&bin_dir) {
            let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else {
                continue;
            };
            let (crate_name, version) = crate_map
                .iter()
                .find(|(_, info)| info.binaries.iter().any(|b| b == &name))
                .map(|(cn, info)| (Some(cn.clone()), Some(info.version.clone())))
                .unwrap_or((None, None));

            let mut tool = Tool::new(name.clone(), Source::Cargo).with_path(path);
            if let Some(v) = version {
                tool = tool.with_version(v);
            }
            if let Some(c) = crate_name {
                tool = tool.with_package(c);
            }
            if let Some(cat) = infer_category(&name) {
                tool = tool.with_category(cat);
            }
            tools.push(tool);
        }

        Ok(tools)
    }
}

struct CrateInfo {
    version: String,
    binaries: Vec<String>,
}

/// Parse the output of `cargo install --list`:
///
/// ```text
/// foundry-zksync v0.3.0:
///     forge
///     cast
/// bat v0.24.0:
///     bat
/// ```
fn parse_cargo_install_list(text: &str) -> Vec<(String, CrateInfo)> {
    let mut out: Vec<(String, CrateInfo)> = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            // binary name; append to last entry
            if let Some(last) = out.last_mut() {
                last.1.binaries.push(line.trim().to_string());
            }
            continue;
        }
        // header line: "<crate> v<version>:" or "<crate> v<ver> (git+...):"
        let header = line.trim_end_matches(':').trim();
        if let Some((name, rest)) = header.split_once(' ') {
            let version = rest
                .trim_start_matches('v')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            out.push((
                name.to_string(),
                CrateInfo {
                    version,
                    binaries: Vec::new(),
                },
            ));
        }
    }
    out
}

fn infer_category(name: &str) -> Option<Category> {
    if matches!(name, "forge" | "cast" | "anvil" | "chisel") {
        return Some(Category::BlockchainSecurity);
    }
    if matches!(name, "slither" | "mythril" | "manticore" | "echidna" | "aderyn" | "halmos") {
        return Some(Category::SmartContractAudit);
    }
    if matches!(name, "ripgrep" | "rg" | "fd" | "bat" | "exa" | "eza" | "dust" | "bottom" | "btm" | "procs" | "sd") {
        return Some(Category::Development);
    }
    None
}
