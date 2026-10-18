//! Reading release notes for breaking changes.
//! Two independent signals are combined: a keyword scan (deterministic, always available) and, when configured,
//! the opinion of aruaru-llm (a small local model, accelerated by open-cuda when it runs on a GPU machine).
//! The model's answer is only ever used to *hold a change back for review*, never to let something through.

use crate::services::ask_breaking;

const KEYWORDS: &[&str] = &["breaking", "incompatible", "removed", "removes", "migration", "migrate", "deprecat", "no longer", "renamed", "backwards-incompatible"];

/// Lines of the notes that look like breaking changes (at most 6, trimmed).
pub fn breaking_signals(notes: &str) -> Vec<String> {
    notes
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| {
            let low = l.to_ascii_lowercase();
            KEYWORDS.iter().any(|k| low.contains(k))
        })
        .take(6)
        .map(|l| l.chars().take(200).collect())
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Assessment {
    pub lines: Vec<String>,
    /// aruaru-llm's yes/no, `None` when it is not configured or gave no usable answer
    pub ai_opinion: Option<bool>,
}

impl Assessment {
    /// Whether the change should be held back for a human.
    pub fn suspicious(&self) -> bool {
        !self.lines.is_empty() || self.ai_opinion == Some(true)
    }
}

pub async fn assess(notes: &str, llm_url: Option<&str>) -> (Assessment, Option<String>) {
    let lines = breaking_signals(notes);
    let mut note = None;
    let ai = match llm_url {
        Some(u) if !notes.trim().is_empty() => match ask_breaking(u, notes).await {
            Ok(a) => a,
            Err(e) => {
                note = Some(format!("llm unavailable: {e}"));
                None
            }
        },
        _ => None,
    };
    (Assessment { lines, ai_opinion: ai }, note)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_breaking_lines() {
        let notes = "## v0.8\n- Added a thing\n- **BREAKING**: `view!` now requires braces\n- Removed the old `create_signal`\n\nThanks!";
        let l = breaking_signals(notes);
        assert_eq!(l.len(), 2);
        assert!(l[0].contains("BREAKING"));
    }

    #[test]
    fn quiet_notes_are_not_suspicious() {
        assert!(!Assessment { lines: breaking_signals("- Fixed a typo\n- Faster builds"), ai_opinion: None }.suspicious());
        assert!(Assessment { lines: vec![], ai_opinion: Some(true) }.suspicious());
        assert!(!Assessment { lines: vec![], ai_opinion: Some(false) }.suspicious());
    }
}
