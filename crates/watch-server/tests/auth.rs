//! Login flow tests: the one-time password is requested/verified through a mock of open-easy-web.

use poem::http::StatusCode;
use poem::test::TestClient;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use watch_core::config::Config;
use watch_core::github::GithubClient;
use watch_core::State;
use watch_server::{app, AppState};

type Log = Arc<Mutex<Vec<(String, String)>>>;

/// Mock open-easy-web: code 123456 logs in as owner@example.org, "999999" as another account, "totp" asks for TOTP.
async fn mock_otp() -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let l2 = log.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { break };
            let log = l2.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = text.split_whitespace().nth(1).unwrap_or("").to_string();
                let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                log.lock().unwrap().push((path.clone(), body.clone()));
                let (status, out) = if path.ends_with("/request-otp") {
                    (200, json!({"message_en": "sent"}))
                } else if body.contains("123456") {
                    (200, json!({"token": "t", "email": "owner@example.org"}))
                } else if body.contains("999999") {
                    (200, json!({"token": "t", "email": "someone@else.org"}))
                } else if body.contains("totp") {
                    (200, json!({"totp_required": true}))
                } else {
                    (401, json!({"error": "bad code"}))
                };
                let b = out.to_string();
                let resp = format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", b.len(), b);
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    (format!("http://{addr}"), log)
}

fn make_app(otp: &str, dist: &std::path::Path) -> Arc<AppState> {
    let text = format!(
        r#"
state_file = "{state}"
ui_dist = "{dist}"
dry_run = true
[auth]
allowed_emails = ["owner@example.org"]
otp_base_url = "{otp}"
[[upstream]]
name = "poem"
repo = "poem-web/poem"
"#,
        state = dist.join("state.json").to_string_lossy().replace('\\', "/"),
        dist = dist.to_string_lossy().replace('\\', "/"),
    );
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("c.toml");
    std::fs::write(&p, text).unwrap();
    let cfg = Config::load(&p).unwrap();
    AppState::new(cfg, State::default(), GithubClient::new("http://127.0.0.1:1", None).unwrap())
}

fn cookie_of(resp: &poem::test::TestResponse) -> Option<String> {
    resp.0.headers().get_all("set-cookie").iter().find_map(|v| v.to_str().ok().map(|s| s.split(';').next().unwrap().to_string())).filter(|c| c.starts_with("uw_session="))
}

#[tokio::test]
async fn the_dashboard_is_locked_until_the_one_time_password_is_verified() {
    let (otp, log) = mock_otp().await;
    let dist = tempfile::tempdir().unwrap();
    std::fs::write(dist.path().join("index.html"), "<html>dashboard</html>").unwrap();
    let cli = TestClient::new(app(make_app(&otp, dist.path())));

    // locked
    let r = cli.get("/leptos-rs/api/status").send().await;
    r.assert_status(StatusCode::UNAUTHORIZED);
    let r = cli.get("/leptos-rs/api/session").send().await;
    r.assert_status_is_ok();
    assert_eq!(r.json().await.value().object().get("authenticated").bool(), false);

    // an address that is not on the list: same generic answer, and nothing is forwarded
    let r = cli.post("/leptos-rs/api/login/request").body_json(&json!({"email": "intruder@example.org"})).send().await;
    r.assert_status_is_ok();
    assert!(log.lock().unwrap().is_empty(), "a non-allowed address must never trigger a mail");

    // the allowed address: the request is forwarded to open-easy-web
    let r = cli.post("/leptos-rs/api/login/request").body_json(&json!({"email": "OWNER@example.org"})).send().await;
    r.assert_status_is_ok();
    assert!(log.lock().unwrap().iter().any(|(p, b)| p.ends_with("/api/auth/request-otp") && b.contains("owner@example.org")));

    // wrong code
    let r = cli.post("/leptos-rs/api/login/verify").body_json(&json!({"email": "owner@example.org", "code": "000000"})).send().await;
    r.assert_status(StatusCode::UNAUTHORIZED);
    assert!(cookie_of(&r).is_none());

    // an account that open-easy-web accepts but that is not on our list
    let r = cli.post("/leptos-rs/api/login/verify").body_json(&json!({"email": "owner@example.org", "code": "999999"})).send().await;
    r.assert_status(StatusCode::UNAUTHORIZED);

    // TOTP requirement is passed through (no session yet)
    let r = cli.post("/leptos-rs/api/login/verify").body_json(&json!({"email": "owner@example.org", "code": "totp"})).send().await;
    r.assert_status_is_ok();
    assert!(cookie_of(&r).is_none());
    assert_eq!(r.json().await.value().object().get("totp_required").bool(), true);

    // right code
    let r = cli.post("/leptos-rs/api/login/verify").body_json(&json!({"email": "owner@example.org", "code": "123456"})).send().await;
    r.assert_status_is_ok();
    let raw = r.0.headers().get("set-cookie").unwrap().to_str().unwrap().to_ascii_lowercase();
    assert!(raw.contains("httponly") && raw.contains("samesite=strict") && raw.contains("path=/leptos-rs") && raw.contains("secure"), "{raw}");
    let cookie = cookie_of(&r).expect("session cookie");

    // unlocked
    let r = cli.get("/leptos-rs/api/status").header("Cookie", &cookie).send().await;
    r.assert_status_is_ok();
    let v: Value = serde_json::from_str(&r.0.into_body().into_string().await.unwrap()).unwrap();
    assert_eq!(v["upstreams"][0]["name"], "poem");
    assert_eq!(v["config"]["dry_run"], true);

    // logout locks it again
    let r = cli.post("/leptos-rs/api/logout").header("Cookie", &cookie).send().await;
    r.assert_status_is_ok();
    let r = cli.get("/leptos-rs/api/status").header("Cookie", &cookie).send().await;
    r.assert_status(StatusCode::UNAUTHORIZED);

    // check-now also needs the session
    let r = cli.post("/leptos-rs/api/check-now").send().await;
    r.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_attempts_are_rate_limited() {
    let (otp, _log) = mock_otp().await;
    let dist = tempfile::tempdir().unwrap();
    let cli = TestClient::new(app(make_app(&otp, dist.path())));
    let mut last = StatusCode::OK;
    for _ in 0..12 {
        last = cli.post("/leptos-rs/api/login/request").body_json(&json!({"email": "x@y.z"})).send().await.0.status();
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn demo_and_static_files_are_public() {
    let (otp, _log) = mock_otp().await;
    let dist = tempfile::tempdir().unwrap();
    std::fs::write(dist.path().join("index.html"), "<html>dashboard</html>").unwrap();
    std::fs::write(dist.path().join("demo-status.json"), "{\"sample\":true}").unwrap();
    let cli = TestClient::new(app(make_app(&otp, dist.path())));
    for p in ["/leptos-rs/demo", "/leptos-rs/demo/", "/leptos-rs/", "/leptos-rs/index.html"] {
        let r = cli.get(p).send().await;
        r.assert_status_is_ok();
        assert!(r.0.into_body().into_string().await.unwrap().contains("dashboard"), "{p}");
    }
    let r = cli.get("/leptos-rs/demo-status.json").send().await;
    r.assert_status_is_ok();
    // nothing outside the base path is served
    cli.get("/api/status").send().await.assert_status(StatusCode::NOT_FOUND);
}
