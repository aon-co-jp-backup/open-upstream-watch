//! Tolerant version handling for upstream tags such as `v19.1.0`, `router@0.100.1`, `tauri-v2.1.0`, `poem-v3.1.5`.

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub parts: Vec<u64>,
    /// the version text without the tag prefix, e.g. `3.1.5`
    pub text: String,
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let n = self.parts.len().max(other.parts.len());
        for i in 0..n {
            let a = self.parts.get(i).copied().unwrap_or(0);
            let b = other.parts.get(i).copied().unwrap_or(0);
            if a != b {
                return a.cmp(&b);
            }
        }
        Ordering::Equal
    }
}

/// Parses a tag into a version. Pre-releases (`-rc.1`, `-beta`, `-next`, ...) are rejected on purpose:
/// only stable releases trigger porting work.
pub fn parse_tag(tag: &str, prefix: Option<&str>) -> Option<Version> {
    let mut rest = tag;
    if let Some(p) = prefix {
        rest = rest.strip_prefix(p)?;
    } else {
        rest = rest.strip_prefix('v').unwrap_or(rest);
    }
    // a leading "v" may remain after a custom prefix such as "tauri-"
    rest = rest.strip_prefix('v').unwrap_or(rest);
    if rest.is_empty() || !rest.chars().next()?.is_ascii_digit() {
        return None;
    }
    if rest.chars().any(|c| !(c.is_ascii_digit() || c == '.')) {
        return None; // "-rc.1", "+build", "-beta" ... : not a plain stable version
    }
    let parts: Option<Vec<u64>> = rest.split('.').map(|s| s.parse::<u64>().ok()).collect();
    let parts = parts?;
    if parts.is_empty() {
        return None;
    }
    Some(Version { parts, text: rest.to_string() })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bump {
    Patch,
    Minor,
    Major,
}

impl Bump {
    pub fn parse(s: &str) -> Bump {
        match s {
            "patch" => Bump::Patch,
            "major" => Bump::Major,
            _ => Bump::Minor,
        }
    }
}

/// How big the step from `from` to `to` is (a changed first component is Major, the second Minor, anything else Patch).
pub fn bump_kind(from: &str, to: &str) -> Bump {
    let f = parse_tag(from, None);
    let t = parse_tag(to, None);
    let (Some(f), Some(t)) = (f, t) else { return Bump::Major };
    let g = |v: &Version, i: usize| v.parts.get(i).copied().unwrap_or(0);
    if g(&f, 0) != g(&t, 0) {
        Bump::Major
    } else if g(&f, 1) != g(&t, 1) {
        Bump::Minor
    } else {
        Bump::Patch
    }
}

/// `major.minor` keeps the first two components, `full` the whole version.
pub fn format_version(v: &str, format: &str) -> String {
    if format == "full" {
        return v.to_string();
    }
    v.split('.').take(2).collect::<Vec<_>>().join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_tag_shapes() {
        assert_eq!(parse_tag("v19.1.0", None).unwrap().text, "19.1.0");
        assert_eq!(parse_tag("router@0.100.1", Some("router@")).unwrap().parts, vec![0, 100, 1]);
        assert_eq!(parse_tag("tauri-v2.1.0", Some("tauri-")).unwrap().text, "2.1.0");
        assert_eq!(parse_tag("poem-v3.1.5", Some("poem-v")).unwrap().text, "3.1.5");
        assert_eq!(parse_tag("3.0", None).unwrap().parts, vec![3, 0]);
    }

    #[test]
    fn rejects_prereleases_and_foreign_tags() {
        assert!(parse_tag("v19.0.0-rc.1", None).is_none());
        assert!(parse_tag("v2.0.0-beta", None).is_none());
        assert!(parse_tag("cli@1.0.0", Some("router@")).is_none());
        assert!(parse_tag("nightly", None).is_none());
    }

    #[test]
    fn classifies_bumps_and_formats() {
        assert_eq!(bump_kind("0.7.4", "0.7.5"), Bump::Patch);
        assert_eq!(bump_kind("0.7.4", "0.8.0"), Bump::Minor);
        assert_eq!(bump_kind("0.8.2", "1.0.0"), Bump::Major);
        assert!(Bump::Patch < Bump::Minor && Bump::Minor < Bump::Major);
        assert_eq!(format_version("0.8.3", "major.minor"), "0.8");
        assert_eq!(format_version("0.8.3", "full"), "0.8.3");
    }

    #[test]
    fn orders_numerically_not_lexically() {
        let a = parse_tag("v1.9.0", None).unwrap();
        let b = parse_tag("v1.10.0", None).unwrap();
        assert!(b > a);
        // trailing zeros do not matter for ordering
        assert_eq!(parse_tag("v1.2", None).unwrap().cmp(&parse_tag("v1.2.0", None).unwrap()), Ordering::Equal);
    }
}
