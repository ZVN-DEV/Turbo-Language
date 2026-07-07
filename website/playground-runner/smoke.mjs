import {
  assertExecRejectedRun,
  assertResourceAbuseContained,
  assertSafeRun,
  endpointUrl,
  isMainModule,
  readJson,
  requiredUrl,
} from "../playground-smoke-utils.mjs";

const safeSource = 'fn main() { print("runner smoke ok") }';

export function runnerHealthUrl(rawRunnerUrl) {
  return endpointUrl(rawRunnerUrl, "/healthz");
}

export async function runSmoke(options = {}) {
  const runnerUrl = requiredUrl(
    options.runnerUrl ?? process.env.TURBO_PLAYGROUND_RUNNER_URL,
    "Set TURBO_PLAYGROUND_RUNNER_URL or pass the runner /run URL as the first argument."
  );
  const fetcher = options.fetcher ?? fetch;
  const token = normalizeToken(options.token ?? process.env.TURBO_PLAYGROUND_RUNNER_TOKEN);
  const log = options.log ?? (() => {});

  const health = await fetcher(runnerHealthUrl(runnerUrl), { method: "GET" });
  const healthBody = await readJson(health, "runner health check");
  if (!health.ok || healthBody?.ok !== true) {
    throw new Error(`runner health check failed with HTTP ${health.status}`);
  }

  await assertSafeRun(fetcher, runnerUrl, safeSource, "runner smoke ok\n", {
    headers: runnerHeaders(token),
    label: "runner",
    jsonLabel: "runner execution",
  });
  await assertExecRejectedRun(fetcher, runnerUrl, {
    headers: runnerHeaders(token),
    label: "runner",
    jsonLabel: "runner execution",
  });
  await assertResourceAbuseContained(fetcher, runnerUrl, {
    headers: runnerHeaders(token),
    label: "runner",
    jsonLabel: "runner execution",
  });

  log("playground runner smoke passed");
  return { ok: true };
}

function runnerHeaders(token) {
  const headers = {};
  if (token) headers.authorization = `Bearer ${token}`;
  return headers;
}

function normalizeToken(value) {
  return typeof value === "string" ? value.trim() : "";
}

if (isMainModule(import.meta.url)) {
  runSmoke({
    runnerUrl: process.argv[2] ?? process.env.TURBO_PLAYGROUND_RUNNER_URL,
    token: process.argv[3] ?? process.env.TURBO_PLAYGROUND_RUNNER_TOKEN,
    log: console.log,
  }).catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  });
}
