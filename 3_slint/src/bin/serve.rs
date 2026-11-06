//! Minimal std-only static file server on http://localhost:5173.
//! Serves index.html + pkg/ (wasm-pack output) and accepts
//! `POST /save?name=<file>` to persist the web bench JSON into ../results/.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

fn content_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("css") => "text/css",
        Some("map") => "application/json",
        _ => "application/octet-stream",
    }
}

fn respond(s: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
    let _ = s.flush();
}

fn handle(mut s: TcpStream, root: &Path, results: &Path) {
    let mut req = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    // Read headers + possibly the body in one or more reads.
    let head_end;
    loop {
        match s.read(&mut chunk) {
            Ok(0) => return,
            Ok(n) => {
                req.extend_from_slice(&chunk[..n]);
                if let Some(pos) = find_subslice(&req, b"\r\n\r\n") {
                    head_end = pos + 4;
                    break;
                }
                if req.len() > 1 << 20 {
                    return;
                }
            }
            Err(_) => return,
        }
    }
    let head = String::from_utf8_lossy(&req[..head_end]).to_string();
    let mut lines = head.lines();
    let mut parts = lines.next().unwrap_or("").split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let content_len: usize = head
        .lines()
        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:")?.trim().parse().ok())
        .unwrap_or(0);

    // Read the rest of the body if needed.
    while req.len() - head_end < content_len {
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => req.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
    }
    let body = &req[head_end..head_end + content_len.min(req.len() - head_end)];

    if method == "POST" && target.starts_with("/save") {
        let name = target
            .split("name=")
            .nth(1)
            .and_then(|v| v.split('&').next())
            .filter(|n| !n.is_empty() && !n.contains("..") && !n.contains('/') && !n.contains('\\'))
            .unwrap_or("slint-web.json");
        let _ = std::fs::create_dir_all(results);
        let path = results.join(name);
        return match std::fs::write(&path, body) {
            Ok(()) => {
                println!("saved {}", path.display());
                respond(&mut s, "200 OK", "text/plain", b"ok");
            }
            Err(e) => respond(&mut s, "500 Internal Server Error", "text/plain", e.to_string().as_bytes()),
        };
    }

    if method != "GET" {
        return respond(&mut s, "405 Method Not Allowed", "text/plain", b"method not allowed");
    }

    let path_only = target.split('?').next().unwrap_or("/");
    let rel = if path_only == "/" { "/index.html" } else { path_only };
    // Percent-decode the few escapes that matter.
    let rel = rel.replace("%20", " ");
    if rel.contains("..") {
        return respond(&mut s, "403 Forbidden", "text/plain", b"forbidden");
    }
    let file = root.join(rel.trim_start_matches('/'));
    match std::fs::read(&file) {
        Ok(data) => respond(&mut s, "200 OK", content_type(&file), &data),
        Err(_) => respond(&mut s, "404 Not Found", "text/plain", b"not found"),
    }
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let results = root.join("..").join("results");
    if !root.join("pkg").join("slint_testbed.js").exists() {
        eprintln!("warning: pkg/ not found — run ./build-web.sh first");
    }
    let listener = match TcpListener::bind("127.0.0.1:5173") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot bind 127.0.0.1:5173: {e}");
            std::process::exit(1);
        }
    };
    println!("Serving {} at http://localhost:5173 (Ctrl-C to stop)", root.display());
    for stream in listener.incoming().flatten() {
        handle(stream, &root, &results);
    }
}
