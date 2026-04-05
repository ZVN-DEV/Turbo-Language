# Turbo Standard Library Reference

> All functions that take a value as their first argument support both free-function and method syntax via UFCS (Uniform Function Call Syntax):
> `trim(s)` and `s.trim()` are equivalent.

---

## I/O

### print

```turbo
print("hello")
print(42)
print(true)
```

Prints a value to stdout followed by a newline. Accepts any type.

### read_line

```turbo
let name = read_line()
print("Hello, {name}!")
```

Reads a line of text from stdin. Returns the input as a string (without trailing newline).

### read_file

```turbo
let contents = read_file("data.txt")
print(contents)
```

Reads the entire contents of a file and returns it as a string. The path is relative to the current working directory.

### write_file

```turbo
write_file("output.txt", "Hello, file!")
let data = "line1\nline2\nline3"
write_file("lines.txt", data)
```

Writes a string to a file, creating the file if it does not exist or overwriting it if it does.

---

## String Operations

### len (string)

```turbo
let n = "hello".len()    // 5
let n = len("hello")     // equivalent
```

Returns the length of a string in bytes.

### trim

```turbo
let cleaned = "  hello  ".trim()    // "hello"
let cleaned = trim("  hello  ")     // equivalent
```

Strips leading and trailing whitespace from a string.

### upper

```turbo
let loud = "hello".upper()    // "HELLO"
let loud = upper("hello")     // equivalent
```

Converts all characters in a string to uppercase.

### lower

```turbo
let quiet = "HELLO".lower()    // "hello"
let quiet = lower("HELLO")     // equivalent
```

Converts all characters in a string to lowercase.

### split

```turbo
let parts = "a,b,c".split(",")    // ["a", "b", "c"]
let words = split("hello world", " ")
```

Splits a string by a separator and returns an array of substrings.

### contains

```turbo
let found = "hello world".contains("world")    // true
let found = contains("hello world", "xyz")     // false
```

Returns `true` if the string contains the given substring.

### starts_with

```turbo
let yes = "https://turbolang.dev".starts_with("https")    // true
let yes = starts_with("hello", "he")
```

Returns `true` if the string starts with the given prefix.

### ends_with

```turbo
let yes = "main.tb".ends_with(".tb")    // true
let yes = ends_with("photo.png", ".png")
```

Returns `true` if the string ends with the given suffix.

### replace

```turbo
let fixed = "hello world".replace("world", "turbo")    // "hello turbo"
let fixed = replace("aaa", "a", "b")                   // "bbb"
```

Replaces all occurrences of a substring with a replacement string.

### index_of

```turbo
let i = "hello".index_of("ll")    // 2
let i = "hello".index_of("xyz")   // -1
```

Returns the byte index of the first occurrence of a substring, or `-1` if not found.

### char_at

```turbo
let ch = "hello".char_at(0)    // "h"
let ch = "hello".char_at(4)    // "o"
```

Returns the character at the given byte index as a single-character string.

### repeat

```turbo
let stars = "*".repeat(5)        // "*****"
let wall = "ab".repeat(3)       // "ababab"
```

Repeats a string `n` times and returns the result.

### join

```turbo
let csv = join(["a", "b", "c"], ",")    // "a,b,c"
let path = join(["usr", "local", "bin"], "/")
```

Joins an array of strings with a separator between each element.

### to_str

```turbo
let s = to_str(42)       // "42"
let s = to_str(true)     // "true"
let s = to_str(3.14)     // "3.14"
```

Converts any value to its string representation.

---

## Array Operations

### len (array)

```turbo
let n = [1, 2, 3].len()    // 3
let n = len([10, 20])      // 2
```

Returns the number of elements in an array.

### push

```turbo
let mut arr = [1, 2, 3]
arr = arr.push(4)          // [1, 2, 3, 4]
arr = push(arr, 5)         // [1, 2, 3, 4, 5]
```

Appends an element to the end of an array. Returns a new array (copy-on-write semantics). Reassign the result to the original variable.

---

## Functional

### map

```turbo
let doubled = [1, 2, 3].map(|x: i64| -> i64 { x * 2 })    // [2, 4, 6]
let names = ["a", "b"].map(|s: str| -> str { s.upper() })
```

Transforms each element of an array by applying a function, returning a new array of the results.

### filter

```turbo
let big = [1, 2, 3, 4, 5].filter(|x: i64| -> bool { x > 3 })    // [4, 5]
let long = words.filter(|w: str| -> bool { w.len() > 3 })
```

Returns a new array containing only the elements for which the predicate returns `true`.

### reduce

```turbo
let sum = reduce([1, 2, 3, 4], 0, |acc: i64, x: i64| -> i64 { acc + x })    // 10
let product = reduce([2, 3, 4], 1, |acc: i64, x: i64| -> i64 { acc * x })
```

Folds an array into a single value by applying a function to an accumulator and each element from left to right.

---

## Math

### abs

```turbo
let a = abs(-42)    // 42
let b = abs(10)     // 10
```

Returns the absolute value of an integer.

### min

```turbo
let smallest = min(3, 7)     // 3
let smallest = min(-1, 0)    // -1
```

Returns the smaller of two integer values.

### max

```turbo
let largest = max(3, 7)     // 7
let largest = max(-1, 0)    // 0
```

Returns the larger of two integer values.

### pow

```turbo
let result = pow(2, 10)    // 1024
let cubed = pow(3, 3)      // 27
```

Raises a base to an integer exponent and returns the result.

### sqrt

```turbo
let s = sqrt(144.0)    // 12.0
let s = sqrt(2.0)      // 1.4142135623730951
```

Returns the square root of a floating-point number.

---

## HashMap

### hashmap

```turbo
let m = hashmap()
```

Creates and returns a new, empty hash map.

### hashmap_set

```turbo
let m = hashmap()
hashmap_set(m, "name", "Turbo")
hashmap_set(m, "version", "0.3")
```

Sets a key-value pair in the hash map. If the key already exists, its value is overwritten.

### hashmap_get

```turbo
let name = hashmap_get(m, "name")    // "Turbo"
```

Returns the value associated with the given key. The key must exist in the map.

### hashmap_has

```turbo
let exists = hashmap_has(m, "name")    // true
let missing = hashmap_has(m, "foo")    // false
```

Returns `true` if the hash map contains the given key.

### hashmap_len

```turbo
let count = hashmap_len(m)    // 2
```

Returns the number of key-value pairs in the hash map. Also available as `hashmap_size`.

### hashmap_keys

```turbo
let keys = hashmap_keys(m)    // ["name", "version"]
```

Returns an array of all keys in the hash map.

### hashmap_remove

```turbo
hashmap_remove(m, "version")
```

Removes the key-value pair with the given key from the hash map.

---

## JSON

### json_get

```turbo
let json = "{\"name\":\"Turbo\",\"version\":\"0.3\"}"
let name = json_get(json, "name")    // "Turbo"
```

Extracts a value from a JSON string by key. Returns the value as a string.

### json_stringify

```turbo
let json = json_stringify("name", "Turbo")    // "{\"name\":\"Turbo\"}"
```

Creates a JSON string containing a single key-value pair.

### to_json

```turbo
@derive(Display)
struct Point { x: i64, y: i64 }

let p = Point { x: 1, y: 2 }
let json = to_json(p)    // "{\"x\":1,\"y\":2}"
```

Serializes a struct to a JSON string.

### to_json_array

```turbo
let points = [Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]
let json = to_json_array(points)    // "[{\"x\":1,\"y\":2},{\"x\":3,\"y\":4}]"
```

Serializes an array of structs to a JSON array string.

---

## HTTP Client

### http_get

```turbo
let body = http_get("https://api.example.com/data")
print(body)
```

Performs an HTTP GET request and returns the response body as a string.

### http_post

```turbo
let response = http_post("https://api.example.com/items", "{\"name\":\"turbo\"}")
print(response)
```

Performs an HTTP POST request with the given body string and returns the response body as a string.

---

## HTTP Server

### http_server

```turbo
let app = http_server(8080)
```

Creates a new HTTP server bound to the given port. The server does not start listening until `http_listen` is called.

### route

```turbo
route(app, "GET", "/", |req: str| -> str {
    respond(200, "Hello, Turbo!")
})

route(app, "POST", "/api/echo", |req: str| -> str {
    let body = request_body(req)
    respond(200, body)
})
```

Registers a route handler for the given HTTP method and path. The handler receives the request as a string and must return a response string (use `respond` to construct it).

### http_listen

```turbo
let app = http_server(3000)
route(app, "GET", "/", |req: str| -> str { respond(200, "ok") })
http_listen(app)
```

Starts the HTTP server and begins accepting connections. This call blocks until the server is stopped.

### respond

```turbo
let res = respond(200, "{\"status\":\"ok\"}")
let err = respond(404, "Not Found")
```

Constructs an HTTP response string with the given status code and body.

### request_body

```turbo
route(app, "POST", "/data", |req: str| -> str {
    let body = request_body(req)
    respond(200, body)
})
```

Extracts the body from an HTTP request string.

### request_method

```turbo
route(app, "GET", "/info", |req: str| -> str {
    let method = request_method(req)    // "GET"
    respond(200, method)
})
```

Returns the HTTP method (GET, POST, PUT, DELETE, etc.) from a request string.

### request_path

```turbo
let path = request_path(req)    // "/api/users"
```

Returns the path portion of an HTTP request string.

### request_query

```turbo
// For a request to /search?q=turbo
let query = request_query(req, "q")    // "turbo"
```

Extracts the value of a query parameter from an HTTP request string by key.

### request_header

```turbo
let content_type = request_header(req, "Content-Type")
let auth = request_header(req, "Authorization")
```

Returns the value of a named HTTP header from a request string.

---

## Concurrency

### channel

```turbo
let ch = channel()
```

Creates an unbounded channel for sending and receiving integer values between concurrent tasks.

### send

```turbo
let ch = channel()
send(ch, 42)
```

Sends a value into a channel. Does not block (the channel is unbounded).

### recv

```turbo
let value = recv(ch)    // blocks until a value is available
print(value)
```

Receives a value from a channel. Blocks the current task until a value is available.

### mutex

```turbo
let m = mutex(0)
```

Creates a mutex wrapping an initial integer value. Use `mutex_get` and `mutex_set` to read and write the value safely across concurrent tasks.

### mutex_get

```turbo
let m = mutex(0)
let value = mutex_get(m)    // 0
```

Acquires the mutex lock and returns the current value.

### mutex_set

```turbo
mutex_set(m, 42)
let value = mutex_get(m)    // 42
```

Acquires the mutex lock and sets the value.

### sleep

```turbo
sleep(1000)    // sleep for 1 second
sleep(100)     // sleep for 100ms
```

Suspends the current task for the given number of milliseconds.

### clone

```turbo
@derive(Clone)
struct Point { x: i64, y: i64 }

let a = Point { x: 1, y: 2 }
let b = clone(a)
```

Creates a deep copy of a struct. The struct must have `@derive(Clone)`.

---

## Testing

### assert

```turbo
assert(2 + 2 == 4)
assert(x > 0, "x must be positive")
```

Asserts that a condition is `true`. Aborts with an error if the condition is `false`. An optional message can be provided.

### assert_eq

```turbo
assert_eq(add(2, 3), 5)
assert_eq("hello".len(), 5)
```

Asserts that two values are equal. Aborts with an error showing the expected and actual values if they differ.

### assert_ne

```turbo
assert_ne(a, b)
assert_ne("hello", "world")
```

Asserts that two values are not equal. Aborts with an error if they are equal.

### panic

```turbo
panic("something went wrong")
panic()
```

Immediately aborts execution with an error message. If no message is provided, a default message is used. This function never returns.

---

## Unsafe

> These functions are only available inside `unsafe` blocks. They provide direct memory access and should be used with extreme caution.

### deref

```turbo
unsafe {
    let value = deref(addr)
}
```

Reads and returns the 64-bit integer stored at the given memory address.

### store

```turbo
unsafe {
    store(addr, 42)
}
```

Writes a 64-bit integer value to the given memory address.

---

## Quick Reference

| Category | Functions |
|----------|-----------|
| **I/O** | `print`, `read_line`, `read_file`, `write_file` |
| **Strings** | `len`, `trim`, `upper`, `lower`, `split`, `contains`, `starts_with`, `ends_with`, `replace`, `index_of`, `char_at`, `repeat`, `join`, `to_str` |
| **Arrays** | `len`, `push` |
| **Functional** | `map`, `filter`, `reduce` |
| **Math** | `abs`, `min`, `max`, `pow`, `sqrt` |
| **HashMap** | `hashmap`, `hashmap_set`, `hashmap_get`, `hashmap_has`, `hashmap_len`, `hashmap_keys`, `hashmap_remove` |
| **JSON** | `json_get`, `json_stringify`, `to_json`, `to_json_array` |
| **HTTP Client** | `http_get`, `http_post` |
| **HTTP Server** | `http_server`, `route`, `http_listen`, `respond`, `request_body`, `request_method`, `request_path`, `request_query`, `request_header` |
| **Concurrency** | `channel`, `send`, `recv`, `mutex`, `mutex_get`, `mutex_set`, `sleep`, `clone` |
| **Testing** | `assert`, `assert_eq`, `assert_ne`, `panic` |
| **Unsafe** | `deref`, `store` |
