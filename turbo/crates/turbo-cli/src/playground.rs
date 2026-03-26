use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::time::Duration;

const HTML: &str = include_str!("playground.html");
const BENCHMARKS_HTML: &str = include_str!("benchmarks.html");

pub fn serve(port: u16) {
    let addr = format!("127.0.0.1:{port}");
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
    { let _ = Command::new("open").arg(format!("http://localhost:{port}")).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = Command::new("xdg-open").arg(format!("http://localhost:{port}")).spawn(); }

    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        let mut buf = [0u8; 16384];
        let n = match stream.read(&mut buf) {
            Ok(n) => n,
            Err(_) => continue,
        };
        if n == 0 { continue; }
        let request = String::from_utf8_lossy(&buf[..n]);

        if request.starts_with("GET / ") || request.starts_with("GET / HTTP") {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                HTML.len(),
                HTML
            );
            let _ = stream.write_all(response.as_bytes());
        } else if request.starts_with("GET /benchmarks") {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                BENCHMARKS_HTML.len(),
                BENCHMARKS_HTML
            );
            let _ = stream.write_all(response.as_bytes());
        } else if request.starts_with("POST /api/run") {
            // Extract body (after \r\n\r\n)
            let body = request
                .find("\r\n\r\n")
                .map(|i| request[i + 4..].trim_end_matches('\0'))
                .unwrap_or("");

            let result = run_code(body);
            let json = format_json(&result.0, &result.1, result.2);

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(response.as_bytes());
        } else if request.starts_with("GET /favicon") {
            let response = "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n";
            let _ = stream.write_all(response.as_bytes());
        } else if request.starts_with("OPTIONS") {
            let response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n";
            let _ = stream.write_all(response.as_bytes());
        } else {
            let response = "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\nNot Found";
            let _ = stream.write_all(response.as_bytes());
        }
    }
}

fn run_code(source: &str) -> (String, String, bool) {
    // Write source to temp file
    let tmp = std::env::temp_dir().join("playground.tb");
    if std::fs::write(&tmp, source).is_err() {
        return (String::new(), "error: could not write temp file".to_string(), false);
    }

    // Find our own binary
    let exe = std::env::current_exe().unwrap_or_else(|_| "turbo".into());

    // Run with timeout
    let result = Command::new(&exe)
        .arg("run")
        .arg(&tmp)
        .output();

    let _ = std::fs::remove_file(&tmp);

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            // Replace temp filename in error messages with a friendlier name
            let stderr = stderr.replace("playground.tb", "playground");
            let ok = output.status.success();
            (stdout, stderr, ok)
        }
        Err(e) => (String::new(), format!("error: {e}"), false),
    }
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
