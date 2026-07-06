"use client";

import { useEffect, useMemo, useReducer, useState } from "react";
import Link from "next/link";
import type { PlaygroundRunResult } from "@/lib/playground-runner";
import {
  MAX_SHARE_URL_LENGTH,
  commandFor,
  defaultExample,
  examples,
  lineNumbersFor,
  shareUrlFor,
} from "@/lib/playground";

type CopyTarget = "command" | "share";
type CopyState = { status: "copied" | "manual"; target: CopyTarget } | null;
type OutputMode = "expected" | "command" | "result";
type PlaygroundState = {
  code: string;
  manualCopyText: string | null;
  outputMode: OutputMode;
  runResult: PlaygroundRunResult | null;
  shareError: string | null;
};
type PlaygroundAction =
  | { type: "apply-shared-code"; code: string }
  | { type: "copy-failed-manual"; target: CopyTarget; text: string }
  | { type: "copy-succeeded"; target: CopyTarget }
  | { type: "edit-code"; code: string }
  | { type: "load-example"; code: string }
  | { type: "run-finished"; result: PlaygroundRunResult }
  | { type: "run-started" }
  | { type: "run-stopped"; result: PlaygroundRunResult }
  | { type: "share-too-large"; message: string }
  | { type: "show-command" };

const initialPlaygroundState: PlaygroundState = {
  code: defaultExample.code,
  manualCopyText: null,
  outputMode: "expected",
  runResult: null,
  shareError: null,
};

function playgroundReducer(
  state: PlaygroundState,
  action: PlaygroundAction
): PlaygroundState {
  switch (action.type) {
    case "apply-shared-code":
      return {
        ...state,
        code: action.code,
        manualCopyText: null,
        outputMode: "command",
        runResult: null,
        shareError: null,
      };
    case "copy-failed-manual":
      return {
        ...state,
        manualCopyText: action.text,
        shareError: action.target === "share" ? null : state.shareError,
      };
    case "copy-succeeded":
      return {
        ...state,
        manualCopyText: null,
        shareError: action.target === "share" ? null : state.shareError,
      };
    case "edit-code":
      return {
        ...state,
        code: action.code,
        outputMode: "command",
        shareError: null,
      };
    case "load-example":
      return {
        ...state,
        code: action.code,
        manualCopyText: null,
        outputMode: "expected",
        runResult: null,
        shareError: null,
      };
    case "run-finished":
      return {
        ...state,
        runResult: action.result,
      };
    case "run-started":
      return {
        ...state,
        manualCopyText: null,
        outputMode: "result",
        runResult: null,
        shareError: null,
      };
    case "run-stopped":
      return {
        ...state,
        outputMode: "result",
        runResult: action.result,
      };
    case "share-too-large":
      return {
        ...state,
        manualCopyText: null,
        outputMode: "command",
        shareError: action.message,
      };
    case "show-command":
      return {
        ...state,
        outputMode: "command",
      };
  }
}

export default function PlaygroundClient() {
  const [exampleId, setExampleId] = useState(defaultExample.id);
  const [{ code, manualCopyText, outputMode, runResult, shareError }, dispatch] =
    useReducer(playgroundReducer, initialPlaygroundState);
  const [copyState, setCopyState] = useState<CopyState>(null);
  const [isRunning, setIsRunning] = useState(false);

  const selectedExample =
    examples.find((example) => example.id === exampleId) ?? defaultExample;
  const lineNumbers = useMemo(() => lineNumbersFor(code), [code]);
  const runCommand = useMemo(
    () => commandFor(selectedExample, code),
    [code, selectedExample]
  );

  useEffect(() => {
    const sharedCode = new URLSearchParams(window.location.search).get("code");
    if (!sharedCode) return;

    dispatch({ type: "apply-shared-code", code: sharedCode });
  }, []);

  function loadExample(id: string) {
    const next = examples.find((example) => example.id === id) ?? defaultExample;
    setExampleId(next.id);
    dispatch({ type: "load-example", code: next.code });
  }

  function setTimedCopyState(nextState: NonNullable<CopyState>) {
    setCopyState(nextState);
    window.setTimeout(() => setCopyState(null), 1800);
  }

  async function copyText(text: string, target: CopyTarget) {
    try {
      await navigator.clipboard.writeText(text);
      dispatch({ type: "copy-succeeded", target });
      setTimedCopyState({ status: "copied", target });
    } catch {
      const fallback = document.createElement("textarea");
      fallback.value = text;
      fallback.setAttribute("readonly", "");
      fallback.style.position = "fixed";
      fallback.style.left = "-9999px";
      document.body.appendChild(fallback);
      fallback.focus();
      fallback.select();
      const copiedFallback = document.execCommand("copy");
      document.body.removeChild(fallback);
      if (copiedFallback) {
        dispatch({ type: "copy-succeeded", target });
        setTimedCopyState({ status: "copied", target });
      } else {
        dispatch({ type: "copy-failed-manual", target, text });
        setTimedCopyState({ status: "manual", target });
      }
    }
  }

  async function runCode() {
    if (isRunning) return;
    if (code.trim().length === 0) {
      dispatch({
        type: "run-stopped",
        result: {
          stdout: "",
          stderr: "Enter Turbo source before running it.",
          success: false,
        },
      });
      return;
    }

    setIsRunning(true);
    dispatch({ type: "run-started" });

    const started = performance.now();
    try {
      const response = await fetch("/api/playground/run", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ source: code }),
      });
      const result = (await response.json()) as Partial<PlaygroundRunResult>;
      const durationMs =
        typeof result.durationMs === "number" && Number.isFinite(result.durationMs)
          ? result.durationMs
          : Math.max(0, Math.round(performance.now() - started));

      dispatch({
        type: "run-finished",
        result: {
          stdout: typeof result.stdout === "string" ? result.stdout : "",
          stderr:
            typeof result.stderr === "string"
              ? result.stderr
              : `Playground request failed with HTTP ${response.status}.`,
          success: response.ok && result.success === true,
          durationMs,
          unavailable: result.unavailable === true,
        },
      });
    } catch {
      dispatch({
        type: "run-finished",
        result: {
          stdout: "",
          stderr:
            "Could not reach hosted execution. Copy the local command to run this source with the Turbo CLI.",
          success: false,
          durationMs: Math.max(0, Math.round(performance.now() - started)),
          unavailable: true,
        },
      });
    } finally {
      setIsRunning(false);
    }
  }

  function shareCode() {
    const shareUrl = shareUrlFor(window.location.href, code);

    if (shareUrl.length > MAX_SHARE_URL_LENGTH) {
      dispatch({
        type: "share-too-large",
        message: `Share links are limited to ${MAX_SHARE_URL_LENGTH.toLocaleString()} encoded URL characters. Copy the local run command instead.`,
      });
      return;
    }

    void copyText(shareUrl, "share");
  }

  function copyCommand() {
    dispatch({ type: "show-command" });
    void copyText(runCommand, "command");
  }

  const commandButtonLabel =
    copyState?.target === "command"
      ? copyState.status === "copied"
        ? "Copied"
        : "Copy manually"
      : "Copy command";
  const shareButtonLabel =
    copyState?.target === "share"
      ? copyState.status === "copied"
        ? "Link copied"
        : "Copy manually"
      : "Share";
  const runButtonLabel = isRunning ? "Running..." : "Run";

  return (
    <div className="min-h-[calc(100vh-4rem)] bg-background font-[family-name:var(--font-geist-sans)]">
      <section className="border-b border-border">
        <div className="mx-auto max-w-6xl px-6 py-10 md:py-12">
          <div className="flex flex-col gap-6 md:flex-row md:items-end md:justify-between">
            <div className="max-w-2xl">
              <p className="mb-3 text-xs font-semibold uppercase tracking-[0.28em] text-accent">
                Turbo Playground
              </p>
              <h1 className="text-4xl font-bold leading-tight text-white md:text-5xl">
                Try Turbo in the browser
              </h1>
              <p className="mt-4 max-w-xl text-base leading-7 text-gray-400 md:text-lg">
                Load real Turbo examples, shape the code in a hosted editor, and
                run it through a configured sandbox or the local CLI.
              </p>
            </div>
            <div className="flex flex-wrap gap-3">
              <button
                type="button"
                onClick={runCode}
                disabled={isRunning}
                className="inline-flex items-center gap-2 rounded-lg bg-accent px-5 py-3 text-sm font-semibold text-[#06110a] transition-colors hover:bg-[#00cc6a] disabled:cursor-not-allowed disabled:opacity-70"
              >
                <svg
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  fill="currentColor"
                  aria-hidden="true"
                >
                  <path d="M8 5v14l11-7z" />
                </svg>
                {runButtonLabel}
              </button>
              <button
                type="button"
                onClick={copyCommand}
                className="inline-flex items-center gap-2 rounded-lg border border-border px-5 py-3 text-sm font-semibold text-gray-300 transition-colors hover:border-accent hover:text-accent"
              >
                <svg
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  aria-hidden="true"
                >
                  <rect x="9" y="9" width="13" height="13" rx="2" />
                  <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                </svg>
                {commandButtonLabel}
              </button>
            </div>
          </div>
        </div>
      </section>

      <section className="mx-auto grid max-w-6xl gap-4 px-6 py-6 lg:grid-cols-[minmax(0,1.2fr)_minmax(360px,0.8fr)]">
        <div className="min-w-0 overflow-hidden rounded-lg border border-border bg-surface">
          <div className="flex flex-col gap-3 border-b border-border bg-[#0d0d12] px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
            <div className="flex min-w-0 items-center gap-3">
              <span className="text-xs font-semibold uppercase tracking-[0.2em] text-gray-400">
                Editor
              </span>
              <span className="hidden h-1 w-1 rounded-full bg-gray-700 sm:block" />
              <span className="truncate text-xs text-gray-500">
                {selectedExample.filename}
              </span>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <select
                aria-label="Load example"
                value={exampleId}
                onChange={(event) => loadExample(event.target.value)}
                className="h-9 rounded-md border border-border bg-background px-3 text-sm text-gray-300 outline-none transition-colors hover:border-accent focus:border-accent"
              >
                {examples.map((example) => (
                  <option key={example.id} value={example.id}>
                    {example.label}
                  </option>
                ))}
              </select>
              <button
                type="button"
                onClick={shareCode}
                className="h-9 rounded-md border border-border px-3 text-sm text-gray-400 transition-colors hover:border-accent hover:text-accent"
              >
                {shareButtonLabel}
              </button>
            </div>
          </div>

          <div className="grid min-h-[520px] min-w-0 grid-cols-[48px_minmax(0,1fr)] bg-[#0a0a0d]">
            <div className="select-none border-r border-border bg-[#0d0d12] px-3 py-4 text-right font-[family-name:var(--font-geist-mono)] text-sm leading-6 text-gray-600">
              {lineNumbers.map((line) => (
                <div key={line}>{line}</div>
              ))}
            </div>
            <textarea
              value={code}
              onChange={(event) =>
                dispatch({ type: "edit-code", code: event.target.value })
              }
              spellCheck={false}
              className="min-h-[520px] min-w-0 resize-none bg-transparent p-4 font-[family-name:var(--font-geist-mono)] text-sm leading-6 text-gray-200 outline-none selection:bg-accent/20"
              aria-label="Turbo source editor"
            />
          </div>
        </div>

        <div className="grid min-w-0 gap-4">
          <div className="min-w-0 rounded-lg border border-border bg-surface">
            <div className="flex items-center justify-between border-b border-border bg-[#0d0d12] px-4 py-3">
              <span className="text-xs font-semibold uppercase tracking-[0.2em] text-gray-400">
                Output
              </span>
              <span className="rounded-full border border-border px-2 py-1 text-xs text-gray-500">
                {code.length} chars
              </span>
            </div>
            <div className="min-h-[260px] p-4">
              {outputMode === "expected" ? (
                <pre className="whitespace-pre-wrap font-[family-name:var(--font-geist-mono)] text-sm leading-6 text-gray-300">
                  {selectedExample.expected}
                </pre>
              ) : outputMode === "result" ? (
                <div className="space-y-4">
                  {isRunning && (
                    <p className="text-sm leading-6 text-gray-400">
                      Running through the configured sandbox...
                    </p>
                  )}

                  {!isRunning && runResult && (
                    <>
                      <div
                        className={`rounded-md border p-3 ${
                          runResult.success
                            ? "border-[#00ff8830] bg-[#00ff880d]"
                            : "border-[#fbbf2430] bg-[#fbbf240d]"
                        }`}
                      >
                        <p
                          className={`text-xs font-semibold uppercase tracking-[0.18em] ${
                            runResult.success ? "text-accent" : "text-[#fbbf24]"
                          }`}
                        >
                          {runResult.success ? "Exited successfully" : "Run stopped"}
                        </p>
                        {typeof runResult.durationMs === "number" && (
                          <p className="mt-1 text-xs text-gray-500">
                            {Math.round(runResult.durationMs)} ms
                          </p>
                        )}
                      </div>

                      {runResult.stdout || runResult.stderr ? (
                        <pre className="overflow-x-auto rounded-md border border-border bg-background p-3 font-[family-name:var(--font-geist-mono)] text-xs leading-5 text-gray-300">
                          <code>
                            {runResult.stdout}
                            {runResult.stdout && runResult.stderr ? "\n" : ""}
                            {runResult.stderr}
                          </code>
                        </pre>
                      ) : (
                        <p className="text-sm leading-6 text-gray-400">
                          Program finished with no output.
                        </p>
                      )}

                      {runResult.unavailable && (
                        <div className="rounded-md border border-border bg-background p-3">
                          <p className="mb-2 text-sm leading-6 text-gray-400">
                            Hosted execution needs a configured sandbox runner.
                            This command runs the same source locally.
                          </p>
                          <pre className="overflow-x-auto font-[family-name:var(--font-geist-mono)] text-xs leading-5 text-gray-300">
                            <code>{runCommand}</code>
                          </pre>
                        </div>
                      )}
                    </>
                  )}

                  {!isRunning && !runResult && (
                    <p className="text-sm leading-6 text-gray-400">
                      Run the source to see stdout and diagnostics.
                    </p>
                  )}
                </div>
              ) : (
                <div className="space-y-4">
                  <p className="text-sm leading-6 text-gray-400">
                    Copy this command to run the current source through the
                    released CLI. Hosted execution only runs through a
                    configured sandbox runner.
                  </p>
                  <pre className="overflow-x-auto rounded-md border border-border bg-background p-3 font-[family-name:var(--font-geist-mono)] text-xs leading-5 text-gray-300">
                    <code>{runCommand}</code>
                  </pre>
                  {shareError && (
                    <div className="rounded-md border border-[#fbbf2430] bg-[#fbbf240d] p-3">
                      <p className="mb-1 text-xs font-semibold uppercase tracking-[0.18em] text-[#fbbf24]">
                        Share link too large
                      </p>
                      <p className="text-sm leading-6 text-gray-300">
                        {shareError}
                      </p>
                    </div>
                  )}
                  {manualCopyText && (
                    <div className="rounded-md border border-[#fbbf2430] bg-[#fbbf240d] p-3">
                      <p className="mb-2 text-xs font-semibold uppercase tracking-[0.18em] text-[#fbbf24]">
                        Clipboard unavailable
                      </p>
                      <pre className="max-h-40 overflow-auto whitespace-pre-wrap font-[family-name:var(--font-geist-mono)] text-xs leading-5 text-gray-300">
                        <code>{manualCopyText}</code>
                      </pre>
                    </div>
                  )}
                </div>
              )}
            </div>
          </div>

          <div className="min-w-0 rounded-lg border border-border bg-surface p-5">
            <h2 className="mb-4 text-sm font-semibold uppercase tracking-[0.2em] text-gray-400">
              Reference
            </h2>
            <div className="grid gap-2 font-[family-name:var(--font-geist-mono)] text-xs text-gray-400">
              <code className="rounded-md bg-background px-3 py-2 text-gray-300">
                fn greet(name: str) -&gt; str {"{ ... }"}
              </code>
              <code className="rounded-md bg-background px-3 py-2 text-gray-300">
                let value: i64? = some(42)
              </code>
              <code className="rounded-md bg-background px-3 py-2 text-gray-300">
                match result {"{ ok(v) => v, err(e) => 0 }"}
              </code>
            </div>
            <div className="mt-5 flex flex-wrap gap-3">
              <Link
                href="/docs/hello-world"
                className="text-sm font-semibold text-accent hover:text-[#00cc6a]"
              >
                Hello World docs
              </Link>
              <Link
                href="/docs/examples"
                className="text-sm font-semibold text-accent hover:text-[#00cc6a]"
              >
                More examples
              </Link>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
