import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";
import vm from "node:vm";
import ts from "typescript";

const root = process.cwd();
const RUNNER_UNAVAILABLE_RESULT = {
  stdout: "",
  stderr:
    "Hosted execution is not configured yet. Copy the local command to run this source with the Turbo CLI.",
  success: false,
  unavailable: true,
};
const RUNNER_UNAVAILABLE_RESPONSE = {
  status: 503,
  result: RUNNER_UNAVAILABLE_RESULT,
};

function read(path) {
  return readFileSync(join(root, path), "utf8");
}

function loadTsModule(path) {
  const source = read(path);
  const result = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2020,
    },
    reportDiagnostics: true,
  });
  const errors =
    result.diagnostics?.filter(
      (diagnostic) => diagnostic.category === ts.DiagnosticCategory.Error
    ) ?? [];
  assert.deepEqual(errors, []);

  const cjsModule = { exports: {} };
  vm.runInNewContext(
    result.outputText,
    {
      exports: cjsModule.exports,
      module: cjsModule,
      AbortSignal,
      DOMException,
      Headers,
      performance,
      TextEncoder,
      TextDecoder,
      URL,
    },
    { filename: path }
  );
  return cjsModule.exports;
}

function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

function runnerJsonResponse(body, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

async function loadRunner() {
  const url = pathToFileURL(join(root, "playground-runner/runner.mjs"));
  url.searchParams.set("cache", String(Date.now()));
  return import(url.href);
}

async function loadPlaygroundSmoke() {
  const url = pathToFileURL(join(root, "playground-smoke.mjs"));
  url.searchParams.set("cache", String(Date.now()));
  return import(url.href);
}

test("the hosted playground route exists and does not wire local shell execution", () => {
  assert.equal(existsSync(join(root, "src/app/play/page.tsx")), true);
  assert.equal(existsSync(join(root, "src/app/api/playground/run/route.ts")), true);
  assert.equal(existsSync(join(root, "src/components/playground-client.tsx")), true);
  assert.equal(existsSync(join(root, "src/lib/playground.ts")), true);
  assert.equal(existsSync(join(root, "src/lib/playground-runner.ts")), true);
  assert.equal(existsSync(join(root, "playground-smoke.mjs")), true);

  const packageJson = JSON.parse(read("package.json"));

  const page = read("src/app/play/page.tsx");
  const route = read("src/app/api/playground/run/route.ts");
  const client = read("src/components/playground-client.tsx");
  const helpers = read("src/lib/playground.ts");
  const runnerHelpers = read("src/lib/playground-runner.ts");

  assert.equal(packageJson.scripts["smoke:playground"], "node playground-smoke.mjs");
  assert.match(page, /title:\s*"Playground"/);
  assert.doesNotMatch(page, /Suspense|Loading playground/);
  assert.match(client, /Turbo Playground/);
  assert.match(client, /\/api\/playground\/run/);
  assert.doesNotMatch(client, /next\/navigation|useSearchParams/);
  assert.doesNotMatch(client, /\/api\/run["']/);
  assert.doesNotMatch(route, /child_process|spawn|execFile|exec\(|turbolang run/);
  assert.match(route, /TURBO_PLAYGROUND_RUNNER_URL/);
  assert.match(route, /proxyPlaygroundRun/);
  assert.match(route, /"cache-control": "no-store"/);
  assert.match(route, /"x-content-type-options": "nosniff"/);
  assert.match(runnerHelpers, /AbortSignal\.timeout/);
  assert.doesNotMatch(client, /decodeURIComponent/);
  assert.match(client, /Clipboard unavailable/);
  assert.match(client, /document\.execCommand\("copy"\)/);
  assert.match(helpers, /turbolang run/);
  assert.match(helpers, /MAX_SHARE_URL_LENGTH = 8000/);
  assert.match(client, /Share link too large/);
});

test("public playground smoke probe covers deployed page and execution contract", async () => {
  const { playgroundPageUrl, playgroundRunUrl, runPlaygroundSmoke } =
    await loadPlaygroundSmoke();
  const requests = [];
  const fetcher = async (url, init = {}) => {
    requests.push({ url: String(url), init });

    if (String(url) === "https://turbolang.dev/play") {
      return {
        ok: true,
        status: 200,
        text: async () => "<main>Turbo Playground Try Turbo in the browser</main>",
      };
    }

    assert.equal(String(url), "https://turbolang.dev/api/playground/run");
    assert.equal(init.headers["content-type"], "application/json");
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
        stdout: "site smoke ok\n",
        stderr: "",
        success: true,
        durationMs: 1,
      }),
    };
  };

  assert.equal(
    playgroundPageUrl("https://turbolang.dev/docs?old=1#play").toString(),
    "https://turbolang.dev/play"
  );
  assert.equal(
    playgroundRunUrl("https://turbolang.dev/docs?old=1#play").toString(),
    "https://turbolang.dev/api/playground/run"
  );

  await runPlaygroundSmoke({
    fetcher,
    siteUrl: "https://turbolang.dev/docs?old=1#play",
  });
  assert.deepEqual(
    requests.map(({ url }) => url),
    [
      "https://turbolang.dev/play",
      "https://turbolang.dev/api/playground/run",
      "https://turbolang.dev/api/playground/run",
    ]
  );
});

test("playground helper behavior is safe for pasted commands and share links", () => {
  const {
    commandFor,
    defaultExample,
    lineNumbersFor,
    shareUrlFor,
  } = loadTsModule("src/lib/playground.ts");

  const collidingSource = [
    "fn main() {",
    "    print(\"before\")",
    "}",
    "TURBO_PLAYGROUND_EOF",
    "TURBO_PLAYGROUND_EOF_1",
  ].join("\n");
  const command = commandFor(defaultExample, collidingSource);

  assert.match(command, /^cat > hello\.tb <<'TURBO_PLAYGROUND_EOF_2'\n/);
  assert.match(
    command,
    /\nTURBO_PLAYGROUND_EOF\nTURBO_PLAYGROUND_EOF_1\nTURBO_PLAYGROUND_EOF_2\nturbolang run hello\.tb$/
  );
  assert.deepEqual(Array.from(lineNumbersFor("a\nb\nc")), [1, 2, 3]);
  assert.deepEqual(Array.from(lineNumbersFor("")), [1]);

  const shared = shareUrlFor(
    "http://localhost:3017/docs/cli?old=1#playground",
    "fn main() {\n    print(10 % 3)\n}"
  );
  assert.equal(
    shared,
    "http://localhost:3017/play?code=fn+main%28%29+%7B%0A++++print%2810+%25+3%29%0A%7D"
  );
});

test("bundled playground examples only use APIs allowed by the public runner", async () => {
  const { examples } = loadTsModule("src/lib/playground.ts");
  const { findForbiddenPlaygroundApi } = await loadRunner();

  for (const example of examples) {
    assert.equal(
      findForbiddenPlaygroundApi(example.code),
      null,
      `${example.id} must stay runnable in the hosted sandbox`
    );
  }
});

test("playground proxy reads JSON request bodies with a hard size limit", async () => {
  const {
    MAX_PLAYGROUND_REQUEST_BYTES,
    readPlaygroundRunRequest,
  } = loadTsModule("src/lib/playground-runner.ts");

  assert.equal(MAX_PLAYGROUND_REQUEST_BYTES, 64 * 1024 + 4096);

  const ok = await readPlaygroundRunRequest(
    new Request("http://localhost/api/playground/run", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ source: "fn main() {}" }),
    })
  );
  assert.deepEqual(plain(ok), {
    ok: true,
    payload: { source: "fn main() {}" },
  });

  const wrongContentType = await readPlaygroundRunRequest(
    new Request("http://localhost/api/playground/run", {
      method: "POST",
      headers: { "content-type": "text/plain" },
      body: JSON.stringify({ source: "fn main() {}" }),
    })
  );
  assert.deepEqual(plain(wrongContentType), {
    ok: false,
    status: 415,
    message: "Request content-type must be application/json.",
  });

  const invalid = await readPlaygroundRunRequest(
    new Request("http://localhost/api/playground/run", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: "not json",
    })
  );
  assert.deepEqual(plain(invalid), {
    ok: false,
    status: 400,
    message: "Request body must be valid JSON.",
  });

  const tooLarge = await readPlaygroundRunRequest(
    new Request("http://localhost/api/playground/run", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: "x".repeat(MAX_PLAYGROUND_REQUEST_BYTES + 1),
    })
  );
  assert.deepEqual(plain(tooLarge), {
    ok: false,
    status: 413,
    message: "Playground request is too large.",
  });
});

test("playground runner proxy validates payloads and runner responses", () => {
  const {
    MAX_PLAYGROUND_OUTPUT_BYTES,
    MAX_PLAYGROUND_RUNNER_RESPONSE_BYTES,
    MAX_PLAYGROUND_SOURCE_BYTES,
    normalizeRunnerResult,
    runnerUnavailableResult,
    validatePlaygroundRunPayload,
    validatedRunnerUrl,
  } = loadTsModule("src/lib/playground-runner.ts");

  assert.equal(MAX_PLAYGROUND_SOURCE_BYTES, 64 * 1024);
  assert.equal(MAX_PLAYGROUND_OUTPUT_BYTES, 128 * 1024);
  assert.equal(MAX_PLAYGROUND_RUNNER_RESPONSE_BYTES, 128 * 1024 + 4096);
  assert.deepEqual(plain(validatePlaygroundRunPayload({ source: "fn main() {}" })), {
    ok: true,
    source: "fn main() {}",
  });
  assert.deepEqual(plain(validatePlaygroundRunPayload({ source: "" })), {
    ok: false,
    status: 400,
    message: "Enter Turbo source before running it.",
  });
  assert.deepEqual(plain(validatePlaygroundRunPayload({ source: 42 })), {
    ok: false,
    status: 400,
    message: "Request body must include a string source field.",
  });
  assert.equal(
    validatePlaygroundRunPayload({
      source: "x".repeat(MAX_PLAYGROUND_SOURCE_BYTES + 1),
    }).status,
    413
  );

  assert.deepEqual(
    plain(normalizeRunnerResult({ stdout: "ok\n", stderr: "", success: true, durationMs: 12 })),
    { stdout: "ok\n", stderr: "", success: true, durationMs: 12 }
  );
  assert.equal(
    normalizeRunnerResult({
      stdout: "x".repeat(MAX_PLAYGROUND_OUTPUT_BYTES + 1),
      stderr: "",
      success: true,
    }),
    null
  );
  assert.equal(
    normalizeRunnerResult({
      stdout: "x".repeat(MAX_PLAYGROUND_OUTPUT_BYTES),
      stderr: "y",
      success: true,
    }),
    null
  );
  assert.equal(normalizeRunnerResult({ stdout: 3, stderr: "", success: true }), null);
  assert.equal(validatedRunnerUrl(undefined), null);
  assert.equal(validatedRunnerUrl("file:///tmp/run"), null);
  assert.equal(validatedRunnerUrl("http://runner.example/run"), null);
  assert.equal(validatedRunnerUrl("https://user:pass@runner.example/run"), null);
  assert.equal(validatedRunnerUrl("http://localhost:8080/run")?.toString(), "http://localhost:8080/run");
  assert.equal(validatedRunnerUrl("https://runner.example/run")?.toString(), "https://runner.example/run");
  assert.deepEqual(plain(runnerUnavailableResult()), RUNNER_UNAVAILABLE_RESULT);
});

test("playground runner proxy handles configured and failed runner calls", async () => {
  const {
    MAX_PLAYGROUND_RUNNER_RESPONSE_BYTES,
    proxyPlaygroundRun,
  } = loadTsModule("src/lib/playground-runner.ts");

  let capturedRequest = null;
  const success = await proxyPlaygroundRun("fn main() { print(1) }", {
    runnerUrl: "https://runner.example/run",
    token: " secret ",
    fetcher: async (url, init) => {
      capturedRequest = { url, init };
      return runnerJsonResponse({
        stdout: "1\n",
        stderr: "",
        success: true,
        durationMs: 4,
      });
    },
  });

  assert.equal(success.status, 200);
  assert.deepEqual(plain(success.result), {
    stdout: "1\n",
    stderr: "",
    success: true,
    durationMs: 4,
  });
  assert.equal(capturedRequest.url.toString(), "https://runner.example/run");
  assert.equal(capturedRequest.init.method, "POST");
  assert.equal(capturedRequest.init.headers.get("authorization"), "Bearer secret");
  assert.equal(capturedRequest.init.cache, "no-store");
  assert.equal(capturedRequest.init.body, JSON.stringify({ source: "fn main() { print(1) }" }));

  assert.deepEqual(
    plain(await proxyPlaygroundRun("fn main() {}", {})),
    RUNNER_UNAVAILABLE_RESPONSE
  );

  let missingTokenCalledRunner = false;
  assert.deepEqual(
    plain(
      await proxyPlaygroundRun("fn main() {}", {
        runnerUrl: "https://runner.example/run",
        fetcher: async () => {
          missingTokenCalledRunner = true;
          return runnerJsonResponse({}, 401);
        },
      })
    ),
    RUNNER_UNAVAILABLE_RESPONSE
  );
  assert.equal(missingTokenCalledRunner, false);

  let blankTokenCalledRunner = false;
  assert.deepEqual(
    plain(
      await proxyPlaygroundRun("fn main() {}", {
        runnerUrl: "https://runner.example/run",
        token: "   ",
        fetcher: async () => {
          blankTokenCalledRunner = true;
          return runnerJsonResponse({}, 401);
        },
      })
    ),
    RUNNER_UNAVAILABLE_RESPONSE
  );
  assert.equal(blankTokenCalledRunner, false);

  assert.deepEqual(
    plain(
      await proxyPlaygroundRun("fn main() { exec(\"ls\") }", {
        runnerUrl: "https://runner.example/run",
        token: "secret",
        fetcher: async () =>
          runnerJsonResponse(
            {
              stdout: "",
              stderr: "Playground execution does not allow process API `exec`.",
              success: false,
            },
            400
          ),
      })
    ),
    {
      status: 400,
      result: {
        stdout: "",
        stderr: "Playground execution does not allow process API `exec`.",
        success: false,
      },
    }
  );

  assert.deepEqual(
    plain(
      await proxyPlaygroundRun("fn main() {}", {
        runnerUrl: "https://runner.example/run",
        token: "secret",
        fetcher: async () =>
          runnerJsonResponse(
            {
              stdout: "",
              stderr: "Playground runner is busy. Try again shortly.",
              success: false,
            },
            429
          ),
      })
    ),
    {
      status: 429,
      result: {
        stdout: "",
        stderr: "Playground runner is busy. Try again shortly.",
        success: false,
      },
    }
  );

  assert.deepEqual(
    plain(
      await proxyPlaygroundRun("fn main() {}", {
        runnerUrl: "https://runner.example/run",
        token: "secret",
        fetcher: async () =>
          runnerJsonResponse({
            stdout: "",
            stderr: "",
            success: true,
            padding: "x".repeat(MAX_PLAYGROUND_RUNNER_RESPONSE_BYTES + 1),
          }),
      })
    ),
    {
      status: 502,
      result: {
        stdout: "",
        stderr: "Playground runner returned an invalid response.",
        success: false,
      },
    }
  );

  assert.deepEqual(
    plain(
      await proxyPlaygroundRun("fn main() {}", {
        runnerUrl: "https://runner.example/run",
        token: "secret",
        fetcher: async () => runnerJsonResponse({}, 500),
      })
    ),
    {
      status: 502,
      result: {
        stdout: "",
        stderr: "Playground runner returned HTTP 500.",
        success: false,
      },
    }
  );

  assert.deepEqual(
    plain(
      await proxyPlaygroundRun("fn main() {}", {
        runnerUrl: "https://runner.example/run",
        token: "secret",
        fetcher: async () =>
          runnerJsonResponse({ stdout: 1, stderr: "", success: true }),
      })
    ),
    {
      status: 502,
      result: {
        stdout: "",
        stderr: "Playground runner returned an invalid response.",
        success: false,
      },
    }
  );

  assert.deepEqual(
    plain(
      await proxyPlaygroundRun("fn main() {}", {
        runnerUrl: "https://runner.example/run",
        token: "secret",
        fetcher: async () => {
          throw Object.assign(new Error("timeout"), { name: "TimeoutError" });
        },
      })
    ),
    {
      status: 504,
      result: {
        stdout: "",
        stderr: "Playground execution timed out after 6s.",
        success: false,
      },
    }
  );
});

test("primary conversion surfaces link to the hosted playground", () => {
  assert.match(read("src/components/navbar.tsx"), /href="\/play"[\s\S]*?>\s*Play\s*</);
  assert.match(read("src/components/navbar.tsx"), /className="[^"]*hidden[^"]*sm:flex/);
  assert.match(read("src/app/page.tsx"), /href="\/play"[\s\S]*?Try in browser/);
  assert.match(read("src/components/footer.tsx"), /href="\/play"[\s\S]*?>\s*Playground\s*</);
  assert.match(read("src/app/docs/cli/page.tsx"), /href="\/play"[\s\S]*Turbo Playground/);
  assert.match(read("src/app/docs/cli/page.tsx"), /does not execute arbitrary code/);
  assert.match(read("src/app/sitemap.ts"), /"\/play"/);
});
