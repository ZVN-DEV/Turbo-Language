use std::time::Instant;

/// Simple deterministic pseudo-random number generator (xorshift32).
struct Rng {
    state: u32,
}

impl Rng {
    fn new(seed: u32) -> Self {
        Rng { state: seed }
    }

    fn next(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// Generate a printable ASCII character (32..127).
    fn next_ascii(&mut self) -> u8 {
        (self.next() % 95 + 32) as u8
    }
}

/// Generate a deterministic 1 MB string of ASCII text with spaces
/// inserted regularly to create word boundaries.
fn generate_text(size: usize) -> String {
    let mut rng = Rng::new(42);
    let mut buf = Vec::with_capacity(size);

    for i in 0..size {
        // Insert a space roughly every 5 characters to create words,
        // and occasionally insert "the " to have a countable pattern.
        if i % 200 == 0 && i + 4 <= size {
            // Deterministically insert "the " at fixed intervals
            // (we only do it when we're at exactly the right position)
            buf.push(b't');
        } else if i % 200 == 1 {
            buf.push(b'h');
        } else if i % 200 == 2 {
            buf.push(b'e');
        } else if i % 200 == 3 {
            buf.push(b' ');
        } else if i % 5 == 0 {
            buf.push(b' ');
        } else {
            let ch = rng.next_ascii();
            // Replace any control-like chars with 'a'
            if ch < 32 {
                buf.push(b'a');
            } else {
                buf.push(ch);
            }
        }
    }

    // Safety: all bytes are valid ASCII, which is valid UTF-8
    unsafe { String::from_utf8_unchecked(buf) }
}

/// Count words (split by whitespace).
fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Count occurrences of a substring.
fn count_occurrences(text: &str, pattern: &str) -> usize {
    text.matches(pattern).count()
}

/// Compute a simple hash of a string (djb2).
fn simple_hash(text: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in text.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

pub fn run() {
    let size = 1_000_000; // 1 MB

    let start = Instant::now();

    // Generate the text
    let text = generate_text(size);

    // Count words
    let word_count = count_words(&text);

    // Count occurrences of "the"
    let _the_count = count_occurrences(&text, "the");

    // Reverse the whole string
    let reversed: String = text.chars().rev().collect();

    // Compute hash of the reversed string
    let _hash = simple_hash(&reversed);

    let elapsed = start.elapsed();
    let time_ms = elapsed.as_secs_f64() * 1000.0;

    crate::print_result("string_processing", time_ms, &word_count.to_string());
}
