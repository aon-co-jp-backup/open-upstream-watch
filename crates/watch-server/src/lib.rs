//! HTTP side of open-upstream-watch.
//!
//! Everything lives under `base_path` (default `/leptos-rs`), so the same routes work locally and behind the site's
//! reverse proxy.  The dashboard is protected by an e-mail one-time password: the code is requested and verified
//! through the existing open-easy-web server (same account, same SMTP), and only addresses on the allow-list get in.
//! The demo page is public and serves sample data from a static file.

use poem::http::StatusCode;
use poem::middleware::Tracing;
use poem::middleware::CookieJarManager;
use poem::web::cookie::{Cookie, CookieJar, SameSite};
use poem::web::{Data, Json};
use poem::{get, handler, post, EndpointExt, IntoResponse, Response, Route};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use watch_core::config::Config;
use watch_core::github::GithubClient;
use watch_core::notify::{AnyNotifier, Notifier};
use watch_core::pipeline::{now_rfc3339, run_cycle};
use watch_core::State;
use watch_types::{ConfigView, StatusResponse, UpstreamView};

const COOKIE: &str = "uw_session";

pub struct AppState {
    pub cfg: Config,
    pub state: Mutex<State>,
    pub running: AtomicBool,
    pub notifier: AnyNotifier,
    sessions: Mutex<HashMap<String, Instant>>,
    attempts: Mutex<Vec<Instant>>,
    http: reqwest::Client,
    pub github: GithubClient,
}

impl AppState {
    pub fn new(cfg: Config, state: State, github: GithubClient) -> Arc<AppState> {
        let notifier = AnyNotifier::from_config(&cfg);
        Arc::new(AppState {
            cfg,
            state: Mutex::new(state),
            running: AtomicBool::new(false),
            notifier,
            sessions: Mutex::new(HashMap::new()),
            attempts: Mutex::new(Vec::new()),
            http: reqwest::Client::builder().timeout(Duration::from_secs(20)).build().expect("http client"),
            github,
        })
    }

    fn allowed(&self, email: &str) -> bool {
        self.canonical(email).is_some()
    }

    /// The allow-list entry matching the typed address (that spelling is what open-easy-web gets).
    fn canonical(&self, email: &str) -> Option<String> {
        self.cfg.auth.allowed_emails.iter().find(|a| a.eq_ignore_ascii_case(email.trim())).cloned()
    }

    /// At most `max` login attempts per 10 minutes, for requests and for verifications separately kept by the caller.
    fn rate_ok(&self, max: usize) -> bool {
        let mut a = self.attempts.lock().unwrap();
        let cutoff = Instant::now() - Duration::from_secs(600);
        a.retain(|t| *t > cutoff);
        if a.len() >= max {
            return false;
        }
        a.push(Instant::now());
        true
    }

    fn new_session(&self) -> String {
        use rand::RngCore;
        let mut b = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut b);
        let token: String = b.iter().map(|x| format!("{x:02x}")).collect();
        let mut s = self.sessions.lock().unwrap();
        let now = Instant::now();
        s.retain(|_, exp| *exp > now);
        s.insert(token.clone(), now + Duration::from_secs(self.cfg.auth.session_hours * 3600));
        token
    }

    fn authed(&self, jar: &CookieJar) -> bool {
        let Some(c) = jar.get(COOKIE) else { return false };
        let token = c.value_str().to_string();
        let mut s = self.sessions.lock().unwrap();
        match s.get(&token) {
            Some(exp) if *exp > Instant::now() => true,
            Some(_) => {
                s.remove(&token);
                false
            }
            None => false,
        }
    }

    pub fn view(&self) -> StatusResponse {
        let st = self.state.lock().unwrap();
        let upstreams = self
            .cfg
            .upstreams
            .iter()
            .map(|u| {
                let s = st.upstreams.get(&u.name).cloned().unwrap_or_default();
                UpstreamView {
                    name: u.name.clone(),
                    repo: u.repo.clone(),
                    tag_prefix: u.tag_prefix.clone(),
                    area: u.area.clone(),
                    baseline: s.baseline,
                    last_seen: s.last_seen,
                    last_checked: s.last_checked,
                    last_error: s.last_error,
                    health: if s.health.is_empty() { "ok".into() } else { s.health },
                    mode: u.mode.clone(),
                }
            })
            .collect();
        let jobs = st.jobs.iter().rev().take(50).cloned().collect();
        StatusResponse {
            generated_at: now_rfc3339(),
            config: ConfigView {
                dry_run: self.cfg.dry_run,
                auto_merge: self.cfg.rpoem.auto_merge,
                allow_code_merge: self.cfg.rpoem.allow_code_merge,
                poll_interval_hours: self.cfg.poll_interval_hours,
                porter: self.cfg.porter.kind.clone(),
                search_enabled: self.cfg.search.is_some(),
                llm_enabled: self.cfg.llm.is_some(),
                check_now_enabled: true,
                alert_enabled: self.notifier.is_mail(),
            },
            upstreams,
            jobs,
            running: self.running.load(Ordering::SeqCst),
            demo: false,
        }
    }

    /// Runs one cycle unless one is already running. Returns the log lines (empty when skipped).
    pub async fn cycle(&self) -> Vec<String> {
        if self.running.swap(true, Ordering::SeqCst) {
            return vec![];
        }
        let mut st = self.state.lock().unwrap().clone();
        let lines = run_cycle(&self.cfg, &mut st, &self.github, &self.notifier).await;
        if let Err(e) = st.save(&self.cfg.state_file) {
            tracing::error!("saving state failed: {e}");
        }
        *self.state.lock().unwrap() = st;
        self.running.store(false, Ordering::SeqCst);
        lines
    }
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "ok": false, "error": msg }))).into_response()
}

#[handler]
fn session(Data(s): Data<&Arc<AppState>>, jar: &CookieJar) -> Json<Value> {
    Json(json!({ "authenticated": s.authed(jar), "login_available": !s.cfg.auth.allowed_emails.is_empty() }))
}

#[derive(Deserialize)]
struct EmailReq {
    email: String,
}

#[handler]
async fn login_request(Data(s): Data<&Arc<AppState>>, Json(req): Json<EmailReq>) -> Response {
    if !s.rate_ok(8) {
        return err(StatusCode::TOO_MANY_REQUESTS, "too many attempts, try again later");
    }
    // The answer never reveals whether the address is allowed.
    if let Some(contact) = s.canonical(&req.email) {
        let url = format!("{}/api/auth/request-otp", s.cfg.auth.otp_base_url.trim_end_matches('/'));
        match s.http.post(url).json(&json!({ "contact": contact })).send().await {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => tracing::warn!("open-easy-web request-otp answered {}", r.status()),
            Err(e) => {
                tracing::warn!("open-easy-web request-otp failed: {e}");
                return err(StatusCode::BAD_GATEWAY, "the mail service is unavailable");
            }
        }
    }
    Json(json!({ "ok": true, "message": "If the address is allowed, a one-time password was sent. / 許可されたアドレスであれば、ワンタイムパスワードを送信しました。" })).into_response()
}

#[derive(Deserialize)]
struct VerifyReq {
    email: String,
    code: String,
    #[serde(default)]
    totp_code: Option<String>,
}

#[handler]
async fn login_verify(Data(s): Data<&Arc<AppState>>, jar: &CookieJar, Json(req): Json<VerifyReq>) -> Response {
    if !s.rate_ok(15) {
        return err(StatusCode::TOO_MANY_REQUESTS, "too many attempts, try again later");
    }
    let Some(contact) = s.canonical(&req.email) else {
        return err(StatusCode::UNAUTHORIZED, "invalid or expired code");
    };
    let url = format!("{}/api/auth/verify-otp", s.cfg.auth.otp_base_url.trim_end_matches('/'));
    let mut body = json!({ "contact": contact, "code": req.code.trim() });
    if let Some(t) = &req.totp_code {
        body["totp_code"] = json!(t.trim());
    }
    let resp = match s.http.post(url).json(&body).send().await {
        Ok(r) => r,
        Err(_) => return err(StatusCode::BAD_GATEWAY, "the mail service is unavailable"),
    };
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return err(StatusCode::UNAUTHORIZED, "invalid or expired code");
    }
    if v.get("totp_required").and_then(Value::as_bool).unwrap_or(false) {
        return (StatusCode::OK, Json(json!({ "ok": false, "totp_required": true }))).into_response();
    }
    // open-easy-web names the account; it must be on our allow-list too (defence in depth)
    let account = v.get("email").and_then(Value::as_str).unwrap_or("");
    if v.get("token").and_then(Value::as_str).is_none() || !s.allowed(account) {
        return err(StatusCode::UNAUTHORIZED, "this login mode is not supported or the account is not allowed");
    }
    let token = s.new_session();
    let mut c = Cookie::new_with_str(COOKIE, token);
    c.set_path(s.cfg.base_path.clone());
    c.set_http_only(true);
    c.set_secure(s.cfg.auth.cookie_secure);
    c.set_same_site(SameSite::Strict);
    c.set_max_age(Duration::from_secs(s.cfg.auth.session_hours * 3600));
    jar.add(c);
    Json(json!({ "ok": true })).into_response()
}

#[handler]
fn logout(Data(s): Data<&Arc<AppState>>, jar: &CookieJar) -> Json<Value> {
    if let Some(c) = jar.get(COOKIE) {
        s.sessions.lock().unwrap().remove(c.value_str());
    }
    jar.remove(COOKIE);
    Json(json!({ "ok": true }))
}

#[handler]
fn api_status(Data(s): Data<&Arc<AppState>>, jar: &CookieJar) -> Response {
    if !s.authed(jar) {
        return err(StatusCode::UNAUTHORIZED, "login required");
    }
    Json(s.view()).into_response()
}

#[handler]
async fn check_now(Data(s): Data<&Arc<AppState>>, jar: &CookieJar) -> Response {
    if !s.authed(jar) {
        return err(StatusCode::UNAUTHORIZED, "login required");
    }
    if s.running.load(Ordering::SeqCst) {
        return err(StatusCode::CONFLICT, "a cycle is already running");
    }
    let s2 = Arc::clone(s);
    tokio::spawn(async move {
        for l in s2.cycle().await {
            tracing::info!("{l}");
        }
    });
    (StatusCode::ACCEPTED, Json(json!({ "ok": true, "started": true }))).into_response()
}

/// `/demo` and `/demo/` serve the dashboard page itself; the page switches to its sample data by the URL.
#[handler]
fn demo_page(Data(s): Data<&Arc<AppState>>) -> Response {
    match std::fs::read_to_string(s.cfg.ui_dist.join("index.html")) {
        Ok(html) => Response::builder().content_type("text/html; charset=utf-8").body(html),
        Err(_) => err(StatusCode::NOT_FOUND, "dashboard is not built (run trunk build in ui/)"),
    }
}

pub fn app(state: Arc<AppState>) -> impl poem::Endpoint {
    let base = state.cfg.base_path.clone();
    let dist = state.cfg.ui_dist.clone();
    let api = Route::new()
        .at("/session", get(session))
        .at("/login/request", post(login_request))
        .at("/login/verify", post(login_verify))
        .at("/logout", post(logout))
        .at("/status", get(api_status))
        .at("/check-now", post(check_now));
    let inner = Route::new()
        .nest("/api", api)
        .at("/demo", get(demo_page))
        .at("/demo/", get(demo_page))
        .nest("/", poem::endpoint::StaticFilesEndpoint::new(dist).index_file("index.html"));
    Route::new().nest(base, inner).data(state).with(CookieJarManager::new()).with(Tracing)
}

/// Sends a test alert (used by `--test-alert`): proves the SMTP path end to end.
pub async fn send_test_alert(state: &AppState) -> anyhow::Result<()> {
    state
        .notifier
        .notify("[open-upstream-watch] test alert", "This is a test alert from open-upstream-watch. If you can read it, alert mail works.\nこれはテスト通知です。")
        .await
}
