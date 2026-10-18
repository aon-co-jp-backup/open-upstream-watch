//! Types shared by the core, the server and the Leptos dashboard. Timestamps are RFC 3339 strings so this crate
//! has no time dependency and compiles for wasm32.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// New upstream version seen, not processed yet.
    Detected,
    /// `dry_run`: the plan was generated but nothing was written to the RPoem repository.
    DryRun,
    /// Passed the gate and was pushed to the RPoem base branch.
    Merged,
    /// Prepared on a branch; a human has to look (code change, limit reached, auto-merge off, ...).
    NeedsReview,
    /// Refused by the gate (a path outside the allow-lists, a sensitive file, failing tests).
    Blocked,
    /// A technical failure (network, git, porter); it is retried on the next cycle.
    Failed,
    /// A watch-only upstream (for example Leptos or trunk): the new version is recorded, nothing is ported.
    Notified,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub upstream: String,
    pub from: Option<String>,
    pub to: String,
    pub status: JobStatus,
    pub created: String,
    pub updated: String,
    pub branch: String,
    pub release_url: String,
    pub summary: String,
    pub log: Vec<String>,
    pub merged_commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpstreamView {
    pub name: String,
    pub repo: String,
    pub tag_prefix: Option<String>,
    pub area: String,
    pub baseline: Option<String>,
    pub last_seen: Option<String>,
    pub last_checked: Option<String>,
    pub last_error: Option<String>,
    /// `ok`, `archived`, `renamed` or `missing` (the upstream repository itself).
    pub health: String,
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigView {
    pub dry_run: bool,
    pub auto_merge: bool,
    pub allow_code_merge: bool,
    pub poll_interval_hours: u64,
    pub porter: String,
    pub search_enabled: bool,
    pub llm_enabled: bool,
    pub check_now_enabled: bool,
    pub alert_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub generated_at: String,
    pub config: ConfigView,
    pub upstreams: Vec<UpstreamView>,
    pub jobs: Vec<Job>,
    pub running: bool,
    /// true in the public demo: the jobs are samples, only the version numbers are real
    #[serde(default)]
    pub demo: bool,
}
