# PORTING.md — open-upstream-watch(お引越し・再開 / hand-off)

**日本語 → English**

## 日本語

### 現状(2026-10-19)
- **実装済み・テスト済み**: 監視(GitHub リリース/タグ/crates.io)・基準版の記録・ジョブ作成・追従計画の生成・`pin`(Leptos の版数の書き換え)・関門(許可リスト・テスト・版上げの大きさ・破壊的変更の兆候・1 日の上限)・専用クローンでの作業と main への push・消失アラート(2 回確認して 1 回だけ)・メール送信部品・OTP ログイン(`open-easy-web` に委譲)・Leptos ダッシュボードとデモ。
  `cargo test --workspace`(単体 + 実 git の結合)と `node scripts/e2e-ui.js`(実 Chromium、デスクトップ/スマホ幅)が通る。実際の GitHub・crates.io に対する試走(基準版を古くして新版を検出)も確認済み。
- **実測**: ダッシュボードの WASM(release)は約 0.79 MB。初回の trunk ビルドは約 3 分。
- **未確認**: 実 SMTP からのアラートメール、実 RPoem / open-vsc-claude への自動マージ(一時的な bare リモートでのみ試験)、`aruaru-llm` の判定精度。

### 配置(VPS `conoha`、手順)
1. `/root/easy-web.tokyo/open-upstream-watch` に clone し、`cargo build --release -p watch-server`。UI は PC で `trunk build --release` して `ui/dist` を転送(VPS に trunk は無い)。
2. `config.toml`(git 管理外): `auth.allowed_emails`・`alert.to` に所有者のアドレス、`[search]`/`[llm]` は VPS 内のループバック、`otp_base_url = http://127.0.0.1:8080`、最初は `dry_run = true`。
3. SMTP は `open-easy-web` と同じ環境変数(`OPEN_EASYWEB_SMTP_*`)を、root 専用(600)の `smtp.env` にサーバー上で複製して `EnvironmentFile=` で渡す(値を表示しない)。
4. systemd サービス `open-upstream-watch.service`(ポート 4680、127.0.0.1)。`/root/open-web-server/domains.toml` に `path_prefix = "/leptos-rs"`(`strip_prefix = false`、アプリが `base_path` で受ける)を追加して `open-web-server` を再起動。変更前は `domains.toml.bak-…` に退避。
5. 確認: `/leptos-rs/demo/` が 200、`/leptos-rs/api/status` が 401(未ログイン)、`--test-alert` でメールが届く、ログインできる。

### 配置後に分かったこと(2026-10-19)
- **VPS に配置済み**(`https://easy-web.tokyo/leptos-rs/`、デモ `/demo/`。サービス `open-upstream-watch.service`、設定 `/root/open-upstream-watch-data/config.toml`、SMTP 設定 `/root/open-upstream-watch-smtp.env`〈600〉)。公開 URL・実ブラウザ(デスクトップ/スマホ幅)で確認済み。
- 最初の周回で、**消失アラートが本物の変更を検出**: `facebook/react` は `react/react` へ移転していた(設定を更新済み)。
- **本番の `open-easy-web` の Gmail 認証情報が無効**(SMTP 535)。`open-easy-web` 自身の `request-otp` も 500 を返す。複製した設定は元と同一(ハッシュ一致)なので、原因は元の設定。**直さない限り、ログイン用の OTP メールもアラートメールも届かない**。対処: Google アカウントでアプリパスワードを作り直し、`open-easy-web` の環境変数と `/root/open-upstream-watch-smtp.env` の両方を更新して、`systemctl restart` + `--test-alert`。
- **メールは未解決(2026-10-20)**: SMTP の認証は通り(ロリポップ `info@aon.co.jp`、587)、サーバーは送信を受け付けるが、**メールが届かない**。原因は DNS: `aon.co.jp`(ネームサーバーは Value-Domain)に **MX・SPF・DKIM・DMARC が無く**、A レコードは VPS(160.251.237.162、25 番ポートは未使用)を指す。このため (1) `@aon.co.jp` 宛(自分宛のテストを含む)は VPS へ配送しようとして失敗し、不達通知も届かない、(2) Gmail 宛は送信ドメインを認証できない。**対処(所有者が Value-Domain の DNS で行う)**: MX(優先度 10、ロリポップの受信サーバー。候補 `mx01.lolipop.jp`、正確な値はロリポップのユーザー専用ページで確認)、TXT `v=spf1 include:_spf.lolipop.jp ~all`(送信サーバー 157.7.107.234 を含むことを DNS で確認済み)、必要なら DKIM と `_dmarc`。反映後に自分宛テストが受信箱へ届くこと、続けて `--test-alert` が Gmail に届くことを確認する。

### 次の手
1. 配置と上の確認。2. `dry_run = true` のまま数周回し、計画の中身を確認。3. 問題なければ `dry_run = false`(RPoem は docs のみ自動マージ)。4. Leptos 0.8 への版上げが `npm run test:leptos` で通るかを PC で確認(通らなければテンプレート側の修正が先)。

## English

### Status (2026-10-19)
- **Implemented and tested**: watching (GitHub releases / tags / crates.io), baseline recording, jobs, plan generation, `pin` (rewrites the Leptos version), the gate (allow-lists, tests, bump size, breaking-change hints, daily limit), work in a dedicated clone and the push to main, disappearance alerts (two confirmations, once), the mail component, OTP login (delegated to `open-easy-web`), and the Leptos dashboard + demo. `cargo test --workspace` (unit + real-git integration) and `node scripts/e2e-ui.js` (real Chromium, desktop and phone widths) pass. A dry run against the real GitHub / crates.io (stale baselines → new versions detected) was done too.
- **Measured**: the release WASM of the dashboard is about 0.79 MB; the first trunk build takes about 3 minutes.
- **Not verified**: alert mail through the real SMTP, an automatic merge into the real RPoem / open-vsc-claude (tested only against temporary bare remotes), `aruaru-llm` accuracy.

### Deployment (VPS `conoha`)
1. Clone to `/root/easy-web.tokyo/open-upstream-watch`, `cargo build --release -p watch-server`; build the UI on the PC (`trunk build --release`) and copy `ui/dist` (no trunk on the VPS).
2. `config.toml` (git-ignored): the owner's address in `auth.allowed_emails` and `alert.to`; `[search]`/`[llm]` on the VPS loopback; `otp_base_url = http://127.0.0.1:8080`; start with `dry_run = true`.
3. SMTP: copy the same `OPEN_EASYWEB_SMTP_*` variables open-easy-web uses into a root-only (600) `smtp.env` on the server and load it with `EnvironmentFile=` (never print the values).
4. systemd unit `open-upstream-watch.service` (127.0.0.1:4680); add `path_prefix = "/leptos-rs"` (`strip_prefix = false`, the app handles `base_path` itself) to `/root/open-web-server/domains.toml` and restart `open-web-server`; back the file up first (`domains.toml.bak-…`).
5. Check: `/leptos-rs/demo/` is 200, `/leptos-rs/api/status` is 401 when logged out, `--test-alert` delivers a mail, login works.

### After deployment (2026-10-19)
- **Deployed on the VPS** (`https://easy-web.tokyo/leptos-rs/`, demo at `/demo/`; service `open-upstream-watch.service`, config `/root/open-upstream-watch-data/config.toml`, SMTP settings `/root/open-upstream-watch-smtp.env` (600)). Verified on the public URLs in a real browser (desktop and phone widths).
- The first cycle's **disappearance alert caught a real change**: `facebook/react` moved to `react/react` (config updated).
- **The Gmail credentials of the production `open-easy-web` are invalid** (SMTP 535); `open-easy-web`'s own `request-otp` returns 500 as well. The copied settings are identical to the original (hashes match), so the cause is the original setting. **Until it is fixed neither login OTP mails nor alert mails arrive.** Fix: create a new Google app password, update both the `open-easy-web` environment and `/root/open-upstream-watch-smtp.env`, then `systemctl restart` and `--test-alert`.
- **Mail is NOT resolved yet (2026-10-20)**: SMTP authentication works (Lolipop `info@aon.co.jp`, port 587) and the server accepts the mail, but **it does not arrive**. The cause is DNS: `aon.co.jp` (name servers at Value-Domain) has **no MX, SPF, DKIM or DMARC**, and its A record points to the VPS (160.251.237.162, nothing listens on port 25). So (1) mail to `@aon.co.jp` (including the self-test) is routed to the VPS and fails, and the bounce never arrives either, and (2) Gmail cannot authenticate the sending domain. **Fix (the owner does it in Value-Domain's DNS)**: an MX (priority 10, Lolipop's inbound server; candidate `mx01.lolipop.jp`, confirm the exact value in Lolipop's user page), TXT `v=spf1 include:_spf.lolipop.jp ~all` (DNS shows it covers the sending server 157.7.107.234), and, if offered, DKIM and `_dmarc`. After it propagates, verify that a self-addressed test reaches the inbox, then that `--test-alert` reaches Gmail.

### Next
Deploy and verify; run a few cycles in `dry_run = true` and read the plans; then switch to `dry_run = false` (RPoem merges docs only); check on a PC that the Leptos 0.8 bump passes `npm run test:leptos` (if not, the template needs fixing first).
