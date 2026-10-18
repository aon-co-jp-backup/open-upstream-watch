//! Git through the `git` CLI, in the bot's own clone of the RPoem repository (the owner's working tree is never touched).

use crate::config::{GitIdentity, RpoemConfig};
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Clones on first use, otherwise fetches; then checks out a fresh work branch from `origin/<base>`.
pub fn prepare_branch(cfg: &RpoemConfig, branch: &str) -> Result<()> {
    let dir = &cfg.clone_dir;
    if !dir.join(".git").exists() {
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let parent = dir.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let name = dir.file_name().context("clone_dir has no name")?.to_string_lossy().to_string();
        let out = Command::new("git")
            .args(["clone", &cfg.remote, &name])
            .current_dir(parent)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .context("running git clone")?;
        if !out.status.success() {
            bail!("git clone failed: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
    }
    run(dir, &["fetch", "origin", &cfg.base_branch])?;
    run(dir, &["checkout", "-B", branch, &format!("origin/{}", cfg.base_branch)])?;
    run(dir, &["clean", "-fd"])?;
    Ok(())
}

/// Files changed in the work tree relative to HEAD (tracked, modified or new).
pub fn changed_files(dir: &Path) -> Result<Vec<String>> {
    // `git status --porcelain` lines start with a significant space (" M path"); trimming the output would eat it.
    // These two commands print bare paths: tracked changes (staged or not) and new untracked files.
    let tracked = run(dir, &["diff", "--name-only", "HEAD"])?;
    let untracked = run(dir, &["ls-files", "--others", "--exclude-standard"])?;
    let mut files: Vec<String> = tracked.lines().chain(untracked.lines()).map(|l| l.trim().trim_matches('"').to_string()).filter(|l| !l.is_empty()).collect();
    files.sort();
    files.dedup();
    Ok(files)
}

pub fn commit_all(dir: &Path, id: &GitIdentity, message: &str) -> Result<String> {
    run(dir, &["add", "-A"])?;
    run(dir, &["-c", &format!("user.name={}", id.name), "-c", &format!("user.email={}", id.email), "commit", "-q", "-m", message])?;
    run(dir, &["rev-parse", "HEAD"])
}

/// Fast-forward push of the work branch to the base branch (fails if the base moved: the caller retries later).
pub fn push_to_base(cfg: &RpoemConfig) -> Result<()> {
    run(&cfg.clone_dir, &["push", "origin", &format!("HEAD:refs/heads/{}", cfg.base_branch)]).map(|_| ())
}

/// Pushes the branch itself, for a human to review.
pub fn push_branch(cfg: &RpoemConfig, branch: &str) -> Result<()> {
    run(&cfg.clone_dir, &["push", "origin", &format!("HEAD:refs/heads/{branch}")]).map(|_| ())
}

/// Runs the configured test command in the clone through the platform shell.
pub fn run_tests(dir: &Path, cmd: &str) -> Result<(bool, String)> {
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    let out = c.current_dir(dir).output().with_context(|| format!("running test command: {cmd}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    let tail: String = text.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    Ok((out.status.success(), tail))
}
