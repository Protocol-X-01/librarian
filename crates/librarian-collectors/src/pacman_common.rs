//! Shared subprocess + parsing helpers for the pacman and aur collectors.

use anyhow::{bail, Context, Result};
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;

/// Bin/sbin directories where pacman-installed executables typically live. A package may
/// install files outside these (libexec, /opt/foo/bin), so we also accept anything under
/// `/opt/*/bin/` and `/usr/lib/*/bin/` via [`is_bin_path`].
const BIN_DIRS: &[&str] = &[
    "/usr/bin/",
    "/usr/sbin/",
    "/usr/local/bin/",
    "/usr/local/sbin/",
];

static OPT_BIN_RE: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"^/opt/[^/]+/bin/[^/]+$").expect("opt bin regex"));
static LIBEXEC_BIN_RE: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"^/usr/lib/[^/]+/bin/[^/]+$").expect("libexec bin regex"));

/// A single installed package's metadata, as returned by `pacman -Qi`.
#[derive(Debug, Default, Clone)]
pub struct PacmanPkg {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub url: Option<String>,
    pub groups: Vec<String>,
    pub architecture: Option<String>,
}

impl PacmanPkg {
    fn is_valid(&self) -> bool {
        !self.name.is_empty() && !self.version.is_empty()
    }
}

/// Run `pacman -Qi` and return one [`PacmanPkg`] per installed package.
pub fn query_installed_packages() -> Result<Vec<PacmanPkg>> {
    let output = Command::new("pacman")
        .arg("-Qi")
        .output()
        .context("invoking `pacman -Qi`")?;
    if !output.status.success() {
        bail!(
            "`pacman -Qi` exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(parse_qi_output(&text))
}

/// Run `pacman -Qg` → map of package name → list of group memberships.
///
/// This is the authoritative source for groups (the `pacman -Qi` "Groups" field is sparse
/// and unreliable — many BlackArch packages report `Groups: None` despite being members
/// of the bare `blackarch` group surfaced by `pacman -Qg`).
pub fn query_group_membership() -> Result<HashMap<String, Vec<String>>> {
    let output = Command::new("pacman")
        .arg("-Qg")
        .output()
        .context("invoking `pacman -Qg`")?;
    if !output.status.success() {
        return Ok(HashMap::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for line in text.lines() {
        let Some((group, pkg)) = line.split_once(' ') else {
            continue;
        };
        map.entry(pkg.trim().to_string())
            .or_default()
            .push(group.trim().to_string());
    }
    Ok(map)
}

/// Run `pacman -Qm` → set of foreign (AUR / locally built) package names.
pub fn query_foreign_packages() -> Result<HashSet<String>> {
    let output = Command::new("pacman")
        .arg("-Qm")
        .output()
        .context("invoking `pacman -Qm`")?;
    if !output.status.success() {
        // -Qm may exit non-zero if no foreign packages; treat as empty set.
        return Ok(HashSet::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .filter_map(|l| l.split_whitespace().next().map(String::from))
        .collect())
}

/// Run `pacman -Ql` → map of package name → list of file paths.
///
/// Filters out directory entries; keeps only files in standard bin/sbin paths.
pub fn query_package_files() -> Result<HashMap<String, Vec<PathBuf>>> {
    let output = Command::new("pacman")
        .arg("-Ql")
        .output()
        .context("invoking `pacman -Ql`")?;
    if !output.status.success() {
        bail!(
            "`pacman -Ql` exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let text = String::from_utf8_lossy(&output.stdout);

    let mut map: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for line in text.lines() {
        let Some((pkg, path)) = line.split_once(' ') else {
            continue;
        };
        let path = path.trim();
        if path.ends_with('/') {
            continue; // directory
        }
        if !is_bin_path(path) {
            continue;
        }
        map.entry(pkg.to_string())
            .or_default()
            .push(PathBuf::from(path));
    }
    Ok(map)
}

/// True if `path` is in a directory we treat as "PATH-visible / executable".
fn is_bin_path(path: &str) -> bool {
    if BIN_DIRS.iter().any(|d| path.starts_with(d) && !path[d.len()..].contains('/')) {
        return true;
    }
    OPT_BIN_RE.is_match(path) || LIBEXEC_BIN_RE.is_match(path)
}

/// Parse the multi-record output of `pacman -Qi` into structured packages.
fn parse_qi_output(text: &str) -> Vec<PacmanPkg> {
    let mut packages = Vec::new();
    let mut cur = PacmanPkg::default();
    let mut last_field: Option<String> = None;

    for raw in text.lines() {
        if raw.trim().is_empty() {
            if cur.is_valid() {
                packages.push(std::mem::take(&mut cur));
            } else {
                cur = PacmanPkg::default();
            }
            last_field = None;
            continue;
        }

        // Continuation line (whitespace-indented, no ` : ` separator at the front).
        if raw.starts_with(' ') && !raw.contains(" : ") {
            if let Some(field) = last_field.as_deref() {
                append_continuation(&mut cur, field, raw.trim());
            }
            continue;
        }

        // New `Field : Value` line — pacman pads field names so split on " : ".
        let Some((field, value)) = raw.split_once(" : ") else {
            continue;
        };
        let field = field.trim();
        let value = value.trim();
        set_field(&mut cur, field, value);
        last_field = Some(field.to_string());
    }

    if cur.is_valid() {
        packages.push(cur);
    }

    packages
}

fn set_field(pkg: &mut PacmanPkg, field: &str, value: &str) {
    match field {
        "Name" => pkg.name = value.to_string(),
        "Version" => pkg.version = value.to_string(),
        "Description" => {
            if value != "None" {
                pkg.description = Some(value.to_string());
            }
        }
        "URL" => {
            if value != "None" {
                pkg.url = Some(value.to_string());
            }
        }
        "Architecture" => {
            if value != "None" {
                pkg.architecture = Some(value.to_string());
            }
        }
        "Groups" => {
            if value != "None" {
                pkg.groups = value
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
            }
        }
        _ => {}
    }
}

fn append_continuation(pkg: &mut PacmanPkg, field: &str, addition: &str) {
    match field {
        "Description" => {
            if let Some(d) = pkg.description.as_mut() {
                d.push(' ');
                d.push_str(addition);
            }
        }
        "Groups" => {
            for g in addition.split_whitespace() {
                pkg.groups.push(g.to_string());
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_packages() {
        let sample = r#"Name            : zlib
Version         : 1:1.3.1-2
Description     : Compression library implementing the deflate compression method
Architecture    : x86_64
URL             : https://www.zlib.net/
Licenses        : custom:ZLIB
Groups          : None
Provides        : libz.so=1-64
Depends On      : glibc

Name            : nmap
Version         : 7.94-3
Description     : Utility for network discovery and security auditing
Architecture    : x86_64
URL             : https://nmap.org
Licenses        : custom
Groups          : blackarch  blackarch-scanner
"#;
        let pkgs = parse_qi_output(sample);
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "zlib");
        assert_eq!(pkgs[1].name, "nmap");
        assert_eq!(pkgs[1].groups, vec!["blackarch", "blackarch-scanner"]);
        assert!(pkgs[1].description.as_ref().unwrap().contains("network"));
    }

    #[test]
    fn is_bin_path_basic() {
        assert!(is_bin_path("/usr/bin/nmap"));
        assert!(is_bin_path("/usr/local/sbin/foo"));
        assert!(is_bin_path("/opt/foundry/bin/forge"));
        assert!(is_bin_path("/usr/lib/postgresql/bin/psql"));
        assert!(!is_bin_path("/usr/share/man/man1/foo.1.gz"));
        assert!(!is_bin_path("/etc/hosts"));
        assert!(!is_bin_path("/usr/bin/subdir/foo")); // nested
    }
}
