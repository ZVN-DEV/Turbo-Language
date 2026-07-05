import assert from "node:assert/strict";
import { chmod, mkdtemp, rm, writeFile } from "node:fs/promises";
import { existsSync, readFileSync } from "node:fs";
import net from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

const root = process.cwd();

function read(path) {
  return readFileSync(join(root, path), "utf8");
}

function compilerBuiltinNames() {
  const source = read("../turbo/crates/turbo-sema/src/type_check/mod.rs");
  const match = source.match(
    /pub\(crate\) const BUILTIN_FNS: &\[&str\] = &\[(?<body>[\s\S]*?)\];/
  );
  assert.ok(match?.groups?.body);

  return [...match.groups.body.matchAll(/"([^"]+)"/g)].map(([, name]) => name);
}

function isCompilerHostAccessBuiltin(name) {
  if (name.startsWith("path_")) return false;

  return (
    name.startsWith("http_") ||
    /(^|_)file($|_)/.test(name) ||
    name === "args" ||
    name === "deref" ||
    name === "env_get" ||
    name === "exec" ||
    name === "list_dir" ||
    name === "mkdir" ||
    name === "read_line" ||
    name === "route" ||
    name === "shell_exec" ||
    name === "store"
  );
}

async function loadRunner() {
  const url = pathToFileURL(join(root, "playground-runner/runner.mjs"));
  url.searchParams.set("cache", String(Date.now()));
  return import(url.href);
}

async function loadRunnerSmoke() {
  const url = pathToFileURL(join(root, "playground-runner/smoke.mjs"));
  url.searchParams.set("cache", String(Date.now()));
  return import(url.href);
}

function readRawHttpResponse(port, requestText, timeoutMs = 750) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ host: "127.0.0.1", port });
    let data = "";
    const timer = setTimeout(() => {
      socket.destroy();
      resolve(data);
    }, timeoutMs);

    socket.setEncoding("utf8");
    socket.on("connect", () => {
      socket.write(requestText);
    });
    socket.on("data", (chunk) => {
      data += chunk;
      if (data.includes("Playground request is too large")) {
        clearTimeout(timer);
        socket.destroy();
        resolve(data);
      }
    });
    socket.on("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
  });
}

test("standalone playground runner artifacts exist and stay container-oriented", () => {
  assert.equal(existsSync(join(root, "playground-runner/runner.mjs")), true);
  assert.equal(existsSync(join(root, "playground-runner/Dockerfile")), true);
  assert.equal(existsSync(join(root, "playground-runner/fly.toml")), true);
  assert.equal(existsSync(join(root, "playground-runner/README.md")), true);
  assert.equal(existsSync(join(root, "playground-runner/smoke.mjs")), true);

  const packageJson = JSON.parse(read("package.json"));
  const runner = read("playground-runner/runner.mjs");
  const dockerfile = read("playground-runner/Dockerfile");
  const flyConfig = read("playground-runner/fly.toml");
  const readme = read("playground-runner/README.md");
  const ciWorkflow = read("../.github/workflows/ci.yml");

  assert.equal(
    packageJson.scripts["smoke:playground-runner"],
    "node playground-runner/smoke.mjs"
  );
  assert.match(runner, /execFile/);
  assert.doesNotMatch(runner, /exec\(/);
  assert.doesNotMatch(runner, /shell:\s*true/);
  assert.doesNotMatch(runner, /createHash|token=\$\{fingerprint\}/);
  assert.match(runner, /killSignal:\s*"SIGKILL"/);
  assert.match(runner, /TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST/);
  assert.match(dockerfile, /FROM rust:1\.88-slim AS builder/);
  assert.match(dockerfile, /COPY docs\/errors\/ \.\/docs\/errors\//);
  assert.match(dockerfile, /USER turbo/);
  assert.match(dockerfile, /TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST=1/);
  assert.match(dockerfile, /HEALTHCHECK/);
  assert.match(dockerfile, /\/healthz/);
  assert.doesNotMatch(dockerfile, /\b(curl|wget)\b/);
  assert.match(flyConfig, /app = "turbolang-playground-runner"/);
  assert.match(flyConfig, /dockerfile = "website\/playground-runner\/Dockerfile"/);
  assert.match(flyConfig, /internal_port = 8787/);
  assert.match(flyConfig, /auto_stop_machines = "off"/);
  assert.match(flyConfig, /auto_start_machines = true/);
  assert.match(flyConfig, /min_machines_running = 1/);
  assert.match(flyConfig, /path = "\/healthz"/);
  assert.match(flyConfig, /memory_mb = 256/);
  assert.match(readme, /--read-only/);
  assert.match(readme, /--cap-drop=ALL/);
  assert.match(readme, /flyctl deploy --config website\/playground-runner\/fly\.toml/);
  assert.match(readme, /TURBO_PLAYGROUND_RUNNER_URL/);
  assert.match(ciWorkflow, /name: Playground runner image/);
  assert.match(ciWorkflow, /docker build -t turbo-playground-runner:ci -f website\/playground-runner\/Dockerfile \./);
  assert.match(ciWorkflow, /npm run smoke:playground-runner/);
});

test("standalone playground runner smoke probe covers deploy contract", async () => {
  const { runnerHealthUrl, runSmoke } = await loadRunnerSmoke();
  const requests = [];
  const fetcher = async (url, init = {}) => {
    requests.push({ url: String(url), init });

    if (String(url).endsWith("/healthz")) {
      return { ok: true, status: 200, json: async () => ({ ok: true }) };
    }

    assert.equal(String(url), "https://runner.example/run");
    assert.equal(init.headers.authorization, "Bearer secret");
    const { source } = JSON.parse(init.body);

    if (source.includes("exec(")) {
      return {
        ok: false,
        status: 400,
        json: async () => ({
          stdout: "",
          stderr: "Playground execution does not allow process API `exec`.",
          success: false,
        }),
      };
    }

    return {
      ok: true,
      status: 200,
      json: async () => ({
        stdout: "runner smoke ok\n",
        stderr: "",
        success: true,
        durationMs: 1,
      }),
    };
  };

  assert.equal(
    runnerHealthUrl("https://runner.example/run").toString(),
    "https://runner.example/healthz"
  );
  await runSmoke({
    fetcher,
    runnerUrl: "https://runner.example/run",
    token: " secret ",
  });
  assert.deepEqual(
    requests.map(({ url }) => url),
    [
      "https://runner.example/healthz",
      "https://runner.example/run",
      "https://runner.example/run",
    ]
  );
});

test("standalone playground runner denies compiler host-access builtins", async () => {
  const { findForbiddenPlaygroundApi } = await loadRunner();
  const hostAccessBuiltins = compilerBuiltinNames().filter(isCompilerHostAccessBuiltin);

  assert.ok(hostAccessBuiltins.includes("http_post_with_headers"));
  assert.ok(hostAccessBuiltins.includes("read_file"));
  assert.ok(hostAccessBuiltins.includes("args"));
  assert.equal(hostAccessBuiltins.includes("path_join"), false);

  const missed = hostAccessBuiltins.filter(
    (name) => !findForbiddenPlaygroundApi(`fn main() { ${name}() }`)
  );
  assert.deepEqual(missed, []);
});

test("standalone playground runner validates payloads, auth, and output", async () => {
  const {
    MAX_SOURCE_BYTES,
    authorizeRequest,
    findForbiddenPlaygroundApi,
    sanitizeRunnerOutput,
    startupConfigError,
    validateRunPayload,
  } = await loadRunner();

  assert.equal(MAX_SOURCE_BYTES, 64 * 1024);
  assert.deepEqual(validateRunPayload({ source: "fn main() {}" }), {
    ok: true,
    source: "fn main() {}",
  });
  assert.deepEqual(validateRunPayload({ source: "" }), {
    ok: false,
    status: 400,
    message: "Enter Turbo source before running it.",
  });
  assert.deepEqual(validateRunPayload({ source: 3 }), {
    ok: false,
    status: 400,
    message: "Request body must include a string source field.",
  });
  assert.equal(
    validateRunPayload({ source: "x".repeat(MAX_SOURCE_BYTES + 1) }).status,
    413
  );
  assert.deepEqual(findForbiddenPlaygroundApi("fn main() { exec(\"ls\") }"), {
    name: "exec",
    category: "process",
  });
  assert.deepEqual(findForbiddenPlaygroundApi("fn main() { read_file(\"x\") }"), {
    name: "read_file",
    category: "filesystem",
  });
  assert.deepEqual(findForbiddenPlaygroundApi("fn main() { http_get(\"http://x\") }"), {
    name: "http_get",
    category: "network",
  });
  assert.deepEqual(
    findForbiddenPlaygroundApi(
      'fn main() { http_post_with_headers("https://x", "{}", "authorization: secret") }'
    ),
    {
      name: "http_post_with_headers",
      category: "network",
    }
  );
  assert.deepEqual(findForbiddenPlaygroundApi("fn main() { print(args()) }"), {
    name: "args",
    category: "process",
  });
  assert.deepEqual(
    findForbiddenPlaygroundApi('fn main() { print("value {exec(\\"pwd\\")}") }'),
    {
      name: "exec",
      category: "process",
    }
  );
  assert.deepEqual(
    findForbiddenPlaygroundApi('fn main() { print("file {read_file(\\"/etc/passwd\\")}") }'),
    {
      name: "read_file",
      category: "filesystem",
    }
  );
  assert.deepEqual(
    findForbiddenPlaygroundApi(
      'fn main() { print("outer {\\"inner {exec(\\\\\\"pwd\\\\\\")}\\"}") }'
    ),
    {
      name: "exec",
      category: "process",
    }
  );
  assert.deepEqual(
    findForbiddenPlaygroundApi('fn main() { print("""x {read_file("/etc/passwd")} y""") }'),
    {
      name: "read_file",
      category: "filesystem",
    }
  );
  assert.deepEqual(
    findForbiddenPlaygroundApi('fn main() { print("""x {http_get("https://example.com")} y""") }'),
    {
      name: "http_get",
      category: "network",
    }
  );
  assert.deepEqual(
    findForbiddenPlaygroundApi('fn main() { print("""x {shell_exec("pwd")} y""") }'),
    {
      name: "shell_exec",
      category: "process",
    }
  );
  assert.deepEqual(findForbiddenPlaygroundApi("import { secret } from \"/etc/passwd\""), {
    name: "import",
    category: "file import",
  });
  assert.deepEqual(findForbiddenPlaygroundApi("@unsafe fn main() { deref(0) }"), {
    name: "unsafe",
    category: "unsafe/FFI",
  });
  assert.deepEqual(findForbiddenPlaygroundApi("@unsafe extern \"C\" { fn system(cmd: str) -> i64 }"), {
    name: "unsafe",
    category: "unsafe/FFI",
  });
  assert.deepEqual(findForbiddenPlaygroundApi("fn main() { store(0, 1) }"), {
    name: "store",
    category: "raw memory",
  });
  assert.equal(
    findForbiddenPlaygroundApi(
      [
        "fn main() {",
        "    let p = path_join(\"docs\", \"intro.tb\")",
        "    print(path_dir(p))",
        "    print(path_base(p))",
        "    print(path_ext(p))",
        "}",
      ].join("\n")
    ),
    null
  );
  assert.equal(
    findForbiddenPlaygroundApi(
      [
        "fn main() {",
        "    // exec(\"ignored\")",
        "    // import { ignored } from \"../ignored\"",
        "    // @unsafe fn ignored() {}",
        "    /* read_file(\"ignored\") */",
        "    print(\"http_get\")",
        "    print(r\"write_file\")",
        "    print(\"extern store deref unsafe\")",
        "    print(\"literal escaped \\{exec(\\\"pwd\\\")}\")",
        "    print(\"\"\"list_dir\"\"\")",
        "    print(\"\"\"literal escaped \\{read_file(\\\"/etc/passwd\\\")}\"\"\")",
        "}",
      ].join("\n")
    ),
    null
  );
  assert.deepEqual(validateRunPayload({ source: "fn main() { shell_exec(\"pwd\") }" }), {
    ok: false,
    status: 400,
    message: "Playground execution does not allow process API `shell_exec`.",
  });
  assert.deepEqual(validateRunPayload({ source: "import { read } from \"../secret\"" }), {
    ok: false,
    status: 400,
    message: "Playground execution does not allow file imports.",
  });
  assert.deepEqual(validateRunPayload({ source: "@unsafe fn main() { deref(0) }" }), {
    ok: false,
    status: 400,
    message: "Playground execution does not allow unsafe or FFI features.",
  });

  assert.equal(authorizeRequest(undefined, undefined), true);
  assert.equal(authorizeRequest(undefined, "Bearer anything"), true);
  assert.equal(authorizeRequest("   ", undefined), false);
  assert.equal(authorizeRequest("   ", "Bearer    "), false);
  assert.equal(authorizeRequest("secret", undefined), false);
  assert.equal(authorizeRequest("secret", "Bearer wrong"), false);
  assert.equal(authorizeRequest("secret", "Bearer secret"), true);
  assert.equal(authorizeRequest(" secret ", "Bearer  secret "), false);
  assert.equal(authorizeRequest(" secret ", "Bearer secret"), true);

  assert.equal(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST: "1",
      TURBO_PLAYGROUND_RUNNER_TOKEN: "secret",
      TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT: " 3 ",
      TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS: "5000",
      PORT: " 8787 ",
    }),
    null
  );
  assert.match(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST: "1",
      TURBO_PLAYGROUND_RUNNER_TOKEN: "secret",
      PORT: "0",
    }),
    /PORT/
  );
  assert.match(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST: "1",
      TURBO_PLAYGROUND_RUNNER_TOKEN: "secret",
      PORT: "65536",
    }),
    /PORT/
  );
  assert.match(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST: "1",
      TURBO_PLAYGROUND_RUNNER_TOKEN: "secret",
      PORT: "8787x",
    }),
    /PORT/
  );
  assert.match(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST: "1",
      TURBO_PLAYGROUND_RUNNER_TOKEN: "secret",
      TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT: "0",
    }),
    /TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT/
  );
  assert.match(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST: "1",
      TURBO_PLAYGROUND_RUNNER_TOKEN: "secret",
      TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT: "many",
    }),
    /TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT/
  );
  assert.match(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST: "1",
      TURBO_PLAYGROUND_RUNNER_TOKEN: "secret",
      TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS: "-1",
    }),
    /TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS/
  );
  assert.match(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST: "1",
      TURBO_PLAYGROUND_RUNNER_TOKEN: "secret",
      TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS: "5s",
    }),
    /TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS/
  );
  assert.match(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST: "1",
      TURBO_PLAYGROUND_RUNNER_TOKEN: "   ",
    }),
    /TURBO_PLAYGROUND_RUNNER_TOKEN/
  );
  assert.match(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST: "1",
    }),
    /TURBO_PLAYGROUND_RUNNER_TOKEN/
  );
  assert.equal(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST: "1",
      TURBO_PLAYGROUND_RUNNER_ALLOW_MISSING_TOKEN: "1",
    }),
    null
  );
  assert.equal(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST: "1",
      TURBO_PLAYGROUND_RUNNER_TOKEN: "   ",
      TURBO_PLAYGROUND_RUNNER_ALLOW_MISSING_TOKEN: "1",
    }),
    null
  );
  assert.match(
    startupConfigError({
      TURBO_PLAYGROUND_RUNNER_TOKEN: "secret",
    }),
    /documented sandbox container/
  );

  const sanitized = sanitizeRunnerOutput(
    "\u001b[31merror\u001b[0m /tmp/turbo-playground-runner-abcd/main.tb:1:1"
  );
  assert.equal(sanitized, "error playground.tb:1:1");
});

test("standalone playground runner executes through execFile and sanitizes temp paths", async () => {
  const { runTurboSource } = await loadRunner();
  const tmpRoot = await mkdtemp(join(tmpdir(), "turbo-runner-test-"));
  const fakeTurbo = join(tmpRoot, "turbolang");
  const previousPath = process.env.PATH;

  await writeFile(
    fakeTurbo,
    [
      `#!${process.execPath}`,
      "process.stdout.write(`PATH=${process.env.PATH}\\n`);",
      'process.stdout.write("hello from fake turbo\\n");',
      'process.stderr.write(`diagnostic ${process.argv[3]}:2:5\\n`);',
      "",
    ].join("\n"),
    "utf8"
  );
  await chmod(fakeTurbo, 0o755);
  process.env.PATH = "/host/path/that/must/not/leak";

  try {
    const result = await runTurboSource("fn main() { print(1) }", {
      tmpRoot,
      turboBin: fakeTurbo,
      timeoutMs: 5000,
    });

    assert.equal(result.success, true);
    assert.equal(result.stdout, "PATH=/usr/local/bin:/usr/bin:/bin\nhello from fake turbo\n");
    assert.equal(result.stderr, "diagnostic playground.tb:2:5\n");
    assert.equal(typeof result.durationMs, "number");
  } finally {
    if (previousPath === undefined) {
      delete process.env.PATH;
    } else {
      process.env.PATH = previousPath;
    }
    await rm(tmpRoot, { recursive: true, force: true });
  }
});

test("standalone playground runner preserves failed execution stderr", async () => {
  const { runTurboSource } = await loadRunner();
  const tmpRoot = await mkdtemp(join(tmpdir(), "turbo-runner-fail-test-"));
  const fakeTurbo = join(tmpRoot, "turbolang");

  await writeFile(
    fakeTurbo,
    [
      "#!/bin/sh",
      "printf 'compiler says no at %s:4:2\\n' \"$2\" >&2",
      "exit 42",
      "",
    ].join("\n"),
    "utf8"
  );
  await chmod(fakeTurbo, 0o755);

  try {
    const result = await runTurboSource("fn main() { nope }", {
      tmpRoot,
      turboBin: fakeTurbo,
      timeoutMs: 5000,
    });

    assert.equal(result.success, false);
    assert.equal(result.stdout, "");
    assert.equal(result.stderr, "compiler says no at playground.tb:4:2\n");
  } finally {
    await rm(tmpRoot, { recursive: true, force: true });
  }
});

test("standalone playground runner reports execution timeout", async () => {
  const { runTurboSource } = await loadRunner();
  const tmpRoot = await mkdtemp(join(tmpdir(), "turbo-runner-timeout-test-"));
  const fakeTurbo = join(tmpRoot, "turbolang");

  await writeFile(
    fakeTurbo,
    ["#!/bin/sh", "sleep 1", "printf 'too late\\n'", ""].join("\n"),
    "utf8"
  );
  await chmod(fakeTurbo, 0o755);

  try {
    const result = await runTurboSource("fn main() { loop {} }", {
      tmpRoot,
      turboBin: fakeTurbo,
      timeoutMs: 50,
    });

    assert.equal(result.success, false);
    assert.match(result.stderr, /execution timed out after 50ms/);
  } finally {
    await rm(tmpRoot, { recursive: true, force: true });
  }
});

test("standalone playground runner reports oversized output", async () => {
  const { MAX_OUTPUT_BYTES, runTurboSource } = await loadRunner();
  const tmpRoot = await mkdtemp(join(tmpdir(), "turbo-runner-output-test-"));
  const fakeTurbo = join(tmpRoot, "turbolang");

  await writeFile(
    fakeTurbo,
    [
      "#!/bin/sh",
      `head -c ${MAX_OUTPUT_BYTES + 4096} /dev/zero | tr '\\0' x`,
      "",
    ].join("\n"),
    "utf8"
  );
  await chmod(fakeTurbo, 0o755);

  try {
    const result = await runTurboSource("fn main() { print(\"lots\") }", {
      tmpRoot,
      turboBin: fakeTurbo,
      timeoutMs: 5000,
    });

    assert.equal(result.success, false);
    assert.match(result.stderr, /playground output exceeded 128 KiB/);
  } finally {
    await rm(tmpRoot, { recursive: true, force: true });
  }
});

test("standalone playground runner reports oversized combined output", async () => {
  const { MAX_OUTPUT_BYTES, runTurboSource } = await loadRunner();
  const tmpRoot = await mkdtemp(join(tmpdir(), "turbo-runner-combined-output-test-"));
  const fakeTurbo = join(tmpRoot, "turbolang");
  const stdoutBytes = Math.floor(MAX_OUTPUT_BYTES * 0.75);
  const stderrBytes = MAX_OUTPUT_BYTES - stdoutBytes + 1;

  await writeFile(
    fakeTurbo,
    [
      "#!/bin/sh",
      `head -c ${stdoutBytes} /dev/zero | tr '\\0' x`,
      `head -c ${stderrBytes} /dev/zero | tr '\\0' y >&2`,
      "",
    ].join("\n"),
    "utf8"
  );
  await chmod(fakeTurbo, 0o755);

  try {
    const result = await runTurboSource("fn main() { print(\"lots\") }", {
      tmpRoot,
      turboBin: fakeTurbo,
      timeoutMs: 5000,
    });

    assert.equal(result.success, false);
    assert.equal(result.stdout, "");
    assert.match(result.stderr, /playground output exceeded 128 KiB/);
  } finally {
    await rm(tmpRoot, { recursive: true, force: true });
  }
});

test("standalone playground runner serves health, auth, and run responses", async () => {
  const {
    MAX_REQUEST_BYTES,
    SERVER_HEADERS_TIMEOUT_MS,
    SERVER_KEEP_ALIVE_TIMEOUT_MS,
    SERVER_REQUEST_TIMEOUT_MS,
    SERVER_SOCKET_TIMEOUT_MS,
    createPlaygroundRunnerServer,
  } = await loadRunner();
  const tmpRoot = await mkdtemp(join(tmpdir(), "turbo-runner-http-test-"));
  const fakeTurbo = join(tmpRoot, "turbolang");
  const previousToken = process.env.TURBO_PLAYGROUND_RUNNER_TOKEN;
  const previousBin = process.env.TURBO_PLAYGROUND_RUNNER_BIN;
  const previousTimeout = process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS;

  await writeFile(
    fakeTurbo,
    [
      "#!/bin/sh",
      "printf 'http runner ok\\n'",
      "printf 'from %s:3:9\\n' \"$2\" >&2",
      "exit 0",
      "",
    ].join("\n"),
    "utf8"
  );
  await chmod(fakeTurbo, 0o755);

  process.env.TURBO_PLAYGROUND_RUNNER_TOKEN = "server-secret";
  process.env.TURBO_PLAYGROUND_RUNNER_BIN = fakeTurbo;
  process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS = "5000";

  const server = createPlaygroundRunnerServer();
  assert.equal(server.headersTimeout, SERVER_HEADERS_TIMEOUT_MS);
  assert.equal(server.keepAliveTimeout, SERVER_KEEP_ALIVE_TIMEOUT_MS);
  assert.equal(server.requestTimeout, SERVER_REQUEST_TIMEOUT_MS);
  assert.equal(server.timeout, SERVER_SOCKET_TIMEOUT_MS);

  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  const baseUrl = `http://127.0.0.1:${port}`;

  try {
    const health = await fetch(`${baseUrl}/healthz`);
    assert.equal(health.status, 200);
    assert.deepEqual(await health.json(), { ok: true });

    const unauthorized = await fetch(`${baseUrl}/run`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ source: "fn main() {}" }),
    });
    assert.equal(unauthorized.status, 401);
    assert.equal((await unauthorized.json()).success, false);

    const wrongContentType = await fetch(`${baseUrl}/run`, {
      method: "POST",
      headers: {
        authorization: "Bearer server-secret",
        "content-type": "text/plain",
      },
      body: JSON.stringify({ source: "fn main() {}" }),
    });
    assert.equal(wrongContentType.status, 415);
    assert.deepEqual(await wrongContentType.json(), {
      stdout: "",
      stderr: "Request content-type must be application/json.",
      success: false,
    });

    const invalidUtf8 = await fetch(`${baseUrl}/run`, {
      method: "POST",
      headers: {
        authorization: "Bearer server-secret",
        "content-type": "application/json",
      },
      body: Buffer.concat([
        Buffer.from('{"source":"'),
        Buffer.from([0xc3, 0x28]),
        Buffer.from('"}'),
      ]),
    });
    assert.equal(invalidUtf8.status, 400);
    assert.deepEqual(await invalidUtf8.json(), {
      stdout: "",
      stderr: "Request body must be valid JSON.",
      success: false,
    });

    const declaredTooLarge = await readRawHttpResponse(
      port,
      [
        "POST /run HTTP/1.1",
        "Host: 127.0.0.1",
        "Authorization: Bearer server-secret",
        "Content-Type: application/json",
        `Content-Length: ${MAX_REQUEST_BYTES + 1}`,
        "",
        "",
      ].join("\r\n")
    );
    assert.match(declaredTooLarge, /^HTTP\/1\.1 413 /);
    assert.match(declaredTooLarge, /Playground request is too large/);

    const run = await fetch(`${baseUrl}/run`, {
      method: "POST",
      headers: {
        authorization: "Bearer server-secret",
        "content-type": "application/json",
      },
      body: JSON.stringify({ source: "fn main() { print(1) }" }),
    });
    const body = await run.json();

    assert.equal(run.status, 200);
    assert.equal(body.success, true);
    assert.equal(body.stdout, "http runner ok\n");
    assert.equal(body.stderr, "from playground.tb:3:9\n");
    assert.equal(typeof body.durationMs, "number");
  } finally {
    await new Promise((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    );
    if (previousToken === undefined) {
      delete process.env.TURBO_PLAYGROUND_RUNNER_TOKEN;
    } else {
      process.env.TURBO_PLAYGROUND_RUNNER_TOKEN = previousToken;
    }
    if (previousBin === undefined) {
      delete process.env.TURBO_PLAYGROUND_RUNNER_BIN;
    } else {
      process.env.TURBO_PLAYGROUND_RUNNER_BIN = previousBin;
    }
    if (previousTimeout === undefined) {
      delete process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS;
    } else {
      process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS = previousTimeout;
    }
    await rm(tmpRoot, { recursive: true, force: true });
  }
});

test("standalone playground runner treats blank token as missing in isolated tokenless mode", async () => {
  const { createPlaygroundRunnerServer } = await loadRunner();
  const tmpRoot = await mkdtemp(join(tmpdir(), "turbo-runner-tokenless-test-"));
  const fakeTurbo = join(tmpRoot, "turbolang");
  const previousToken = process.env.TURBO_PLAYGROUND_RUNNER_TOKEN;
  const previousAllowMissing = process.env.TURBO_PLAYGROUND_RUNNER_ALLOW_MISSING_TOKEN;
  const previousBin = process.env.TURBO_PLAYGROUND_RUNNER_BIN;
  const previousTimeout = process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS;

  await writeFile(
    fakeTurbo,
    ["#!/bin/sh", "printf 'tokenless ok\\n'", "exit 0", ""].join("\n"),
    "utf8"
  );
  await chmod(fakeTurbo, 0o755);

  process.env.TURBO_PLAYGROUND_RUNNER_TOKEN = "   ";
  process.env.TURBO_PLAYGROUND_RUNNER_ALLOW_MISSING_TOKEN = "1";
  process.env.TURBO_PLAYGROUND_RUNNER_BIN = fakeTurbo;
  process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS = "5000";

  const server = createPlaygroundRunnerServer();
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  const baseUrl = `http://127.0.0.1:${port}`;

  try {
    const run = await fetch(`${baseUrl}/run`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ source: "fn main() { print(1) }" }),
    });
    const body = await run.json();

    assert.equal(run.status, 200);
    assert.equal(body.success, true);
    assert.equal(body.stdout, "tokenless ok\n");
  } finally {
    await new Promise((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    );
    if (previousToken === undefined) {
      delete process.env.TURBO_PLAYGROUND_RUNNER_TOKEN;
    } else {
      process.env.TURBO_PLAYGROUND_RUNNER_TOKEN = previousToken;
    }
    if (previousAllowMissing === undefined) {
      delete process.env.TURBO_PLAYGROUND_RUNNER_ALLOW_MISSING_TOKEN;
    } else {
      process.env.TURBO_PLAYGROUND_RUNNER_ALLOW_MISSING_TOKEN = previousAllowMissing;
    }
    if (previousBin === undefined) {
      delete process.env.TURBO_PLAYGROUND_RUNNER_BIN;
    } else {
      process.env.TURBO_PLAYGROUND_RUNNER_BIN = previousBin;
    }
    if (previousTimeout === undefined) {
      delete process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS;
    } else {
      process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS = previousTimeout;
    }
    await rm(tmpRoot, { recursive: true, force: true });
  }
});

test("standalone playground runner limits concurrent executions", async () => {
  const { createPlaygroundRunnerServer } = await loadRunner();
  const tmpRoot = await mkdtemp(join(tmpdir(), "turbo-runner-concurrency-test-"));
  const fakeTurbo = join(tmpRoot, "turbolang");
  const previousToken = process.env.TURBO_PLAYGROUND_RUNNER_TOKEN;
  const previousBin = process.env.TURBO_PLAYGROUND_RUNNER_BIN;
  const previousTimeout = process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS;
  const previousMaxConcurrent = process.env.TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT;

  await writeFile(
    fakeTurbo,
    [
      "#!/bin/sh",
      "sleep 0.3",
      "printf 'slow runner ok\\n'",
      "exit 0",
      "",
    ].join("\n"),
    "utf8"
  );
  await chmod(fakeTurbo, 0o755);

  process.env.TURBO_PLAYGROUND_RUNNER_TOKEN = "server-secret";
  process.env.TURBO_PLAYGROUND_RUNNER_BIN = fakeTurbo;
  process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS = "5000";
  process.env.TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT = "1";

  const server = createPlaygroundRunnerServer();
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  const baseUrl = `http://127.0.0.1:${port}`;
  const request = () =>
    fetch(`${baseUrl}/run`, {
      method: "POST",
      headers: {
        authorization: "Bearer server-secret",
        "content-type": "application/json",
      },
      body: JSON.stringify({ source: "fn main() { print(1) }" }),
    });

  try {
    const first = request();
    await new Promise((resolve) => setTimeout(resolve, 50));
    const busy = await request();

    assert.equal(busy.status, 429);
    assert.deepEqual(await busy.json(), {
      stdout: "",
      stderr: "Playground runner is busy. Try again shortly.",
      success: false,
    });

    const completed = await first;
    assert.equal(completed.status, 200);
    assert.equal((await completed.json()).stdout, "slow runner ok\n");
  } finally {
    await new Promise((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    );
    if (previousToken === undefined) {
      delete process.env.TURBO_PLAYGROUND_RUNNER_TOKEN;
    } else {
      process.env.TURBO_PLAYGROUND_RUNNER_TOKEN = previousToken;
    }
    if (previousBin === undefined) {
      delete process.env.TURBO_PLAYGROUND_RUNNER_BIN;
    } else {
      process.env.TURBO_PLAYGROUND_RUNNER_BIN = previousBin;
    }
    if (previousTimeout === undefined) {
      delete process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS;
    } else {
      process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS = previousTimeout;
    }
    if (previousMaxConcurrent === undefined) {
      delete process.env.TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT;
    } else {
      process.env.TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT = previousMaxConcurrent;
    }
    await rm(tmpRoot, { recursive: true, force: true });
  }
});
