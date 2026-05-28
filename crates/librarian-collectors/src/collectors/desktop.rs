//! Desktop-entry collector — parses `.desktop` files from system + user application dirs.
//!
//! Catches GUI tools the menu hides and provides a categorization layer that doesn't depend
//! on package-manager metadata. We deliberately exclude `NoDisplay=true` and `Hidden=true`
//! entries because those are component definitions (mime handlers etc.), not user-visible tools.

use crate::util::home;
use anyhow::Result;
use librarian_core::{Category, Collector, Source, Tool};
use std::path::{Path, PathBuf};

pub struct DesktopCollector;
impl DesktopCollector {
    pub fn new() -> Self { Self }
    fn dirs(&self) -> Vec<PathBuf> {
        let mut out = vec![
            PathBuf::from("/usr/share/applications"),
            PathBuf::from("/usr/local/share/applications"),
            PathBuf::from("/var/lib/flatpak/exports/share/applications"),
            home().join(".local/share/applications"),
            home().join(".local/share/flatpak/exports/share/applications"),
        ];
        if let Some(xdg_data) = std::env::var_os("XDG_DATA_DIRS") {
            for d in std::env::split_paths(&xdg_data) {
                let candidate = d.join("applications");
                if !out.contains(&candidate) {
                    out.push(candidate);
                }
            }
        }
        out
    }
}
impl Default for DesktopCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for DesktopCollector {
    fn source(&self) -> Source { Source::Desktop }
    fn is_available(&self) -> bool { self.dirs().iter().any(|d| d.is_dir()) }
    fn collect(&self) -> Result<Vec<Tool>> {
        let mut tools = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for dir in self.dirs() {
            let Ok(read) = std::fs::read_dir(&dir) else { continue; };
            for entry in read.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                let app_id = path.file_stem().and_then(|n| n.to_str()).unwrap_or("").to_string();
                if app_id.is_empty() || !seen_ids.insert(app_id.clone()) {
                    continue; // user-level override of a system entry; first one wins
                }
                if let Some(tool) = parse_desktop_file(&path) {
                    tools.push(tool);
                }
            }
        }
        Ok(tools)
    }
}

fn parse_desktop_file(path: &Path) -> Option<Tool> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut in_main_section = false;
    let mut name: Option<String> = None;
    let mut comment: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut try_exec: Option<String> = None;
    let mut categories_raw: Option<String> = None;
    let mut no_display = false;
    let mut hidden = false;
    let mut entry_type: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main_section = line == "[Desktop Entry]";
            continue;
        }
        if !in_main_section || line.starts_with('#') || line.is_empty() {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue; };
        let k = k.trim();
        let v = v.trim();
        match k {
            "Name" => name = Some(v.to_string()),
            "Comment" | "GenericName" => {
                if comment.is_none() {
                    comment = Some(v.to_string());
                }
            }
            "Exec" => exec = Some(v.to_string()),
            "TryExec" => try_exec = Some(v.to_string()),
            "Categories" => categories_raw = Some(v.to_string()),
            "NoDisplay" => no_display = v.eq_ignore_ascii_case("true"),
            "Hidden" => hidden = v.eq_ignore_ascii_case("true"),
            "Type" => entry_type = Some(v.to_string()),
            _ => {}
        }
    }

    if no_display || hidden { return None; }
    if entry_type.as_deref() != Some("Application") { return None; }
    let name = name?;
    let app_id = path.file_stem()?.to_str()?.to_string();

    // Resolve the executable: TryExec wins if absolute, else the first token of Exec.
    let exec_path: Option<PathBuf> = if let Some(t) = &try_exec {
        if t.starts_with('/') { Some(PathBuf::from(t)) } else { which_path(t) }
    } else {
        None
    }
    .or_else(|| {
        exec.as_deref().and_then(|e| {
            let first = e.split_whitespace().next()?;
            if first.starts_with('/') {
                Some(PathBuf::from(first))
            } else {
                which_path(first)
            }
        })
    });

    let category = categories_raw
        .as_deref()
        .and_then(|c| classify_categories(c));

    let mut tool = Tool::new(app_id.clone(), Source::Desktop);
    tool.display_name = Some(name);
    if let Some(p) = exec_path { tool = tool.with_path(p); }
    if let Some(c) = comment { tool = tool.with_description(c); }
    if let Some(cat) = category { tool = tool.with_category(cat); }
    tool.metadata = serde_json::json!({
        "categories_raw": categories_raw,
        "exec": exec,
        "try_exec": try_exec,
    });
    Some(tool)
}

fn which_path(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p).find_map(|d| {
            let candidate = d.join(bin);
            candidate.is_file().then_some(candidate)
        })
    })
}

/// Map `.desktop` Categories (semicolon-delimited) to our [`Category`] enum.
/// Picks the most informative match; falls back to None if nothing useful.
fn classify_categories(raw: &str) -> Option<Category> {
    let cats: Vec<&str> = raw.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    for c in &cats {
        let m = match *c {
            "Security" => Some(Category::Defensive),
            "Network" => Some(Category::Networking),
            "WebBrowser" | "Web" => Some(Category::Webapp),
            "Database" => Some(Category::Database),
            "Development" | "IDE" => Some(Category::Development),
            "TextEditor" => Some(Category::Editor),
            "TerminalEmulator" => Some(Category::Shell),
            "System" | "Settings" => Some(Category::SystemAdmin),
            "Documentation" => Some(Category::Documentation),
            "Education" | "Science" => Some(Category::DataScience),
            "Game" => None, // skip categorization for games
            _ => None,
        };
        if m.is_some() { return m; }
    }
    None
}
