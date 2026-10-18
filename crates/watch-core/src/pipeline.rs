//! One polling cycle: check each upstream's health, detect new versions, then process the resulting jobs.
//! Nothing here panics on bad input; every failure is recorded on the job / upstream and retried on a later cycle.

use crate::analysis::assess;
use crate::config::{Config, Upstream};
use crate::version::{bump_kind, Bump};
use crate::gate::{decide, Decision};
use crate::github::{latest, GithubClient, Health, Release};
use crate::notify::Notifier;
use crate::porter::{render_plan, run_porter, JobContext};
use crate::services::{search, summarize};
use crate::state::State;
use crate::version::parse_tag;
use crate::{gitops, state::UpState};
use watch_types::{Job, JobStatus};

const MAX_ATTEMPTS: usize = 5;

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn log(job: &mut Job, msg: impl Into<String>) {
    job.log.push(format!("{} {}", now_rfc3339(), msg.into()));
    job.updated = now_rfc3339();
}

fn attempts(job: &Job) -> usize {
    job.log.iter().filter(|l| l.contains("attempt started")).count()
}

fn alert_text(u: &Upstream, health: &str, detail: &str) -> (String, String) {
    let subject = format!("[open-upstream-watch] {}: upstream {} ({})", u.name, health, u.repo);
    let body = format!(
        "The upstream that open-upstream-watch follows changed state.\n\n\
         name:   {name}\nrepo:   https://github.com/{repo}\nstate:  {health}\ndetail: {detail}\nmode:   {mode}\nRPoem area: {area}\n\n\
         What it means: new versions of this upstream can no longer be detected (or no longer come from the same place).\n\
         Suggested next steps: open the repository link, find where the project moved to (or whether it was archived),\n\
         then update the [[upstream]] entry in the configuration or drop it.\n\n\
         This alert is sent once per change of state.\n\n\
         アップストリーム「{name}」({repo})が「{health}」になりました。新しい版を検出できなくなる可能性があります。\n\
         リポジトリの移転・アーカイブ・削除を確認し、設定の[[upstream]]を更新するか削除してください。この通知は状態が変わったときに1回だけ送ります。\n",
        name = u.name,
        repo = u.repo,
        health = health,
        detail = detail,
        mode = u.mode,
        area = if u.area.is_empty() { "-" } else { &u.area },
    );
    (subject, body)
}

/// Runs one full cycle. Returns human-readable lines for the log.
pub async fn run_cycle<N: Notifier>(cfg: &Config, state: &mut State, gh: &GithubClient, notifier: &N) -> Vec<String> {
    let mut out = Vec::new();
    let now = now_rfc3339();

    // 1) health + detect
    let mut to_process: Vec<(String, Option<Release>)> = Vec::new();
    for u in &cfg.upstreams {
        let health = gh.repo_health(&u.repo, &u.kind).await;
        {
            let entry = state.upstreams.entry(u.name.clone()).or_insert_with(UpState::default);
            let mut detail = String::new();
            match &health {
                Health::Ok => {
                    entry.health = "ok".into();
                    entry.missing_count = 0;
                    entry.alerted = None;
                }
                Health::Unknown(e) => {
                    entry.last_error = Some(e.clone());
                    if entry.health.is_empty() {
                        entry.health = "ok".into();
                    }
                }
                Health::Missing => {
                    entry.missing_count += 1;
                    detail = format!("the repository answered not found {} time(s) in a row", entry.missing_count);
                    if entry.missing_count >= 2 {
                        entry.health = "missing".into();
                    }
                }
                Health::Archived => {
                    entry.health = "archived".into();
                    detail = "the repository is archived or disabled (read-only)".into();
                }
                Health::Renamed(new) => {
                    entry.health = "renamed".into();
                    detail = format!("the repository is now {new}");
                    entry.last_error = Some(detail.clone());
                }
            }
            if entry.health.is_empty() {
                entry.health = "ok".into();
            }
            if entry.health != "ok" && entry.alerted.as_deref() != Some(entry.health.as_str()) {
                let h = entry.health.clone();
                let (subject, body) = alert_text(u, &h, &detail);
                match notifier.notify(&subject, &body).await {
                    Ok(()) => {
                        entry.alerted = Some(h.clone());
                        out.push(format!("{}: ALERT sent ({h})", u.name));
                    }
                    Err(e) => out.push(format!("{}: alert could not be sent ({h}): {e}", u.name)),
                }
            }
            if entry.health == "missing" {
                continue; // nothing to fetch
            }
        }

        let entry = state.upstreams.entry(u.name.clone()).or_insert_with(UpState::default);
        let releases = match gh.releases(&u.repo, &u.kind).await {
            Ok(r) => r,
            Err(e) => {
                entry.last_error = Some(e.to_string());
                out.push(format!("{}: check failed: {e}", u.name));
                continue;
            }
        };
        if !matches!(health, Health::Renamed(_)) {
            entry.last_error = None;
        }
        entry.last_checked = Some(now.clone());
        let Some((v, rel)) = latest(&releases, u.tag_prefix.as_deref()) else {
            out.push(format!("{}: no stable release found", u.name));
            continue;
        };
        match entry.last_seen.clone() {
            None => {
                entry.baseline = Some(v.text.clone());
                entry.last_seen = Some(v.text.clone());
                out.push(format!("{}: baseline {} (no job on the first run)", u.name, v.text));
            }
            Some(prev) => {
                let newer = parse_tag(&prev, None).map(|p| v > p).unwrap_or(true);
                if newer {
                    let id = format!("{}-{}", u.name, v.text);
                    let watch_only = u.mode == "notify";
                    if !state.jobs.iter().any(|j| j.id == id) {
                        state.jobs.push(Job {
                            id: id.clone(),
                            upstream: u.name.clone(),
                            from: Some(prev),
                            to: v.text.clone(),
                            status: if watch_only { JobStatus::Notified } else { JobStatus::Detected },
                            created: now.clone(),
                            updated: now.clone(),
                            branch: format!("upstream/{}-{}", u.name, v.text),
                            release_url: rel.url.clone(),
                            summary: String::new(),
                            log: vec![format!("{now} detected {} {}{}", u.name, v.text, if watch_only { " (watch only)" } else { "" })],
                            merged_commit: None,
                        });
                        out.push(format!("{}: NEW version {}", u.name, v.text));
                    }
                    if let Some(e) = state.upstreams.get_mut(&u.name) {
                        e.last_seen = Some(v.text.clone());
                    }
                    if !watch_only {
                        to_process.push((id, Some(rel)));
                    }
                }
            }
        }
    }

    // jobs left over from earlier cycles (Detected / Failed) are retried
    for j in &state.jobs {
        if matches!(j.status, JobStatus::Detected | JobStatus::Failed) && !to_process.iter().any(|(id, _)| *id == j.id) {
            to_process.push((j.id.clone(), None));
        }
    }

    // 2) process
    for (id, rel) in to_process {
        let Some(idx) = state.jobs.iter().position(|j| j.id == id) else { continue };
        let upstream = cfg.upstreams.iter().find(|u| u.name == state.jobs[idx].upstream).cloned();
        let Some(upstream) = upstream else { continue };
        if attempts(&state.jobs[idx]) >= MAX_ATTEMPTS {
            let j = &mut state.jobs[idx];
            j.status = JobStatus::Blocked;
            log(j, format!("giving up after {MAX_ATTEMPTS} attempts"));
            continue;
        }
        let rel = match rel {
            Some(r) => Some(r),
            None => gh
                .releases(&upstream.repo, &upstream.kind)
                .await
                .ok()
                .and_then(|rs| rs.into_iter().find(|r| parse_tag(&r.tag, upstream.tag_prefix.as_deref()).map(|v| v.text) == Some(state.jobs[idx].to.clone()))),
        };
        let Some(rel) = rel else {
            let j = &mut state.jobs[idx];
            log(j, "release not available right now; will retry");
            continue;
        };
        let line = process_job(cfg, state, idx, &upstream, rel).await;
        out.push(line);
    }
    state.trim();
    out
}

async fn process_job(cfg: &Config, state: &mut State, idx: usize, upstream: &Upstream, release: Release) -> String {
    let today = now_rfc3339()[..10].to_string();
    let merges_today = state.merges_today(&today);
    let (from, to, branch, id) = {
        let j = &mut state.jobs[idx];
        log(j, "attempt started");
        (j.from.clone(), j.to.clone(), j.branch.clone(), j.id.clone())
    };

    // context (best effort)
    let mut hits = Vec::new();
    let mut ai = None;
    let mut notes = Vec::new();
    if let Some(s) = &cfg.search {
        match search(&s.url, &format!("{} {} release notes changelog", upstream.name, to), 5).await {
            Ok(h) => hits = h,
            Err(e) => notes.push(format!("search unavailable: {e}")),
        }
    }
    if let Some(l) = &cfg.llm {
        let body: String = release.body.chars().take(1500).collect();
        match summarize(&l.url, &format!("Summarize this software release in two sentences.\n{body}\nSummary:")).await {
            Ok(a) => ai = a,
            Err(e) => notes.push(format!("llm unavailable: {e}")),
        }
    }
    let (assessment, assess_note) = assess(&release.body, cfg.llm.as_ref().map(|l| l.url.as_str())).await;
    if let Some(n) = assess_note {
        notes.push(n);
    }
    let ctx = JobContext { upstream: upstream.clone(), from, to: to.clone(), release, hits, ai_summary: ai, assessment, generated_on: today };
    let plan = render_plan(&ctx);
    {
        let j = &mut state.jobs[idx];
        j.summary = plan;
        for n in notes {
            log(j, n);
        }
    }

    if cfg.dry_run {
        let j = &mut state.jobs[idx];
        j.status = JobStatus::DryRun;
        log(j, "dry_run: plan generated, nothing written to the RPoem repository");
        return format!("{id}: dry-run plan generated");
    }

    let result = real_run(cfg, state, idx, &ctx, &branch, merges_today);
    let j = &mut state.jobs[idx];
    match result {
        Ok(()) => format!("{id}: {:?}", j.status),
        Err(e) => {
            j.status = JobStatus::Failed;
            log(j, format!("error: {e}"));
            format!("{id}: failed: {e}")
        }
    }
}

fn real_run(cfg: &Config, state: &mut State, idx: usize, ctx: &JobContext, branch: &str, merges_today: usize) -> anyhow::Result<()> {
    let r = cfg.target(&ctx.upstream.target).ok_or_else(|| anyhow::anyhow!("unknown target {}", ctx.upstream.target))?;
    gitops::prepare_branch(r, branch)?;
    run_porter(&cfg.porter, &r.clone_dir, ctx)?;
    let changed = gitops::changed_files(&r.clone_dir)?;
    let mut decision = decide(&changed, r, merges_today);
    // Extra holds, only ever towards "needs a human": a bigger bump than the pin allows, or release notes that look breaking.
    if decision == Decision::Merge {
        if let (Some(pin), Some(from)) = (&ctx.upstream.pin, &ctx.from) {
            if bump_kind(from, &ctx.to) > Bump::parse(&pin.max_auto_bump) {
                decision = Decision::Review(format!("a {:?} bump ({from} -> {}) is above max_auto_bump = {}", bump_kind(from, &ctx.to), ctx.to, pin.max_auto_bump));
            }
        }
        let touches_code = changed.iter().any(|f| !r.docs_paths.iter().any(|d| f.replace('\\', "/").starts_with(d.as_str())));
        if decision == Decision::Merge && touches_code && r.review_on_breaking && ctx.assessment.suspicious() {
            decision = Decision::Review("the release notes mention possible breaking changes (see the plan)".into());
        }
    }
    let message = format!("chore(upstream): {} {} (open-upstream-watch)", ctx.upstream.name, ctx.to);
    let j = &mut state.jobs[idx];
    log(j, format!("changed files: {}", if changed.is_empty() { "(none)".to_string() } else { changed.join(", ") }));

    match decision {
        Decision::Block(why) => {
            j.status = JobStatus::Blocked;
            log(j, format!("blocked: {why}"));
            Ok(())
        }
        Decision::Review(why) => {
            if !changed.is_empty() {
                gitops::commit_all(&r.clone_dir, &cfg.git, &message)?;
                if r.push_review_branch {
                    gitops::push_branch(r, branch)?;
                    log(j, format!("pushed branch {branch} for review"));
                }
            }
            j.status = JobStatus::NeedsReview;
            log(j, format!("needs review: {why}"));
            Ok(())
        }
        Decision::Merge => {
            let (ok, tail) = gitops::run_tests(&r.clone_dir, &r.test_cmd)?;
            if !ok {
                j.status = JobStatus::Blocked;
                log(j, format!("tests failed, not merged:\n{tail}"));
                return Ok(());
            }
            let sha = gitops::commit_all(&r.clone_dir, &cfg.git, &message)?;
            gitops::push_to_base(r)?;
            j.status = JobStatus::Merged;
            j.merged_commit = Some(sha.clone());
            log(j, format!("merged into {} as {}", r.base_branch, &sha[..sha.len().min(10)]));
            state.merges.push(now_rfc3339());
            Ok(())
        }
    }
}
