# TurboLang Standard Library Roadmap
Generated: 2026-05-15
Goal: Batteries-included language — zero third-party dependencies needed for real-world software.

## Philosophy
Every dependency is liability. TurboLang ships no package manager by design.
The stdlib must be complete enough that developers never need one.

---

## What We Have (67 builtins)

### Strings (12) ✅
trim, upper, lower, split, join, replace, contains, starts_with, ends_with, index_of, char_at, repeat, to_str, len

### Math (5) ✅
abs, min, max, pow, sqrt

### Collections — Arrays (5) ✅
len, push, map, filter, reduce

### Collections — HashMap (9) ✅
hashmap, hashmap_set, hashmap_get, hashmap_set_int, hashmap_get_int, hashmap_has, hashmap_len, hashmap_keys, hashmap_remove

### JSON (4) ✅
json_get, json_stringify, to_json, to_json_array

### HTTP Client (2) ✅
http_get, http_post

### HTTP Server (11) ✅
http_server, http_server_public, route, http_listen, respond, respond_html, respond_json, request_body, request_method, request_path, request_query, request_header

### Async/Concurrency (7) ✅
channel, send, recv, mutex, mutex_get, mutex_set, sleep

### I/O (6) ✅
print, read_line, read_file, write_file, try_read_file, try_write_file

### System (2) ✅
exec, env_get

### Testing (4) ✅
assert, assert_eq, assert_ne, panic

### Other (2) ✅
clone, to_str

---

## Tier 1 — Can't Build Anything Real Without These

### System Essentials
- [ ] T1-01: `args()` — CLI argument array (runtime exists, register in sema)
- [ ] T1-02: `exit(code)` — Exit process with status code
- [ ] T1-03: `type_of(val)` — Get type name as string

### Date/Time
- [ ] T1-04: `time_now()` — Current Unix timestamp in seconds (f64)
- [ ] T1-05: `time_ms()` — Current Unix timestamp in milliseconds (i64)
- [ ] T1-06: `format_time(timestamp, format)` — Format timestamp to string (basic: RFC3339, ISO8601)

### Filesystem
- [ ] T1-07: `file_exists(path)` — Check if file exists (bool)
- [ ] T1-08: `delete_file(path)` — Delete a file
- [ ] T1-09: `list_dir(path)` — List directory contents as array of strings
- [ ] T1-10: `mkdir(path)` — Create directory (recursive)
- [ ] T1-11: `path_join(a, b)` — Join path segments
- [ ] T1-12: `path_dir(path)` — Get directory component
- [ ] T1-13: `path_base(path)` — Get filename component
- [ ] T1-14: `path_ext(path)` — Get file extension

### Collections
- [ ] T1-15: `sort(arr)` — Sort array (COW, returns new array)
- [ ] T1-16: `reverse(arr)` — Reverse array (COW)
- [ ] T1-17: `array_contains(arr, val)` — Check if value in array
- [ ] T1-18: `find(arr, closure)` — Find first element matching predicate
- [ ] T1-19: `any(arr, closure)` — True if any element matches
- [ ] T1-20: `all(arr, closure)` — True if all elements match
- [ ] T1-21: `zip(arr1, arr2)` — Combine two arrays into array of tuples/pairs
- [ ] T1-22: `flatten(arr)` — Flatten nested array one level
- [ ] T1-23: `slice(arr, start, end)` — Sub-array extraction
- [ ] T1-24: `array_remove(arr, index)` — Remove element at index (COW)

### Math
- [ ] T1-25: `floor(x)` — Floor of float → i64
- [ ] T1-26: `ceil(x)` — Ceiling of float → i64
- [ ] T1-27: `round(x)` — Round float → i64
- [ ] T1-28: `random()` — Random f64 in [0.0, 1.0)
- [ ] T1-29: `random_range(min, max)` — Random i64 in [min, max]
- [ ] T1-30: `sin(x)`, `cos(x)`, `tan(x)` — Trig functions (f64 → f64)
- [ ] T1-31: `log(x)`, `log2(x)`, `log10(x)` — Logarithms (f64 → f64)
- [ ] T1-32: `exp(x)` — e^x (f64 → f64)
- [ ] T1-33: `PI`, `E` — Math constants

### HTTP Client
- [ ] T1-34: `http_request(method, url, headers, body)` — General HTTP with custom headers
- [ ] T1-35: `http_status(response)` — Get status code from response
- [ ] T1-36: `http_headers(response)` — Get response headers

### String Additions
- [ ] T1-37: `substring(s, start, end)` — Extract substring by index
- [ ] T1-38: `pad_left(s, width, char)` — Left-pad string
- [ ] T1-39: `pad_right(s, width, char)` — Right-pad string
- [ ] T1-40: `str_to_int(s)` — Parse string to integer (returns Result)
- [ ] T1-41: `str_to_float(s)` — Parse string to float (returns Result)
- [ ] T1-42: `format(template, args...)` — sprintf-style formatting (or just ensure interpolation covers it)

---

## Tier 2 — Needed for Web/API Work

### Encoding
- [ ] T2-01: `base64_encode(s)` — Base64 encode string
- [ ] T2-02: `base64_decode(s)` — Base64 decode string (Result)
- [ ] T2-03: `url_encode(s)` — Percent-encode string
- [ ] T2-04: `url_decode(s)` — Percent-decode string (Result)
- [ ] T2-05: `hex_encode(s)` — Hex encode bytes
- [ ] T2-06: `hex_decode(s)` — Hex decode string (Result)

### Crypto/Hashing
- [ ] T2-07: `sha256(s)` — SHA-256 hash as hex string
- [ ] T2-08: `md5(s)` — MD5 hash as hex string
- [ ] T2-09: `hmac_sha256(key, msg)` — HMAC-SHA256

### Regex
- [ ] T2-10: `regex_match(pattern, s)` — Check if string matches pattern (bool)
- [ ] T2-11: `regex_find(pattern, s)` — Find first match (str? optional)
- [ ] T2-12: `regex_find_all(pattern, s)` — Find all matches (array)
- [ ] T2-13: `regex_replace(pattern, s, replacement)` — Regex replace

### HTTP Extensions
- [ ] T2-14: `http_put(url, body)` — HTTP PUT
- [ ] T2-15: `http_delete(url)` — HTTP DELETE
- [ ] T2-16: `http_patch(url, body)` — HTTP PATCH

### Data Formats
- [ ] T2-17: `csv_parse(s)` — Parse CSV string to array of arrays
- [ ] T2-18: `csv_stringify(data)` — Convert to CSV string
- [ ] T2-19: `toml_parse(s)` — Parse TOML to hashmap
- [ ] T2-20: `json_parse(s)` — Full JSON parse to typed values

---

## Tier 3 — Differentiators (Vendored Libraries)

### Database
- [ ] T3-01: `db_open(path)` — Open SQLite database
- [ ] T3-02: `db_exec(db, sql)` — Execute SQL statement
- [ ] T3-03: `db_query(db, sql)` — Query returning array of rows

### Networking
- [ ] T3-04: `tcp_connect(host, port)` — TCP client socket
- [ ] T3-05: `tcp_listen(port)` — TCP server socket
- [ ] T3-06: `udp_socket()` — UDP socket
- [ ] T3-07: `dns_lookup(hostname)` — DNS resolution

### TUI/Terminal
- [ ] T3-08: `color(text, color)` — ANSI colored text
- [ ] T3-09: `bold(text)`, `italic(text)`, `underline(text)` — Text styling
- [ ] T3-10: `cursor_move(row, col)` — Move terminal cursor
- [ ] T3-11: `clear_screen()` — Clear terminal
- [ ] T3-12: `term_size()` — Get terminal width/height

---

## Progress Tracker

| Tier | Total | Done | Remaining |
|------|-------|------|-----------|
| Tier 1 | 42 | 0 | 42 |
| Tier 2 | 20 | 0 | 20 |
| Tier 3 | 12 | 0 | 12 |
| **Total** | **74** | **0** | **74** |
