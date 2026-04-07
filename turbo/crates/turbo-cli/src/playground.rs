use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
            let token_ok = supplied_token.as_deref() == Some(token.as_str());
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
    // Write source to temp file
    let tmp = std::env::temp_dir().join(format!(
        "playground-{}-{}.tb",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    if std::fs::write(&tmp, source).is_err() {
        return (
            String::new(),
            "error: could not write temp file".to_string(),
            false,
        );
    }

    // Find our own binary
    let exe = std::env::current_exe().unwrap_or_else(|_| "turbolang".into());

    let mut child = match Command::new(&exe)
        .arg("run")
        .arg(&tmp)
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
                let _ = std::fs::remove_file(&tmp);
                return (String::new(), format!("error: {e}"), false);
            }
        }
    };

    let result = child.wait_with_output();

    let _ = std::fs::remove_file(&tmp);

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            // Replace temp filename in error messages with a friendlier name
            let stderr = stderr.replace(&tmp.display().to_string(), "playground");
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
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("turbo-{}-{ts:x}", std::process::id())
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
