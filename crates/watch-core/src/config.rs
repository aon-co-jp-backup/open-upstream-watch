//! Configuration (a local, trusted TOML file). Secrets are never stored here: tokens are read from txt files whose
//! paths are given, and their values are never printed or logged.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Poll interval in hours.
    #[serde(default = "d_poll")]
    pub poll_interval_hours: u64,
    /// When true nothing is written to the RPoem repository: jobs only produce a plan (safe first run).
    #[serde(default = "d_true")]
    pub dry_run: bool,
    /// Where the state file lives.
    #[serde(default = "d_state")]
    pub state_file: PathBuf,
    /// URL path the whole app is mounted under (the site serves it at easy-web.tokyo/leptos-rs).
    #[serde(default = "d_base")]
    pub base_path: String,
    /// Directory with the built Leptos dashboard (trunk `dist`).
    #[serde(default = "d_ui")]
    pub ui_dist: PathBuf,
    /// Optional txt file with a GitHub token (raises the API limit; read access is enough).
    pub github_token_file: Option<PathBuf>,
    #[serde(default)]
    pub rpoem: RpoemConfig,
    /// Further repositories that receive work, e.g. open-vsc-claude (its Leptos / trunk pins). Referenced by `upstream.target`.
    #[serde(default, rename = "target")]
    pub targets: Vec<NamedTarget>,
    #[serde(default)]
    pub search: Option<ServiceConfig>,
    #[serde(default)]
    pub llm: Option<ServiceConfig>,
    #[serde(default)]
    pub porter: PorterConfig,
    #[serde(default)]
    pub alert: AlertConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub git: GitIdentity,
    #[serde(default, rename = "upstream")]
    pub upstreams: Vec<Upstream>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RpoemConfig {
    #[serde(default = "d_remote")]
    pub remote: String,
    /// The bot keeps its own clone here, so the owner's working tree is never touched.
    #[serde(default = "d_clone")]
    pub clone_dir: PathBuf,
    #[serde(default = "d_main")]
    pub base_branch: String,
    /// Run inside the clone before any merge; a non-zero exit blocks the merge.
    #[serde(default = "d_test")]
    pub test_cmd: String,
    #[serde(default = "d_true")]
    pub auto_merge: bool,
    /// Code changes (anything under `code_paths`) are only merged automatically when this is true.
    #[serde(default)]
    pub allow_code_merge: bool,
    #[serde(default = "d_docs")]
    pub docs_paths: Vec<String>,
    #[serde(default = "d_code")]
    pub code_paths: Vec<String>,
    #[serde(default = "d_max_merges")]
    pub max_merges_per_day: usize,
    /// Push the prepared branch (for review) when the job is not merged.
    #[serde(default = "d_true")]
    pub push_review_branch: bool,
    /// Release notes that look like breaking changes keep a code change from merging automatically.
    #[serde(default = "d_true")]
    pub review_on_breaking: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NamedTarget {
    pub name: String,
    #[serde(flatten)]
    pub cfg: RpoemConfig,
}

impl Default for RpoemConfig {
    fn default() -> Self {
        Self {
            remote: d_remote(),
            clone_dir: d_clone(),
            base_branch: d_main(),
            test_cmd: d_test(),
            auto_merge: true,
            allow_code_merge: false,
            docs_paths: d_docs(),
            code_paths: d_code(),
            max_merges_per_day: d_max_merges(),
            push_review_branch: true,
            review_on_breaking: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PorterConfig {
    /// `plan` (default): writes a porting plan under `docs/upstream/`. `command`: runs `command` in the clone.
    #[serde(default = "d_plan")]
    pub kind: String,
    /// For `command`: a shell command; the env vars UPSTREAM, VERSION, FROM, RELEASE_URL and PROMPT_FILE are set.
    pub command: Option<String>,
}

impl Default for PorterConfig {
    fn default() -> Self {
        Self { kind: d_plan(), command: None }
    }
}

/// Login: e-mail one-time password, delegated to the existing open-easy-web server (same account, same SMTP).
#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    /// Only these e-mail addresses may log in. Empty = nobody (the dashboard stays locked; the demo stays public).
    #[serde(default)]
    pub allowed_emails: Vec<String>,
    /// open-easy-web's address as seen from this server (its `/api/auth/request-otp` and `/verify-otp` are used).
    #[serde(default = "d_otp_base")]
    pub otp_base_url: String,
    #[serde(default = "d_session_hours")]
    pub session_hours: u64,
    /// Mark the session cookie `Secure` (keep true behind https; turn off only for plain-http local tests).
    #[serde(default = "d_true")]
    pub cookie_secure: bool,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self { allowed_emails: vec![], otp_base_url: d_otp_base(), session_hours: d_session_hours(), cookie_secure: true }
    }
}

/// Who receives alerts (for example when an upstream repository disappears). Kept in the local config, never in the repo.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AlertConfig {
    #[serde(default)]
    pub to: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitIdentity {
    #[serde(default = "d_name")]
    pub name: String,
    #[serde(default = "d_email")]
    pub email: String,
}

impl Default for GitIdentity {
    fn default() -> Self {
        Self { name: d_name(), email: d_email() }
    }
}

/// A version pin inside a file of the target repository.
#[derive(Debug, Clone, Deserialize)]
pub struct Pin {
    /// Path inside the target repository.
    pub file: String,
    /// A regular expression with exactly one capture group: the text that holds the version.
    pub pattern: String,
    /// `major.minor` (default) or `full`.
    #[serde(default = "d_mm")]
    pub format: String,
    /// The biggest bump that may be merged automatically: `patch`, `minor` (default) or `major`. Larger bumps wait for review.
    #[serde(default = "d_minor")]
    pub max_auto_bump: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Upstream {
    pub name: String,
    /// `owner/repo` on GitHub, or the crate name when `kind = "crate"`.
    pub repo: String,
    /// Only tags starting with this are considered (monorepos such as `router@`, `tauri-v`).
    pub tag_prefix: Option<String>,
    /// `release` (default), `tag` (GitHub) or `crate` (crates.io).
    #[serde(default = "d_release")]
    pub kind: String,
    /// The RPoem area that re-implements this upstream (shown in the plan).
    #[serde(default)]
    pub area: String,
    /// Which target repository receives the work: `rpoem` (default, the `[rpoem]` section) or a `[[target]]` name.
    #[serde(default = "d_rpoem")]
    pub target: String,
    /// Keep a version pin in a target file up to date (for example the Leptos version used by a generated template).
    #[serde(default)]
    pub pin: Option<Pin>,
    /// `port` (default): a new version creates porting work. `notify`: watch only (Leptos, trunk ...); disappearance alerts apply to both.
    #[serde(default = "d_port")]
    pub mode: String,
}

fn d_poll() -> u64 { 6 }
fn d_true() -> bool { true }
fn d_state() -> PathBuf { PathBuf::from("state.json") }
fn d_base() -> String { "/leptos-rs".into() }
fn d_ui() -> PathBuf { PathBuf::from("ui/dist") }
fn d_remote() -> String { "https://github.com/aon-co-jp/RPoem.git".into() }
fn d_clone() -> PathBuf { PathBuf::from("work/RPoem") }
fn d_main() -> String { "main".into() }
fn d_test() -> String { "cargo check -p open-runo-gateway".into() }
fn d_docs() -> Vec<String> { vec!["docs/upstream/".into()] }
fn d_code() -> Vec<String> { vec!["crates/".into()] }
fn d_max_merges() -> usize { 3 }
fn d_plan() -> String { "plan".into() }
fn d_name() -> String { "open-upstream-watch".into() }
fn d_email() -> String { "open-upstream-watch@users.noreply.github.com".into() }
fn d_release() -> String { "release".into() }
fn d_port() -> String { "port".into() }
fn d_rpoem() -> String { "rpoem".into() }
fn d_mm() -> String { "major.minor".into() }
fn d_minor() -> String { "minor".into() }
fn d_otp_base() -> String { "http://127.0.0.1:8080".into() }
fn d_session_hours() -> u64 { 12 }

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let cfg: Config = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// The target configuration for a name (`rpoem` or a `[[target]]`).
    pub fn target(&self, name: &str) -> Option<&RpoemConfig> {
        if name == "rpoem" {
            return Some(&self.rpoem);
        }
        self.targets.iter().find(|t| t.name == name).map(|t| &t.cfg)
    }

    pub fn validate(&self) -> Result<()> {
        if self.poll_interval_hours == 0 {
            bail!("poll_interval_hours must be at least 1");
        }
        if !self.base_path.starts_with('/') || self.base_path.len() > 1 && self.base_path.ends_with('/') {
            bail!("base_path must start with '/' and have no trailing '/' (e.g. /leptos-rs)");
        }
        if self.porter.kind == "command" && self.porter.command.is_none() {
            bail!("porter.kind = \"command\" needs porter.command");
        }
        if !matches!(self.porter.kind.as_str(), "plan" | "command") {
            bail!("porter.kind must be \"plan\" or \"command\"");
        }
        let mut tnames = std::collections::BTreeSet::new();
        for t in &self.targets {
            if t.name == "rpoem" || !tnames.insert(&t.name) {
                bail!("target name {} is reserved or duplicated", t.name);
            }
        }
        let mut names = std::collections::BTreeSet::new();
        for u in &self.upstreams {
            if u.kind != "crate" && !u.repo.contains('/') {
                bail!("upstream {}: repo must be owner/repo (or a crate name with kind = \"crate\")", u.name);
            }
            if !names.insert(&u.name) {
                bail!("duplicate upstream name {}", u.name);
            }
            if !matches!(u.kind.as_str(), "release" | "tag" | "crate") {
                bail!("upstream {}: kind must be release, tag or crate", u.name);
            }
            if !matches!(u.mode.as_str(), "port" | "notify") {
                bail!("upstream {}: mode must be port or notify", u.name);
            }
            if self.target(&u.target).is_none() {
                bail!("upstream {}: unknown target {}", u.name, u.target);
            }
            if let Some(p) = &u.pin {
                let re = regex::Regex::new(&p.pattern).with_context(|| format!("upstream {}: bad pin pattern", u.name))?;
                if re.captures_len() != 2 {
                    bail!("upstream {}: the pin pattern needs exactly one capture group", u.name);
                }
                if !matches!(p.format.as_str(), "major.minor" | "full") || !matches!(p.max_auto_bump.as_str(), "patch" | "minor" | "major") {
                    bail!("upstream {}: pin.format / pin.max_auto_bump has an unknown value", u.name);
                }
                if p.file.contains("..") || p.file.starts_with('/') || p.file.contains(':') {
                    bail!("upstream {}: pin.file must be a relative path inside the target", u.name);
                }
            }
        }
        Ok(())
    }

    /// Reads the GitHub token from its txt file (first non-empty line). The value is never logged.
    pub fn github_token(&self) -> Option<String> {
        let p = self.github_token_file.as_ref()?;
        let text = std::fs::read_to_string(p).ok()?;
        text.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_config_gets_safe_defaults() {
        let c: Config = toml::from_str("").unwrap();
        c.validate().unwrap();
        assert!(c.dry_run, "first run must be a dry run");
        assert!(!c.rpoem.allow_code_merge, "code merges are opt-in");
        assert_eq!(c.base_path, "/leptos-rs");
    }

    #[test]
    fn rejects_bad_values() {
        let c: Config = toml::from_str("base_path = \"/x/\"").unwrap();
        assert!(c.validate().is_err());
        let c: Config = toml::from_str("[porter]\nkind = \"command\"").unwrap();
        assert!(c.validate().is_err());
        let c: Config = toml::from_str("[[upstream]]\nname=\"a\"\nrepo=\"nope\"").unwrap();
        assert!(c.validate().is_err());
        let c: Config = toml::from_str("[[upstream]]\nname=\"a\"\nrepo=\"o/r\"\n[[upstream]]\nname=\"a\"\nrepo=\"o/r\"").unwrap();
        assert!(c.validate().is_err());
    }
}
