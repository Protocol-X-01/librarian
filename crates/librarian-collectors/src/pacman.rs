//! Pacman collector — installed packages from official Arch + BlackArch repos.
//!
//! For each non-foreign package, emits one [`Tool`] row per binary the package installs
//! into a standard bin/sbin directory. BlackArch group membership (`blackarch-recon`,
//! `blackarch-fuzzer`, etc.) maps directly to [`Category`] for free.

use crate::pacman_common::{
    query_foreign_packages, query_group_membership, query_installed_packages, query_package_files,
};
use anyhow::Result;
use librarian_core::{Category, Collector, Source, Tool};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct PacmanCollector;

impl PacmanCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PacmanCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for PacmanCollector {
    fn source(&self) -> Source {
        Source::Pacman
    }

    fn is_available(&self) -> bool {
        which("pacman")
    }

    fn collect(&self) -> Result<Vec<Tool>> {
        let foreign = query_foreign_packages().unwrap_or_default();
        let installed = query_installed_packages()?;
        let files = query_package_files()?;
        let groups = query_group_membership().unwrap_or_default();

        Ok(build_tools(&installed, &files, &groups, &foreign, false, Source::Pacman))
    }
}

/// AUR collector mirrors pacman's logic but keeps only foreign packages.
pub(crate) fn build_aur_tools() -> Result<Vec<Tool>> {
    let foreign = query_foreign_packages().unwrap_or_default();
    let installed = query_installed_packages()?;
    let files = query_package_files()?;
    let groups = query_group_membership().unwrap_or_default();
    Ok(build_tools(&installed, &files, &groups, &foreign, true, Source::Aur))
}

fn build_tools(
    installed: &[crate::pacman_common::PacmanPkg],
    files: &HashMap<String, Vec<PathBuf>>,
    groups: &HashMap<String, Vec<String>>,
    foreign: &std::collections::HashSet<String>,
    foreign_only: bool,
    source: Source,
) -> Vec<Tool> {
    let mut tools = Vec::new();

    for pkg in installed {
        let is_foreign = foreign.contains(&pkg.name);
        if foreign_only && !is_foreign {
            continue;
        }
        if !foreign_only && is_foreign {
            continue;
        }

        // Authoritative groups come from `pacman -Qg`; fall back to whatever `-Qi` reported.
        let group_list: Vec<String> = groups
            .get(&pkg.name)
            .cloned()
            .unwrap_or_else(|| pkg.groups.clone());

        let category = categorize(&pkg.name, &group_list);

        let Some(pkg_files) = files.get(&pkg.name) else {
            continue; // no bin files in standard paths
        };

        for path in pkg_files {
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            let mut tool = Tool::new(name, source)
                .with_path(path.clone())
                .with_version(pkg.version.clone())
                .with_package(pkg.name.clone());
            if let Some(desc) = pkg.description.clone() {
                tool = tool.with_description(desc);
            }
            if let Some(cat) = category {
                tool = tool.with_category(cat);
            }

            tool.metadata = serde_json::json!({
                "groups": group_list,
                "url": pkg.url,
                "architecture": pkg.architecture,
                "foreign": is_foreign,
            });

            tools.push(tool);
        }
    }

    tools
}

/// Resolve a category from a package's name + group memberships. Order matters:
/// 1. Specific BlackArch sub-group (recon/fuzzer/scanner/…) — most informative
/// 2. Heuristic from package name (catches well-known tools without group tags)
/// 3. Bare `blackarch` group → `Misc` so the user can at least filter "security tools"
/// 4. Common Arch groups → Development / BuildSystem etc.
fn categorize(name: &str, groups: &[String]) -> Option<Category> {
    // 1. Most specific BlackArch sub-group wins.
    for g in groups {
        if g.starts_with("blackarch-") {
            if let Some(c) = Category::from_blackarch_group(g) {
                return Some(c);
            }
        }
    }
    // 2. Name-based heuristic (well-known tools without proper group tags).
    if let Some(c) = infer_category_from_pkg(name) {
        return Some(c);
    }
    // 3. Bare `blackarch` group: at least flag it as a security tool.
    if groups.iter().any(|g| g == "blackarch") {
        return Some(Category::Misc);
    }
    // 4. Generic Arch groups.
    for g in groups {
        match g.as_str() {
            "base" | "base-devel" => return Some(Category::SystemAdmin),
            "kde-applications" | "gnome" | "xfce4" | "lxqt" | "plasma" => {
                return Some(Category::Development); // GUI/desktop apps — weak signal
            }
            _ => {}
        }
    }
    None
}

/// Best-effort category for packages with no BlackArch sub-group tag. Conservative —
/// returns None when uncertain so we don't mislabel. Cheaper-than-LLM keyword matching;
/// the Bright Data enrichment pass fills in the rest at sync time.
fn infer_category_from_pkg(name: &str) -> Option<Category> {
    let n = name.to_ascii_lowercase();
    let n = n.as_str();

    // ---- Editors / shells / system ----
    if matches!(n, "vim" | "neovim" | "nvim" | "emacs" | "nano" | "helix" | "hx" | "kakoune" | "kak" | "micro" | "ed" | "vi") {
        return Some(Category::Editor);
    }
    if matches!(n, "bash" | "zsh" | "fish" | "dash" | "tcsh" | "ksh" | "nushell" | "nu" | "xonsh" | "elvish") {
        return Some(Category::Shell);
    }
    if n.starts_with("docker") || n.starts_with("podman") || n.starts_with("containerd")
        || matches!(n, "buildah" | "skopeo" | "runc" | "crun" | "kubectl" | "minikube" | "k3s" | "kind" | "helm")
    {
        return Some(Category::Container);
    }

    // ---- Build systems / compilers ----
    if matches!(
        n,
        "cmake" | "ninja" | "meson" | "make" | "autoconf" | "automake"
        | "bazel" | "gcc" | "g++" | "clang" | "clang++" | "llvm"
        | "rustc" | "cargo" | "rustup" | "go" | "gopls"
        | "javac" | "mvn" | "gradle" | "sbt"
    ) || n.starts_with("gcc-") || n.starts_with("llvm-") || n.starts_with("clang-")
    {
        return Some(Category::BuildSystem);
    }

    // ---- Language runtimes / interpreters ----
    if matches!(
        n,
        "python" | "python3" | "python2" | "ruby" | "perl" | "node" | "nodejs"
        | "deno" | "bun" | "lua" | "tclsh" | "php" | "ghc" | "ocaml"
        | "erl" | "elixir" | "crystal" | "scala" | "julia" | "swift" | "kotlin"
    ) || n.starts_with("python3.") || n.starts_with("ruby") || n.starts_with("perl")
    {
        return Some(Category::LanguageRuntime);
    }

    // ---- Network / recon / scanner ----
    if matches!(
        n,
        "nmap" | "masscan" | "zmap" | "rustscan" | "naabu" | "unicornscan"
        | "amass" | "subfinder" | "assetfinder" | "findomain" | "dnsrecon"
        | "dnsenum" | "dnsx" | "fierce" | "theharvester" | "recon-ng" | "spiderfoot"
        | "shodan" | "censys" | "httpx" | "ffuf" | "gobuster" | "feroxbuster"
        | "dirb" | "dirbuster" | "wfuzz" | "katana" | "hakrawler"
    ) {
        return Some(Category::Recon);
    }

    // ---- Web app testing / scanners ----
    if matches!(
        n,
        "nikto" | "wpscan" | "joomscan" | "droopescan" | "sqlmap" | "nosqlmap"
        | "xsstrike" | "wapiti" | "skipfish" | "owtf" | "vega" | "arachni"
        | "burpsuite" | "zaproxy" | "zap" | "caido"
    ) {
        return Some(Category::Webapp);
    }

    // ---- Sniffers ----
    if matches!(n, "tcpdump" | "wireshark" | "tshark" | "dumpcap" | "ngrep" | "tcpflow" | "termshark" | "ettercap") {
        return Some(Category::Sniffer);
    }

    // ---- Crackers / password ----
    if matches!(n, "john" | "hashcat" | "hydra" | "medusa" | "ncrack" | "patator" | "crowbar")
        || n.starts_with("john-") || n.starts_with("hashcat-")
    {
        return Some(Category::Cracker);
    }

    // ---- Crypto / hashing ----
    if matches!(n, "openssl" | "gpg" | "gpg2" | "age" | "sops" | "pass" | "openssh") {
        return Some(Category::Crypto);
    }

    // ---- Wireless ----
    if n.starts_with("aircrack") || n.starts_with("airmon") || n.starts_with("airodump")
        || matches!(n, "kismet" | "wifite" | "reaver" | "bully" | "fluxion" | "mdk3" | "mdk4" | "hcxtools" | "hcxdumptool")
    {
        return Some(Category::Wireless);
    }

    // ---- Reverse engineering / binary analysis ----
    if matches!(
        n,
        "gdb" | "lldb" | "radare2" | "r2" | "rizin" | "cutter" | "iaito"
        | "ghidra" | "ida" | "binwalk" | "objdump" | "readelf" | "nm"
        | "strings" | "strace" | "ltrace" | "checksec"
    ) {
        return Some(Category::ReverseEngineering);
    }

    // ---- Exploitation frameworks ----
    if matches!(
        n,
        "msfconsole" | "msfvenom" | "msfdb" | "msfrpcd" | "armitage"
        | "exploit" | "searchsploit" | "sliver" | "cobaltstrike" | "havoc"
    ) || n.starts_with("metasploit") || n.starts_with("msf")
    {
        return Some(Category::Exploitation);
    }

    // ---- Forensics ----
    if matches!(
        n,
        "volatility" | "vol2" | "vol3" | "autopsy" | "sleuthkit" | "foremost"
        | "scalpel" | "bulk_extractor" | "exiftool" | "photorec" | "testdisk"
    ) {
        return Some(Category::Forensic);
    }

    // ---- Blockchain / smart contract audit ----
    if matches!(
        n,
        "forge" | "cast" | "anvil" | "chisel" | "foundryup"
        | "slither" | "mythril" | "manticore" | "echidna" | "aderyn"
        | "solc" | "vyper" | "halmos" | "medusa-fuzz"
    ) {
        return Some(Category::BlockchainSecurity);
    }

    None
}

fn which(bin: &str) -> bool {
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
