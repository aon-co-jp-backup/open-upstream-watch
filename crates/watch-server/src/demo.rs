//! `--make-demo <path>`: builds the public demo data. Version numbers come from the real GitHub API at that moment;
//! the job history is SAMPLE data and says so on every job.

use watch_core::config::Config;
use watch_core::github::{latest, GithubClient};
use watch_core::pipeline::now_rfc3339;
use watch_types::{ConfigView, Job, JobStatus, StatusResponse, UpstreamView};

pub async fn make_demo(cfg: &Config, gh: &GithubClient) -> StatusResponse {
    let now = now_rfc3339();
    let mut ups = Vec::new();
    let mut versions = Vec::new();
    for u in &cfg.upstreams {
        let mut view = UpstreamView {
            name: u.name.clone(),
            repo: u.repo.clone(),
            tag_prefix: u.tag_prefix.clone(),
            area: if u.area.is_empty() { u.target.clone() } else { u.area.clone() },
            baseline: None,
            last_seen: None,
            last_checked: Some(now.clone()),
            last_error: None,
            health: "ok".into(),
            mode: u.mode.clone(),
        };
        match gh.releases(&u.repo, &u.kind).await {
            Ok(rs) => {
                if let Some((v, rel)) = latest(&rs, u.tag_prefix.as_deref()) {
                    view.baseline = Some(v.text.clone());
                    view.last_seen = Some(v.text.clone());
                    versions.push((u.name.clone(), v.text, rel.url));
                }
            }
            Err(e) => view.last_error = Some(e.to_string()),
        }
        ups.push(view);
    }

    let sample = |upstream: &str, from: &str, to: &str, status: JobStatus, url: &str, summary: &str, log: &[&str], commit: Option<&str>| Job {
        id: format!("{upstream}-{to}"),
        upstream: upstream.into(),
        from: Some(from.into()),
        to: to.into(),
        status,
        created: now.clone(),
        updated: now.clone(),
        branch: format!("upstream/{upstream}-{to}"),
        release_url: url.into(),
        summary: format!("SAMPLE DATA — not a real job.\n\n{summary}"),
        log: log.iter().map(|l| format!("SAMPLE {l}")).collect(),
        merged_commit: commit.map(str::to_string),
    };
    let v = |n: &str| versions.iter().find(|(x, _, _)| x == n).cloned();
    let mut jobs = Vec::new();
    if let Some((_, to, url)) = v("poem") {
        jobs.push(sample("poem", "(previous)", &to, JobStatus::Merged, &url, "# poem — porting plan (sample)\n\nA docs-only plan under docs/upstream/ passed the gate and was merged.", &["detected", "plan written", "changed files: docs/upstream/poem/…md", "merged into main"], Some("0123456789abcdef")));
    }
    if let Some((_, to, url)) = v("leptos") {
        jobs.push(sample("leptos", "(previous)", &to, JobStatus::NeedsReview, &url, "# leptos — pin update (sample)\n\nThe release notes mention possible breaking changes, so the template pin was prepared on a branch for review instead of being merged.", &["detected", "pin src/catalog.ts updated", "needs review: the release notes mention possible breaking changes"], None));
    }
    if let Some((_, to, url)) = v("trunk") {
        jobs.push(sample("trunk", "(previous)", &to, JobStatus::Notified, &url, "Watch-only upstream: the new version is recorded, nothing is ported.", &["detected (watch only)"], None));
    }
    jobs.reverse();

    StatusResponse {
        generated_at: now,
        config: ConfigView {
            dry_run: true,
            auto_merge: true,
            allow_code_merge: false,
            poll_interval_hours: cfg.poll_interval_hours,
            porter: cfg.porter.kind.clone(),
            search_enabled: true,
            llm_enabled: true,
            check_now_enabled: false,
            alert_enabled: true,
        },
        upstreams: ups,
        jobs,
        running: false,
        demo: true,
    }
}
