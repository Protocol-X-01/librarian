//! Submodules grouping non-pacman collectors. Pacman, AUR, Path, and Extra Paths live at
//! the crate root so the file tree mirrors the rough installation hierarchy
//! (system → language → app-level).

pub mod cargo;
pub mod conda;
pub mod desktop;
pub mod docker;
pub mod extra_paths;
pub mod foundry;
pub mod npm;
pub mod pip;
pub mod pipx;
pub mod rpm;
pub mod rustup;
pub mod sagemath;
pub mod uv;
