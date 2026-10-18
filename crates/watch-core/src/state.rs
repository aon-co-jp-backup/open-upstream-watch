//! Persistent state (a JSON file written atomically).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use watch_types::Job;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpState {
    pub baseline: Option<String>,
    pub last_seen: Option<String>,
    pub last_checked: Option<String>,
    pub last_error: Option<String>,
    /// `ok` (default), `archived`, `renamed` or `missing`.
    #[serde(default)]
    pub health: String,
    /// consecutive 404 / 410 / 451 answers for the repository (two are needed before it counts as missing)
    #[serde(default)]
    pub missing_count: u32,
    /// the health value an alert was last sent for (so each problem is reported once)
    #[serde(default)]
    pub alerted: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub upstreams: BTreeMap<String, UpState>,
    #[serde(default)]
    pub jobs: Vec<Job>,
    /// RFC 3339 timestamps of automatic merges (for the daily limit).
    #[serde(default)]
    pub merges: Vec<String>,
}

impl State {
    pub fn load(path: &Path) -> Result<State> {
        if !path.exists() {
            return Ok(State::default());
        }
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Atomic: write a temp file next to the target, then rename.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn merges_today(&self, today: &str) -> usize {
        self.merges.iter().filter(|m| m.starts_with(today)).count()
    }

    /// Keeps the file small: newest 200 jobs, newest 200 merge stamps.
    pub fn trim(&mut self) {
        if self.jobs.len() > 200 {
            let drop = self.jobs.len() - 200;
            self.jobs.drain(0..drop);
        }
        if self.merges.len() > 200 {
            let drop = self.merges.len() - 200;
            self.merges.drain(0..drop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_and_counts_today() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub").join("state.json");
        let mut s = State::default();
        s.upstreams.insert("poem".into(), UpState { baseline: Some("3.1.0".into()), ..Default::default() });
        s.merges = vec!["2026-10-18T01:00:00Z".into(), "2026-10-18T05:00:00Z".into(), "2026-10-17T23:00:00Z".into()];
        s.save(&p).unwrap();
        let back = State::load(&p).unwrap();
        assert_eq!(back.upstreams["poem"].baseline.as_deref(), Some("3.1.0"));
        assert_eq!(back.merges_today("2026-10-18"), 2);
        assert!(State::load(&dir.path().join("missing.json")).unwrap().jobs.is_empty());
    }
}
