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
const utf8Decoder = new TextDecoder("utf-8");
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

// Category sentinel used for the fail-closed path: an identifier that is
// *called* but is neither a user definition nor a known-safe builtin. The
// denylist above catches today's dangerous APIs; this catches the ones that
// don't exist yet (the stdlib roadmap keeps adding builtins) so a newly added
// dangerous builtin is rejected by default instead of exposed by default.
const UNAVAILABLE_BUILTIN_CATEGORY = "unavailable builtin";

// Allowlist of pure/compute builtins that are always safe in the playground:
// string/array/hashmap/math/json/print/assert plus the inert HTTP request &
// response accessors (harmless without a server, which is denylisted). This is
// the union of turbo-sema's `BUILTIN_FNS` and the codegen `compile_call`
// dispatch arms, MINUS every host-access builtin (network/filesystem/process/
// env/FFI/raw-memory) — those live in `forbiddenPlaygroundApis` above. Keep in
// sync when new safe builtins land (CLAUDE.md "Adding a new built-in function").
const SAFE_PLAYGROUND_BUILTINS = new Set([
  "abs", "all", "any", "array_contains", "assert", "assert_eq", "assert_ne",
  "ceil", "channel", "char_at", "clone", "contains", "cos", "ends_with",
  "exit", "exp", "filter", "float_to_int", "floor", "format_time", "hashmap",
  "hashmap_get", "hashmap_get_int", "hashmap_has", "hashmap_inc", "hashmap_keys",
  "hashmap_len", "hashmap_remove", "hashmap_set", "hashmap_set_int",
  "hashmap_size", "index_of", "int_to_float", "join", "json_build", "json_get",
  "json_stringify", "len", "log", "log10", "log2", "lower", "map", "max", "min",
  "mutex", "mutex_get", "mutex_set", "mutex_update", "pad_left", "pad_right",
  "panic", "path_base", "path_dir", "path_ext", "path_join", "pow", "print",
  "push", "random", "random_range", "recv", "reduce", "repeat", "replace",
  "request_body", "request_header", "request_method", "request_path",
  "request_query", "respond", "respond_html", "respond_json", "respond_text",
  "reverse", "round", "send", "sin", "sleep", "slice", "sort", "split", "sqrt",
  "starts_with", "str_from_char", "str_to_float", "str_to_int", "substring",
  "tan", "time_ms", "time_now", "to_json", "to_json_array", "to_str", "trim",
  "type_of", "upper",
]);

// Turbo keywords (from turbo-lexer). Some of these read as calls lexically —
// `some(x)`, `ok(x)`, `err(x)`, `while(cond)` — so they must never be flagged
// as unknown builtins. `import`/`unsafe`/`extern` are also denylisted above and
// are caught there first.
const TURBO_KEYWORDS = new Set([
  "as", "async", "await", "break", "const", "continue", "defer", "else", "err",
  "extern", "false", "fn", "for", "from", "if", "impl", "import", "in", "let",
  "match", "mut", "none", "ok", "pub", "return", "self", "some", "spawn",
  "struct", "trait", "true", "type", "while",
]);

// Keywords that introduce a binding whose following identifier is a
// user-defined name (`fn foo`, `let x`, `let mut x`, `const N`).
const DEFINER_KEYWORDS = new Set(["fn", "let", "const", "mut"]);

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
  const tokens = identifiersOutsideStringsAndComments(source);
  const definedNames = definedNamesFromTokens(tokens);

  for (const token of tokens) {
    // Denylist first: any occurrence (call or not) of a known-dangerous
    // identifier is rejected, preserving the conservative behaviour that even
    // naming `store`/`args`/`unsafe` is refused.
    const category = forbiddenPlaygroundApis.get(token.name);
    if (category) return { name: token.name, category };

    // Fail-closed: a *call* to a lowercase name that is neither user-defined
    // nor a known-safe builtin is an unknown (possibly future-dangerous)
    // builtin — refuse it. User-defined names shadow builtins in the compiler
    // (turbo-codegen-cranelift resolves `user_fns` before builtin dispatch),
    // so allowing calls to names the program itself declares is sound.
    if (token.isCall && isUnavailableBuiltinCall(token.name, definedNames)) {
      return { name: token.name, category: UNAVAILABLE_BUILTIN_CATEGORY };
    }
  }
  return null;
}

// True when a called identifier is not something the playground permits:
// not a keyword/constructor, not a Capitalized enum-variant/type constructor,
// not a known-safe builtin, and not declared by the submitted source itself.
function isUnavailableBuiltinCall(name, definedNames) {
  if (TURBO_KEYWORDS.has(name)) return false;
  // Enum variants, struct/type constructors are Capitalized by convention
  // (`Some(x)`, `Point(...)`, `Shape::Circle(r)`); every builtin is snake_case
  // lowercase, so an uppercase-initial call can never be a dangerous builtin.
  if (/^[A-Z]/.test(name)) return false;
  if (SAFE_PLAYGROUND_BUILTINS.has(name)) return false;
  if (definedNames.has(name)) return false;
  return true;
}

// Collect the names the source declares itself: function names, `let`/`const`
// bindings (including closures bound to a variable, e.g. `let f = |x| ...`),
// and parameter/field names (identifier immediately before a `:`, e.g.
// `fn apply(f: fn(i64) -> i64, ...)` where `f` is later called). These must
// never be treated as unknown builtins.
function definedNamesFromTokens(tokens) {
  const defined = new Set();
  for (const token of tokens) {
    if (token.isDef && !TURBO_KEYWORDS.has(token.name)) defined.add(token.name);
  }
  return defined;
}

function forbiddenPlaygroundMessage(forbidden) {
  if (forbidden.category === UNAVAILABLE_BUILTIN_CATEGORY) {
    return (
      "Playground execution only allows Turbo's safe standard library; " +
      `\`${forbidden.name}\` is not an available builtin here.`
    );
  }

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
  // Name of the immediately preceding code identifier, used to detect
  // `fn NAME` / `let NAME` / `const NAME` definitions. Reset by any non-space,
  // non-identifier character (and by strings/comments) so adjacency is exact.
  let prevName = null;
  let i = 0;

  while (i < source.length) {
    if (source.startsWith("//", i)) {
      i = skipLineComment(source, i + 2);
      prevName = null;
      continue;
    }
    if (source.startsWith("/*", i)) {
      i = skipBlockComment(source, i + 2);
      prevName = null;
      continue;
    }
    if (source.startsWith('"""', i)) {
      i = scanTripleString(source, i + 3, idents);
      prevName = null;
      continue;
    }
    if (source.startsWith('r"', i)) {
      i = skipRawString(source, i + 2);
      prevName = null;
      continue;
    }
    if (source[i] === '"') {
      i = scanInterpolatedString(source, i + 1, idents);
      prevName = null;
      continue;
    }

    if (isIdentStart(source[i])) {
      const start = i;
      i += 1;
      while (i < source.length && isIdentChar(source[i])) i += 1;
      const name = source.slice(start, i);
      const isDef =
        (prevName !== null && DEFINER_KEYWORDS.has(prevName)) ||
        peekIsFnTypedColon(source, i);
      idents.push({ name, isCall: peekIsCall(source, i), isDef });
      prevName = name;
      continue;
    }

    // Whitespace keeps `let   x` adjacency; any other character breaks it.
    if (!isInlineOrLineSpace(source[i])) prevName = null;
    i += 1;
  }

  return idents;
}

// Peek past inline spaces/tabs AND newlines for the next significant character.
// Turbo's parser filters newlines, so `foo\n(x)` still calls `foo`; the peek must
// cross the newline or a split call slips past the fail-closed check. The rare
// cost is a false positive when the next statement legitimately begins with `(`,
// which only makes the check stricter — acceptable for a security allowlist.
function peekIsCall(source, i) {
  while (i < source.length && isInlineOrLineSpace(source[i])) i += 1;
  return source[i] === "(";
}

// A `name:` annotation only makes `name` call-able when its type is a function
// type (`f: fn(i64) -> i64`), the closure-parameter case. A plain `name: int`
// struct field or scalar parameter is not callable, so treating it as a defined
// name would wrongly whitelist a same-named builtin call elsewhere in the source.
function peekIsFnTypedColon(source, i) {
  while (i < source.length && (source[i] === " " || source[i] === "\t")) i += 1;
  if (source[i] !== ":") return false;
  i += 1;
  while (i < source.length && isInlineOrLineSpace(source[i])) i += 1;
  return source.startsWith("fn", i) && !isIdentChar(source[i + 2]);
}

function isInlineOrLineSpace(char) {
  return char === " " || char === "\t" || char === "\r" || char === "\n";
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

function scanTripleString(source, i, idents) {
  while (i < source.length) {
    if (source.startsWith('"""', i)) {
      return i + 3;
    }
    if (
      source[i] === "\\" &&
      (source[i + 1] === "{" || source[i + 1] === "}")
    ) {
      i += 2;
      continue;
    }
    if (source[i] === "{") {
      i = scanStringInterpolation(source, i + 1, idents);
      continue;
    }

    i += 1;
  }
  return source.length;
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
      i = scanTripleString(source, i + 3, idents);
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
      // Interpolation expressions never carry `fn`/`let` definitions, so isDef
      // is always false here; isCall still matters for catching a denylisted
      // call smuggled inside a `"{ ... }"` interpolation.
      idents.push({
        name: source.slice(start, i),
        isCall: peekIsCall(source, i),
        isDef: false,
      });
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
    const durationMs = Math.max(0, Math.round(performance.now() - started));
    const stdout = sanitizeRunnerOutput(result.stdout);
    const stderr = sanitizeRunnerOutput(result.stderr);
    if (result.outputLimitExceeded) {
      return {
        stdout: "",
        stderr: playgroundOutputLimitMessage(),
        success: false,
        durationMs,
      };
    }

    if (encodedByteLength(stdout) + encodedByteLength(stderr) > MAX_OUTPUT_BYTES) {
      const truncated = truncateCombinedOutput(stdout, stderr);
      return {
        ...truncated,
        success: result.success,
        durationMs,
      };
    }

    return {
      stdout,
      stderr,
      success: result.success,
      durationMs,
    };
  } finally {
    await rm(runDir, { recursive: true, force: true });
  }
}

// OS-level resource caps for the child, applied via `prlimit(1)` when the
// runner is told the host has it (Linux container). This is defence-in-depth
// on top of the wall-clock SIGKILL timeout and the source/output byte caps:
//
//   --cpu     hard CPU-seconds cap so a tight compute loop can't peg a core
//             past the wall timeout window.
//   --nproc   caps processes/threads to stop fork bombs.
//   --fsize   caps any single file the child writes (JIT `run` writes only the
//             tmp source, so a few MiB is ample) — blocks filling the disk.
//   --nofile  caps open file descriptors.
//   --as      caps the child's virtual address space (memory) so it can't
//             exhaust the container.
//
// Off by default (macOS dev/CI have no prlimit); the Dockerfile turns it on.
export const CHILD_RLIMITS = Object.freeze({
  cpu: 10,
  nproc: 64,
  fsizeBytes: 8 * 1024 * 1024,
  nofile: 256,
  asBytes: 512 * 1024 * 1024,
});

export function buildChildInvocation(turboBin, sourcePath, env = process.env) {
  const turboArgs = ["run", sourcePath];
  if (env.TURBO_PLAYGROUND_RUNNER_RLIMIT !== "1") {
    return { command: turboBin, args: turboArgs };
  }

  const prlimitArgs = [
    `--cpu=${CHILD_RLIMITS.cpu}`,
    `--nproc=${CHILD_RLIMITS.nproc}`,
    `--fsize=${CHILD_RLIMITS.fsizeBytes}`,
    `--nofile=${CHILD_RLIMITS.nofile}`,
    `--as=${CHILD_RLIMITS.asBytes}`,
    "--",
    turboBin,
    ...turboArgs,
  ];
  return { command: "prlimit", args: prlimitArgs };
}

function execTurbo(turboBin, sourcePath, cwd, timeoutMs) {
  const { command, args } = buildChildInvocation(turboBin, sourcePath);
  return new Promise((resolve) => {
    execFile(
      command,
      args,
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
        let outputLimitExceeded = false;
        if (error) {
          if (error.killed || error.signal === "SIGTERM") {
            nextStderr = appendLine(nextStderr, `error: execution timed out after ${timeoutMs}ms`);
          } else if (error.code === "ERR_CHILD_PROCESS_STDIO_MAXBUFFER") {
            outputLimitExceeded = true;
            nextStderr = appendLine(nextStderr, playgroundOutputLimitMessage());
          } else if (!nextStderr.trim()) {
            nextStderr = `error: ${error.message}`;
          }
        }

        resolve({
          stdout: stdout ?? "",
          stderr: nextStderr,
          success: !error,
          outputLimitExceeded,
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
    // /usr/bin is where Debian's `prlimit` lives; it must stay on PATH or the
    // bare-command `prlimit` wrapper (buildChildInvocation) silently fails to
    // resolve and the resource limits stop applying.
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

function encodedByteLength(value) {
  return encoder.encode(value).length;
}

function playgroundOutputLimitMessage() {
  return "error: playground output exceeded 128 KiB";
}

function playgroundOutputTruncatedMessage() {
  return "note: output truncated at 128 KiB";
}

function truncateCombinedOutput(stdout, stderr) {
  const notice = playgroundOutputTruncatedMessage();
  const stdoutBudget = Math.max(0, MAX_OUTPUT_BYTES - encodedByteLength(notice));
  const nextStdout = truncateUtf8ToBytes(stdout, stdoutBudget);
  const stderrBudget = Math.max(
    0,
    MAX_OUTPUT_BYTES - encodedByteLength(nextStdout) - encodedByteLength(notice) - 1
  );
  const nextStderr = appendLine(truncateUtf8ToBytes(stderr, stderrBudget), notice);

  return { stdout: nextStdout, stderr: nextStderr };
}

function truncateUtf8ToBytes(value, maxBytes) {
  const bytes = encoder.encode(value);
  if (bytes.byteLength <= maxBytes) return value;

  let truncated = utf8Decoder.decode(bytes.subarray(0, maxBytes));
  while (encodedByteLength(truncated) > maxBytes) {
    truncated = truncated.slice(0, -1);
  }
  return truncated;
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

  if (declaredContentLengthExceeds(request.headers["content-length"], MAX_REQUEST_BYTES)) {
    writeJson(response, 413, {
      stdout: "",
      stderr: "Playground request is too large.",
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

function declaredContentLengthExceeds(contentLength, maxBytes) {
  if (Array.isArray(contentLength)) {
    return contentLength.some((value) => declaredContentLengthExceeds(value, maxBytes));
  }
  if (typeof contentLength !== "string") return false;

  const normalized = contentLength.trim();
  if (!/^\d+$/.test(normalized)) return false;

  const length = Number(normalized);
  return !Number.isSafeInteger(length) || length > maxBytes;
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
