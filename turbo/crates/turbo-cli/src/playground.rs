use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::time::Duration;

use tempfile::Builder;

const HTML: &str = include_str!("playground.html");
const BENCHMARKS_HTML: &str = include_str!("benchmarks.html");

pub fn serve(port: u16) {
    let addr = format!("127.0.0.1:{port}");
    let origin_localhost = format!("http://localhost:{port}");
    let origin_loopback = format!("http://127.0.0.1:{port}");
    let token = generate_playground_token();
    let listener = TcpListener::bind(&addr).unwrap_or_else(|e| {
        eprintln!("error: could not bind to {addr}: {e}");
        std::process::exit(1);
    });

    println!();
    println!("  \x1b[1;35m⚡ Turbo Playground\x1b[0m");
    println!("  \x1b[90m─────────────────────────────\x1b[0m");
    println!("  \x1b[1mhttp://localhost:{port}\x1b[0m");
    println!("  \x1b[1mhttp://localhost:{port}/benchmarks\x1b[0m");
    println!();
    println!("  \x1b[90mPress Ctrl+C to stop\x1b[0m");
    println!();

    // Try to open browser
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open")
            .arg(format!("http://localhost:{port}"))
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("xdg-open")
            .arg(format!("http://localhost:{port}"))
            .spawn();
    }

    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        let mut buf = [0u8; 16384];
        let n = match stream.read(&mut buf) {
            Ok(n) => n,
            Err(_) => continue,
        };
        if n == 0 {
            continue;
        }
        let request = String::from_utf8_lossy(&buf[..n]);

        if request.starts_with("GET / ") || request.starts_with("GET / HTTP") {
            let html = HTML.replace("__PLAYGROUND_TOKEN__", &token);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nContent-Security-Policy: default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: strict-origin-when-cross-origin\r\n\r\n{}",
                html.len(),
                html
            );
            let _ = stream.write_all(response.as_bytes());
        } else if request.starts_with("GET /benchmarks") {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nContent-Security-Policy: default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: strict-origin-when-cross-origin\r\n\r\n{}",
                BENCHMARKS_HTML.len(),
                BENCHMARKS_HTML
            );
            let _ = stream.write_all(response.as_bytes());
        } else if request.starts_with("POST /api/run") {
            let origin = header_value(&request, "Origin");
            let supplied_token = header_value(&request, "X-Playground-Token");
            let origin_ok = matches!(
                origin.as_deref(),
                Some(o) if o == origin_localhost || o == origin_loopback
            );
            let token_ok = supplied_token
                .as_deref()
                .map(|s| constant_time_eq(s.as_bytes(), token.as_bytes()))
                .unwrap_or(false);
            if !origin_ok || !token_ok {
                let response = "HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 9\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\n\r\nForbidden";
                let _ = stream.write_all(response.as_bytes());
                continue;
            }
            // Extract body (after \r\n\r\n)
            let body = request
                .find("\r\n\r\n")
                .map(|i| request[i + 4..].trim_end_matches('\0'))
                .unwrap_or("");

            let result = run_code(body);
            let json = format_json(&result.0, &result.1, result.2);

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nContent-Security-Policy: default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: strict-origin-when-cross-origin\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(response.as_bytes());
        } else if request.starts_with("GET /favicon") {
            let response = "HTTP/1.1 204 No Content\r\nConnection: close\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\n\r\n";
            let _ = stream.write_all(response.as_bytes());
        } else if request.starts_with("OPTIONS") {
            let response =
                "HTTP/1.1 405 Method Not Allowed\r\nConnection: close\r\nContent-Length: 0\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\n\r\n";
            let _ = stream.write_all(response.as_bytes());
        } else {
            let response = "HTTP/1.1 404 Not Found\r\nConnection: close\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\n\r\nNot Found";
            let _ = stream.write_all(response.as_bytes());
        }
    }
}

fn run_code(source: &str) -> (String, String, bool) {
    // Write source to a tempfile opened with O_EXCL|O_CREAT plus a random
    // suffix. This closes the TOCTOU race that existed when the path was
    // predictable (playground-{pid}-{ts}.tb): an attacker on the same host
    // could have pre-created a symlink at the predicted path.
    let mut tmp = match Builder::new()
        .prefix("turbo-playground-")
        .suffix(".tb")
        .tempfile()
    {
        Ok(f) => f,
        Err(e) => {
            return (
                String::new(),
                format!("error: could not create temp file: {e}"),
                false,
            );
        }
    };
    if let Err(e) = tmp.write_all(source.as_bytes()) {
        return (
            String::new(),
            format!("error: could not write temp file: {e}"),
            false,
        );
    }
    if let Err(e) = tmp.as_file_mut().sync_all() {
        return (
            String::new(),
            format!("error: could not flush temp file: {e}"),
            false,
        );
    }
    let tmp_path = tmp.path().to_path_buf();

    // Find our own binary
    let exe = std::env::current_exe().unwrap_or_else(|_| "turbolang".into());

    let mut child = match Command::new(&exe)
        .arg("run")
        .arg(&tmp_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return (String::new(), format!("error: {e}"), false),
    };

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let timed_out = loop {
        match child.try_wait() {
            Ok(Some(_)) => break false,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break true;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => {
                // `tmp` is dropped at the end of the function and
                // auto-deletes the file — no explicit cleanup needed.
                return (String::new(), format!("error: {e}"), false);
            }
        }
    };

    let result = child.wait_with_output();

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            // Replace temp filename in error messages with a friendlier name
            let stderr = stderr.replace(&tmp_path.display().to_string(), "playground");
            let stderr = stderr.replace("playground.tb", "playground");
            let stderr = if timed_out {
                if stderr.is_empty() {
                    "error: execution timed out after 5s".to_string()
                } else {
                    format!("{stderr}\nerror: execution timed out after 5s")
                }
            } else {
                stderr
            };
            let ok = output.status.success() && !timed_out;
            // Drop `tmp` here; NamedTempFile removes the file on drop.
            drop(tmp);
            (stdout, stderr, ok)
        }
        Err(e) => (String::new(), format!("error: {e}"), false),
    }
}

fn header_value(request: &str, header_name: &str) -> Option<String> {
    let needle = format!("{header_name}:");
    for line in request.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case(header_name) {
                return Some(value.trim().to_string());
            }
        } else if line.eq_ignore_ascii_case(&needle) {
            return Some(String::new());
        }
    }
    None
}

fn generate_playground_token() -> String {
    // 128 bits of OS randomness. Previous PID+timestamp tokens were
    // predictable to a local attacker, enabling CSRF-style attacks against
    // the playground's /api/run endpoint.
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("OS RNG failure");
    let mut hex = String::with_capacity(32);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02x}");
    }
    format!("turbo-{hex}")
}

/// Constant-time byte-slice equality. Length mismatch short-circuits (a
/// length difference is not a secret), but same-length comparisons run in
/// time that depends only on the length of the inputs.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Manual JSON serialization (no external dependency needed)
fn format_json(stdout: &str, stderr: &str, success: bool) -> String {
    format!(
        r#"{{"stdout":{},"stderr":{},"success":{}}}"#,
        json_string(stdout),
        json_string(stderr),
        success
    )
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn playground_token_is_random_and_well_formed() {
        let mut seen = HashSet::new();
        for _ in 0..100 {
            let tok = generate_playground_token();
            // Format: "turbo-" + 32 lowercase hex chars.
            assert!(tok.starts_with("turbo-"), "unexpected prefix: {tok}");
            let hex = &tok["turbo-".len()..];
            assert_eq!(hex.len(), 32, "hex length mismatch in {tok}");
            assert!(
                hex.chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
                "non-hex characters in {tok}"
            );
            assert!(seen.insert(tok), "duplicate token generated");
        }
    }

    #[test]
    fn constant_time_eq_matches_expected() {
        assert!(constant_time_eq(b"", b""));
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"abcd", b"abc"));
    }

    #[test]
    fn forged_token_off_by_one_is_rejected() {
        // Simulate the check performed in the /api/run handler.
        let real = generate_playground_token();
        // Flip the last hex character.
        let mut bytes: Vec<u8> = real.as_bytes().to_vec();
        let last = bytes.last_mut().expect("non-empty token");
        *last = if *last == b'0' { b'1' } else { b'0' };
        let forged = String::from_utf8(bytes).unwrap();
        assert_ne!(forged, real);
        assert!(!constant_time_eq(forged.as_bytes(), real.as_bytes()));
    }

    #[test]
    fn concurrent_source_file_creation_does_not_collide() {
        // We can't invoke `run_code` in a unit test (it would shell out to
        // `turbolang run`), but the collision risk lives in the tempfile
        // creation — exercise that directly in the same way `run_code`
        // does, from many threads.
        let mut handles = Vec::new();
        let seen: Arc<std::sync::Mutex<HashSet<std::path::PathBuf>>> =
            Arc::new(std::sync::Mutex::new(HashSet::new()));
        for i in 0..10 {
            let seen = Arc::clone(&seen);
            handles.push(thread::spawn(move || {
                let mut tmp = Builder::new()
                    .prefix("turbo-playground-")
                    .suffix(".tb")
                    .tempfile()
                    .expect("tempfile creation failed");
                let body = format!("fn main() {{ print({i}) }}\n");
                tmp.write_all(body.as_bytes())
                    .expect("write to tempfile failed");
                let path = tmp.path().to_path_buf();
                let mut guard = seen.lock().unwrap();
                assert!(
                    guard.insert(path.clone()),
                    "tempfile path collided: {path:?}"
                );
            }));
        }
        for h in handles {
            h.join().expect("worker thread panicked");
        }
        assert_eq!(seen.lock().unwrap().len(), 10);
    }
}
