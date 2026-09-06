//! Exercise the real example and serializer, through JIT and native AOT.
//! HTTP tests are Unix-only until Windows AOT networking is supported.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

fn compiler() -> Command {
    Command::new(env!("CARGO_BIN_EXE_turbolang"))
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn build(source: &Path, binary: &Path) {
    let output = compiler()
        .arg("build")
        .arg(source)
        .arg("-o")
        .arg(binary)
        .output()
        .expect("build compiler invocation");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn serializers_roundtrip_every_non_nul_control_character() {
    let dir = tempfile::tempdir().unwrap();
    let source = repo().join("turbo/tests/phase1/json_serialize_controls.tb");
    let mut title = "quote: \" slash: \\ Unicode: café 🌍 ".to_owned();
    title.extend((1u8..32).map(char::from));
    let expected = json!({"title": title, "count": 7, "active": true});
    let binary = dir.path().join("serialize.exe");
    build(&source, &binary);
    let mut encodings = Vec::new();
    for mut command in [
        {
            let mut c = compiler();
            c.arg("run").arg(&source);
            c
        },
        Command::new(&binary),
    ] {
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let values: Vec<Value> = stdout
            .lines()
            .map(|line| {
                serde_json::from_str(line).unwrap_or_else(|e| panic!("invalid JSON: {e}: {line:?}"))
            })
            .collect();
        assert_eq!(
            values,
            vec![
                expected.clone(),
                json!([expected]),
                json!({"title": title}),
                json!(title)
            ]
        );
        encodings.push(stdout);
    }
    assert_eq!(encodings[0], encodings[1], "JIT/AOT JSON encoding drift");
}

#[cfg(unix)]
mod http {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::process::{Child, Stdio};
    use std::time::{Duration, Instant};

    struct Server(Child);

    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn request(port: u16, method: &str, path: &str, body: &str) -> std::io::Result<(u16, Value)> {
        let mut stream =
            TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(1))?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_write_timeout(Some(Duration::from_secs(3)))?;
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len())?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        let (head, body) = response
            .split_once("\r\n\r\n")
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "HTTP framing"))?;
        let status = head
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .parse()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let value = serde_json::from_str(body).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e}: {body:?}"))
        })?;
        Ok((status, value))
    }

    fn start(mut command: Command, dir: &Path, port: u16, ready_path: &str) -> Option<Server> {
        let error_path = dir.join("server.stderr");
        let mut server = Server(
            command
                .current_dir(dir)
                .stdout(Stdio::null())
                .stderr(std::fs::File::create(&error_path).unwrap())
                .spawn()
                .unwrap(),
        );
        // Fresh native binaries can be delayed by host code validation (and
        // slow CI). This is a correctness/readiness guard, not a startup
        // performance benchmark; per-request I/O deadlines remain unchanged.
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(status) = server.0.try_wait().unwrap() {
                let stderr = std::fs::read_to_string(&error_path).unwrap();
                if stderr.contains("Address already in use") || stderr.contains("AddrInUse") {
                    return None;
                }
                panic!("server exited before ready ({status}): {stderr}");
            }
            // A unique route proves this is our child, not the process that
            // might have raced us for the just-released ephemeral port.
            if let Ok((200, Value::Bool(true))) = request(port, "GET", ready_path, "") {
                return Some(server);
            }
            assert!(Instant::now() < deadline, "server did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn launch(aot: bool, dir: &Path) -> (Server, u16) {
        let source = dir.join("main.tb");
        let binary = dir.join("api");
        let example =
            std::fs::read_to_string(repo().join("examples/http-sqlite-api/main.tb")).unwrap();
        assert_eq!(example.matches("http_server(8080)").count(), 1);
        let ready_path = format!(
            "/__turbo_ready_{}",
            dir.file_name().unwrap().to_str().unwrap()
        );
        for _ in 0..3 {
            // Hold the reservation while building. Retry only a diagnosed bind
            // collision, never a compiler failure or a business-logic assertion.
            let reservation = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = reservation.local_addr().unwrap().port();
            let listener = format!("http_server({port})\n    route(app, \"GET\", \"{ready_path}\", |req: str| -> str {{ respond_json(200, \"true\") }})");
            std::fs::write(&source, example.replace("http_server(8080)", &listener)).unwrap();
            if aot {
                build(&source, &binary);
            }
            let command = if aot {
                Command::new(&binary)
            } else {
                let mut c = compiler();
                c.arg("run").arg(&source);
                c
            };
            drop(reservation);
            if let Some(server) = start(command, dir, port, &ready_path) {
                return (server, port);
            }
        }
        panic!("three ephemeral-port bind collisions");
    }

    fn roundtrip(aot: bool) {
        let dir = tempfile::tempdir().unwrap();
        let (server, port) = launch(aot, dir.path());
        let titles = [
            "line one\nline two\t\r\u{8}\u{c}".to_owned(),
            "quote \" backslash \\ café 🌍 '); DROP TABLE todos; --".to_owned(),
            (1u8..32).map(char::from).collect::<String>(),
            String::new(),
        ];
        let mut inserted = Vec::new();
        for title in &titles {
            let (status, result) = request(port, "POST", "/todos", title).unwrap();
            assert_eq!(status, 201);
            assert_eq!(result, json!({"ok":true, "title":title}));
            let (status, list) = request(port, "GET", "/todos", "").unwrap();
            assert_eq!(status, 200);
            let last = list.as_array().unwrap().last().unwrap().clone();
            assert_eq!(last["title"], *title);
            assert_eq!(last["done"], 0);
            inserted.push(last);
        }
        drop(server); // Abrupt shutdown also proves committed SQLite persistence.
        let (_restarted, port) = launch(aot, dir.path());
        let (status, list) = request(port, "GET", "/todos", "").unwrap();
        assert_eq!(status, 200);
        for row in inserted {
            assert!(list.as_array().unwrap().contains(&row));
        }
    }

    #[test]
    fn jit_http_sqlite_post_get_restart() {
        roundtrip(false);
    }

    #[test]
    fn aot_http_sqlite_post_get_restart() {
        roundtrip(true);
    }

    #[test]
    fn startup_identifies_bind_collision() {
        let dir = tempfile::tempdir().unwrap();
        let reservation = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = reservation.local_addr().unwrap().port();
        let source = dir.path().join("collision.tb");
        std::fs::write(
            &source,
            format!("fn main() {{ http_listen(http_server({port})) }}"),
        )
        .unwrap();
        let mut command = compiler();
        command.arg("run").arg(source);
        assert!(start(command, dir.path(), port, "/not_ready").is_none());
    }

    #[test]
    #[should_panic(expected = "server exited before ready")]
    fn startup_does_not_retry_compiler_errors() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("invalid.tb");
        std::fs::write(&source, "fn main( {").unwrap();
        let mut command = compiler();
        command.arg("run").arg(source);
        let _ = start(command, dir.path(), 0, "/not_ready");
    }
}
