/** Quick visual check: load the app, wait for charts, save a screenshot. */
import puppeteer from 'puppeteer-core';
import { existsSync } from 'node:fs';

const CHROME = [
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
].find((p) => existsSync(p));

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  args: ['--disable-gpu', '--no-sandbox'],
});
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1920, height: 1080 });
  page.on('pageerror', (e) => console.error('[pageerror]', String(e)));
  await page.goto('http://localhost:5171/', { waitUntil: 'networkidle2', timeout: 120_000 });
  await new Promise((r) => setTimeout(r, 6000));
  await page.screenshot({ path: 'tests/screenshot.png' });
  console.log('screenshot -> tests/screenshot.png');
} finally {
  await browser.close();
}
