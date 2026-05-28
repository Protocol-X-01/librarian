//! AUR collector — foreign packages (`pacman -Qm`). Yay installs land here automatically
//! because yay calls pacman for the actual install step.

use anyhow::Result;
use librarian_core::{Collector, Source, Tool};

pub struct AurCollector;

impl AurCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AurCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for AurCollector {
    fn source(&self) -> Source {
        Source::Aur
    }

    fn is_available(&self) -> bool {
        std::env::var_os("PATH")
            .map(|p| {
                std::env::split_paths(&p).any(|d| {
                    d.join("pacman")
                        .metadata()
                        .map(|m| m.is_file())
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    }

    fn collect(&self) -> Result<Vec<Tool>> {
        crate::pacman::build_aur_tools()
    }
}
