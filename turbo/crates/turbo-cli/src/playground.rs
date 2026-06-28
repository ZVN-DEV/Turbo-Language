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
            // The compiler renders diagnostics with ariadne, which colorizes
            // output unconditionally (even when stderr is a pipe, as it is
            // here) and prints against the temp file's *basename*. Strip the
            // ANSI escapes and scrub the random temp filename before the text
            // reaches the JSON response — the page renders it through
            // escapeHtml(), which would otherwise show literal escape
            // sequences and leak `turbo-playground-XXXXXX.tb`.
            let stderr = sanitize_stderr(&stderr, &tmp_path);
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

/// Sanitize captured compiler stderr for display in the playground.
///
/// Two transformations, in order:
///   1. Strip ANSI escape sequences (ariadne colorizes unconditionally, even
///      when stderr is piped, so the captured bytes contain SGR color codes).
///   2. Replace the internal temp filename with a stable, friendly name. The
///      full path is replaced *before* the basename: the basename is a
///      substring of the full path, so doing it the other way around would
///      leave the temp directory prefix behind.
fn sanitize_stderr(stderr: &str, tmp_path: &std::path::Path) -> String {
    const FRIENDLY: &str = "playground.tb";
    let stderr = strip_ansi(stderr);
    let stderr = stderr.replace(&tmp_path.display().to_string(), FRIENDLY);
    match tmp_path.file_name().and_then(|n| n.to_str()) {
        Some(base) => stderr.replace(base, FRIENDLY),
        None => stderr,
    }
}

/// Strip ANSI escape sequences from a string.
///
/// The page renders compiler output as plain escaped HTML, so any leftover
/// terminal escape sequences would surface as literal gibberish (e.g.
/// `\x1b[31m`). Handles the two forms ariadne can emit:
///
/// * CSI / SGR: `ESC [ ... <final byte 0x40-0x7E>` (e.g. `ESC[31m`, `ESC[0m`)
/// * OSC: `ESC ] ... (BEL | ST)` where ST is the two-byte `ESC \`
///
/// Any other lone `ESC` is simply dropped.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next(); // consume '['
                              // Consume parameter/intermediate bytes up to and including the
                              // final byte in the range 0x40..=0x7E.
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if ('\u{40}'..='\u{7e}').contains(&nc) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next(); // consume ']'
                              // Consume until BEL or the two-byte ST (`ESC \`).
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc == '\x07' {
                        break;
                    }
                    if nc == '\x1b' {
                        if chars.peek().copied() == Some('\\') {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            // Lone ESC or an unrecognized escape: drop just the ESC byte.
            _ => {}
        }
    }
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
    fn strip_ansi_removes_color_codes() {
        // A representative ariadne-style colored error fragment.
        let colored = "\x1b[31mError:\x1b[0m something \x1b[1;33mwrong\x1b[0m";
        let plain = strip_ansi(colored);
        assert_eq!(plain, "Error: something wrong");
        assert!(!plain.contains('\x1b'), "escape leaked: {plain:?}");
    }

    #[test]
    fn strip_ansi_preserves_plain_text() {
        let s = "no escapes here\nline two\tindented";
        assert_eq!(strip_ansi(s), s);
    }

    #[test]
    fn sanitized_stderr_has_no_ansi_or_temp_name() {
        // Mirror exactly what the compiler subprocess emits: colorized ariadne
        // output referencing the temp file by full path and by basename.
        let tmp_path = std::path::PathBuf::from("/var/folders/xy/turbo-playground-95SRiR.tb");
        let raw = "\x1b[31mError:\x1b[0m error[E0109] at \
                   /var/folders/xy/turbo-playground-95SRiR.tb:1:13\n   \
                   turbo-playground-95SRiR.tb:1:13 expected expression\x1b[0m";
        let sanitized = sanitize_stderr(raw, &tmp_path);
        assert!(
            !sanitized.contains('\x1b'),
            "ANSI escape leaked: {sanitized:?}"
        );
        assert!(
            !sanitized.contains("turbo-playground-"),
            "temp basename leaked: {sanitized:?}"
        );
        assert!(
            sanitized.contains("playground.tb:1:13"),
            "friendly name missing: {sanitized:?}"
        );
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
