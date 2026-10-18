//! End-to-end tests of a polling cycle against a mock GitHub API and a real (local, bare) git remote.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use watch_core::config::Config;
use watch_core::github::GithubClient;
use watch_core::notify::Recorder;
use watch_core::pipeline::run_cycle;
use watch_core::State;
use watch_types::JobStatus;

type Repo = Arc<Mutex<(u16, String)>>;

fn default_repo() -> Repo {
    Arc::new(Mutex::new((200, r#"{"full_name":"poem-web/poem","archived":false}"#.to_string())))
}

/// Minimal HTTP server: `/repos/<o>/<r>` answers with the repo state, anything else (the releases list) with `body`.
async fn mock_github(body: Arc<Mutex<String>>, repo: Repo) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { break };
            let body = body.clone();
            let repo = repo.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                let (status, b) = if path.contains("/releases") || path.contains("/tags") {
                    (200u16, body.lock().unwrap().clone())
                } else {
                    repo.lock().unwrap().clone()
                };
                let reason = if status == 200 { "OK" } else { "Not Found" };
                let resp = format!("HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", b.len(), b);
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

fn releases(tag: &str, notes: &str) -> String {
    serde_json::json!([{ "tag_name": tag, "name": tag, "body": notes, "html_url": format!("https://example.org/{tag}"), "draft": false, "prerelease": false, "published_at": "2026-10-19T00:00:00Z" },
        { "tag_name": "v0.0.1", "name": "old", "body": "", "html_url": "", "draft": false, "prerelease": false }])
    .to_string()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A bare "RPoem" remote with one commit on main, plus a work area for the bot.
fn make_remote(root: &Path) -> PathBuf {
    let bare = root.join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    let seed = root.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "-q", "-b", "main"]);
    std::fs::write(seed.join("README.md"), "RPoem\n").unwrap();
    git(&seed, &["add", "-A"]);
    git(&seed, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init"]);
    git(&seed, &["push", "-q", bare.to_str().unwrap(), "main"]);
    bare
}

fn config(root: &Path, remote: &Path, extra: &str) -> Config {
    let text = format!(
        r#"
poll_interval_hours = 1
dry_run = false
state_file = "{state}"
[rpoem]
remote = "{remote}"
clone_dir = "{clone}"
test_cmd = "git --version"
[[upstream]]
name = "poem"
repo = "poem-web/poem"
tag_prefix = "v"
area = "crates/open-runo-poem-compat"
{extra}
"#,
        state = root.join("state.json").to_string_lossy().replace('\\', "/"),
        remote = remote.to_string_lossy().replace('\\', "/"),
        clone = root.join("clone").to_string_lossy().replace('\\', "/"),
    );
    let c: Config = toml_from(&text);
    c.validate().unwrap();
    c
}

fn toml_from(text: &str) -> Config {
    // Config derives Deserialize; parse through the public loader to exercise validation too.
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("c.toml");
    std::fs::write(&p, text).unwrap();
    Config::load(&p).unwrap()
}

fn has_python() -> bool {
    Command::new("python").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

#[tokio::test]
async fn first_run_only_records_a_baseline() {
    let body = Arc::new(Mutex::new(releases("v3.1.0", "x")));
    let base = mock_github(body.clone(), default_repo()).await;
    let root = tempfile::tempdir().unwrap();
    let remote = make_remote(root.path());
    let cfg = config(root.path(), &remote, "");
    let gh = GithubClient::new(&base, None).unwrap();
    let mut st = State::default();
    run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    assert!(st.jobs.is_empty());
    assert_eq!(st.upstreams["poem"].baseline.as_deref(), Some("3.1.0"));
    // same version again: still nothing
    run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    assert!(st.jobs.is_empty());
}

#[tokio::test]
async fn dry_run_generates_a_plan_and_touches_no_repository() {
    let body = Arc::new(Mutex::new(releases("v3.1.0", "x")));
    let base = mock_github(body.clone(), default_repo()).await;
    let root = tempfile::tempdir().unwrap();
    let remote = make_remote(root.path());
    let mut cfg = config(root.path(), &remote, "");
    cfg.dry_run = true;
    let gh = GithubClient::new(&base, None).unwrap();
    let mut st = State::default();
    run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    *body.lock().unwrap() = releases("v3.2.0", "Adds things\n```\n# not a heading\n```");
    run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    assert_eq!(st.jobs.len(), 1);
    let j = &st.jobs[0];
    assert_eq!(j.status, JobStatus::DryRun);
    assert_eq!(j.to, "3.2.0");
    assert!(j.summary.contains("poem 3.2.0"));
    assert!(!root.path().join("clone").exists(), "dry run must not clone anything");
    // the same version is not turned into a second job
    run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    assert_eq!(st.jobs.len(), 1);
}

#[tokio::test]
async fn docs_only_change_is_merged_into_main() {
    let body = Arc::new(Mutex::new(releases("v3.1.0", "x")));
    let base = mock_github(body.clone(), default_repo()).await;
    let root = tempfile::tempdir().unwrap();
    let remote = make_remote(root.path());
    let cfg = config(root.path(), &remote, "");
    let gh = GithubClient::new(&base, None).unwrap();
    let mut st = State::default();
    run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    *body.lock().unwrap() = releases("v3.2.0", "notes");
    let log = run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    assert_eq!(st.jobs[0].status, JobStatus::Merged, "log: {log:?} job: {:?}", st.jobs[0].log);
    let shown = git(&remote, &["show", "main:docs/upstream/poem/3.2.0.md"]);
    assert!(shown.contains("poem 3.2.0"));
    assert!(st.jobs[0].merged_commit.is_some());
    assert_eq!(st.merges.len(), 1);
}

#[tokio::test]
async fn failing_tests_block_the_merge() {
    let body = Arc::new(Mutex::new(releases("v3.1.0", "x")));
    let base = mock_github(body.clone(), default_repo()).await;
    let root = tempfile::tempdir().unwrap();
    let remote = make_remote(root.path());
    let mut cfg = config(root.path(), &remote, "");
    cfg.rpoem.test_cmd = "exit 1".into();
    let gh = GithubClient::new(&base, None).unwrap();
    let mut st = State::default();
    run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    *body.lock().unwrap() = releases("v3.2.0", "notes");
    run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    assert_eq!(st.jobs[0].status, JobStatus::Blocked);
    let tree = git(&remote, &["ls-tree", "-r", "--name-only", "main"]);
    assert!(!tree.contains("docs/upstream"), "nothing may reach main when tests fail");
}

#[tokio::test]
async fn code_changes_wait_for_review_and_foreign_paths_are_blocked() {
    if !has_python() {
        eprintln!("skip: python not available");
        return;
    }
    for (path, expect, on_main) in [("crates/x/lib.rs", JobStatus::NeedsReview, false), ("Cargo.toml", JobStatus::Blocked, false)] {
        let body = Arc::new(Mutex::new(releases("v3.1.0", "x")));
        let base = mock_github(body.clone(), default_repo()).await;
        let root = tempfile::tempdir().unwrap();
        let remote = make_remote(root.path());
        // a helper script avoids shell-quoting differences between cmd.exe and sh
        let script = root.path().join("touch.py");
        std::fs::write(&script, "import os, sys\np = sys.argv[1]\nos.makedirs(os.path.dirname(p) or '.', exist_ok=True)\nopen(p, 'w').write('x')\n").unwrap();
        let cmd = format!("python {} {}", script.to_string_lossy().replace('\\', "/"), path);
        let extra = format!("[porter]\nkind = \"command\"\ncommand = '''{cmd}'''\n");
        let cfg = config(root.path(), &remote, &extra);
        let gh = GithubClient::new(&base, None).unwrap();
        let mut st = State::default();
        run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
        *body.lock().unwrap() = releases("v3.2.0", "notes");
        run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
        assert_eq!(st.jobs[0].status, expect, "{path}: {:?}", st.jobs[0].log);
        let tree = git(&remote, &["ls-tree", "-r", "--name-only", "main"]);
        assert_eq!(tree.contains(path), on_main, "{path} must not be on main");
        if expect == JobStatus::NeedsReview {
            let branches = git(&remote, &["branch", "--list"]);
            assert!(branches.contains("upstream/poem-3.2.0"), "review branch should be pushed: {branches}");
        }
    }
}

#[tokio::test]
async fn daily_merge_limit_is_enforced() {
    let body = Arc::new(Mutex::new(releases("v3.1.0", "x")));
    let base = mock_github(body.clone(), default_repo()).await;
    let root = tempfile::tempdir().unwrap();
    let remote = make_remote(root.path());
    let mut cfg = config(root.path(), &remote, "");
    cfg.rpoem.max_merges_per_day = 0;
    let gh = GithubClient::new(&base, None).unwrap();
    let mut st = State::default();
    run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    *body.lock().unwrap() = releases("v3.2.0", "notes");
    run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    assert_eq!(st.jobs[0].status, JobStatus::NeedsReview);
}

#[tokio::test]
async fn a_disappeared_upstream_is_reported_once_after_two_confirmations() {
    let body = Arc::new(Mutex::new(releases("v3.1.0", "x")));
    let repo = default_repo();
    let base = mock_github(body.clone(), repo.clone()).await;
    let root = tempfile::tempdir().unwrap();
    let remote = make_remote(root.path());
    let mut cfg = config(root.path(), &remote, "");
    cfg.dry_run = true;
    let gh = GithubClient::new(&base, None).unwrap();
    let rec = Recorder::default();
    let mut st = State::default();
    run_cycle(&cfg, &mut st, &gh, &rec).await;
    assert!(rec.sent.lock().unwrap().is_empty());

    *repo.lock().unwrap() = (404, r#"{"message":"Not Found"}"#.to_string());
    run_cycle(&cfg, &mut st, &gh, &rec).await;
    assert!(rec.sent.lock().unwrap().is_empty(), "one 404 can be a blip: wait for a second confirmation");
    run_cycle(&cfg, &mut st, &gh, &rec).await;
    assert_eq!(rec.sent.lock().unwrap().len(), 1);
    let (subject, text) = rec.sent.lock().unwrap()[0].clone();
    assert!(subject.contains("poem") && subject.contains("missing"), "{subject}");
    assert!(text.contains("poem-web/poem"));
    assert_eq!(st.upstreams["poem"].health, "missing");
    run_cycle(&cfg, &mut st, &gh, &rec).await;
    assert_eq!(rec.sent.lock().unwrap().len(), 1, "the same problem is reported only once");

    // it comes back, then disappears again: a new alert
    *repo.lock().unwrap() = (200, r#"{"full_name":"poem-web/poem","archived":false}"#.to_string());
    run_cycle(&cfg, &mut st, &gh, &rec).await;
    assert_eq!(st.upstreams["poem"].health, "ok");
    *repo.lock().unwrap() = (404, "{}".to_string());
    run_cycle(&cfg, &mut st, &gh, &rec).await;
    run_cycle(&cfg, &mut st, &gh, &rec).await;
    assert_eq!(rec.sent.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn archived_and_renamed_upstreams_are_reported() {
    for (repo_body, expect) in [
        (r#"{"full_name":"poem-web/poem","archived":true}"#, "archived"),
        (r#"{"full_name":"new-org/poem","archived":false}"#, "renamed"),
    ] {
        let body = Arc::new(Mutex::new(releases("v3.1.0", "x")));
        let repo = Arc::new(Mutex::new((200u16, repo_body.to_string())));
        let base = mock_github(body.clone(), repo).await;
        let root = tempfile::tempdir().unwrap();
        let remote = make_remote(root.path());
        let mut cfg = config(root.path(), &remote, "");
        cfg.dry_run = true;
        let gh = GithubClient::new(&base, None).unwrap();
        let rec = Recorder::default();
        let mut st = State::default();
        let lines = run_cycle(&cfg, &mut st, &gh, &rec).await;
        assert_eq!(rec.sent.lock().unwrap().len(), 1, "{expect}: {lines:?} {:?}", st.upstreams);
        assert!(rec.sent.lock().unwrap()[0].0.contains(expect));
        assert_eq!(st.upstreams["poem"].health, expect);
    }
}

#[tokio::test]
async fn watch_only_upstreams_record_new_versions_without_porting() {
    let body = Arc::new(Mutex::new(releases("v0.7.0", "x")));
    let base = mock_github(body.clone(), default_repo()).await;
    let root = tempfile::tempdir().unwrap();
    let remote = make_remote(root.path());
    let cfg = config(root.path(), &remote, "").clone_with_mode("notify");
    let gh = GithubClient::new(&base, None).unwrap();
    let mut st = State::default();
    run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    *body.lock().unwrap() = releases("v0.8.0", "new");
    run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
    assert_eq!(st.jobs.len(), 1);
    assert_eq!(st.jobs[0].status, JobStatus::Notified);
    assert!(!root.path().join("clone").exists(), "a watch-only upstream never touches the RPoem repository");
}

trait WithMode {
    fn clone_with_mode(self, mode: &str) -> Config;
}

impl WithMode for Config {
    fn clone_with_mode(mut self, mode: &str) -> Config {
        self.upstreams[0].mode = mode.to_string();
        self
    }
}

fn make_template_remote(root: &Path) -> PathBuf {
    let bare = root.join("tpl.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    let seed = root.join("tplseed");
    std::fs::create_dir_all(seed.join("src")).unwrap();
    git(&seed, &["init", "-q", "-b", "main"]);
    std::fs::write(seed.join("src/catalog.ts"), "x\nleptos = { version = \"0.7\", features = [\"csr\"] }\ngloo-net = { version = \"0.6\" }\n").unwrap();
    git(&seed, &["add", "-A"]);
    git(&seed, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init"]);
    git(&seed, &["push", "-q", bare.to_str().unwrap(), "main"]);
    bare
}

#[tokio::test]
async fn template_pins_follow_the_bump_breaking_and_test_rules() {
    // (new tag, notes, test command, expected status, expect the pin on main to be 0.8)
    let cases = [
        ("v0.8.0", "Faster builds and fixes", "git --version", JobStatus::Merged, true),
        ("v0.8.0", "BREAKING: the view! macro changed", "git --version", JobStatus::NeedsReview, false),
        ("v1.0.0", "Stable release", "git --version", JobStatus::NeedsReview, false),
        ("v0.8.0", "Faster builds and fixes", "exit 1", JobStatus::Blocked, false),
        ("v0.7.9", "bug fixes", "git --version", JobStatus::NeedsReview, false), // 0.7 already covers 0.7.9: nothing to change
    ];
    for (tag, notes, test_cmd, expect, on_main) in cases {
        let body = Arc::new(Mutex::new(releases("v0.7.0", "x")));
        let base = mock_github(body.clone(), default_repo()).await;
        let root = tempfile::tempdir().unwrap();
        let tpl = make_template_remote(root.path());
        let text = format!(
            r#"
dry_run = false
state_file = "{state}"
[[target]]
name = "tpl"
remote = "{remote}"
clone_dir = "{clone}"
test_cmd = "{test_cmd}"
docs_paths = ["docs/"]
code_paths = ["src/catalog.ts"]
allow_code_merge = true
[[upstream]]
name = "leptos"
repo = "leptos-rs/leptos"
tag_prefix = "v"
target = "tpl"
[upstream.pin]
file = "src/catalog.ts"
pattern = 'leptos = \{{ version = "([0-9.]+)"'
max_auto_bump = "minor"
"#,
            state = root.path().join("state.json").to_string_lossy().replace('\\', "/"),
            remote = tpl.to_string_lossy().replace('\\', "/"),
            clone = root.path().join("clone").to_string_lossy().replace('\\', "/"),
        );
        let cfg = toml_from(&text);
        let gh = GithubClient::new(&base, None).unwrap();
        let mut st = State::default();
        run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
        *body.lock().unwrap() = releases(tag, notes);
        run_cycle(&cfg, &mut st, &gh, &Recorder::default()).await;
        let j = &st.jobs[0];
        assert_eq!(j.status, expect, "{tag} / {notes}: {:?}", j.log);
        let shown = git(&tpl, &["show", "main:src/catalog.ts"]);
        assert_eq!(shown.contains("version = \"0.8\""), on_main, "{tag}: main has\n{shown}");
        assert!(shown.contains("gloo-net = { version = \"0.6\" }"), "other pins must stay untouched");
    }
}

#[tokio::test]
async fn crates_io_upstreams_are_followed_and_reported_missing() {
    let body = Arc::new(Mutex::new("[]".to_string()));
    let repo = Arc::new(Mutex::new((200u16, r#"{"crate":{"max_stable_version":"3.1.0"}}"#.to_string())));
    let base = mock_github(body, repo.clone()).await;
    let root = tempfile::tempdir().unwrap();
    let remote = make_remote(root.path());
    let mut cfg = config(root.path(), &remote, "");
    cfg.dry_run = true;
    cfg.upstreams[0].kind = "crate".into();
    cfg.upstreams[0].repo = "poem".into();
    let gh = GithubClient::new("http://127.0.0.1:1", None).unwrap().with_crates_base(&base);
    let rec = Recorder::default();
    let mut st = State::default();
    run_cycle(&cfg, &mut st, &gh, &rec).await;
    assert_eq!(st.upstreams["poem"].baseline.as_deref(), Some("3.1.0"));
    *repo.lock().unwrap() = (200, r#"{"crate":{"max_stable_version":"3.2.0"}}"#.to_string());
    run_cycle(&cfg, &mut st, &gh, &rec).await;
    assert_eq!(st.jobs.len(), 1);
    assert_eq!(st.jobs[0].to, "3.2.0");
    assert_eq!(st.jobs[0].status, JobStatus::DryRun);
    // the crate disappears: reported after the second confirmation
    *repo.lock().unwrap() = (404, "{}".to_string());
    run_cycle(&cfg, &mut st, &gh, &rec).await;
    run_cycle(&cfg, &mut st, &gh, &rec).await;
    assert_eq!(rec.sent.lock().unwrap().len(), 1);
    assert!(rec.sent.lock().unwrap()[0].0.contains("missing"));
}
