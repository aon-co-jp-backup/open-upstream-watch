# open-upstream-watch

**日本語 → English**

RPoem が互換実装している元(Poem・WunderGraph Cosmo・Tauri・React など)と、Rust + Leptos(WASM・trunk)テンプレートの部品(Leptos・trunk)の**新しい版を定期的に監視**し、新版が出たら Rust 版の追従作業を準備して、**厳しい関門を通ったものだけを main へ自動マージ**する仕組みです。画面は Leptos(WASM)で、メールのワンタイムパスワードでログインします。

- 本番(要ログイン): https://easy-web.tokyo/leptos-rs/
- デモ(誰でも見られる・サンプルデータ): https://easy-web.tokyo/leptos-rs/demo/
- ライセンス: MIT

## 何をするか

1. **監視**: GitHub のリリース(または crates.io)を 6 時間おきに確認。安定版だけを対象にします(`-rc` などは無視)。最初の確認は「基準の版」を記録するだけで、ジョブは作りません。
2. **ジョブ作成**: 新版が出たら、リリースノート・`aruaru-search`(自前の無制限メタ検索)の関連ページ・`aruaru-llm` による要約と「破壊的変更か」の判定を集めた**追従計画**を作ります。`aruaru-llm` は小さなローカルモデルで(GPU のあるマシンでは `open-cuda` が推論を加速)、**出力は低信頼の参考情報として必ず明記**し、「見送り(要確認)」の方向にしか使いません。
3. **作業**: ボットは**専用のクローン**で作業し、あなたの作業ツリーには触れません。既定の処理は `docs/upstream/<名前>/<版>.md` に計画を書くだけです。`pin` を設定した元(例: Leptos)は、対象ファイルの版数を書き換えます。外部コマンド(例: Claude Code のヘッドレス実行)を porter にもできます。
4. **関門(自動マージの条件)**: すべて満たしたときだけ main へ push します。
   - 変更されたパスが許可リスト内(既定は `docs/upstream/` のみ。コードは `allow_code_merge = true` を明示した場合だけ)。`.github/`・`.env`・鍵・トークン等は常に拒否。
   - テストコマンドが成功(例: Leptos の更新は `npm run test:leptos` = 実ブラウザの往復テスト)。
   - `pin` の版上げが `max_auto_bump` 以内。リリースノートに破壊的変更の兆候(キーワード or `aruaru-llm` の判定)があれば、コードの変更は人の確認待ち。
   - 1 日の自動マージ数の上限内。
   通らなかったものは**作業ブランチに push して要確認**(または拒否)にします。
5. **消失アラート**: 監視元のリポジトリが削除・アーカイブ・名称変更・crate の消失をしたら、**メールで 1 回だけ**知らせます(404 は 2 回連続で確認してから)。メールは `open-easy-web` と同じ SMTP の設定(環境変数)を使います。
6. **監視のみの元**(`mode = "notify"`、例: trunk): 新版を記録するだけで、何も移植しません。

## 安全な始め方

- **既定は試走(`dry_run = true`)**: 対象のリポジトリには何も書かず、計画を画面で読めます。計画が妥当と分かってから `false` にしてください。
- ログインを許可するメールアドレスは**設定ファイル(`config.toml`、git 管理外)**に書きます。リポジトリには入れません。トークンは txt ファイルのパスで渡し、値は表示・記録しません。
- Leptos の版上げ(`0.7 → 0.8` など)は API が変わることがあります。そのときはテストが失敗して**自動マージされず**、作業ブランチが残ります。

## 使い方

```bash
cp config.example.toml config.toml        # 許可メール・通知先などを編集
cargo build --release -p watch-server
(cd ui && trunk build --release)           # ダッシュボード(WASM、約 0.8 MB)
target/release/watch-server --config config.toml          # サーバー + 定期監視
target/release/watch-server --config config.toml --once   # 1 回だけ実行してログを表示
target/release/watch-server --config config.toml --test-alert   # アラートメールの送信試験
```

`/leptos-rs/` 配下で配信します(`base_path`)。ログインは既存の `open-easy-web`(`/api/auth/request-otp`・`/verify-otp`)に委譲し、同じアカウント・同じ SMTP を使います。

## テスト(いずれも実際に実行して確認済み)

| コマンド | 内容 |
| --- | --- |
| `cargo test --workspace` | 単体 + 結合: 実際の git(ローカルの bare リモート)へのマージ・関門・ピン規則・消失アラート・OTP ログインの流れ |
| `node scripts/e2e-ui.js` | 実 Chromium: デモ、施錠、OTP ログイン(誤コード拒否)、ログアウト(デスクトップ幅とスマホ幅) |

## 確認できていないこと

- 実際の SMTP からのアラートメール送信(VPS への配置後に `--test-alert` で確認します)。
- 実際の RPoem / open-vsc-claude への自動マージ(試験は一時的な bare リモートで行っています。最初は `dry_run = true` で運用してください)。
- `aruaru-llm` の判定の精度(小さなモデルのため参考情報の扱い)。

---

## English

Watches, on a schedule, the upstreams that RPoem re-implements (Poem, WunderGraph Cosmo, Tauri, React, ...) and the parts of the Rust + Leptos (WASM, trunk) template (Leptos, trunk). When a new version appears it **prepares the Rust-side work and merges it into main automatically, but only through a strict gate**. The dashboard is a Leptos (WASM) app; login is an e-mail one-time password.

- Live (login required): https://easy-web.tokyo/leptos-rs/
- Demo (public, sample data): https://easy-web.tokyo/leptos-rs/demo/
- License: MIT

### What it does

1. **Watch**: checks GitHub releases (or crates.io) every 6 hours; stable versions only. The first check just records a baseline.
2. **Job**: collects a **porting plan**: release notes, related pages from `aruaru-search` (the project's own unlimited meta-search), and `aruaru-llm`'s summary and "breaking changes?" opinion. `aruaru-llm` is a small local model (`open-cuda` accelerates inference on a GPU machine); its output is always labelled as low-reliability and is only ever used to **hold a change back for review**.
3. **Work**: the bot works in **its own clone**, never your working tree. By default it writes a plan to `docs/upstream/<name>/<version>.md`. An upstream with a `pin` (e.g. Leptos) gets the version in a target file rewritten. An external command (e.g. Claude Code headless) can be the porter.
4. **Gate**: pushed to main only if every condition holds: changed paths are within the allow-lists (default `docs/upstream/` only; code needs an explicit `allow_code_merge = true`; `.github/`, `.env`, keys and tokens are always refused); the test command passes (for Leptos: `npm run test:leptos`, a real-browser round trip); the pin bump is within `max_auto_bump` and the notes show no sign of breaking changes (keywords or `aruaru-llm`); the daily merge limit is not reached. Anything else goes to a **review branch** (or is blocked).
5. **Disappearance alert**: if an upstream repository or crate is deleted, archived or renamed, you get **one e-mail** (a 404 must repeat twice first), sent through the same SMTP account as open-easy-web.
6. **Watch-only upstreams** (`mode = "notify"`, e.g. trunk): the new version is recorded; nothing is ported.

### Starting safely

The default is **`dry_run = true`**: nothing is written to the target repositories; read the plans in the dashboard, then switch it off. The allowed login e-mail lives in `config.toml` (git-ignored), never in the repository; tokens are passed as txt-file paths and never printed. A Leptos `0.7 → 0.8` bump can break the template: then the test fails, nothing is merged and a branch is left behind.

### Not verified yet

Alert mail through the real SMTP (to be checked with `--test-alert` after deployment), an automatic merge into the real RPoem / open-vsc-claude (tested against temporary bare remotes; run in dry-run first), and the accuracy of `aruaru-llm`'s opinion.
