//! open-upstream-watch dashboard (Leptos CSR). Login by e-mail one-time password; the public demo (`…/demo`) shows
//! sample data without any login.

use gloo_net::http::Request;
use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use watch_types::{Job, JobStatus, StatusResponse, UpstreamView};

/// `/leptos-rs/` (or `/` when served from the root), taken from the page URL so the same build works everywhere.
fn base() -> String {
    let path = web_sys::window().and_then(|w| w.location().pathname().ok()).unwrap_or_else(|| "/".into());
    match path.find("/leptos-rs/") {
        Some(i) => path[..i + "/leptos-rs/".len()].to_string(),
        None => "/".to_string(),
    }
}

fn is_demo() -> bool {
    let path = web_sys::window().and_then(|w| w.location().pathname().ok()).unwrap_or_default();
    let rest = path.strip_prefix(&base()).unwrap_or("");
    rest == "demo" || rest.starts_with("demo/")
}

#[derive(Clone, Copy, PartialEq)]
enum Lang {
    Ja,
    En,
}

fn tr(l: Lang, ja: &'static str, en: &'static str) -> &'static str {
    if l == Lang::Ja { ja } else { en }
}

#[derive(Clone, PartialEq)]
enum View {
    Loading,
    NeedLogin { available: bool },
    Dash(Box<StatusResponse>),
    Failed(String),
}

#[derive(Deserialize)]
struct SessionInfo {
    authenticated: bool,
    #[serde(default)]
    login_available: bool,
}

async fn load(set_view: WriteSignal<View>) {
    let b = base();
    if is_demo() {
        match Request::get(&format!("{b}demo-status.json")).send().await {
            Ok(r) if r.ok() => match r.json::<StatusResponse>().await {
                Ok(s) => set_view.set(View::Dash(Box::new(s))),
                Err(e) => set_view.set(View::Failed(e.to_string())),
            },
            _ => set_view.set(View::Failed("demo data not found".into())),
        }
        return;
    }
    let sess = match Request::get(&format!("{b}api/session")).send().await {
        Ok(r) => r.json::<SessionInfo>().await.ok(),
        Err(e) => {
            set_view.set(View::Failed(e.to_string()));
            return;
        }
    };
    let Some(sess) = sess else {
        set_view.set(View::Failed("the server answered something unexpected".into()));
        return;
    };
    if !sess.authenticated {
        set_view.set(View::NeedLogin { available: sess.login_available });
        return;
    }
    match Request::get(&format!("{b}api/status")).send().await {
        Ok(r) if r.ok() => match r.json::<StatusResponse>().await {
            Ok(s) => set_view.set(View::Dash(Box::new(s))),
            Err(e) => set_view.set(View::Failed(e.to_string())),
        },
        Ok(r) if r.status() == 401 => set_view.set(View::NeedLogin { available: true }),
        Ok(r) => set_view.set(View::Failed(format!("HTTP {}", r.status()))),
        Err(e) => set_view.set(View::Failed(e.to_string())),
    }
}

fn status_badge(l: Lang, s: JobStatus) -> (&'static str, &'static str) {
    match s {
        JobStatus::Merged => ("ok", tr(l, "反映済み", "merged")),
        JobStatus::DryRun => ("info", tr(l, "試走(書き込みなし)", "dry run")),
        JobStatus::NeedsReview => ("warn", tr(l, "要確認", "needs review")),
        JobStatus::Blocked => ("bad", tr(l, "拒否", "blocked")),
        JobStatus::Failed => ("bad", tr(l, "失敗(再試行)", "failed (retry)")),
        JobStatus::Detected => ("info", tr(l, "検出", "detected")),
        JobStatus::Notified => ("info", tr(l, "通知のみ", "watch only")),
    }
}

fn health_badge(l: Lang, h: &str) -> (&'static str, &'static str) {
    match h {
        "ok" => ("ok", tr(l, "正常", "ok")),
        "archived" => ("warn", tr(l, "アーカイブ済み", "archived")),
        "renamed" => ("warn", tr(l, "名称変更", "renamed")),
        "missing" => ("bad", tr(l, "消失", "missing")),
        _ => ("info", "?"),
    }
}

/// Writes a signal unless its owner (a component that was replaced meanwhile) is already gone; a plain `.set` would panic.
fn put<T: Send + Sync + 'static>(signal: WriteSignal<T>, value: T) {
    let _ = signal.try_set(value);
}

#[component]
fn Login(set_view: WriteSignal<View>, lang: ReadSignal<Lang>, available: bool) -> impl IntoView {
    let (email, set_email) = signal(String::new());
    let (code, set_code) = signal(String::new());
    let (totp, set_totp) = signal(String::new());
    let (sent, set_sent) = signal(false);
    let (need_totp, set_need_totp) = signal(false);
    let (msg, set_msg) = signal(String::new());
    let (busy, set_busy) = signal(false);

    let request = move |_| {
        put(set_busy, true);
        put(set_msg, String::new());
        let e = email.try_get().unwrap_or_default();
        spawn_local(async move {
            let body = serde_json::json!({ "email": e });
            let r = Request::post(&format!("{}api/login/request", base())).json(&body).unwrap().send().await;
            match r {
                Ok(r) if r.ok() => {
                    put(set_sent, true);
                    put(set_msg, tr(lang.get_untracked(), "許可されたアドレスであれば、コードをメールで送りました(5分間有効)。", "If the address is allowed, a code was sent by e-mail (valid for 5 minutes).").to_string());
                }
                Ok(r) if r.status() == 429 => put(set_msg, tr(lang.get_untracked(), "試行が多すぎます。しばらく待ってください。", "Too many attempts, wait a while.").to_string()),
                _ => put(set_msg, tr(lang.get_untracked(), "メールを送れませんでした。", "Could not send the mail.").to_string()),
            }
            put(set_busy, false);
        });
    };
    let verify = move |_| {
        put(set_busy, true);
        put(set_msg, String::new());
        let (e, c, t) = (email.try_get().unwrap_or_default(), code.try_get().unwrap_or_default(), totp.try_get().unwrap_or_default());
        spawn_local(async move {
            let mut body = serde_json::json!({ "email": e, "code": c });
            if !t.is_empty() {
                body["totp_code"] = serde_json::json!(t);
            }
            let r = Request::post(&format!("{}api/login/verify", base())).json(&body).unwrap().send().await;
            match r {
                Ok(r) if r.ok() => {
                    let v: serde_json::Value = r.json().await.unwrap_or_default();
                    if v.get("totp_required").and_then(|x| x.as_bool()).unwrap_or(false) {
                        put(set_need_totp, true);
                        put(set_msg, tr(lang.get_untracked(), "認証アプリの6桁コードも入力してください。", "Enter the 6-digit authenticator code too.").to_string());
                    } else {
                        load(set_view).await;
                    }
                }
                Ok(r) if r.status() == 429 => put(set_msg, tr(lang.get_untracked(), "試行が多すぎます。しばらく待ってください。", "Too many attempts, wait a while.").to_string()),
                _ => put(set_msg, tr(lang.get_untracked(), "コードが違うか、期限切れです。", "The code is wrong or expired.").to_string()),
            }
            put(set_busy, false);
        });
    };

    view! {
        <div class="card login">
            <h1>"open-upstream-watch"</h1>
            <p class="sub">{move || tr(lang.get(), "管理画面にはメールのワンタイムパスワードでログインします。", "Log in to the console with an e-mail one-time password.")}</p>
            {if !available {
                view! { <p class="msg bad">{move || tr(lang.get(), "ログインが設定されていません(許可メールが空です)。", "Login is not configured (no allowed e-mail).")}</p> }.into_any()
            } else {
                view! {
                    <div class="row">
                        <input type="email" placeholder="e-mail" autocomplete="username"
                            prop:value=move || email.try_get().unwrap_or_default()
                            on:input:target=move |ev| set_email.set(ev.target().value()) />
                        <button class="primary" disabled=move || busy.try_get().unwrap_or_default() || email.try_get().unwrap_or_default().is_empty() on:click=request>
                            {move || tr(lang.get(), "コードを送る", "Send code")}
                        </button>
                    </div>
                    <Show when=move || sent.try_get().unwrap_or_default()>
                        <div class="row">
                            <input inputmode="numeric" placeholder="123456" autocomplete="one-time-code"
                                prop:value=move || code.try_get().unwrap_or_default()
                                on:input:target=move |ev| set_code.set(ev.target().value()) />
                            <button class="primary" disabled=move || busy.try_get().unwrap_or_default() || code.try_get().unwrap_or_default().is_empty() on:click=verify>
                                {move || tr(lang.get(), "ログイン", "Log in")}
                            </button>
                        </div>
                        <Show when=move || need_totp.try_get().unwrap_or_default()>
                            <div class="row">
                                <input inputmode="numeric" placeholder="TOTP"
                                    prop:value=move || totp.try_get().unwrap_or_default()
                                    on:input:target=move |ev| set_totp.set(ev.target().value()) />
                            </div>
                        </Show>
                    </Show>
                    <p class="msg">{move || msg.try_get().unwrap_or_default()}</p>
                }.into_any()
            }}
        </div>
    }
}

#[component]
fn UpstreamRow(u: UpstreamView, lang: ReadSignal<Lang>) -> impl IntoView {
    let (hc, ht) = health_badge(lang.get_untracked(), &u.health);
    let version = match (&u.baseline, &u.last_seen) {
        (Some(b), Some(s)) if b != s => format!("{b} → {s}"),
        (_, Some(s)) => s.clone(),
        _ => "—".to_string(),
    };
    let mode = if u.mode == "notify" { tr(lang.get_untracked(), "監視のみ", "watch only") } else { tr(lang.get_untracked(), "Rust版を追従", "port to Rust") };
    let link = format!("https://github.com/{}", u.repo);
    view! {
        <tr>
            <td><strong>{u.name.clone()}</strong><br/><a href=link target="_blank" rel="noopener">{u.repo.clone()}</a></td>
            <td class="mono">{version}</td>
            <td><span class=format!("badge {hc}")>{ht}</span></td>
            <td>{mode}</td>
            <td class="mono">{u.last_checked.clone().unwrap_or_else(|| "—".into())}</td>
            <td class="mono">{u.area.clone()}{u.last_error.clone().map(|e| format!(" ⚠ {e}")).unwrap_or_default()}</td>
        </tr>
    }
}

#[component]
fn JobCard(j: Job, lang: ReadSignal<Lang>) -> impl IntoView {
    let (c, t) = status_badge(lang.get_untracked(), j.status);
    let title = format!("{} {} → {}", j.upstream, j.from.clone().unwrap_or_else(|| "?".into()), j.to);
    let release = j.release_url.clone();
    let has_summary = !j.summary.is_empty();
    view! {
        <div class="card">
            <div class="job-head">
                <strong class="mono">{title}</strong>
                <span class=format!("badge {c}")>{t}</span>
                <span class="grow"></span>
                <span class="mono">{j.updated.clone()}</span>
            </div>
            {(!release.is_empty()).then(|| view! { <a href=release target="_blank" rel="noopener">{tr(lang.get_untracked(), "リリースページ", "release page")}</a> })}
            {j.merged_commit.clone().map(|m| view! { <div class="mono">{format!("commit {}", &m[..m.len().min(10)])}</div> })}
            {has_summary.then(|| view! { <details><summary>{tr(lang.get_untracked(), "追従計画(自動生成)", "porting plan (generated)")}</summary><pre>{j.summary.clone()}</pre></details> })}
            <details><summary>{tr(lang.get_untracked(), "ログ", "log")}</summary><pre>{j.log.join("\n")}</pre></details>
        </div>
    }
}

#[component]
fn Dashboard(s: StatusResponse, set_view: WriteSignal<View>, lang: ReadSignal<Lang>) -> impl IntoView {
    let demo = s.demo || is_demo();
    let (busy, set_busy) = signal(s.running);
    let cfg = s.config.clone();
    let check = move |_| {
        put(set_busy, true);
        spawn_local(async move {
            let _ = Request::post(&format!("{}api/check-now", base())).send().await;
            TimeoutFuture::new(4000).await;
            load(set_view).await;
        });
    };
    let logout = move |_| {
        spawn_local(async move {
            let _ = Request::post(&format!("{}api/logout", base())).send().await;
            load(set_view).await;
        });
    };
    let jobs = s.jobs.clone();
    let ups = s.upstreams.clone();
    let generated = s.generated_at.clone();
    view! {
        <main>
            <div class="bar">
                <h1>"open-upstream-watch"</h1>
                <span class="grow"></span>
                <button on:click=move |_| lang_toggle(lang)>"日本語 / English"</button>
            </div>
            <p class="sub">{move || tr(lang.get(), "RPoemが互換実装している元のリリースを監視し、新版が出たらRust版の追従作業を準備します。", "Watches the upstreams RPoem re-implements and prepares the Rust-side work when a new version appears.")}</p>
            <div class="bar">
                <span class=format!("badge {}", if cfg.dry_run { "info" } else { "warn" })>{move || if cfg.dry_run { tr(lang.get(), "試走モード(書き込みなし)", "dry-run mode") } else { tr(lang.get(), "本番モード", "live mode") }}</span>
                <span class=format!("badge {}", if cfg.auto_merge { "ok" } else { "info" })>{move || if cfg.auto_merge { tr(lang.get(), "自動マージ ON(関門あり)", "auto-merge on (gated)") } else { tr(lang.get(), "自動マージ OFF", "auto-merge off") }}</span>
                <span class=format!("badge {}", if cfg.alert_enabled { "ok" } else { "info" })>{move || if cfg.alert_enabled { tr(lang.get(), "消失アラートのメール ON", "disappearance alert mail on") } else { tr(lang.get(), "アラートはログのみ", "alerts are logged only") }}</span>
                <span class="badge">{format!("every {} h", cfg.poll_interval_hours)}</span>
                <span class="grow"></span>
                {(!demo).then(|| view! {
                    <button class="primary" disabled=move || busy.try_get().unwrap_or_default() on:click=check>{move || if busy.try_get().unwrap_or_default() { tr(lang.get(), "確認中…", "checking…") } else { tr(lang.get(), "今すぐ確認", "Check now") }}</button>
                    <button on:click=logout>{move || tr(lang.get(), "ログアウト", "Log out")}</button>
                })}
            </div>
            <h2>{move || tr(lang.get(), "監視している元", "Watched upstreams")}</h2>
            <div class="card">
                <table>
                    <thead><tr><th>{move || tr(lang.get(), "名前", "name")}</th><th>{move || tr(lang.get(), "版", "version")}</th><th>{move || tr(lang.get(), "状態", "health")}</th><th>{move || tr(lang.get(), "方式", "mode")}</th><th>{move || tr(lang.get(), "最終確認", "last check")}</th><th>"RPoem / target"</th></tr></thead>
                    <tbody>
                        {ups.into_iter().map(|u| view! { <UpstreamRow u=u lang=lang /> }).collect_view()}
                    </tbody>
                </table>
            </div>
            <h2>{move || tr(lang.get(), "ジョブ(新しい順)", "Jobs (newest first)")}</h2>
            {if jobs.is_empty() {
                view! { <p class="empty">{move || tr(lang.get(), "まだジョブはありません(最初の確認は基準版を記録するだけです)。", "No jobs yet (the first check only records a baseline).")}</p> }.into_any()
            } else {
                jobs.into_iter().map(|j| view! { <JobCard j=j lang=lang /> }).collect_view().into_any()
            }}
            <p class="sub mono">{generated}</p>
        </main>
    }
}

fn lang_toggle(lang: ReadSignal<Lang>) {
    // the signal's writer lives in App; toggling goes through a custom event kept simple: reload with ?lang
    let next = if lang.get_untracked() == Lang::Ja { "en" } else { "ja" };
    if let Some(w) = web_sys::window() {
        let _ = w.location().set_hash(next);
        let _ = w.location().reload();
    }
}

#[component]
fn App() -> impl IntoView {
    let hash = web_sys::window().and_then(|w| w.location().hash().ok()).unwrap_or_default();
    let initial = if hash == "#en" { Lang::En } else { Lang::Ja };
    let (lang, _set_lang) = signal(initial);
    let (view_state, set_view) = signal(View::Loading);
    spawn_local(async move {
        loop {
            load(set_view).await;
            TimeoutFuture::new(30_000).await;
        }
    });
    let demo = is_demo();
    view! {
        {demo.then(|| view! {
            <div class="banner">
                {move || tr(lang.get(), "DEMO: ジョブ履歴はサンプルです(実際のデータではありません)。版の番号は取得時点の実際の値です。 ", "DEMO: the job history is sample data (not real). Version numbers are the real ones at build time. ")}
                <a href="https://github.com/aon-co-jp/open-upstream-watch" target="_blank" rel="noopener">"GitHub →"</a>
            </div>
        })}
        {move || match view_state.get() {
            View::Loading => view! { <main><p class="empty">"…"</p></main> }.into_any(),
            View::NeedLogin { available } => view! { <Login set_view=set_view lang=lang available=available /> }.into_any(),
            View::Failed(e) => view! { <main><p class="msg bad">{e}</p></main> }.into_any(),
            View::Dash(s) => view! { <Dashboard s=*s set_view=set_view lang=lang /> }.into_any(),
        }}
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
