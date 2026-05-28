//! Collector implementations. Each module is one source.
//!
//! Adding a new package manager: implement `librarian_core::Collector`, add the module,
//! and register it in [`all_collectors`] / [`collector_for`].

use librarian_core::{Collector, Source};

pub mod pacman_common;
pub mod util;

pub mod path;
pub mod pacman;
pub mod aur;

pub mod collectors;

/// Return every available collector for the current system, in scan order.
///
/// Pacman / AUR run first so their authoritative category data populates the store
/// before less-informative sources are processed. The Path collector is intentionally
/// excluded — the CLI's `refresh` runs it in a separate phase with knowledge of which
/// paths the authoritative collectors already claimed.
pub fn all_collectors() -> Vec<Box<dyn Collector>> {
    let candidates: Vec<Box<dyn Collector>> = vec![
        Box::new(pacman::PacmanCollector::new()),
        Box::new(aur::AurCollector::new()),
        Box::new(collectors::rpm::RpmCollector::new()),
        Box::new(collectors::cargo::CargoCollector::new()),
        Box::new(collectors::rustup::RustupCollector::new()),
        Box::new(collectors::foundry::FoundryCollector::new()),
        Box::new(collectors::pipx::PipxCollector::new()),
        Box::new(collectors::pip::PipCollector::new()),
        Box::new(collectors::pip::Pip2Collector::new()),
        Box::new(collectors::uv::UvCollector::new()),
        Box::new(collectors::conda::CondaCollector::new()),
        Box::new(collectors::npm::NpmCollector::new()),
        Box::new(collectors::npm::YarnCollector::new()),
        Box::new(collectors::npm::PnpmCollector::new()),
        Box::new(collectors::npm::BunCollector::new()),
        Box::new(collectors::sagemath::SagemathCollector::new()),
        Box::new(collectors::docker::DockerCollector::new()),
        Box::new(collectors::desktop::DesktopCollector::new()),
        Box::new(collectors::extra_paths::ExtraPathsCollector::new()),
        // Path collector is intentionally excluded — orchestrator runs it last.
    ];

    candidates.into_iter().filter(|c| c.is_available()).collect()
}

/// Return a single collector by source name, if available.
pub fn collector_for(source: Source) -> Option<Box<dyn Collector>> {
    let c: Box<dyn Collector> = match source {
        Source::Pacman => Box::new(pacman::PacmanCollector::new()),
        Source::Aur => Box::new(aur::AurCollector::new()),
        Source::Rpm => Box::new(collectors::rpm::RpmCollector::new()),
        Source::Path => Box::new(path::PathCollector::new()),
        Source::Cargo => Box::new(collectors::cargo::CargoCollector::new()),
        Source::Rustup => Box::new(collectors::rustup::RustupCollector::new()),
        Source::Foundry => Box::new(collectors::foundry::FoundryCollector::new()),
        Source::Pip => Box::new(collectors::pip::PipCollector::new()),
        Source::Pip2 => Box::new(collectors::pip::Pip2Collector::new()),
        Source::Pipx => Box::new(collectors::pipx::PipxCollector::new()),
        Source::Uv => Box::new(collectors::uv::UvCollector::new()),
        Source::Conda => Box::new(collectors::conda::CondaCollector::new()),
        Source::Npm => Box::new(collectors::npm::NpmCollector::new()),
        Source::Yarn => Box::new(collectors::npm::YarnCollector::new()),
        Source::Pnpm => Box::new(collectors::npm::PnpmCollector::new()),
        Source::Bun => Box::new(collectors::npm::BunCollector::new()),
        Source::Sagemath => Box::new(collectors::sagemath::SagemathCollector::new()),
        Source::Docker => Box::new(collectors::docker::DockerCollector::new()),
        Source::Desktop => Box::new(collectors::desktop::DesktopCollector::new()),
        Source::ExtraPaths => Box::new(collectors::extra_paths::ExtraPathsCollector::new()),
    };
    c.is_available().then_some(c)
}
