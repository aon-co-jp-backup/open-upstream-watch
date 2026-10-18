//! `watch-server [--config config.toml] [--once] [--test-alert] [--bind 127.0.0.1:4680]`

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use watch_core::config::Config;
use watch_core::github::GithubClient;
use watch_core::State;
use watch_server::{app, send_test_alert, AppState};

mod demo;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into())).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg_path = PathBuf::from(arg(&args, "--config").unwrap_or_else(|| "config.toml".into()));
    let cfg = Config::load(&cfg_path)?;
    let state = State::load(&cfg.state_file)?;
    let github = GithubClient::new("https://api.github.com", cfg.github_token())?;
    if let Some(out) = arg(&args, "--make-demo") {
        let status = demo::make_demo(&cfg, &github).await;
        std::fs::write(&out, serde_json::to_string_pretty(&status)?)?;
        println!("demo data written to {out} ({} upstreams, {} sample jobs)", status.upstreams.len(), status.jobs.len());
        return Ok(());
    }
    let app_state = AppState::new(cfg, state, github);

    if args.iter().any(|a| a == "--test-alert") {
        send_test_alert(&app_state).await?;
        println!("test alert sent (or logged when no mail is configured)");
        return Ok(());
    }
    if args.iter().any(|a| a == "--once") {
        for l in app_state.cycle().await {
            println!("{l}");
        }
        return Ok(());
    }

    // scheduler: first cycle shortly after start, then every poll_interval_hours (off with --serve-only, used by tests)
    let s = Arc::clone(&app_state);
    if !args.iter().any(|a| a == "--serve-only") {
      tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            for l in s.cycle().await {
                tracing::info!("{l}");
            }
            tokio::time::sleep(Duration::from_secs(s.cfg.poll_interval_hours * 3600)).await;
        }
      });
    }

    let bind = arg(&args, "--bind").unwrap_or_else(|| "127.0.0.1:4680".into());
    tracing::info!("listening on http://{bind}{}/", app_state.cfg.base_path);
    poem::Server::new(poem::listener::TcpListener::bind(bind)).run(app(app_state)).await?;
    Ok(())
}
