//! Docker collector — locally pulled images recorded as tools. For security work, an image
//! IS the tool delivery mechanism: `metasploit/metasploit-framework:latest`, `kalilinux/kali-rolling`,
//! `pentestgpt:latest`, etc. The "path" stored is `docker run <image>` as the invocation hint.
//!
//! Requires the docker daemon to be running. Returns empty (not an error) if the daemon is
//! unreachable — common when Docker Desktop hasn't been started.

use crate::util::{run_capture, which};
use anyhow::Result;
use librarian_core::{Category, Collector, Source, Tool};
use std::path::PathBuf;

pub struct DockerCollector;
impl DockerCollector {
    pub fn new() -> Self { Self }
}
impl Default for DockerCollector {
    fn default() -> Self { Self::new() }
}
impl Collector for DockerCollector {
    fn source(&self) -> Source { Source::Docker }
    fn is_available(&self) -> bool { which("docker") || which("podman") }
    fn collect(&self) -> Result<Vec<Tool>> {
        let bin = if which("docker") { "docker" } else { "podman" };

        // One JSON object per line (NDJSON). If the daemon is down, docker exits non-zero
        // with empty stdout; we tolerate that and return no rows rather than failing the scan.
        let output = run_capture(bin, &["images", "--format", "{{json .}}"], true)
            .unwrap_or_default();

        let mut tools = Vec::new();
        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }
            let img: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let repo = img.get("Repository").and_then(|v| v.as_str()).unwrap_or("");
            let tag = img.get("Tag").and_then(|v| v.as_str()).unwrap_or("");
            // Skip dangling images (`<none>:<none>`) — they're build intermediates, not tools.
            if repo == "<none>" || repo.is_empty() {
                continue;
            }
            let id = img.get("ID").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let size = img.get("Size").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let created = img.get("CreatedAt").and_then(|v| v.as_str()).unwrap_or("").to_string();

            let name_full = if tag.is_empty() || tag == "<none>" {
                repo.to_string()
            } else {
                format!("{repo}:{tag}")
            };

            // The "path" field is a docker invocation hint rather than a filesystem path —
            // local AIs reading the manifest get a runnable command rather than a dead link.
            let invocation = PathBuf::from(format!("docker run --rm -it {name_full}"));

            let mut tool = Tool::new(name_full.clone(), Source::Docker)
                .with_path(invocation)
                .with_package(repo.to_string());
            if !tag.is_empty() && tag != "<none>" {
                tool = tool.with_version(tag.to_string());
            }
            if let Some(cat) = infer_image_category(repo) {
                tool = tool.with_category(cat);
            }
            tool.description = Some(format!(
                "Docker image (id {}, {}, created {})",
                id.chars().take(12).collect::<String>(),
                size,
                created
            ));
            tool.metadata = serde_json::json!({
                "image_id": id,
                "repository": repo,
                "tag": tag,
                "size": size,
                "created": created,
                "engine": bin,
            });
            tools.push(tool);
        }
        Ok(tools)
    }
}

fn infer_image_category(repo: &str) -> Option<Category> {
    let r = repo.to_ascii_lowercase();
    let r = r.as_str();
    if r.contains("kali") || r.contains("blackarch") || r.contains("parrot") {
        return Some(Category::Misc); // distro images — security-flavoured but mixed contents
    }
    if r.contains("metasploit") || r.contains("msf") || r.contains("sliver") || r.contains("havoc") {
        return Some(Category::Exploitation);
    }
    if r.contains("nuclei") || r.contains("nikto") || r.contains("sqlmap") || r.contains("zap") || r.contains("burp") {
        return Some(Category::Webapp);
    }
    if r.contains("nmap") || r.contains("masscan") || r.contains("recon") || r.contains("amass") {
        return Some(Category::Recon);
    }
    if r.contains("pentest") || r.contains("hackingtool") || r.contains("pentestgpt") {
        return Some(Category::Misc);
    }
    if r.contains("postgres") || r.contains("mysql") || r.contains("mariadb") || r.contains("mongo") || r.contains("redis") {
        return Some(Category::Database);
    }
    if r.contains("node") || r.contains("python") || r.contains("ruby") || r.contains("rust") || r.contains("golang") {
        return Some(Category::LanguageRuntime);
    }
    None
}
