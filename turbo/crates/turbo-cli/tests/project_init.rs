use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn cli(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_turbolang"))
        .current_dir(directory)
        .args(args)
        .output()
        .expect("run CLI")
}

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

const OUTPUTS: [&str; 4] = [
    "turbo.toml",
    "src/main.tb",
    "tests/main_test.tb",
    ".gitignore",
];

#[test]
fn init_rejects_every_existing_output_before_writing() {
    for existing in OUTPUTS {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(existing);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"user-owned sentinel\n").unwrap();
        fs::write(dir.path().join("unrelated.txt"), b"keep me").unwrap();

        let output = cli(dir.path(), &["init", "."]);
        assert!(!output.status.success(), "init overwrote {existing}");
        assert_eq!(fs::read(&path).unwrap(), b"user-owned sentinel\n");
        assert_eq!(
            fs::read(dir.path().join("unrelated.txt")).unwrap(),
            b"keep me"
        );
        for other in OUTPUTS {
            if other != existing {
                assert!(
                    !dir.path().join(other).exists(),
                    "partial scaffold: {other}"
                );
            }
        }
    }
}

#[test]
fn init_rejects_non_directory_parents_before_writing() {
    for parent in ["src", "tests"] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(parent), b"not a directory").unwrap();
        let output = cli(dir.path(), &["init", "."]);
        assert!(!output.status.success());
        assert_eq!(
            fs::read(dir.path().join(parent)).unwrap(),
            b"not a directory"
        );
        assert!(!dir.path().join("turbo.toml").exists());
        assert!(!dir.path().join(".gitignore").exists());
        let other_parent = if parent == "src" { "tests" } else { "src" };
        assert!(!dir.path().join(other_parent).exists());
    }
}

#[cfg(unix)]
#[test]
fn init_does_not_follow_scaffold_symlinks() {
    use std::os::unix::fs::symlink;
    for parent in ["src", "tests"] {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), dir.path().join(parent)).unwrap();
        assert!(!cli(dir.path(), &["init", "."]).status.success());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        assert!(!dir.path().join("turbo.toml").exists());
    }
    for output in OUTPUTS {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(output);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        symlink("missing-target", &path).unwrap();
        assert!(!cli(dir.path(), &["init", "."]).status.success());
        assert!(fs::symlink_metadata(path).unwrap().file_type().is_symlink());
        assert!(!dir.path().join(".gitignore").exists());
    }
}

#[test]
fn newly_initialized_projects_complete_the_documented_workflow() {
    for current_directory in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("fresh app");
        if current_directory {
            fs::create_dir(&project).unwrap();
            success(cli(&project, &["init", "."]));
        } else {
            success(cli(dir.path(), &["init", "fresh app"]));
        }
        success(cli(&project, &["check"]));
        let jit = success(cli(&project, &["run"]));
        assert!(String::from_utf8_lossy(&jit.stdout).contains("Hello from fresh app!"));
        let tests = success(cli(&project, &["test"]));
        assert!(String::from_utf8_lossy(&tests.stderr).contains("3 passed, 0 failed"));
        success(cli(&project, &["fmt", "--check"]));
        let binary = if cfg!(windows) { "app.exe" } else { "app" };
        success(cli(&project, &["build", "-o", binary]));
        let aot = success(Command::new(project.join(binary)).output().unwrap());
        // Console newline translation on Windows is not a semantic difference.
        assert_eq!(
            String::from_utf8_lossy(&jit.stdout).replace("\r\n", "\n"),
            String::from_utf8_lossy(&aot.stdout).replace("\r\n", "\n")
        );
    }
}

#[test]
fn init_supports_new_nested_paths_and_preserves_unrelated_files() {
    let dir = tempfile::tempdir().unwrap();
    success(cli(dir.path(), &["init", "nested/fresh app"]));
    assert!(dir.path().join("nested/fresh app/src/main.tb").is_file());
    // The safe default still permits adding a project to a directory that
    // contains unrelated files and real source/test directories.
    let current = dir.path().join("existing");
    fs::create_dir_all(current.join("src")).unwrap();
    fs::create_dir_all(current.join("tests")).unwrap();
    fs::write(current.join("src/keep.tb"), b"fn keep() {}\n").unwrap();
    fs::write(current.join("README.md"), b"user docs").unwrap();
    success(cli(&current, &["init", "."]));
    assert_eq!(
        fs::read(current.join("src/keep.tb")).unwrap(),
        b"fn keep() {}\n"
    );
    assert_eq!(fs::read(current.join("README.md")).unwrap(), b"user docs");
}
