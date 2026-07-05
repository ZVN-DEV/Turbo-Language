import { execFile } from "node:child_process";
import { timingSafeEqual } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

export const MAX_SOURCE_BYTES = 64 * 1024;
export const MAX_REQUEST_BYTES = MAX_SOURCE_BYTES + 4096;
export const MAX_OUTPUT_BYTES = 128 * 1024;
export const DEFAULT_TIMEOUT_MS = 5000;
export const DEFAULT_MAX_CONCURRENT_RUNS = 2;
export const DEFAULT_PORT = 8787;
export const SERVER_HEADERS_TIMEOUT_MS = 5000;
export const SERVER_KEEP_ALIVE_TIMEOUT_MS = 5000;
export const SERVER_REQUEST_TIMEOUT_MS = 10000;
export const SERVER_SOCKET_TIMEOUT_MS = 15000;

const encoder = new TextEncoder();
const jsonDecoder = new TextDecoder("utf-8", { fatal: true });
const friendlySourceName = "playground.tb";
let activeRunCount = 0;
const forbiddenPlaygroundApis = new Map([
  ["args", "process"],
  ["delete_file", "filesystem"],
  ["deref", "raw memory"],
  ["env_get", "environment"],
  ["exec", "process"],
  ["extern", "unsafe/FFI"],
  ["file_exists", "filesystem"],
  ["http_get", "network"],
  ["http_listen", "network"],
  ["http_post", "network"],
  ["http_post_with_headers", "network"],
  ["http_server", "network"],
  ["http_server_public", "network"],
  ["import", "file import"],
  ["list_dir", "filesystem"],
  ["mkdir", "filesystem"],
  ["read_file", "filesystem"],
  ["read_line", "interactive input"],
  ["route", "network"],
  ["shell_exec", "process"],
  ["store", "raw memory"],
  ["try_read_file", "filesystem"],
  ["try_write_file", "filesystem"],
  ["unsafe", "unsafe/FFI"],
  ["write_file", "filesystem"],
]);

export function validateRunPayload(payload) {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    return {
      ok: false,
      status: 400,
      message: "Request body must include a string source field.",
    };
  }

  const { source } = payload;
  if (typeof source !== "string") {
    return {
      ok: false,
      status: 400,
      message: "Request body must include a string source field.",
    };
  }

  if (source.trim().length === 0) {
    return {
      ok: false,
      status: 400,
      message: "Enter Turbo source before running it.",
    };
  }

  if (encoder.encode(source).length > MAX_SOURCE_BYTES) {
    return {
      ok: false,
      status: 413,
      message: "Playground source is limited to 64 KiB.",
    };
  }

  const forbidden = findForbiddenPlaygroundApi(source);
  if (forbidden) {
    return {
      ok: false,
      status: 400,
      message: forbiddenPlaygroundMessage(forbidden),
    };
  }

  return { ok: true, source };
}

export function findForbiddenPlaygroundApi(source) {
  for (const ident of identifiersOutsideStringsAndComments(source)) {
    const category = forbiddenPlaygroundApis.get(ident);
    if (category) return { name: ident, category };
  }
  return null;
}

function forbiddenPlaygroundMessage(forbidden) {
  if (forbidden.name === "import") {
    return "Playground execution does not allow file imports.";
  }

  if (forbidden.category === "unsafe/FFI" || forbidden.category === "raw memory") {
    return "Playground execution does not allow unsafe or FFI features.";
  }

  return `Playground execution does not allow ${forbidden.category} API \`${forbidden.name}\`.`;
}

function identifiersOutsideStringsAndComments(source) {
  const idents = [];
  let i = 0;

  while (i < source.length) {
    if (source.startsWith("//", i)) {
      i = skipLineComment(source, i + 2);
      continue;
    }
    if (source.startsWith("/*", i)) {
      i = skipBlockComment(source, i + 2);
      continue;
    }
    if (source.startsWith('"""', i)) {
      i = skipTripleString(source, i + 3);
      continue;
    }
    if (source.startsWith('r"', i)) {
      i = skipRawString(source, i + 2);
      continue;
    }
    if (source[i] === '"') {
      i = scanInterpolatedString(source, i + 1, idents);
      continue;
    }

    if (isIdentStart(source[i])) {
      const start = i;
      i += 1;
      while (i < source.length && isIdentChar(source[i])) i += 1;
      idents.push(source.slice(start, i));
      continue;
    }

    i += 1;
  }

  return idents;
}

function skipLineComment(source, i) {
  while (i < source.length && source[i] !== "\n") i += 1;
  return i;
}

function skipBlockComment(source, i) {
  let depth = 1;
  while (i < source.length && depth > 0) {
    if (source.startsWith("/*", i)) {
      depth += 1;
      i += 2;
    } else if (source.startsWith("*/", i)) {
      depth -= 1;
      i += 2;
    } else {
      i += 1;
    }
  }
  return i;
}

function skipTripleString(source, i) {
  const end = source.indexOf('"""', i);
  return end === -1 ? source.length : end + 3;
}

function skipRawString(source, i) {
  const end = source.indexOf('"', i);
  return end === -1 ? source.length : end + 1;
}

function scanInterpolatedString(source, i, idents) {
  while (i < source.length) {
    if (source[i] === "\\") {
      i += 2;
    } else if (source[i] === '"') {
      return i + 1;
    } else if (source[i] === "{") {
      i = scanStringInterpolation(source, i + 1, idents);
    } else {
      i += 1;
    }
  }
  return source.length;
}

function scanStringInterpolation(source, i, idents) {
  let depth = 1;
  while (i < source.length && depth > 0) {
    if (source.startsWith("//", i)) {
      i = skipLineComment(source, i + 2);
      continue;
    }
    if (source.startsWith("/*", i)) {
      i = skipBlockComment(source, i + 2);
      continue;
    }
    if (source.startsWith('"""', i)) {
      i = skipTripleString(source, i + 3);
      continue;
    }
    if (source.startsWith('r"', i)) {
      i = skipRawString(source, i + 2);
      continue;
    }
    if (source[i] === '"') {
      i = scanInterpolatedString(source, i + 1, idents);
      continue;
    }
    if (source[i] === "{") {
      depth += 1;
      i += 1;
      continue;
    }
    if (source[i] === "}") {
      depth -= 1;
      i += 1;
      continue;
    }

    if (isIdentStart(source[i])) {
      const start = i;
      i += 1;
      while (i < source.length && isIdentChar(source[i])) i += 1;
      idents.push(source.slice(start, i));
      continue;
    }

    i += 1;
  }
  return i;
}

function isIdentStart(char) {
  return /[A-Za-z_]/.test(char ?? "");
}

function isIdentChar(char) {
  return /[A-Za-z0-9_]/.test(char ?? "");
}

export function authorizeRequest(token, authorizationHeader) {
  if (token === undefined || token === null) return true;
  const normalizedToken = normalizedConfiguredToken(token);
  if (!normalizedToken) return false;
  if (typeof authorizationHeader !== "string") return false;

  const expected = Buffer.from(`Bearer ${normalizedToken}`);
  const actual = Buffer.from(authorizationHeader);
  if (expected.length !== actual.length) return false;

  return timingSafeEqual(expected, actual);
}

function configuredAuthToken(env = process.env) {
  const token = normalizedConfiguredToken(env.TURBO_PLAYGROUND_RUNNER_TOKEN);
  if (token) return token;

  return env.TURBO_PLAYGROUND_RUNNER_ALLOW_MISSING_TOKEN === "1" ? undefined : "";
}

export function sanitizeRunnerOutput(output) {
  return stripAnsi(String(output))
    .replace(
      /(?:\/[^\s:]+)*\/?turbo-playground-runner-[^\s:/]+\/main\.tb/g,
      friendlySourceName
    )
    .replace(/turbo-playground-runner-[^\s:/]+\/main\.tb/g, friendlySourceName);
}

export async function runTurboSource(source, options = {}) {
  const turboBin = options.turboBin ?? process.env.TURBO_PLAYGROUND_RUNNER_BIN ?? "turbolang";
  const configuredTimeoutMs =
    options.timeoutMs ??
    parsePositiveInteger(process.env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS);
  const timeoutMs =
    Number.isFinite(configuredTimeoutMs) && configuredTimeoutMs > 0
      ? configuredTimeoutMs
      : DEFAULT_TIMEOUT_MS;
  const tmpRoot = options.tmpRoot ?? tmpdir();
  const runDir = await mkdtemp(join(tmpRoot, "turbo-playground-runner-"));
  const sourcePath = join(runDir, "main.tb");
  const started = performance.now();

  try {
    await writeFile(sourcePath, source, "utf8");
    const result = await execTurbo(turboBin, sourcePath, runDir, timeoutMs);
    return {
      stdout: sanitizeRunnerOutput(result.stdout),
      stderr: sanitizeRunnerOutput(result.stderr),
      success: result.success,
      durationMs: Math.max(0, Math.round(performance.now() - started)),
    };
  } finally {
    await rm(runDir, { recursive: true, force: true });
  }
}

function execTurbo(turboBin, sourcePath, cwd, timeoutMs) {
  return new Promise((resolve) => {
    execFile(
      turboBin,
      ["run", sourcePath],
      {
        cwd,
        encoding: "utf8",
        env: childEnvironment(),
        killSignal: "SIGKILL",
        maxBuffer: MAX_OUTPUT_BYTES,
        timeout: timeoutMs,
      },
      (error, stdout, stderr) => {
        let nextStderr = stderr ?? "";
        if (error) {
          if (error.killed || error.signal === "SIGTERM") {
            nextStderr = appendLine(nextStderr, `error: execution timed out after ${timeoutMs}ms`);
          } else if (error.code === "ERR_CHILD_PROCESS_STDIO_MAXBUFFER") {
            nextStderr = appendLine(nextStderr, "error: playground output exceeded 128 KiB");
          } else if (!nextStderr.trim()) {
            nextStderr = `error: ${error.message}`;
          }
        }

        resolve({
          stdout: stdout ?? "",
          stderr: nextStderr,
          success: !error,
        });
      }
    );
  });
}

function childEnvironment() {
  return {
    HOME: "/tmp",
    LANG: "C.UTF-8",
    LC_ALL: "C.UTF-8",
    NO_COLOR: "1",
    PATH: "/usr/local/bin:/usr/bin:/bin",
    TMPDIR: "/tmp",
  };
}

function configuredMaxConcurrentRuns(env = process.env) {
  return (
    parsePositiveInteger(env.TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT) ??
    DEFAULT_MAX_CONCURRENT_RUNS
  );
}

function acquireRunSlot() {
  const maxConcurrentRuns = configuredMaxConcurrentRuns();
  if (activeRunCount >= maxConcurrentRuns) return null;

  activeRunCount += 1;
  return () => {
    activeRunCount = Math.max(0, activeRunCount - 1);
  };
}

function appendLine(current, line) {
  return current.trim().length === 0 ? line : `${current.replace(/\s+$/, "")}\n${line}`;
}

function stripAnsi(input) {
  let out = "";
  for (let i = 0; i < input.length; i += 1) {
    const char = input[i];
    if (char !== "\u001b") {
      out += char;
      continue;
    }

    const next = input[i + 1];
    if (next === "[") {
      i += 2;
      while (i < input.length && !/[\u0040-\u007e]/.test(input[i])) i += 1;
    } else if (next === "]") {
      i += 2;
      while (i < input.length && input[i] !== "\u0007") {
        if (input[i] === "\u001b" && input[i + 1] === "\\") {
          i += 1;
          break;
        }
        i += 1;
      }
    }
  }
  return out;
}

async function readJsonRequest(request) {
  const chunks = [];
  let size = 0;

  for await (const chunk of request) {
    size += chunk.length;
    if (size > MAX_REQUEST_BYTES) {
      return {
        ok: false,
        status: 413,
        message: "Playground request is too large.",
      };
    }
    chunks.push(chunk);
  }

  try {
    return { ok: true, payload: JSON.parse(jsonDecoder.decode(Buffer.concat(chunks))) };
  } catch {
    return { ok: false, status: 400, message: "Request body must be valid JSON." };
  }
}

async function handleRun(request, response) {
  const token = configuredAuthToken();
  if (!authorizeRequest(token, request.headers.authorization)) {
    writeJson(response, 401, {
      stdout: "",
      stderr: "Playground runner authorization failed.",
      success: false,
    });
    return;
  }

  if (!hasJsonContentType(request.headers["content-type"])) {
    writeJson(response, 415, {
      stdout: "",
      stderr: "Request content-type must be application/json.",
      success: false,
    });
    return;
  }

  const body = await readJsonRequest(request);
  if (!body.ok) {
    writeJson(response, body.status, { stdout: "", stderr: body.message, success: false });
    return;
  }

  const validated = validateRunPayload(body.payload);
  if (!validated.ok) {
    writeJson(response, validated.status, {
      stdout: "",
      stderr: validated.message,
      success: false,
    });
    return;
  }

  const releaseRunSlot = acquireRunSlot();
  if (!releaseRunSlot) {
    writeJson(response, 429, {
      stdout: "",
      stderr: "Playground runner is busy. Try again shortly.",
      success: false,
    });
    return;
  }

  try {
    writeJson(response, 200, await runTurboSource(validated.source));
  } catch (error) {
    const message =
      error instanceof Error ? error.message : "unknown playground runner error";
    writeJson(response, 500, {
      stdout: "",
      stderr: `Playground runner failed: ${message}`,
      success: false,
    });
  } finally {
    releaseRunSlot();
  }
}

function hasJsonContentType(contentType) {
  if (Array.isArray(contentType)) {
    return contentType.some(hasJsonContentType);
  }
  if (typeof contentType !== "string") return false;

  return contentType.split(";", 1)[0].trim().toLowerCase() === "application/json";
}

function writeJson(response, status, body) {
  const json = JSON.stringify(body);
  response.writeHead(status, {
    "cache-control": "no-store",
    "content-length": Buffer.byteLength(json),
    "content-type": "application/json; charset=utf-8",
    "x-content-type-options": "nosniff",
  });
  response.end(json);
}

export function startupConfigError(env = process.env) {
  if (env.TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST !== "1") {
    return (
      "Refusing to start: run this service inside the documented sandbox container, " +
      "or set TURBO_PLAYGROUND_RUNNER_ALLOW_UNSAFE_HOST=1 only for isolated local testing."
    );
  }

  if (
    !hasConfiguredToken(env.TURBO_PLAYGROUND_RUNNER_TOKEN) &&
    env.TURBO_PLAYGROUND_RUNNER_ALLOW_MISSING_TOKEN !== "1"
  ) {
    return (
      "Refusing to start: TURBO_PLAYGROUND_RUNNER_TOKEN is required. " +
      "Set TURBO_PLAYGROUND_RUNNER_ALLOW_MISSING_TOKEN=1 only for isolated local testing."
    );
  }

  const maxConcurrentError = positiveIntegerConfigError(
    env.TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT,
    "TURBO_PLAYGROUND_RUNNER_MAX_CONCURRENT"
  );
  if (maxConcurrentError) return maxConcurrentError;

  const timeoutError = positiveIntegerConfigError(
    env.TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS,
    "TURBO_PLAYGROUND_RUNNER_TIMEOUT_MS"
  );
  if (timeoutError) return timeoutError;

  const portError = tcpPortConfigError(env.PORT);
  if (portError) return portError;

  return null;
}

function hasConfiguredToken(token) {
  return !!normalizedConfiguredToken(token);
}

function normalizedConfiguredToken(token) {
  if (typeof token !== "string") return null;

  const normalized = token.trim();
  return normalized.length > 0 ? normalized : null;
}

function positiveIntegerConfigError(value, envName) {
  if (value === undefined || value === null || String(value).trim().length === 0) {
    return null;
  }

  if (parsePositiveInteger(value) !== null) return null;

  return `Refusing to start: ${envName} must be a positive integer.`;
}

function tcpPortConfigError(value) {
  if (value === undefined || value === null || String(value).trim().length === 0) {
    return null;
  }

  if (parseTcpPort(value) !== null) return null;

  return "Refusing to start: PORT must be an integer from 1 to 65535.";
}

function parsePositiveInteger(value) {
  if (typeof value !== "string") return null;

  const normalized = value.trim();
  if (!/^[1-9]\d*$/.test(normalized)) return null;

  const parsed = Number.parseInt(normalized, 10);
  return Number.isSafeInteger(parsed) ? parsed : null;
}

function parseTcpPort(value) {
  const port = parsePositiveInteger(value);
  if (port === null || port > 65535) return null;

  return port;
}

function configuredPort(env = process.env) {
  return parseTcpPort(env.PORT) ?? DEFAULT_PORT;
}

function requireSafeStartupConfig() {
  const error = startupConfigError();
  if (!error) return;

  console.error(error);
  process.exit(1);
}

export function createPlaygroundRunnerServer() {
  const server = createServer((request, response) => {
    void handleRequest(request, response).catch((error) => {
      const message =
        error instanceof Error ? error.message : "unknown playground runner error";
      if (!response.headersSent) {
        writeJson(response, 500, {
          stdout: "",
          stderr: `Playground runner failed: ${message}`,
          success: false,
        });
      } else {
        response.destroy(error instanceof Error ? error : undefined);
      }
    });
  });

  server.headersTimeout = SERVER_HEADERS_TIMEOUT_MS;
  server.keepAliveTimeout = SERVER_KEEP_ALIVE_TIMEOUT_MS;
  server.requestTimeout = SERVER_REQUEST_TIMEOUT_MS;
  server.timeout = SERVER_SOCKET_TIMEOUT_MS;

  return server;
}

async function handleRequest(request, response) {
  const requestUrl = new URL(request.url ?? "/", "http://runner.local");
  if (request.method === "GET" && requestUrl.pathname === "/healthz") {
    writeJson(response, 200, { ok: true });
    return;
  }
  if (request.method === "POST" && requestUrl.pathname === "/run") {
    await handleRun(request, response);
    return;
  }

  writeJson(response, 404, { stdout: "", stderr: "Not found.", success: false });
}

function isMainModule() {
  return process.argv[1] === fileURLToPath(import.meta.url);
}

if (isMainModule()) {
  requireSafeStartupConfig();
  const port = configuredPort();
  createPlaygroundRunnerServer().listen(port, "0.0.0.0", () => {
    console.log(`Turbo playground runner listening on :${port}`);
  });
}
