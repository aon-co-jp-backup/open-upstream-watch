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
- **アラートメールは届いた(2026-10-20 03:58 JST、`--test-alert`、Gmail で受信を確認)**。SMTP はロリポップ `info@aon.co.jp`(587)、Gmail 側では「dkim.lolipop.jp 経由」と表示される(ロリポップが DKIM 署名)。**未了**: (1) ログイン用 OTP メールの受信確認(`https://easy-web.tokyo/leptos-rs/` で「コードを送る」)、(2) 推奨の DNS 設定: `aon.co.jp`(ネームサーバーは Value-Domain)には MX・SPF が無く、A はコードを受けない VPS を指すため、**`@aon.co.jp` 宛の受信ができず**、迷惑メール判定もされやすい。Value-Domain の DNS に、MX `10 mx01.lolipop.jp.` と TXT(`@`)`v=spf1 include:_spf.lolipop.jp ~all` を追加する(既存の A は変更しない)。DKIM と `_dmarc` は任意。Value-Domain のログインは、旧ユーザー名・パスワードのまま `https://www.value-domain.com/login.php` から入れる(新入口 Value Domain One に統合)。

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
- **The alert mail arrived (2026-10-20 03:58 JST, `--test-alert`, received in Gmail)**. SMTP is Lolipop `info@aon.co.jp` (587); Gmail shows "via dkim.lolipop.jp" (Lolipop signs with DKIM). **Open**: (1) confirm the login OTP mail arrives ("Send code" at `https://easy-web.tokyo/leptos-rs/`); (2) recommended DNS: `aon.co.jp` (name servers at Value-Domain) has no MX or SPF and its A record points to the VPS, which does not accept mail, so **mail to `@aon.co.jp` cannot be received** and spam filtering is more likely. Add to Value-Domain's DNS an MX `10 mx01.lolipop.jp.` and a TXT (`@`) `v=spf1 include:_spf.lolipop.jp ~all` (leave the existing A alone); DKIM and `_dmarc` are optional. Value-Domain login still works with the old username and password via `https://www.value-domain.com/login.php` (merged into the new Value Domain One entry).

### Next
Deploy and verify; run a few cycles in `dry_run = true` and read the plans; then switch to `dry_run = false` (RPoem merges docs only); check on a PC that the Leptos 0.8 bump passes `npm run test:leptos` (if not, the template needs fixing first).

## API 方針: REST 全廃・gRPC / Connect へ (2026-11-04)

### 日本語
- **REST は全廃する。新しい API は REST で作らない。** ブラウザや `curl` から呼ぶ API は **Connect の単純呼び出し**(`POST /<パッケージ>.<Service>/<Method>`、`content-type: application/json`、本文は JSON)、サービス間は **gRPC / gRPC-web**(RPoem の管理API `open_runo.admin.v1.Admin` と同じ流儀)。ストリーミングは、必要になってから。
- **既存の REST は、触るときに順に移す。** 移したら旧 REST の経路は消す。移行が終わるまで残るが、**新規には追加しない**。
- **JSON は RS-JSON に統一**する(読み込みは `from_str_secure` / `parse_secure` でサイズ・深さ・重複キーの上限つき、書き出しは `rust_json::fast`)。ブラウザの WASM に `prost` は入れない(サイズが増えるため)。
- **例外(自分たちの API ではなく、相手の規格が決めているもの)**: ACME、OAuth/OIDC のコールバック、第三者(GitHub など)からの Webhook、ヘルスチェック、静的ファイル配信、OTLP など。これらは規格どおりの HTTP を受ける。
- 参考実装: `RPoem`(`open_runo.admin.v1.Admin`)、`open-easy-web`(`server/src/connect.rs`、`proto/open_easy_web/admin/v1/admin.proto`)。
- ※ このリポジトリのコードが既に方針どおりかどうかは、各リポジトリの現状による。この節は方針の記録であり、コードの変更を意味しない。

### English
- **REST is being abolished. New APIs are not built as REST.** APIs called from browsers or `curl` use **Connect unary calls** (`POST /<package>.<Service>/<Method>`, `content-type: application/json`, JSON body); service-to-service calls use **gRPC / gRPC-web** (the same style as RPoem's admin API `open_runo.admin.v1.Admin`). Streaming only when it is actually needed.
- **Existing REST endpoints are migrated one by one, when they are touched.** The old REST route is removed once migrated. Until migration is finished they remain, but **no new ones are added**.
- **JSON is unified on RS-JSON**: reading with `from_str_secure` / `parse_secure` (size, depth and duplicate-key limits), writing with `rust_json::fast`. `prost` is not put into browser WASM (it would grow the binary).
- **Exceptions (shapes dictated by someone else's standard, not our own API)**: ACME, OAuth/OIDC callbacks, webhooks from third parties (e.g. GitHub), health checks, static file serving, OTLP, and so on. These accept plain HTTP as the standard requires.
- Reference implementations: `RPoem` (`open_runo.admin.v1.Admin`) and `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Note: whether the code in a given repository already follows this policy depends on that repository. This section records the policy; it does not mean the code has been changed.

### Deutsch (KI-Übersetzung, redaktionell überarbeitet; noch nicht von Muttersprachlern geprüft)
- **REST wird vollständig abgeschafft; neue APIs entstehen nicht mehr als REST.** APIs, die aus dem Browser oder per `curl` aufgerufen werden, verwenden **unäre Connect-Aufrufe** (`POST /<Paket>.<Service>/<Methode>`, `content-type: application/json`, JSON-Body). Zwischen Diensten kommen **gRPC bzw. gRPC-web** zum Einsatz, im selben Stil wie die Admin-API von RPoem (`open_runo.admin.v1.Admin`). Streaming wird erst ergänzt, wenn es tatsächlich gebraucht wird.
- **Bestehende REST-Endpunkte werden schrittweise umgestellt, und zwar immer dann, wenn man ohnehin an ihnen arbeitet.** Sobald ein Endpunkt umgestellt ist, wird die alte REST-Route entfernt. Bis zum Abschluss der Migration bleiben die übrigen bestehen, es werden aber **keine neuen mehr hinzugefügt**.
- **JSON wird einheitlich über RS-JSON verarbeitet**: Gelesen wird mit `from_str_secure` bzw. `parse_secure` (mit Obergrenzen für Größe, Verschachtelungstiefe und doppelte Schlüssel), geschrieben mit `rust_json::fast`. `prost` wird nicht in das Browser-WASM aufgenommen, da es die Datei vergrößern würde.
- **Ausnahmen (Schnittstellen, deren Form ein fremder Standard vorgibt und die nicht zu unserer eigenen API gehören)**: ACME, OAuth/OIDC-Callbacks, Webhooks von Drittanbietern (z. B. GitHub), Health-Checks, die Auslieferung statischer Dateien, OTLP und Ähnliches. Diese nehmen, wie vom jeweiligen Standard verlangt, gewöhnliches HTTP entgegen.
- Referenzimplementierungen: `RPoem` (`open_runo.admin.v1.Admin`) und `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Hinweis: Ob der Code eines einzelnen Repositorys diese Richtlinie bereits erfüllt, hängt vom jeweiligen Repository ab. Dieser Abschnitt dokumentiert lediglich die Richtlinie; er besagt nicht, dass der Code schon angepasst wurde.

### Русский (перевод выполнен с помощью ИИ и отредактирован; носителями языка пока не проверен)
- **От REST мы полностью отказываемся; новые API больше не создаются в стиле REST.** API, к которым обращаются из браузера или через `curl`, используют **унарные вызовы Connect** (`POST /<пакет>.<Service>/<Method>`, `content-type: application/json`, тело запроса — JSON). Для взаимодействия между сервисами применяются **gRPC и gRPC-web** — в том же стиле, что и административный API RPoem (`open_runo.admin.v1.Admin`). Потоковая передача данных добавляется лишь тогда, когда в ней действительно возникает необходимость.
- **Существующие REST-эндпоинты переводятся поэтапно — по мере того, как с ними приходится работать.** После перевода старый REST-маршрут удаляется. Пока миграция не завершена, оставшиеся эндпоинты сохраняются, но **новые не добавляются**.
- **Работа с JSON унифицирована на основе RS-JSON**: чтение — через `from_str_secure` или `parse_secure` (с ограничениями на размер, глубину вложенности и повторяющиеся ключи), запись — через `rust_json::fast`. `prost` в браузерный WASM не включается, поскольку это увеличило бы размер файла.
- **Исключения (интерфейсы, форму которых определяет чужой стандарт, а не наш собственный API)**: ACME, обратные вызовы OAuth/OIDC, вебхуки сторонних сервисов (например, GitHub), проверки работоспособности, раздача статических файлов, OTLP и тому подобное. Такие интерфейсы принимают обычный HTTP, как того требует соответствующий стандарт.
- Эталонные реализации: `RPoem` (`open_runo.admin.v1.Admin`) и `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Примечание: соответствует ли код конкретного репозитория этой политике, зависит от самого репозитория. Данный раздел лишь фиксирует политику и не означает, что код уже изменён.

## API 方針の補足: 用語の整理と使い分け (2026-11-04)

### 日本語
**1つに統一するものではなく、別の層の話です。** 「REST の代わり」になるのは Connect・gRPC・GraphQL だけで、WASM は画面を作る技術です。

| 用語 | 何か | REST の代わりか |
|---|---|---|
| REST | URL と HTTP メソッドで操作を表す API の作り方 | 全廃する側 |
| **Connect** | `POST /サービス名/メソッド名` に JSON を送る形式。`curl` でもブラウザの `fetch` でも呼べる | はい(**採用**) |
| **gRPC** | 同じ「サービス/メソッド」の考え方を、バイナリ(protobuf)と HTTP/2 で行う形式。速く、ストリーミングも得意 | はい(**サービス同士に採用**) |
| gRPC-web | gRPC のブラウザ向けの変形版 | はい(必要なら) |
| GraphQL | 欲しい項目を問い合わせる形式 | 一部だけ(**問い合わせ(読み取り)専用**) |
| WASM | Rust をブラウザの中で動かす仕組み | いいえ(画面を作る技術) |

- **ブラウザの画面や `curl` から呼ぶ API は Connect**(POST、JSON を RS-JSON で読み書き)。ブラウザの WASM は素の gRPC を話せず、Connect なら `fetch` のままで、`prost` を足さずに済む(画面のサイズも増えない)。
- **サービス同士の通信(サーバー間、重い処理、ストリーミング)は gRPC。** 多くは Connect で足りるので、ストリーミングが必要になった所だけ gRPC に上げる。
- **GraphQL は、まとめて問い合わせたいときの読み取り専用。** 書き込み(操作)には使わない。
- **WASM は画面を作る技術として、そのまま使う。** 画面から呼ぶ API が Connect になっただけ。
- 外部の規格が形を決めるもの(ACME、OAuth のコールバック、第三者の Webhook、ヘルスチェック、静的ファイル、OTLP)は、規格どおりの HTTP のまま。
- **迷ったら Connect。**

### English
**These are not alternatives to unify; they sit at different layers.** Only Connect, gRPC and GraphQL can replace REST; WASM is a way to build the UI.

| Term | What it is | Replaces REST? |
|---|---|---|
| REST | API style that expresses operations through URLs and HTTP methods | The thing being abolished |
| **Connect** | `POST /Service/Method` with a JSON body; callable from `curl` and from a browser `fetch` | Yes (**adopted**) |
| **gRPC** | The same service/method idea over binary (protobuf) and HTTP/2; fast, good at streaming | Yes (**for service-to-service**) |
| gRPC-web | A browser-friendly variant of gRPC | Yes (if needed) |
| GraphQL | A query language: ask for exactly the fields you want | Partly (**read-only queries**) |
| WASM | Runs Rust inside the browser | No (a UI technology) |

- **APIs called from browser screens or `curl` use Connect** (POST, JSON read and written with RS-JSON). Browser WASM cannot speak plain gRPC; with Connect it stays a `fetch`, and `prost` is not needed (the binary does not grow).
- **Service-to-service calls (server to server, heavy work, streaming) use gRPC.** Connect is enough in most places; move a call to gRPC only where streaming is actually needed.
- **GraphQL is read-only**, for queries that gather several pieces of data. It is not used for writes (operations).
- **WASM stays as the way to build the UI.** The API it calls simply became Connect.
- Interfaces whose shape an outside standard dictates (ACME, OAuth callbacks, third-party webhooks, health checks, static files, OTLP) stay plain HTTP.
- **When in doubt, use Connect.**

### Deutsch (KI-Übersetzung, sprachlich überarbeitet; noch nicht von Muttersprachlern geprüft)
**Es geht nicht darum, eine einzige Technik auszuwählen – die Begriffe liegen auf unterschiedlichen Ebenen.** Als Ersatz für REST kommen nur Connect, gRPC und GraphQL in Frage; WASM dagegen ist eine Technik, mit der die Benutzeroberfläche gebaut wird.

| Begriff | Was ist das? | Ersatz für REST? |
|---|---|---|
| REST | Ein API-Stil, bei dem Operationen über URLs und HTTP-Methoden ausgedrückt werden | wird abgeschafft |
| **Connect** | `POST /Dienst/Methode` mit JSON im Body; aufrufbar mit `curl` ebenso wie mit `fetch` im Browser | ja (**gewählt**) |
| **gRPC** | Dasselbe Prinzip aus Dienst und Methode, aber über binäres protobuf und HTTP/2; schnell und gut für Streaming geeignet | ja (**zwischen Diensten**) |
| gRPC-web | Eine für den Browser angepasste Variante von gRPC | ja (bei Bedarf) |
| GraphQL | Abfragesprache, mit der man genau die benötigten Felder anfordert | teilweise (**nur für lesende Abfragen**) |
| WASM | Führt Rust-Code direkt im Browser aus | nein (Technik für die Oberfläche) |

- **APIs, die von Browseroberflächen oder per `curl` aufgerufen werden, verwenden Connect** (POST; JSON wird mit RS-JSON gelesen und geschrieben). WASM im Browser kann kein reines gRPC sprechen. Mit Connect bleibt es bei einem gewöhnlichen `fetch`, und `prost` ist nicht nötig – die WASM-Datei wird dadurch nicht größer.
- **Für die Kommunikation zwischen Diensten (Server zu Server, rechenintensive Aufgaben, Streaming) wird gRPC eingesetzt.** In den meisten Fällen reicht Connect aus; auf gRPC wird nur dort umgestellt, wo Streaming tatsächlich gebraucht wird.
- **GraphQL dient ausschließlich lesenden Abfragen**, wenn mehrere Informationen gebündelt abgefragt werden sollen. Für Schreibzugriffe (Operationen) wird es nicht verwendet.
- **WASM bleibt die Technik für die Oberfläche.** Geändert hat sich lediglich, dass die aufgerufene API nun auf Connect beruht.
- Schnittstellen, deren Form durch einen externen Standard festgelegt ist (ACME, OAuth-Callbacks, Webhooks von Drittanbietern, Health-Checks, statische Dateien, OTLP), bleiben gewöhnliches HTTP.
- **Im Zweifel: Connect.**

### Русский (перевод выполнен с помощью ИИ и литературно отредактирован; носителями языка пока не проверен)
**Это не варианты, из которых нужно выбрать один, а технологии разных уровней.** Заменить REST могут только Connect, gRPC и GraphQL; WASM же — это способ создания пользовательского интерфейса.

| Термин | Что это | Заменяет REST? |
|---|---|---|
| REST | Стиль построения API, при котором операции выражаются через URL и HTTP-методы | отменяется |
| **Connect** | `POST /Сервис/Метод` с телом в формате JSON; вызывается как из `curl`, так и из браузерного `fetch` | да (**выбран**) |
| **gRPC** | Тот же принцип «сервис — метод», но поверх двоичного формата protobuf и HTTP/2; работает быстро и хорошо подходит для потоковой передачи | да (**для связи между сервисами**) |
| gRPC-web | Адаптированный для браузера вариант gRPC | да (при необходимости) |
| GraphQL | Язык запросов: клиент запрашивает ровно те поля, которые ему нужны | частично (**только запросы на чтение**) |
| WASM | Позволяет выполнять код на Rust прямо в браузере | нет (технология для интерфейса) |

- **API, которые вызываются из браузерных интерфейсов или через `curl`, строятся на Connect** (POST; JSON читается и записывается через RS-JSON). Браузерный WASM не умеет работать с «чистым» gRPC. С Connect достаточно обычного `fetch`, а `prost` не требуется — размер WASM-файла не увеличивается.
- **Для обмена между сервисами (сервер — сервер, ресурсоёмкие задачи, потоковая передача) применяется gRPC.** В большинстве случаев достаточно Connect; на gRPC переходят только там, где потоковая передача действительно необходима.
- **GraphQL используется только для чтения**, когда нужно получить сразу несколько связанных данных. Для записи (операций) он не применяется.
- **WASM по-прежнему остаётся технологией для интерфейса.** Изменилось лишь то, что вызываемый им API теперь построен на Connect.
- Интерфейсы, форма которых задана внешним стандартом (ACME, обратные вызовы OAuth, вебхуки сторонних сервисов, проверки работоспособности, статические файлы, OTLP), остаются обычным HTTP.
- **Если есть сомнения — выбирайте Connect.**
