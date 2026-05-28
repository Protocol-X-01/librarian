use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Every package manager / discovery channel that produces tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Anything visible on `$PATH` that no other collector claimed.
    Path,
    /// Official Arch repos via pacman.
    Pacman,
    /// AUR foreign packages (`pacman -Qm`); installed via yay or makepkg.
    Aur,
    /// RPM (rare on Arch but present for repackaging workflows).
    Rpm,
    Pip,
    Pip2,
    Pipx,
    Uv,
    Conda,
    Cargo,
    /// Rust toolchains and components managed by rustup (distinct from Cargo-installed crates).
    Rustup,
    Npm,
    Yarn,
    Pnpm,
    Bun,
    /// Foundry toolchain (forge, cast, anvil, chisel) installed via foundryup.
    Foundry,
    /// SageMath wrappers — its internal Python world is intentionally not enumerated.
    Sagemath,
    /// Locally pulled Docker images, treated as tools.
    Docker,
    /// `.desktop` entries from /usr/share/applications and ~/.local/share/applications.
    Desktop,
    /// User-defined directories from /etc/librarian/sources.toml.
    ExtraPaths,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Path => "path",
            Source::Pacman => "pacman",
            Source::Aur => "aur",
            Source::Rpm => "rpm",
            Source::Pip => "pip",
            Source::Pip2 => "pip2",
            Source::Pipx => "pipx",
            Source::Uv => "uv",
            Source::Conda => "conda",
            Source::Cargo => "cargo",
            Source::Rustup => "rustup",
            Source::Npm => "npm",
            Source::Yarn => "yarn",
            Source::Pnpm => "pnpm",
            Source::Bun => "bun",
            Source::Foundry => "foundry",
            Source::Sagemath => "sagemath",
            Source::Docker => "docker",
            Source::Desktop => "desktop",
            Source::ExtraPaths => "extra_paths",
        }
    }

    pub fn all() -> &'static [Source] {
        &[
            Source::Path,
            Source::Pacman,
            Source::Aur,
            Source::Rpm,
            Source::Pip,
            Source::Pip2,
            Source::Pipx,
            Source::Uv,
            Source::Conda,
            Source::Cargo,
            Source::Rustup,
            Source::Npm,
            Source::Yarn,
            Source::Pnpm,
            Source::Bun,
            Source::Foundry,
            Source::Sagemath,
            Source::Docker,
            Source::Desktop,
            Source::ExtraPaths,
        ]
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Source {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        for src in Source::all() {
            if src.as_str().eq_ignore_ascii_case(s) {
                return Ok(*src);
            }
        }
        Err(anyhow::anyhow!("unknown source: {s}"))
    }
}
