//! Foundry collector — forge/cast/anvil/chisel installed via foundryup live in `~/.foundry/bin`.

use crate::util::{home, list_executables, run_capture, which};
use anyhow::Result;
use librarian_core::{Category, Collector, Source, Tool};
use std::path::PathBuf;

pub struct FoundryCollector;
impl FoundryCollector {
    pub fn new() -> Self { Self }
    fn bin_dir(&self) -> PathBuf {
        std::env::var_os("FOUNDRY_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".foundry"))
            .join("bin")
    }
}
impl Default for FoundryCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for FoundryCollector {
    fn source(&self) -> Source { Source::Foundry }
    fn is_available(&self) -> bool {
        self.bin_dir().is_dir() || which("foundryup")
    }
    fn collect(&self) -> Result<Vec<Tool>> {
        let mut tools = Vec::new();
        let bin_dir = self.bin_dir();
        for path in list_executables(&bin_dir) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if name.is_empty() { continue; }

            // Get version per-binary; forge --version, cast --version etc.
            let version = run_capture(&path.to_string_lossy(), &["--version"], true)
                .ok()
                .and_then(|v| v.lines().next().map(|l| l.trim().to_string()));

            let mut tool = Tool::new(name.clone(), Source::Foundry)
                .with_path(path)
                .with_category(Category::BlockchainSecurity)
                .with_package("foundry".to_string());
            if let Some(v) = version {
                tool = tool.with_version(v);
            }
            tool.description = Some(match name.as_str() {
                "forge" => "Foundry's Solidity build / test / fuzz CLI.",
                "cast" => "Foundry's swiss-army knife for EVM RPC interaction.",
                "anvil" => "Foundry's local Ethereum node for testing.",
                "chisel" => "Foundry's Solidity REPL.",
                _ => "Foundry toolchain binary.",
            }.to_string());
            tools.push(tool);
        }
        Ok(tools)
    }
}
