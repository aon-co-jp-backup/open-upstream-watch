//! Optional helpers: aruaru-search (the project's own unlimited meta-search, reachable inside the VPS or through an
//! SSH tunnel) and aruaru-llm (a small local model).  Both are best-effort: a failure only means less context.
//! Output of aruaru-llm is machine text of low reliability and is always labelled as such.

use anyhow::{bail, Result};
use rust_json::{parse_secure, Limits};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub title: String,
    pub link: String,
    pub snippet: String,
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().user_agent("open-upstream-watch").timeout(Duration::from_secs(40)).build()?)
}

fn read_json(text: &str) -> Result<Value> {
    let limits = Limits { max_bytes: 2 * 1024 * 1024, ..Limits::default() };
    parse_secure(text, &limits).map_err(|e| anyhow::anyhow!("rejected response: {e}"))
}

fn encode(q: &str) -> String {
    q.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

pub async fn search(base: &str, query: &str, n: usize) -> Result<Vec<Hit>> {
    let url = format!("{}/v1/search?q={}&hl=en&n={}", base.trim_end_matches('/'), encode(query), n);
    let resp = client()?.get(url).send().await?;
    if !resp.status().is_success() {
        bail!("aruaru-search returned {}", resp.status());
    }
    let v = read_json(&resp.text().await?)?;
    Ok(v.get("results")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    Some(Hit {
                        title: r.get("title")?.as_str()?.to_string(),
                        link: r.get("link")?.as_str()?.to_string(),
                        snippet: r.get("snippet").and_then(Value::as_str).unwrap_or("").to_string(),
                    })
                })
                .take(n)
                .collect()
        })
        .unwrap_or_default())
}

/// Asks aruaru-llm for a very short machine summary. Returns None when the service answers nothing usable.
pub async fn summarize(base: &str, prompt: &str) -> Result<Option<String>> {
    summarize_with(base, prompt, 96).await
}

async fn summarize_with(base: &str, prompt: &str, max_new_tokens: u32) -> Result<Option<String>> {
    let body = serde_json::json!({ "prompt": prompt, "max_new_tokens": max_new_tokens });
    let resp = client()?.post(format!("{}/v1/generate", base.trim_end_matches('/'))).json(&body).send().await?;
    if !resp.status().is_success() {
        bail!("aruaru-llm returned {}", resp.status());
    }
    let v = read_json(&resp.text().await?)?;
    for key in ["text", "generated_text", "output", "reply", "completion"] {
        if let Some(s) = v.get(key).and_then(Value::as_str) {
            let s = s.trim();
            if !s.is_empty() {
                return Ok(Some(s.to_string()));
            }
        }
    }
    Ok(None)
}

/// Asks aruaru-llm whether release notes announce breaking changes. `Some(true/false)` only when the answer starts
/// with yes / no; anything else is treated as "no opinion".
pub async fn ask_breaking(base: &str, notes: &str) -> Result<Option<bool>> {
    let body: String = notes.chars().take(1800).collect();
    let prompt = format!("Release notes:
{body}

Question: Do these release notes announce breaking changes that require code changes? Answer yes or no.
Answer:");
    let answer = summarize_with(base, &prompt, 4).await?;
    Ok(answer.and_then(|a| {
        let a = a.trim().to_ascii_lowercase();
        if a.starts_with("yes") {
            Some(true)
        } else if a.starts_with("no") {
            Some(false)
        } else {
            None
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encodes_queries() {
        assert_eq!(encode("poem 3.1 release"), "poem%203.1%20release");
        assert_eq!(encode("a&b=c"), "a%26b%3Dc");
    }
}
