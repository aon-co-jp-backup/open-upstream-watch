//! The merge gate: a pure decision about whether a prepared change may reach the RPoem base branch automatically.
//! Everything not explicitly allowed is refused. Tests (`test_cmd`) are an additional condition checked by the pipeline.

use crate::config::RpoemConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// May be pushed to the base branch (once the tests pass).
    Merge,
    /// Needs a human (kept on a branch).
    Review(String),
    /// Refused.
    Block(String),
}

/// Paths that are never merged automatically, whatever the allow-lists say.
pub fn is_sensitive(path: &str) -> bool {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    p.starts_with(".git/")
        || p.starts_with(".github/")
        || p.contains("/.git/")
        || p.ends_with(".pem")
        || p.ends_with(".key")
        || p.ends_with(".p12")
        || p.rsplit('/').next().map(|f| f.starts_with(".env") || f.contains("secret") || f.contains("token") || f.contains("credential")).unwrap_or(false)
}

fn under(path: &str, prefixes: &[String]) -> bool {
    let p = path.replace('\\', "/");
    prefixes.iter().any(|d| {
        let d = d.replace('\\', "/");
        p.starts_with(&d) && !p.contains("..")
    })
}

pub fn decide(changed: &[String], cfg: &RpoemConfig, merges_today: usize) -> Decision {
    if changed.is_empty() {
        return Decision::Review("the porter changed nothing".into());
    }
    let mut needs_code = false;
    for f in changed {
        if is_sensitive(f) {
            return Decision::Block(format!("sensitive path touched: {f}"));
        }
        if under(f, &cfg.docs_paths) {
            continue;
        }
        if under(f, &cfg.code_paths) {
            needs_code = true;
            continue;
        }
        return Decision::Block(format!("path outside the allow-lists: {f}"));
    }
    if needs_code && !cfg.allow_code_merge {
        return Decision::Review("code changes are not merged automatically (allow_code_merge = false)".into());
    }
    if !cfg.auto_merge {
        return Decision::Review("auto_merge is off".into());
    }
    if merges_today >= cfg.max_merges_per_day {
        return Decision::Review(format!("daily merge limit reached ({})", cfg.max_merges_per_day));
    }
    Decision::Merge
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> RpoemConfig {
        RpoemConfig::default()
    }
    fn v(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn docs_only_changes_merge() {
        assert_eq!(decide(&v(&["docs/upstream/poem/3.2.0.md"]), &cfg(), 0), Decision::Merge);
    }

    #[test]
    fn code_changes_need_review_unless_opted_in() {
        let c = v(&["crates/open-runo-poem-compat/src/lib.rs"]);
        assert!(matches!(decide(&c, &cfg(), 0), Decision::Review(_)));
        let mut on = cfg();
        on.allow_code_merge = true;
        assert_eq!(decide(&c, &on, 0), Decision::Merge);
    }

    #[test]
    fn everything_else_is_blocked() {
        assert!(matches!(decide(&v(&["docs/upstream/a.md", "Cargo.toml"]), &cfg(), 0), Decision::Block(_)));
        assert!(matches!(decide(&v(&[".github/workflows/ci.yml"]), &cfg(), 0), Decision::Block(_)));
        assert!(matches!(decide(&v(&["docs/upstream/../../etc/passwd"]), &cfg(), 0), Decision::Block(_)));
        assert!(matches!(decide(&v(&["docs/upstream/my-token.txt"]), &cfg(), 0), Decision::Block(_)));
        assert!(matches!(decide(&v(&["docs/upstream/.env"]), &cfg(), 0), Decision::Block(_)));
    }

    #[test]
    fn limits_and_switches_apply() {
        let c = v(&["docs/upstream/x.md"]);
        let mut off = cfg();
        off.auto_merge = false;
        assert!(matches!(decide(&c, &off, 0), Decision::Review(_)));
        assert!(matches!(decide(&c, &cfg(), 3), Decision::Review(_)));
        assert!(matches!(decide(&[], &cfg(), 0), Decision::Review(_)));
    }
}
