import puppeteer from 'puppeteer-core';
import { existsSync } from 'node:fs';

const CHROME_CANDIDATES = [
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
];
const executablePath = CHROME_CANDIDATES.find((p) => existsSync(p));
const browser = await puppeteer.launch({
  executablePath, headless: true,
  args: ['--no-sandbox', '--window-size=1920,1080'],
});
const page = await browser.newPage();
await page.setViewport({ width: 1920, height: 1080 });
page.on('pageerror', (e) => console.error('[pageerror]', String(e)));
await page.goto('http://localhost:5171', { waitUntil: 'domcontentloaded', timeout: 60_000 });
await page.waitForFunction(
  'window.__charts && window.__charts.filter(Boolean).length === 8',
  { timeout: 120_000, polling: 500 },
);
// Zoom every chart to a 60-bar window near the end.
await page.evaluate(() => {
  for (const h of window.__charts) {
    h.chart.timeScale().setVisibleLogicalRange({ from: 23280, to: 23340 });
  }
});
await new Promise((r) => setTimeout(r, 1500));
await page.screenshot({ path: 'zoomed-all.png' });
// Close-up of one panel (chart0's group element).
const el = await page.$('.dv-dockview .dv-view:nth-of-type(1) canvas')
  ?? await page.$('canvas');
await el.screenshot({ path: 'zoomed-panel.png' });
await browser.close();
console.log('saved zoomed-all.png zoomed-panel.png');
