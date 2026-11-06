/**
 * Standalone web server for the production build (npm run build -> dist/).
 *  - serves dist/ on http://localhost:5171 (SPA fallback to index.html)
 *  - POST /__save?name=<file>.json   -> writes <workspace>/results/<file>.json
 *  - GET  /__stats                   -> artifact sizes of dist/ (for bench results)
 */
import http from 'node:http';
import { existsSync, mkdirSync, readdirSync, statSync, writeFileSync, createReadStream } from 'node:fs';
import { extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = fileURLToPath(new URL('.', import.meta.url));
const DIST = resolve(here, '../dist');
const RESULTS = resolve(here, '../../results');
const PORT = 5171;

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.wasm': 'application/wasm',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.map': 'application/json',
};

function dirStats(root) {
  let total = 0, js = 0, wasm = 0;
  const walk = (d) => {
    for (const e of readdirSync(d, { withFileTypes: true })) {
      const p = join(d, e.name);
      if (e.isDirectory()) walk(p);
      else {
        const s = statSync(p).size;
        total += s;
        if (p.endsWith('.js')) js += s;
        if (p.endsWith('.wasm')) wasm += s;
      }
    }
  };
  if (existsSync(root)) walk(root);
  const mb = (b) => +(b / 1048576).toFixed(3);
  return { js_bundle_mb: mb(js), wasm_mb: mb(wasm), total_web_dist_mb: mb(total) };
}

const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://x');

  if (req.method === 'POST' && url.pathname === '/__save') {
    const name = url.searchParams.get('name') || 'results.json';
    if (name.includes('..') || name.includes('/') || name.includes('\\')) {
      res.writeHead(400); res.end('bad name'); return;
    }
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      try {
        mkdirSync(RESULTS, { recursive: true });
        const p = join(RESULTS, name);
        writeFileSync(p, body);
        res.writeHead(200, { 'Content-Type': 'text/plain' });
        res.end(p);
      } catch (e) {
        res.writeHead(500); res.end(String(e));
      }
    });
    return;
  }

  if (req.method === 'GET' && url.pathname === '/__stats') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(dirStats(DIST)));
    return;
  }

  if (req.method !== 'GET') {
    res.writeHead(405); res.end(); return;
  }

  // Static file from dist/, SPA fallback to index.html.
  let p = normalize(decodeURIComponent(url.pathname));
  let file = join(DIST, p);
  if (!file.startsWith(DIST + sep) && file !== DIST) {
    res.writeHead(403); res.end(); return;
  }
  if (!existsSync(file) || statSync(file).isDirectory()) {
    file = join(DIST, 'index.html');
  }
  if (!existsSync(file)) {
    res.writeHead(503, { 'Content-Type': 'text/plain' });
    res.end('dist/ missing - run `npm run build` first');
    return;
  }
  res.writeHead(200, {
    'Content-Type': MIME[extname(file)] || 'application/octet-stream',
    'Content-Length': statSync(file).size,
  });
  createReadStream(file).pipe(res);
});

server.listen(PORT, () => {
  console.log(`serving ${DIST} at http://localhost:${PORT}`);
  console.log(`POST /__save?name=x.json writes ${RESULTS}\\x.json`);
});
