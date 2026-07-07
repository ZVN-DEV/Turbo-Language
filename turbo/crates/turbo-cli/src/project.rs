//! Project scaffolding (`turbolang init`) and `turbo.toml` reading helpers.

use std::path::PathBuf;

use crate::diagnostics::io_reason;

/// Initialize a new Turbo project with the given name.
///
/// Passing `.` (or an empty name) scaffolds into the current directory instead
/// of creating a new one; the package name is then taken from the current
/// directory's name.
pub(crate) fn init_project(name: &str) {
    let into_current = name == "." || name.is_empty();
    let dir = PathBuf::from(if into_current { "." } else { name });
    let pkg_name = if into_current {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "app".to_string())
    } else {
        dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| name.to_string())
    };

    if into_current {
        if dir.join("turbo.toml").exists() {
            eprintln!(
                "\x1b[1;31merror\x1b[0m: `turbo.toml` already exists in the current directory"
            );
            std::process::exit(1);
        }
    } else if dir.exists() {
        eprintln!("\x1b[1;31merror\x1b[0m: directory `{name}` already exists");
        std::process::exit(1);
    }

    std::fs::create_dir_all(dir.join("src")).unwrap_or_else(|e| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: could not create directory: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    });
    std::fs::create_dir_all(dir.join("tests")).unwrap_or_else(|e| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: could not create directory: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    });

    // turbo.toml
    std::fs::write(
        dir.join("turbo.toml"),
        format!(
            "[package]\nname = \"{pkg_name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[dependencies]\n"
        ),
    )
    .unwrap_or_else(|e| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: failed to write turbo.toml: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    });

    // src/main.tb
    std::fs::write(
        dir.join("src/main.tb"),
        format!(
            r#"/// A counter that tracks a value
struct Counter {{
    count: i64,
}}

impl Counter {{
    fn increment(self) -> Counter {{
        Counter {{ count: self.count + 1 }}
    }}

    fn value(self) -> i64 {{
        self.count
    }}
}}

/// Shapes with area calculation
type Shape {{
    Circle(f64),
    Rectangle(f64, f64),
}}

fn area(shape: Shape) -> f64 {{
    match shape {{
        Circle(r) => 3.14159 * r * r
        Rectangle(w, h) => w * h
    }}
}}

fn main() {{
    print("Hello from {pkg_name}!")

    // Struct with methods
    let mut c = Counter {{ count: 0 }}
    c = c.increment()
    c = c.increment()
    c = c.increment()
    print("Counter: " + to_str(c.value()))

    // Enum + pattern matching
    let circle = Shape.Circle(5.0)
    let rect = Shape.Rectangle(4.0, 6.0)
    print("Circle area: " + to_str(area(circle)))
    print("Rectangle area: " + to_str(area(rect)))
}}
"#
        ),
    )
    .unwrap_or_else(|e| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: failed to write src/main.tb: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    });

    // tests/main_test.tb
    std::fs::write(
        dir.join("tests/main_test.tb"),
        r#"struct Counter {
    count: i64,
}

impl Counter {
    fn increment(self) -> Counter {
        Counter { count: self.count + 1 }
    }

    fn value(self) -> i64 {
        self.count
    }
}

type Shape {
    Circle(f64),
    Rectangle(f64, f64),
}

pub(crate) fn area(shape: Shape) -> f64 {
    match shape {
        Circle(r) => 3.14159 * r * r
        Rectangle(w, h) => w * h
    }
}

@test fn test_counter() {
    let c = Counter { count: 0 }
    assert(c.value() == 0, "new counter starts at 0")
    let c2 = c.increment()
    assert(c2.value() == 1, "increment adds 1")
}

@test fn test_area() {
    let rect = Shape.Rectangle(3.0, 4.0)
    assert(area(rect) == 12.0, "rectangle area is w * h")
}

@test fn test_math() {
    assert(1 + 1 == 2, "basic math works")
}
"#,
    )
    .unwrap_or_else(|e| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: failed to write tests/main_test.tb: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    });

    // .gitignore
    std::fs::write(dir.join(".gitignore"), "turbo_modules/\ntarget/\n*.o\n").unwrap_or_else(|e| {
        eprintln!(
            "\x1b[1;31merror\x1b[0m: failed to write .gitignore: {}",
            io_reason(&e)
        );
        std::process::exit(1);
    });

    if into_current {
        eprintln!("\x1b[32m\u{2713}\x1b[0m Created project `{pkg_name}`");
        eprintln!("  turbolang run");
    } else {
        eprintln!("\x1b[32m\u{2713}\x1b[0m Created project `{name}`");
        eprintln!("  cd {name} && turbolang run");
    }
}

/// Read the project name from `turbo.toml` in the current directory, if it exists.
pub(crate) fn read_project_name() -> Option<PathBuf> {
    let toml = std::fs::read_to_string("turbo.toml").ok()?;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name") {
            if let Some(val) = trimmed.split('=').nth(1) {
                let name = val.trim().trim_matches('"').trim_matches('\'');
                if !name.is_empty() {
                    return Some(PathBuf::from(name));
                }
            }
        }
    }
    None
}

/// Extract a quoted value for a given key from a TOML inline table string.
/// e.g. for `{ path = "../utils" }` and key `"path"`, returns `Some("../utils")`.
pub(crate) fn extract_quoted_value(s: &str, key: &str) -> Option<String> {
    let key_pos = s.find(key)?;
    let after_key = &s[key_pos + key.len()..];
    // Skip whitespace and '='
    let after_eq = after_key.trim_start().strip_prefix('=')?.trim_start();
    // Extract quoted string
    let quote_char = after_eq.chars().next()?;
    if quote_char != '"' && quote_char != '\'' {
        return None;
    }
    let inner = &after_eq[1..];
    let end = inner.find(quote_char)?;
    Some(inner[..end].to_string())
}
