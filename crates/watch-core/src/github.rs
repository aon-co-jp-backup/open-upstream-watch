//! GitHub client: lists releases (or tags) of an upstream and picks the newest stable version.

use crate::version::{parse_tag, Version};
use anyhow::{bail, Context, Result};
use rust_json::{parse_secure, Limits};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub body: String,
    pub url: String,
    pub published_at: Option<String>,
}

#[derive(Clone)]
pub struct GithubClient {
    http: reqwest::Client,
    base: String,
    /// crates.io (some upstreams, e.g. poem, publish new versions only there, not as GitHub releases)
    crates_base: String,
    token: Option<String>,
}

impl GithubClient {
    pub fn new(base: &str, token: Option<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("open-upstream-watch")
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { http, base: base.trim_end_matches('/').to_string(), crates_base: "https://crates.io".to_string(), token })
    }

    /// Points the crates.io lookups somewhere else (tests).
    pub fn with_crates_base(mut self, base: &str) -> Self {
        self.crates_base = base.trim_end_matches('/').to_string();
        self
    }

    pub fn public() -> Result<Self> {
        Self::new("https://api.github.com", None)
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let mut req = self.http.get(format!("{}{}", self.base, path)).header("Accept", "application/vnd.github+json");
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.with_context(|| format!("GET {path}"))?;
        let status = resp.status();
        if !status.is_success() {
            bail!("GitHub API {} for {}", status.as_u16(), path);
        }
        let text = resp.text().await?;
        // external, untrusted input: size / depth / duplicate-key limits
        let limits = Limits { max_bytes: 8 * 1024 * 1024, ..Limits::default() };
        parse_secure(&text, &limits).map_err(|e| anyhow::anyhow!("rejected GitHub response: {e}"))
    }

    /// crates.io: the newest stable version of a crate as a single pseudo release tagged `v<version>`.
    async fn crate_releases(&self, name: &str) -> Result<Vec<Release>> {
        let resp = self.http.get(format!("{}/api/v1/crates/{}", self.crates_base, name)).send().await.with_context(|| format!("GET crates.io {name}"))?;
        if !resp.status().is_success() {
            bail!("crates.io answered {} for {}", resp.status().as_u16(), name);
        }
        let limits = Limits { max_bytes: 4 * 1024 * 1024, ..Limits::default() };
        let v = parse_secure(&resp.text().await?, &limits).map_err(|e| anyhow::anyhow!("rejected crates.io response: {e}"))?;
        let ver = v.get("crate").and_then(|c| c.get("max_stable_version")).and_then(Value::as_str).context("crates.io: no max_stable_version")?;
        Ok(vec![Release { tag: format!("v{ver}"), name: format!("{name} {ver}"), body: String::new(), url: format!("https://crates.io/crates/{name}/{ver}"), published_at: None }])
    }

    pub async fn releases(&self, repo: &str, kind: &str) -> Result<Vec<Release>> {
        if kind == "crate" {
            return self.crate_releases(repo).await;
        }
        if kind == "tag" {
            let v = self.get_json(&format!("/repos/{repo}/tags?per_page=50")).await?;
            return Ok(v
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|t| {
                            let tag = t.get("name")?.as_str()?.to_string();
                            Some(Release { url: format!("https://github.com/{repo}/releases/tag/{tag}"), name: tag.clone(), tag, body: String::new(), published_at: None })
                        })
                        .collect()
                })
                .unwrap_or_default());
        }
        let v = self.get_json(&format!("/repos/{repo}/releases?per_page=50")).await?;
        let mut out = Vec::new();
        for r in v.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            if r.get("draft").and_then(Value::as_bool).unwrap_or(false) || r.get("prerelease").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            let Some(tag) = r.get("tag_name").and_then(Value::as_str) else { continue };
            out.push(Release {
                tag: tag.to_string(),
                name: r.get("name").and_then(Value::as_str).unwrap_or(tag).to_string(),
                body: r.get("body").and_then(Value::as_str).unwrap_or("").to_string(),
                url: r.get("html_url").and_then(Value::as_str).unwrap_or("").to_string(),
                published_at: r.get("published_at").and_then(Value::as_str).map(str::to_string),
            });
        }
        Ok(out)
    }
}

/// State of the upstream repository itself (not of its releases).
#[derive(Debug, Clone, PartialEq)]
pub enum Health {
    Ok,
    Archived,
    /// the repository now lives under another name
    Renamed(String),
    /// 404 / 410 / 451 (deleted, made private or taken down); callers wait for a second confirmation
    Missing,
    /// anything else (network trouble, rate limit): says nothing about the repository
    Unknown(String),
}

impl GithubClient {
    pub async fn repo_health(&self, repo: &str, kind: &str) -> Health {
        if kind == "crate" {
            return match self.http.get(format!("{}/api/v1/crates/{}", self.crates_base, repo)).send().await {
                Ok(r) if matches!(r.status().as_u16(), 404 | 410) => Health::Missing,
                Ok(r) if r.status().is_success() => Health::Ok,
                Ok(r) => Health::Unknown(format!("crates.io {}", r.status().as_u16())),
                Err(e) => Health::Unknown(e.to_string()),
            };
        }
        let mut req = self.http.get(format!("{}/repos/{}", self.base, repo)).header("Accept", "application/vnd.github+json");
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return Health::Unknown(e.to_string()),
        };
        let status = resp.status().as_u16();
        if matches!(status, 404 | 410 | 451) {
            return Health::Missing;
        }
        if !resp.status().is_success() {
            return Health::Unknown(format!("GitHub API {status}"));
        }
        let Ok(text) = resp.text().await else { return Health::Unknown("unreadable body".into()) };
        let limits = Limits { max_bytes: 2 * 1024 * 1024, ..Limits::default() };
        let Ok(v) = parse_secure(&text, &limits) else { return Health::Unknown("rejected response".into()) };
        if v.get("archived").and_then(Value::as_bool).unwrap_or(false) || v.get("disabled").and_then(Value::as_bool).unwrap_or(false) {
            return Health::Archived;
        }
        if let Some(full) = v.get("full_name").and_then(Value::as_str) {
            if !full.eq_ignore_ascii_case(repo) {
                return Health::Renamed(full.to_string());
            }
        }
        Health::Ok
    }
}

/// The newest stable version among the releases whose tag matches `prefix`.
pub fn latest(releases: &[Release], prefix: Option<&str>) -> Option<(Version, Release)> {
    releases
        .iter()
        .filter_map(|r| parse_tag(&r.tag, prefix).map(|v| (v, r.clone())))
        .max_by(|a, b| a.0.cmp(&b.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str) -> Release {
        Release { tag: tag.into(), name: tag.into(), body: String::new(), url: String::new(), published_at: None }
    }

    #[test]
    fn picks_highest_stable_matching_prefix() {
        let rs = vec![rel("router@0.9.0"), rel("router@0.10.1"), rel("cli@9.9.9"), rel("router@0.11.0-rc.1")];
        let (v, r) = latest(&rs, Some("router@")).unwrap();
        assert_eq!(v.text, "0.10.1");
        assert_eq!(r.tag, "router@0.10.1");
        assert!(latest(&rs, Some("nothing@")).is_none());
    }
}
