import {
  assertExecRejectedRun,
  assertSafeRun,
  endpointUrl,
  isMainModule,
  requiredUrl,
} from "./playground-smoke-utils.mjs";

const safeSource = 'fn main() { print("site smoke ok") }';

export function playgroundPageUrl(rawSiteUrl) {
  return endpointUrl(rawSiteUrl, "/play");
}

export function playgroundRunUrl(rawSiteUrl) {
  return endpointUrl(rawSiteUrl, "/api/playground/run");
}

export async function runPlaygroundSmoke(options = {}) {
  const siteUrl = requiredUrl(
    options.siteUrl ?? process.env.TURBO_PLAYGROUND_SITE_URL,
    "Set TURBO_PLAYGROUND_SITE_URL or pass the site URL as the first argument."
  );
  const fetcher = options.fetcher ?? fetch;
  const log = options.log ?? (() => {});

  const page = await fetcher(playgroundPageUrl(siteUrl), {
    method: "GET",
    headers: { accept: "text/html" },
  });
  const html = await readText(page, "playground page");
  if (!page.ok) {
    throw new Error(`playground page returned HTTP ${page.status}`);
  }
  if (!html.includes("Turbo Playground") || !html.includes("Try Turbo in the browser")) {
    throw new Error("playground page smoke check did not find the hosted runner UI");
  }

  await assertSafeRun(fetcher, playgroundRunUrl(siteUrl), safeSource, "site smoke ok\n", {
    label: "playground API",
    jsonLabel: "playground API",
  });
  await assertExecRejectedRun(fetcher, playgroundRunUrl(siteUrl), {
    label: "playground API",
    jsonLabel: "playground API",
  });

  log("public playground smoke passed");
  return { ok: true };
}

async function readText(response, label) {
  try {
    return await response.text();
  } catch (error) {
    const message = error instanceof Error ? error.message : "invalid text";
    throw new Error(`${label} returned unreadable text: ${message}`);
  }
}

if (isMainModule(import.meta.url)) {
  runPlaygroundSmoke({
    siteUrl: process.argv[2] ?? process.env.TURBO_PLAYGROUND_SITE_URL,
    log: console.log,
  }).catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  });
}
