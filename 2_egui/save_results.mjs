// save_results.mjs — collect the web benchmark JSON.
//
// The web bench (?bench=1) can't write files itself, so it:
//   1. console.log's the JSON between ===BENCH_JSON_BEGIN===/===BENCH_JSON_END===
//   2. stashes it on `window.__bench_json`
//   3. tries to auto-download `egui-web.json` (works in headed Chrome/Edge)
//
// Usage with any WebDriver/CDP client, or simply:
//   - open http://localhost:5172/?bench=1, wait ~40 s for "bench: DONE"
//   - either grab the downloaded egui-web.json, or run this in the console:
//       copy(window.__bench_json)
//   - save to ../results/egui-web.json
//
// Headless Edge/Chrome quick path (stderr console logging):
//   msedge --headless=new --enable-logging=stderr "http://localhost:5172/?bench=1" 2> bench.log
// then extract the line between the markers, e.g.:
//   node save_results.mjs bench.log ../results/egui-web.json

import { readFileSync, writeFileSync } from "node:fs";

const [logFile, outFile = "../results/egui-web.json"] = process.argv.slice(2);
if (!logFile) {
  console.error("usage: node save_results.mjs <console-log-file> [out.json]");
  process.exit(1);
}
const text = readFileSync(logFile, "utf8");
// stderr logging wraps the console message in quotes: INFO:CONSOLE(n) "{...}"
const m = text.match(/==BENCH_JSON_BEGIN==[^]*?CONSOLE:\d+\] "(\{[^]*?\})", source[^]*?==BENCH_JSON_END==/);
let json;
if (m) {
  json = m[1].replace(/\\"/g, '"').replace(/\\n/g, "\n").replace(/\\\\/g, "\\");
} else {
  // fallback: plain text extraction (e.g. copy-pasted console)
  const a = text.indexOf("==BENCH_JSON_BEGIN==");
  const b = text.indexOf("==BENCH_JSON_END==");
  if (a < 0 || b < 0) {
    console.error("markers not found in log file");
    process.exit(1);
  }
  json = text.slice(a, b);
  const s = json.indexOf("{");
  const e = json.lastIndexOf("}");
  json = json.slice(s, e + 1);
}
JSON.parse(json); // validate
writeFileSync(outFile, json.endsWith("\n") ? json : json + "\n");
console.log(`wrote ${outFile}`);
