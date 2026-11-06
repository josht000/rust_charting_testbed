/**
 * Headless bench driver for the web target.
 * Prereq: `npm run build` once, and `npm run serve:web` running on :5171.
 * Usage: node scripts/run-bench.mjs [--url http://localhost:5171]
 * Opens http://localhost:5171/?bench=1 in headless Chrome; the app POSTs the
 * results JSON to /__save which writes ../results/tauri-web.json.
 */
import puppeteer from 'puppeteer-core';
import { existsSync } from 'node:fs';

const CHROME_CANDIDATES = [
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
];

const argUrl = process.argv.find((a) => a.startsWith('--url='));
const base = argUrl ? argUrl.split('=')[1] : 'http://localhost:5171';
const executablePath = CHROME_CANDIDATES.find((p) => existsSync(p));
if (!executablePath) {
  console.error('no Chrome/Edge found in standard install locations');
  process.exit(1);
}

const browser = await puppeteer.launch({
  executablePath,
  headless: true,
  // Streaming catch-up saturates the main thread for ~1-2 min; CDP evals
  // need a generous protocol timeout to get scheduled.
  protocolTimeout: 900_000,
  args: [
    '--enable-precise-memory-info',
    '--disable-gpu',
    '--no-sandbox',
    '--window-size=1920,1080',
  ],
});

try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1920, height: 1080 });
  page.on('console', (m) => {
    const t = m.text();
    if (t.startsWith('[bench]') || t.includes('error') || t.includes('failed')) {
      console.log('[page]', t.length > 2000 ? t.slice(0, 2000) + '…' : t);
    }
  });
  page.on('pageerror', (e) => console.error('[pageerror]', String(e)));

  console.log(`opening ${base}/?bench=1 with ${executablePath}`);
  await page.goto(`${base}/?bench=1`, { waitUntil: 'domcontentloaded', timeout: 60_000 });
  await page.waitForFunction('window.__benchDone !== undefined', { timeout: 600_000, polling: 2000 });
  const done = await page.evaluate('window.__benchDone');
  console.log('bench finished:', JSON.stringify(done));
} finally {
  await browser.close();
}
