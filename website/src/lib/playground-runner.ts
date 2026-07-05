export type PlaygroundRunResult = {
  stdout: string;
  stderr: string;
  success: boolean;
  durationMs?: number;
  unavailable?: boolean;
};

export type PlaygroundPayloadValidation =
  | { ok: true; source: string }
  | { ok: false; status: number; message: string };

export type PlaygroundRequestReadResult =
  | { ok: true; payload: unknown }
  | { ok: false; status: number; message: string };

export type PlaygroundProxyOptions = {
  fetcher?: typeof fetch;
  runnerUrl?: string;
  token?: string;
};

export type PlaygroundProxyResponse = {
  status: number;
  result: PlaygroundRunResult;
};

export const MAX_PLAYGROUND_SOURCE_BYTES = 64 * 1024;
export const MAX_PLAYGROUND_REQUEST_BYTES = MAX_PLAYGROUND_SOURCE_BYTES + 4096;
export const PLAYGROUND_RUNNER_TIMEOUT_MS = 6000;

const encoder = new TextEncoder();

export async function readPlaygroundRunRequest(
  request: Request,
  maxBytes = MAX_PLAYGROUND_REQUEST_BYTES
): Promise<PlaygroundRequestReadResult> {
  if (!hasJsonContentType(request.headers)) {
    return {
      ok: false,
      status: 415,
      message: "Request content-type must be application/json.",
    };
  }

  if (!request.body) {
    return invalidJsonRequest();
  }

  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    if (!value) continue;

    size += value.byteLength;
    if (size > maxBytes) {
      await reader.cancel().catch(() => undefined);
      return {
        ok: false,
        status: 413,
        message: "Playground request is too large.",
      };
    }

    chunks.push(value);
  }

  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }

  let text: string;
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return invalidJsonRequest();
  }

  try {
    return { ok: true, payload: JSON.parse(text) };
  } catch {
    return invalidJsonRequest();
  }
}

export function validatePlaygroundRunPayload(
  payload: unknown
): PlaygroundPayloadValidation {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    return {
      ok: false,
      status: 400,
      message: "Request body must include a string source field.",
    };
  }

  const source = (payload as { source?: unknown }).source;
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

  if (encoder.encode(source).length > MAX_PLAYGROUND_SOURCE_BYTES) {
    return {
      ok: false,
      status: 413,
      message: "Playground source is limited to 64 KiB.",
    };
  }

  return { ok: true, source };
}

export function validatedRunnerUrl(rawUrl: string | undefined): URL | null {
  if (!rawUrl) return null;

  let url: URL;
  try {
    url = new URL(rawUrl);
  } catch {
    return null;
  }

  if (url.username || url.password) return null;
  if (url.protocol === "https:") return url;

  const localHttp =
    url.protocol === "http:" &&
    (url.hostname === "localhost" ||
      url.hostname === "127.0.0.1" ||
      url.hostname === "[::1]");
  return localHttp ? url : null;
}

export function normalizeRunnerResult(
  value: unknown
): PlaygroundRunResult | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }

  const candidate = value as {
    stdout?: unknown;
    stderr?: unknown;
    success?: unknown;
    durationMs?: unknown;
    unavailable?: unknown;
  };

  if (
    typeof candidate.stdout !== "string" ||
    typeof candidate.stderr !== "string" ||
    typeof candidate.success !== "boolean"
  ) {
    return null;
  }

  const result: PlaygroundRunResult = {
    stdout: candidate.stdout,
    stderr: candidate.stderr,
    success: candidate.success,
  };

  if (typeof candidate.durationMs === "number" && Number.isFinite(candidate.durationMs)) {
    result.durationMs = candidate.durationMs;
  }
  if (candidate.unavailable === true) {
    result.unavailable = true;
  }

  return result;
}

export async function proxyPlaygroundRun(
  source: string,
  options: PlaygroundProxyOptions
): Promise<PlaygroundProxyResponse> {
  const runnerUrl = validatedRunnerUrl(options.runnerUrl);
  const token = normalizedConfiguredToken(options.token);
  if (!runnerUrl || (runnerUrl.protocol === "https:" && !token)) {
    return { status: 503, result: runnerUnavailableResult() };
  }

  const headers = new Headers({
    accept: "application/json",
    "content-type": "application/json",
  });
  if (token) {
    headers.set("authorization", `Bearer ${token}`);
  }

  const started = performance.now();
  const fetcher = options.fetcher ?? fetch;

  try {
    const response = await fetcher(runnerUrl, {
      method: "POST",
      headers,
      body: JSON.stringify({ source }),
      cache: "no-store",
      signal: AbortSignal.timeout(PLAYGROUND_RUNNER_TIMEOUT_MS),
    });
    const runnerResult = normalizeRunnerResult(await response.json().catch(() => null));

    if (!response.ok) {
      if (runnerResult && isSafeRunnerError(response.status)) {
        return { status: response.status, result: runnerResult };
      }

      return {
        status: 502,
        result: runError(`Playground runner returned HTTP ${response.status}.`),
      };
    }

    if (!runnerResult) {
      return {
        status: 502,
        result: runError("Playground runner returned an invalid response."),
      };
    }

    if (runnerResult.durationMs === undefined) {
      runnerResult.durationMs = Math.max(0, Math.round(performance.now() - started));
    }

    return { status: 200, result: runnerResult };
  } catch (error) {
    const timedOut = isTimeoutError(error);
    return {
      status: timedOut ? 504 : 502,
      result: runError(
        timedOut
          ? "Playground execution timed out after 6s."
          : "Playground runner is unavailable."
      ),
    };
  }
}

export function runnerUnavailableResult(): PlaygroundRunResult {
  return {
    stdout: "",
    stderr:
      "Hosted execution is not configured yet. Copy the local command to run this source with the Turbo CLI.",
    success: false,
    unavailable: true,
  };
}

function runError(stderr: string): PlaygroundRunResult {
  return { stdout: "", stderr, success: false };
}

function normalizedConfiguredToken(token: string | undefined): string | null {
  if (typeof token !== "string") return null;

  const normalized = token.trim();
  return normalized.length > 0 ? normalized : null;
}

function invalidJsonRequest(): PlaygroundRequestReadResult {
  return { ok: false, status: 400, message: "Request body must be valid JSON." };
}

function hasJsonContentType(headers: Headers): boolean {
  const contentType = headers.get("content-type");
  if (!contentType) return false;

  return contentType.split(";", 1)[0].trim().toLowerCase() === "application/json";
}

function isTimeoutError(error: unknown): boolean {
  return (
    !!error &&
    typeof error === "object" &&
    "name" in error &&
    error.name === "TimeoutError"
  );
}

function isSafeRunnerError(status: number): boolean {
  return status === 400 || status === 413 || status === 429;
}
