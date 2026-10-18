// End-to-end browser test of the real server + the built Leptos dashboard (needs `trunk build` in ui/ first and
// `cargo build -p watch-server`). The one-time password goes through a mock of open-easy-web; GitHub is not contacted
// (the scheduler is not started: only the HTTP app runs).
//   node scripts/e2e-ui.js [screenshot-dir]
const http = require('http');
const fs = require('fs');
const os = require('os');
const path = require('path');
const net = require('net');
const { spawn } = require('child_process');
let chromium;
try {
  ({ chromium } = require('playwright'));
} catch (e) {
  // no local install: fall back to a checkout that has it (set PLAYWRIGHT_DIR), e.g. open-vsc-claude/node_modules/playwright
  ({ chromium } = require(process.env.PLAYWRIGHT_DIR || path.join(__dirname, '..', '..', 'open-vsc-claude', 'node_modules', 'playwright')));
}

const root = path.resolve(__dirname, '..');
const shots = process.argv[2];
const isWin = process.platform === 'win32';
const bin = path.join(root, 'target', 'debug', 'watch-server' + (isWin ? '.exe' : ''));

function check(c, m) { if (!c) throw new Error('check failed: ' + m); }
function freePort() { return new Promise((r) => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); }); }); }
async function waitPort(p) { for (let i = 0; i < 100; i++) { const ok = await new Promise((r) => { const s = net.connect(p, '127.0.0.1'); s.on('connect', () => { s.destroy(); r(true); }); s.on('error', () => r(false)); }); if (ok) return; await new Promise((r) => setTimeout(r, 200)); } throw new Error('port ' + p); }

(async () => {
  // mock open-easy-web: request-otp accepts, verify-otp accepts code 123456
  const otpLog = [];
  const otp = http.createServer((req, res) => {
    let b = '';
    req.on('data', (d) => (b += d));
    req.on('end', () => {
      otpLog.push(req.url + ' ' + b);
      res.setHeader('content-type', 'application/json');
      if (req.url.endsWith('/request-otp')) return res.end(JSON.stringify({ message_en: 'sent' }));
      if (b.includes('123456')) return res.end(JSON.stringify({ token: 't', email: 'owner@example.org' }));
      res.statusCode = 401; res.end('{}');
    });
  });
  await new Promise((r) => otp.listen(0, '127.0.0.1', r));
  const otpPort = otp.address().port;

  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'ouw-ui-'));
  const state = {
    upstreams: { poem: { baseline: '3.1.0', last_seen: '3.1.12', last_checked: '2026-10-19T00:00:00Z', health: 'ok' } },
    jobs: [{ id: 'poem-3.1.12', upstream: 'poem', from: '3.1.0', to: '3.1.12', status: 'merged', created: '2026-10-19T00:00:00Z', updated: '2026-10-19T00:00:01Z', branch: 'upstream/poem-3.1.12', release_url: 'https://crates.io/crates/poem/3.1.12', summary: '# poem 3.1.12 plan', log: ['detected'], merged_commit: 'abcdef0123456789' }],
    merges: [],
  };
  fs.writeFileSync(path.join(dir, 'state.json'), JSON.stringify(state));
  const cfg = `state_file = "${path.join(dir, 'state.json').replace(/\\/g, '/')}"
ui_dist = "${path.join(root, 'ui', 'dist').replace(/\\/g, '/')}"
dry_run = true
[auth]
allowed_emails = ["owner@example.org"]
otp_base_url = "http://127.0.0.1:${otpPort}"
cookie_secure = false
[[upstream]]
name = "poem"
repo = "poem"
kind = "crate"
tag_prefix = "v"
area = "crates/open-runo-poem-compat"
`;
  fs.writeFileSync(path.join(dir, 'config.toml'), cfg);

  const port = await freePort();
  // --serve-only keeps the scheduler (and so the network) out of the test
  const srv = spawn(bin, ['--config', path.join(dir, 'config.toml'), '--bind', '127.0.0.1:' + port, '--serve-only'], { stdio: 'ignore' });
  const browser = await chromium.launch();
  try {
    await waitPort(port);
    const base = `http://127.0.0.1:${port}/leptos-rs/`;
    for (const [name, vp, mobile] of [['desktop', { width: 1200, height: 900 }, false], ['phone', { width: 390, height: 844 }, true]]) {
      const ctx = await browser.newContext({ viewport: vp, hasTouch: mobile, isMobile: mobile });
      const page = await ctx.newPage();
      const errors = [];
      let step = 'start';
      page.on('pageerror', (e) => errors.push(`[${step}] ` + String(e).split('\n')[0]));
      page.on('console', (m) => { if (m.type() === 'error' && !/401/.test(m.text())) errors.push(`[${step}] ` + m.text().split('\n').slice(0, 2).join(' ')); });

      step = 'demo';
      // 1) the demo is public and shows sample data with a banner
      await page.goto(base + 'demo/');
      await page.waitForSelector('table');
      check((await page.textContent('.banner')).includes('DEMO'), name + ': demo banner');
      check((await page.textContent('body')).includes('leptos-rs/leptos'), name + ': demo upstream list');
      check((await page.$$('button.primary')).length === 0, name + ': the demo has no action buttons');
      if (shots) await page.screenshot({ path: path.join(shots, `demo-${name}.png`), fullPage: true });

      step = 'locked';
      // 2) the real page is locked: login form only
      await page.goto(base);
      await page.waitForSelector('.login');
      check(!(await page.textContent('body')).includes('crates/open-runo-poem-compat'), name + ': locked page must not leak data');
      const resp = await page.request.get(base + 'api/status');
      check(resp.status() === 401, name + ': api/status must be 401 without login');

      step = 'login';
      // 3) one-time password login
      await page.fill('input[type=email]', 'owner@example.org');
      await page.click('button.primary');
      await page.waitForSelector('input[inputmode=numeric]');
      check(otpLog.some((l) => l.includes('/request-otp') && l.includes('owner@example.org')), name + ': request forwarded to open-easy-web');
      await page.fill('input[inputmode=numeric]', '000000');
      await page.click('.login .row:nth-of-type(2) button.primary');
      await page.waitForFunction(() => document.querySelector('.msg') && document.querySelector('.msg').textContent.length > 0);
      check(await page.isVisible('.login'), name + ': a wrong code must not log in');
      await page.fill('input[inputmode=numeric]', '123456');
      await page.click('.login .row:nth-of-type(2) button.primary');
      await page.waitForSelector('table', { timeout: 15000 });
      check((await page.textContent('body')).includes('poem-3.1.12') || (await page.textContent('body')).includes('3.1.12'), name + ': dashboard shows the job');
      check((await page.$$('button.primary')).length === 1, name + ': the "check now" button is there after login');
      if (shots) await page.screenshot({ path: path.join(shots, `dashboard-${name}.png`), fullPage: true });

      step = 'logout';
      // 4) logout locks it again
      await page.click('button:has-text("ログアウト")');
      await page.waitForSelector('.login');
      check(errors.length === 0, name + ': console errors: ' + errors.join(' | '));
      console.log('ok:', name);
      await ctx.close();
    }
    console.log('e2e ui ok');
  } finally {
    await browser.close();
    srv.kill();
    otp.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
})().catch((e) => { console.error('FAIL', e.message); process.exit(1); });
