//! Helpers shared by collectors: `which`, executable detection, binary directory walks.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Is `bin` present anywhere on `$PATH`?
pub fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p).any(|d| {
                d.join(bin)
                    .metadata()
                    .map(|m| m.is_file())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Path to the user's `$HOME`, or `/root` if HOME is unset.
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

/// True when `p` exists, is a regular file, and has any execute bit set.
pub fn is_executable(p: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    meta.permissions().mode() & 0o111 != 0
}

/// Walk one directory and return every executable file (non-recursive).
pub fn list_executables(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in read.flatten() {
        let p = entry.path();
        if is_executable(&p) {
            out.push(p);
        }
    }
    out
}

/// Run a subprocess and return stdout as a String. Returns Err on non-zero exit
/// unless `tolerate_failure` is true (some managers exit non-zero with empty output
/// when nothing is installed).
pub fn run_capture(
    program: &str,
    args: &[&str],
    tolerate_failure: bool,
) -> anyhow::Result<String> {
    let output = std::process::Command::new(program).args(args).output()?;
    if !output.status.success() && !tolerate_failure {
        anyhow::bail!(
            "`{} {}` exited {}: {}",
            program,
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
