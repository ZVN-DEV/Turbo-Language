import { NextResponse } from "next/server";
import {
  proxyPlaygroundRun,
  readPlaygroundRunRequest,
  runError,
  validatePlaygroundRunPayload,
} from "@/lib/playground-runner";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

function jsonRunResult(body: unknown, status: number) {
  return NextResponse.json(body, {
    status,
    headers: {
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
    },
  });
}

export async function POST(request: Request) {
  const body = await readPlaygroundRunRequest(request);
  if (!body.ok) {
    return jsonRunResult(runError(body.message), body.status);
  }

  const validated = validatePlaygroundRunPayload(body.payload);
  if (!validated.ok) {
    return jsonRunResult(runError(validated.message), validated.status);
  }

  const proxied = await proxyPlaygroundRun(validated.source, {
    runnerUrl: process.env.TURBO_PLAYGROUND_RUNNER_URL,
    token: process.env.TURBO_PLAYGROUND_RUNNER_TOKEN,
  });
  return jsonRunResult(proxied.result, proxied.status);
}
