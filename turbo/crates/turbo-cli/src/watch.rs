use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// Entry point for watch mode. Never returns (runs until Ctrl+C).
pub fn run_watch(entry_file: &Path, verbose: bool) -> ! {
    let entry_abs = entry_file.canonicalize().unwrap_or_else(|_| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: could not resolve file `{}`",
            entry_file.display()
        );
        std::process::exit(1);
    });

    let watch_root = find_watch_root(&entry_abs);

    // Set up Ctrl+C handler
    let (ctrlc_tx, ctrlc_rx) = mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = ctrlc_tx.send(());
    })
    .expect("failed to set Ctrl+C handler");

    // Set up file watcher
    let (fs_tx, fs_rx) = mpsc::channel();
    let mut debouncer = new_debouncer(
        Duration::from_millis(300),
        move |res: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
            if let Ok(events) = res {
                for event in events {
                    if event.kind == DebouncedEventKind::Any {
                        if should_ignore(&event.path) {
                            continue;
                        }
                        // Only care about .tb files
                        if event.path.extension().is_some_and(|ext| ext == "tb") {
                            let _ = fs_tx.send(event.path);
                            return; // One notification per batch is enough
                        }
                    }
                }
            }
        },
    )
    .expect("failed to create file watcher");

    debouncer
        .watcher()
        .watch(&watch_root, notify::RecursiveMode::Recursive)
        .unwrap_or_else(|e| {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: could not watch directory `{}`: {e}",
                watch_root.display()
            );
            std::process::exit(1);
        });

    // Initial run
    clear_screen();
    let start = std::time::Instant::now();
    let mut child = spawn_run(&entry_abs, verbose);
    print_banner(&entry_abs, &watch_root, start.elapsed());

    // Event loop: wait for file changes or Ctrl+C
    loop {
        // Check for Ctrl+C (non-blocking)
        if ctrlc_rx.try_recv().is_ok() {
            if let Some(ref mut c) = child {
                kill_child(c);
            }
            eprintln!("\n  \x1b[2mstopped\x1b[0m  watch mode\n");
            std::process::exit(0);
        }

        // Check for file changes (blocking with timeout to allow Ctrl+C checks)
        match fs_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(changed_path) => {
                // Drain any additional queued events
                while fs_rx.try_recv().is_ok() {}

                // Kill running child if any
                if let Some(ref mut c) = child {
                    kill_child(c);
                }

                // Clear and re-run
                clear_screen();
                print_change(&changed_path, &watch_root);

                let start = std::time::Instant::now();
                child = spawn_run(&entry_abs, verbose);
                print_banner(&entry_abs, &watch_root, start.elapsed());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Check if child exited
                if let Some(ref mut c) = child {
                    match c.try_wait() {
                        Ok(Some(status)) => {
                            if status.success() {
                                print_waiting();
                            } else {
                                print_waiting_after_error();
                            }
                            child = None; // Mark as done
                        }
                        Ok(None) => {} // Still running
                        Err(_) => {
                            child = None;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("\x1b[1;31merror\x1b[0m: file watcher disconnected");
                std::process::exit(1);
            }
        }
    }
}

/// Find the project root to watch.
/// If turbo.toml exists in any ancestor, use that directory.
/// Otherwise, use the parent directory of the entry file.
fn find_watch_root(entry_file: &Path) -> PathBuf {
    let abs = entry_file
        .canonicalize()
        .unwrap_or_else(|_| entry_file.to_path_buf());
    let mut dir = abs.parent().unwrap_or(Path::new(".")).to_path_buf();

    // Walk up looking for turbo.toml
    loop {
        if dir.join("turbo.toml").exists() {
            return dir;
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => break,
        }
    }

    // Fallback: parent of entry file
    abs.parent().unwrap_or(Path::new(".")).to_path_buf()
}

/// Returns true if a path should be ignored by the file watcher.
fn should_ignore(path: &Path) -> bool {
    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            let name = name.to_string_lossy();
            if name.starts_with('.')
                || name == "target"
                || name == "turbo_modules"
                || name == "node_modules"
            {
                return true;
            }
        }
    }
    false
}

/// Spawn `turbolang run <file>` as a child process.
/// Returns the Child handle for later termination.
fn spawn_run(entry_file: &Path, verbose: bool) -> Option<Child> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("turbolang"));

    let mut cmd = Command::new(&exe);
    cmd.arg("run");
    cmd.arg(entry_file);
    if verbose {
        cmd.arg("--verbose");
    }
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    // On Unix: spawn in its own process group so we can kill the group
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }

    match cmd.spawn() {
        Ok(child) => Some(child),
        Err(e) => {
            eprintln!("  \x1b[1;31merror\x1b[0m  failed to spawn: {e}");
            None
        }
    }
}

/// Kill a running child process and wait for it to exit.
fn kill_child(child: &mut Child) {
    // On Unix, kill the process group
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        unsafe {
            libc::kill(-pid, libc::SIGTERM); // negative PID = process group
        }
    }

    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }

    // Wait up to 2 seconds for graceful shutdown
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return, // exited
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(2) {
                    // Force kill
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return,
        }
    }
}

/// Clear the terminal screen.
fn clear_screen() {
    eprint!("\x1b[2J\x1b[H");
}

/// Print the watch mode banner after a (re)compile.
fn print_banner(entry_file: &Path, watch_root: &Path, elapsed: Duration) {
    let relative = entry_file.strip_prefix(watch_root).unwrap_or(entry_file);
    eprintln!("\n  \x1b[36mwatching\x1b[0m   {}", relative.display());
    eprintln!("  \x1b[32mready\x1b[0m      in {:.0?}\n", elapsed);
}

fn print_waiting() {
    eprintln!("\n  \x1b[2m[watching for changes... press ctrl+c to stop]\x1b[0m");
}

fn print_waiting_after_error() {
    eprintln!("\n  \x1b[2m[watching for changes... fix errors and save]\x1b[0m");
}

fn print_change(changed_path: &Path, watch_root: &Path) {
    let relative = changed_path
        .strip_prefix(watch_root)
        .unwrap_or(changed_path);
    eprintln!("\n  \x1b[33mchange\x1b[0m     {}", relative.display());
    eprintln!("  \x1b[36mreloading\x1b[0m  ...\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_ignore() {
        assert!(should_ignore(Path::new(".git/config")));
        assert!(should_ignore(Path::new("target/debug/foo")));
        assert!(should_ignore(Path::new("turbo_modules/dep/src/lib.tb")));
        assert!(should_ignore(Path::new("node_modules/foo/bar")));
        assert!(should_ignore(Path::new(".turbo/cache")));
        assert!(!should_ignore(Path::new("src/main.tb")));
        assert!(!should_ignore(Path::new("lib/utils.tb")));
    }

    #[test]
    fn test_find_watch_root_with_turbo_toml() {
        let tmp = std::env::temp_dir().join("turbo_watch_test");
        std::fs::create_dir_all(tmp.join("src")).ok();
        std::fs::write(tmp.join("turbo.toml"), "[package]\nname = \"test\"\n").ok();
        std::fs::write(tmp.join("src/main.tb"), "fn main() {}").ok();

        let root = find_watch_root(&tmp.join("src/main.tb"));
        assert_eq!(root.canonicalize().unwrap(), tmp.canonicalize().unwrap());

        // Cleanup
        std::fs::remove_dir_all(&tmp).ok();
    }
}
