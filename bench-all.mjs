/**
 * Sequential bench re-run for all 3 configs on an idle machine.
 * Runs desktop exes + headless web benches one at a time.
 * Writes results/{config}-{desktop,web}.json.
 */
import { spawn } from 'node:child_process';
import { existsSync, statSync, writeFileSync } from 'node:fs';
import puppeteer from './1_tauri/node_modules/puppeteer-core/lib/esm/puppeteer/puppeteer-core.js';

const ROOT = 'C:/data/trading/rust_testbed';
const C = [
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
];
const browser_path = C.find(existsSync);
const log = (m) => console.log(`[${new Date().toISOString().slice(11, 19)}] ${m}`);

function runExe(exe, args, timeoutMs, resultsFile) {
  // Bench apps write their results JSON then stay open. Resolve once the
  // results file changes (or process exits), then kill.
  log(`run: ${exe} ${args.join(' ')}`);
  return new Promise((resolve) => {
    const before = existsSync(resultsFile) ? statSync(resultsFile).mtimeMs : 0;
    const t0 = Date.now();
    const p = spawn(exe, args, { stdio: ['ignore', 'pipe', 'pipe'] });
    let written = false;
    const finish = (why) => {
      clearInterval(iv);
      try { p.kill(); } catch {}
      log(`done ${exe} (${why})`);
      resolve();
    };
    p.stdout.on('data', (d) => {
      const s = d.toString();
      if (s.includes('BENCH DONE') || s.includes('bench: DONE')) setTimeout(() => finish('stdout marker'), 1500);
    });
    const iv = setInterval(() => {
      if (existsSync(resultsFile) && statSync(resultsFile).mtimeMs > before) {
        written = true;
        setTimeout(() => finish('results written'), 1500);
      }
      if (Date.now() - t0 > timeoutMs) finish(written ? 'timeout' : 'TIMEOUT-no-results');
    }, 2000);
    p.on('exit', () => finish(written ? 'exit' : 'exit-no-results'));
  });
}

async function webBench(url, outFile, doneExpr, timeoutMs) {
  const browser = await puppeteer.launch({
    executablePath: browser_path,
    headless: true,
    protocolTimeout: 900_000,
    args: ['--no-sandbox', '--window-size=1920,1080', '--enable-precise-memory-info'],
  });
  try {
    const page = await browser.newPage();
    await page.setViewport({ width: 1920, height: 1080 });
    page.on('pageerror', (e) => console.error('[pageerror]', String(e).slice(0, 300)));
    const t0 = Date.now();
    await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 });
    await page.waitForFunction(doneExpr, { timeout: timeoutMs, polling: 2000 });
    // Give POSTs/console flushes a moment.
    await new Promise((r) => setTimeout(r, 2000));
    const json = await page.evaluate(`${doneExpr.replace('!== undefined', '')}`);
    if (typeof json === 'string' && json.startsWith('{')) {
      writeFileSync(outFile, json);
      log(`saved ${outFile} (from page var)`);
    }
    log(`done ${url} in ${((Date.now() - t0) / 1000).toFixed(0)}s`);
  } finally {
    await browser.close();
  }
}

const t = (f) => `${ROOT}/results/${f}`;

// 1. egui desktop — already re-run (results fresh from this session). Skip
// unless forced.
if (process.env.REBENCH_ALL === '1') {
  await runExe(`${ROOT}/2_egui/target/release/egui_testbed.exe`, ['--bench'], 300_000, t('egui-desktop.json'));
}

// 2. egui web (?bench=1, window.__bench_json string)
await webBench('http://localhost:5172/?bench=1', t('egui-web.json'),
  'window.__bench_json !== undefined', 300_000);

// 3. slint desktop
await runExe(`${ROOT}/3_slint/target/release/slint-desktop.exe`, ['--bench'], 300_000, t('slint-desktop.json'));

// 4. slint web (POSTs to serve /save; detect by results file mtime change)
{
  const before = existsSync(t('slint-web.json')) ? statSync(t('slint-web.json')).mtimeMs : 0;
  const browser = await puppeteer.launch({
    executablePath: browser_path, headless: true,
    args: ['--no-sandbox', '--window-size=1920,1080'],
  });
  const page = await browser.newPage();
  await page.setViewport({ width: 1920, height: 1080 });
  await page.goto('http://localhost:5173/?bench=1', { waitUntil: 'domcontentloaded', timeout: 60_000 });
  const t0 = Date.now();
  while (Date.now() - t0 < 240_000) {
    await new Promise((r) => setTimeout(r, 3000));
    if (existsSync(t('slint-web.json')) && statSync(t('slint-web.json')).mtimeMs > before) break;
  }
  await browser.close();
  log('slint web done');
}

// 5. tauri desktop (long: ~4-5 min catch-up)
await runExe(`${ROOT}/1_tauri/src-tauri/target/release/app.exe`, ['--bench'], 600_000, t('tauri-desktop.json'));

// 6. tauri web (its own driver script handles everything)
log('run: node 1_tauri/scripts/run-bench.mjs');
const p = spawn('node', [`${ROOT}/1_tauri/scripts/run-bench.mjs`], { stdio: 'inherit' });
await new Promise((res, rej) => p.on('exit', (c) => (c === 0 ? res() : rej(new Error(`exit ${c}`)))));

log('ALL BENCHES COMPLETE');
