//! SageMath collector — registers only the top-level wrappers (`sage`, `sage-jupyter`, …).
//! Its internal 600+ Python packages live in a self-contained world and are intentionally
//! NOT enumerated (user request).

use crate::util::which;
use anyhow::Result;
use librarian_core::{Category, Collector, Source, Tool};
use std::path::PathBuf;

const SAGE_WRAPPERS: &[&str] = &["sage", "sage-jupyter", "sage-cleaner", "sage-num-threads.py"];

pub struct SagemathCollector;
impl SagemathCollector {
    pub fn new() -> Self { Self }
}
impl Default for SagemathCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for SagemathCollector {
    fn source(&self) -> Source { Source::Sagemath }
    fn is_available(&self) -> bool { which("sage") }
    fn collect(&self) -> Result<Vec<Tool>> {
        let path_var = std::env::var_os("PATH");
        let mut tools = Vec::new();
        if let Some(p) = path_var {
            for d in std::env::split_paths(&p) {
                for wrapper in SAGE_WRAPPERS {
                    let candidate = d.join(wrapper);
                    if candidate.exists() {
                        let mut tool = Tool::new((*wrapper).to_string(), Source::Sagemath)
                            .with_path(candidate)
                            .with_package("sagemath".to_string())
                            .with_category(Category::DataScience);
                        tool.description = Some("SageMath top-level wrapper.".to_string());
                        tools.push(tool);
                    }
                }
            }
        }
        // Dedup by (name, path)
        tools.sort_by(|a, b| {
            (a.name.clone(), a.path.clone().unwrap_or_else(PathBuf::new))
                .cmp(&(b.name.clone(), b.path.clone().unwrap_or_else(PathBuf::new)))
        });
        tools.dedup_by(|a, b| a.name == b.name && a.path == b.path);
        Ok(tools)
    }
}
