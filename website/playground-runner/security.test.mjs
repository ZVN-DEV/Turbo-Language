// Regression tests for the playground runner's fail-closed allowlist and the
// prlimit resource-limit wrapper. Run with `node --test website/playground-runner/`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

async function loadRunner() {
  const url = new URL("./runner.mjs", import.meta.url);
  url.searchParams.set("cache", String(Date.now()));
  return import(url.href);
}

test("safe programs pass the allowlist unchanged", async () => {
  const { findForbiddenPlaygroundApi, validateRunPayload } = await loadRunner();

  const safe = [
    "fn main() { print(\"hello\") }",
    // string/array/hashmap/math builtins, method-call sugar, and chaining.
    [
      "fn main() {",
      "    let xs = [3, 1, 2]",
      "    xs.push(4)",
      "    print(len(sort(xs)))",
      "    let m = hashmap()",
      "    hashmap_set(m, \"k\", 1)",
      "    print(to_str(sqrt(pow(2.0, 4.0))))",
      "    print(\"  hi  \".trim().upper())",
      "}",
    ].join("\n"),
  ];

  for (const source of safe) {
    assert.equal(findForbiddenPlaygroundApi(source), null, source);
    assert.deepEqual(validateRunPayload({ source }), { ok: true, source });
  }
});

test("denylisted dangerous builtins are rejected with a category message", async () => {
  const { findForbiddenPlaygroundApi, validateRunPayload } = await loadRunner();

  assert.deepEqual(findForbiddenPlaygroundApi("fn main() { http_get(\"http://x\") }"), {
    name: "http_get",
    category: "network",
  });
  assert.equal(
    validateRunPayload({ source: "fn main() { read_file(\"/etc/passwd\") }" }).message,
    "Playground execution does not allow filesystem API `read_file`."
  );
});

test("unknown/future builtin calls are rejected fail-closed", async () => {
  const { findForbiddenPlaygroundApi, validateRunPayload } = await loadRunner();

  // A plausible future dangerous builtin that isn't on the denylist yet: with a
  // denylist it would run; the allowlist refuses it because it is neither a
  // known-safe builtin nor declared by the source.
  const forbidden = findForbiddenPlaygroundApi("fn main() { gpu_dma_write(0, 1) }");
  assert.deepEqual(forbidden, { name: "gpu_dma_write", category: "unavailable builtin" });

  const validated = validateRunPayload({ source: "fn main() { socket_open(\"1.2.3.4\") }" });
  assert.equal(validated.ok, false);
  assert.equal(validated.status, 400);
  assert.match(validated.message, /only allows Turbo's safe standard library/);
  assert.match(validated.message, /`socket_open`/);
});

test("user-defined names that look builtin-ish are allowed", async () => {
  const { findForbiddenPlaygroundApi } = await loadRunner();

  // A user function whose name is not a known builtin — must not be flagged.
  assert.equal(
    findForbiddenPlaygroundApi(
      ["fn compute(x: i64) -> i64 { x + 1 }", "fn main() { print(compute(41)) }"].join("\n")
    ),
    null
  );

  // Closure bound to a `let`, then called by name (real pattern from
  // tests/phase1/closures.tb).
  assert.equal(
    findForbiddenPlaygroundApi(
      [
        "fn main() {",
        "    let double = |x: i64| -> i64 { x * 2 }",
        "    print(double(5))",
        "}",
      ].join("\n")
    ),
    null
  );

  // A function parameter that is itself called (tests/phase1/closure_pass.tb).
  assert.equal(
    findForbiddenPlaygroundApi(
      [
        "fn apply(f: fn(i64) -> i64, x: i64) -> i64 { f(x) }",
        "fn main() { print(apply(|n: i64| -> i64 { n * 3 }, 4)) }",
      ].join("\n")
    ),
    null
  );

  // Enum-variant / constructor calls (Capitalized) and the lowercase keyword
  // constructors `some`/`ok`/`err` must all pass.
  assert.equal(
    findForbiddenPlaygroundApi(
      [
        "enum Shape { Circle(f64), Square(f64) }",
        "fn area(s: Shape) -> f64 {",
        "    match s { Shape::Circle(r) => r * r, Shape::Square(w) => w * w }",
        "}",
        "fn main() {",
        "    let c = Shape::Circle(2.0)",
        "    let maybe = some(area(c))",
        "    print(to_str(ok(maybe)))",
        "}",
      ].join("\n")
    ),
    null
  );
});

test("dangerous identifiers only in strings or comments are ignored", async () => {
  const { findForbiddenPlaygroundApi } = await loadRunner();

  const source = [
    "fn main() {",
    "    // http_get(\"http://evil\") is only a comment",
    "    /* read_file(\"/etc/passwd\") and gpu_dma_write() too */",
    "    print(\"the words exec and socket_open appear only in this string\")",
    "    print(r\"shell_exec stays raw\")",
    "    print(\"\"\"http_post lives in a triple string\"\"\")",
    "}",
  ].join("\n");

  assert.equal(findForbiddenPlaygroundApi(source), null);
});

test("denylisted call smuggled inside an interpolation is still caught", async () => {
  const { findForbiddenPlaygroundApi } = await loadRunner();

  assert.deepEqual(
    findForbiddenPlaygroundApi('fn main() { print("value {exec(\\"pwd\\")}") }'),
    { name: "exec", category: "process" }
  );
  // An unknown builtin inside interpolation is also fail-closed.
  assert.deepEqual(
    findForbiddenPlaygroundApi('fn main() { print("value {socket_open(\\"x\\")}") }'),
    { name: "socket_open", category: "unavailable builtin" }
  );
});

test("buildChildInvocation wraps in prlimit only when enabled", async () => {
  const { buildChildInvocation, CHILD_RLIMITS } = await loadRunner();

  const direct = buildChildInvocation("/usr/local/bin/turbolang", "/tmp/run/main.tb", {});
  assert.deepEqual(direct, {
    command: "/usr/local/bin/turbolang",
    args: ["run", "/tmp/run/main.tb"],
  });

  const limited = buildChildInvocation("/usr/local/bin/turbolang", "/tmp/run/main.tb", {
    TURBO_PLAYGROUND_RUNNER_RLIMIT: "1",
  });
  assert.equal(limited.command, "prlimit");
  assert.deepEqual(limited.args, [
    `--cpu=${CHILD_RLIMITS.cpu}`,
    `--nproc=${CHILD_RLIMITS.nproc}`,
    `--fsize=${CHILD_RLIMITS.fsizeBytes}`,
    `--nofile=${CHILD_RLIMITS.nofile}`,
    `--as=${CHILD_RLIMITS.asBytes}`,
    "--",
    "/usr/local/bin/turbolang",
    "run",
    "/tmp/run/main.tb",
  ]);
});

// Sanity: the module resolves from its own directory regardless of cwd.
test("runner module path resolves from the test file location", () => {
  assert.ok(fileURLToPath(new URL("./runner.mjs", import.meta.url)).endsWith("runner.mjs"));
});
