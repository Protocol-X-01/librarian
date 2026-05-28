//! Rustup collector — installed Rust toolchains. Distinct from Cargo: rustup manages
//! `rustc`/`cargo`/`rustfmt`/`clippy`/`rust-analyzer` versions; cargo manages installed crates.
//! We record each toolchain as a Tool plus its components.

use crate::util::{home, run_capture, which};
use anyhow::Result;
use librarian_core::{Category, Collector, Source, Tool};
use std::path::PathBuf;

pub struct RustupCollector;

impl RustupCollector {
    pub fn new() -> Self {
        Self
    }
    fn toolchains_dir(&self) -> PathBuf {
        std::env::var_os("RUSTUP_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".rustup"))
            .join("toolchains")
    }
}

impl Default for RustupCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for RustupCollector {
    fn source(&self) -> Source {
        Source::Rustup
    }

    fn is_available(&self) -> bool {
        which("rustup") || self.toolchains_dir().is_dir()
    }

    fn collect(&self) -> Result<Vec<Tool>> {
        let mut tools = Vec::new();
        let toolchains_dir = self.toolchains_dir();

        let listing = run_capture("rustup", &["toolchain", "list"], true).unwrap_or_default();
        let default_marker = "(default)";

        for line in listing.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let is_default = line.contains(default_marker);
            let toolchain = line.replace(default_marker, "").trim().to_string();
            if toolchain.is_empty() {
                continue;
            }

            let path = toolchains_dir.join(&toolchain);
            let mut tool = Tool::new(toolchain.clone(), Source::Rustup)
                .with_path(path.join("bin/rustc"))
                .with_package(toolchain.clone())
                .with_category(Category::LanguageRuntime);

            tool.description = Some(format!(
                "Rust toolchain{}",
                if is_default { " (default)" } else { "" }
            ));

            // Components per toolchain.
            let components = run_capture(
                "rustup",
                &["component", "list", "--installed", "--toolchain", &toolchain],
                true,
            )
            .unwrap_or_default();
            let component_list: Vec<String> = components
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            tool.metadata = serde_json::json!({
                "default": is_default,
                "components": component_list,
            });

            tools.push(tool);
        }

        Ok(tools)
    }
}
