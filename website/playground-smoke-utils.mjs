import { pathToFileURL } from "node:url";

export const forbiddenProcessSource = 'fn main() { exec("pwd") }';

export function endpointUrl(rawUrl, pathname) {
  const url = new URL(rawUrl);
  url.pathname = pathname;
  url.search = "";
  url.hash = "";
  return url;
}

export function requiredUrl(value, message) {
  if (!value) {
    throw new Error(message);
  }
  return new URL(value).toString();
}

export async function readJson(response, label) {
  try {
    return await response.json();
  } catch (error) {
    const message = error instanceof Error ? error.message : "invalid JSON";
    throw new Error(`${label} returned invalid JSON: ${message}`);
  }
}

export async function assertSafeRun(fetcher, runUrl, source, expectedStdout, options = {}) {
  const label = options.label ?? "playground";
  const { response, body } = await postRunSource(fetcher, runUrl, source, options);
  if (!response.ok || body?.success !== true || body.stdout !== expectedStdout) {
    throw new Error(`${label} safe execution smoke check failed`);
  }
}

export async function assertExecRejectedRun(fetcher, runUrl, options = {}) {
  const label = options.label ?? "playground";
  const { response, body } = await postRunSource(
    fetcher,
    runUrl,
    forbiddenProcessSource,
    options
  );
  assertExecRejected(response, body, label);
}

async function postRunSource(fetcher, runUrl, source, options) {
  const response = await fetcher(runUrl, {
    method: "POST",
    headers: {
      accept: "application/json",
      "content-type": "application/json",
      ...(options.headers ?? {}),
    },
    body: JSON.stringify({ source }),
  });
  return { response, body: await readJson(response, options.jsonLabel ?? "playground API") };
}

export function assertExecRejected(response, body, label) {
  if (
    response.status !== 400 ||
    body?.success !== false ||
    !String(body?.stderr ?? "").includes("`exec`")
  ) {
    throw new Error(`${label} source-policy smoke check failed`);
  }
}

export function isMainModule(importMetaUrl) {
  return Boolean(process.argv[1] && importMetaUrl === pathToFileURL(process.argv[1]).href);
}
