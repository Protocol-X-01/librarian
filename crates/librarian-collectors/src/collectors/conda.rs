//! Conda collector — each environment registered with conda. We record envs as Tools
//! (with the env's `bin/python` as path) rather than enumerating every internal package.

use crate::util::{run_capture, which};
use anyhow::Result;
use librarian_core::{Category, Collector, Source, Tool};
use std::path::PathBuf;

pub struct CondaCollector;
impl CondaCollector {
    pub fn new() -> Self { Self }
}
impl Default for CondaCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for CondaCollector {
    fn source(&self) -> Source { Source::Conda }
    fn is_available(&self) -> bool { which("conda") || which("mamba") || which("micromamba") }
    fn collect(&self) -> Result<Vec<Tool>> {
        let bin = if which("conda") { "conda" }
                  else if which("mamba") { "mamba" }
                  else { "micromamba" };
        let json = run_capture(bin, &["env", "list", "--json"], true).unwrap_or_default();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);

        let envs = parsed.get("envs").and_then(|v| v.as_array()).cloned().unwrap_or_default();

        let mut tools = Vec::new();
        for env in envs {
            let Some(env_path) = env.as_str() else { continue; };
            let env_pb = PathBuf::from(env_path);
            let name = env_pb.file_name().and_then(|n| n.to_str()).unwrap_or("base").to_string();
            let mut tool = Tool::new(name.clone(), Source::Conda)
                .with_path(env_pb.join("bin/python"))
                .with_package(format!("conda-env:{name}"))
                .with_category(Category::LanguageRuntime);
            tool.description = Some(format!("Conda environment at {env_path}"));
            tool.metadata = serde_json::json!({ "env_root": env_path });
            tools.push(tool);
        }
        Ok(tools)
    }
}
