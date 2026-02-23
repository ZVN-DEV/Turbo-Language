mod fib;
mod trees;
mod matrix;
mod strings;
mod concurrent;

fn print_result(benchmark: &str, time_ms: f64, result: &str) {
    let output = serde_json::json!({
        "language": "rust",
        "benchmark": benchmark,
        "time_ms": time_ms,
        "result": result
    });
    println!("{}", output);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let benchmark_name = if args.len() > 1 {
        args[1].as_str()
    } else {
        "all"
    };

    match benchmark_name {
        "fib" => fib::run(),
        "trees" => trees::run(),
        "matrix" => matrix::run(),
        "strings" => strings::run(),
        "concurrent" => concurrent::run(),
        "all" => {
            fib::run();
            trees::run();
            matrix::run();
            strings::run();
            concurrent::run();
        }
        other => {
            eprintln!("Unknown benchmark: {}", other);
            eprintln!("Available: fib, trees, matrix, strings, concurrent, all");
            std::process::exit(1);
        }
    }
}
