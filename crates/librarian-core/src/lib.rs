//! Librarian core: shared types, the SQLite-backed store, and the [`Collector`] trait.

pub mod category;
pub mod source;
pub mod store;
pub mod tool;

pub use category::Category;
pub use source::Source;
pub use store::Store;
pub use tool::Tool;

use anyhow::Result;

/// A collector discovers tools from a single source (pacman, cargo, a filesystem path, etc.).
///
/// Collectors are independent and side-effect free: they read the world and return rows.
/// Persisting and deduplication is the [`Store`]'s job.
pub trait Collector {
    /// Stable identifier (also used as the `source` column value).
    fn source(&self) -> Source;

    /// Whether this collector can run on the current system (e.g. is the binary in `$PATH`?).
    fn is_available(&self) -> bool;

    /// Collect all tools currently visible to this source.
    fn collect(&self) -> Result<Vec<Tool>>;

    /// Optional: stat the source's root dir(s) to support lazy stale-checks.
    /// Returning `None` means "always rescan when asked."
    fn root_mtime(&self) -> Option<std::time::SystemTime> {
        None
    }
}
